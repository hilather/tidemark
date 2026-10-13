//! Owned by: W7-B1
//! Staging benchmarks with explicit host-wide cache consent and raw evidence.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File},
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Validated benchmark options.
#[derive(Debug)]
pub struct Options {
    pub root: PathBuf,
    pub modes: Vec<String>,
    pub caches: Vec<String>,
    pub out: PathBuf,
    pub throttle: String,
    pub drop_caches: bool,
    pub force: bool,
    pub watcher: bool,
    pub duration: u64,
    pub rate: u64,
}
/// Parse without running any command or touching host configuration.
pub fn parse_args(args: impl Iterator<Item = String>) -> Result<Options> {
    let mut o = Options {
        root: PathBuf::new(),
        modes: vec!["du".into(), "walk".into(), "bulk".into()],
        caches: vec!["warm".into()],
        out: "docs/perf".into(),
        throttle: "max".into(),
        drop_caches: false,
        force: false,
        watcher: false,
        duration: 10,
        rate: 1_000_000,
    };
    let mut args = args;
    let mut seen = BTreeSet::new();
    while let Some(a) = args.next() {
        if !seen.insert(a.clone()) {
            bail!("duplicate argument {a}");
        }
        match a.as_str() {
            "--drop-caches-on-this-host" => o.drop_caches = true,
            "--force" => o.force = true,
            "--watcher-overhead" => o.watcher = true,
            _ => {
                let v = args.next().with_context(|| format!("{a} needs value"))?;
                match a.as_str() {
                    "--root" => o.root = v.into(),
                    "--out" => o.out = v.into(),
                    "--modes" => o.modes = v.split(',').map(String::from).collect(),
                    "--cache" => o.caches = v.split(',').map(String::from).collect(),
                    "--throttle" => o.throttle = v,
                    "--duration" => o.duration = v.parse()?,
                    "--mutate-rate" => o.rate = v.parse()?,
                    _ => bail!("unknown bench argument {a}"),
                }
            }
        }
    }
    if o.root.as_os_str().is_empty()
        || o.modes.is_empty()
        || o.modes.len() > 3
        || o.caches.len() > 2
        || o.modes.iter().collect::<BTreeSet<_>>().len() != o.modes.len()
        || o.caches.iter().collect::<BTreeSet<_>>().len() != o.caches.len()
        || o.caches.is_empty()
        || o.modes
            .iter()
            .any(|m| !matches!(m.as_str(), "du" | "walk" | "bulk"))
        || o.caches
            .iter()
            .any(|c| !matches!(c.as_str(), "warm" | "cold"))
        || !matches!(o.throttle.as_str(), "gentle" | "normal" | "max")
        || o.duration == 0
        || o.duration > 86400
        || o.rate == 0
        || o.rate > 1_000_000
    {
        bail!("invalid bench options; --root DIR --modes du,walk,bulk --cache warm[,cold] [--out DIR] [--throttle max|normal|gentle]");
    }
    if o.caches.iter().any(|c| c == "cold") && !o.drop_caches {
        bail!("cold cache requires --drop-caches-on-this-host: dropping page caches affects the entire host and requires root");
    }
    if o.watcher && o.caches.iter().any(|c| c == "cold") {
        bail!("watcher experiment uses warm mutation workload, not cold scans");
    }
    Ok(o)
}
/// Command as data, retaining raw OS paths and avoiding a shell.
#[derive(Debug, Clone)]
pub struct Invocation {
    pub program: OsString,
    pub args: Vec<OsString>,
}
impl Invocation {
    fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args);
        c
    }
    fn printable(&self) -> Value {
        json!({"program":self.program.to_string_lossy(),"args":self.args.iter().map(|s|s.to_string_lossy()).collect::<Vec<_>>()})
    }
}
/// Build the two du measurements or a private direct scan invocation.
pub fn commands(
    workspace: &Path,
    o: &Options,
    config: &Path,
    state: &Path,
    mode: &str,
) -> Vec<Invocation> {
    if mode == "du" {
        [false, true]
            .into_iter()
            .map(|apparent| {
                let mut args: Vec<OsString> = vec!["-sx".into(), "--block-size=1".into()];
                if apparent {
                    args.push("--apparent-size".into());
                }
                args.push("--".into());
                args.push(o.root.as_os_str().into());
                Invocation {
                    program: "du".into(),
                    args,
                }
            })
            .collect()
    } else {
        vec![Invocation {
            program: workspace.join("target/release/tidemark").into_os_string(),
            args: vec![
                "--config".into(),
                config.as_os_str().into(),
                "--state-dir".into(),
                state.as_os_str().into(),
                "--json".into(),
                "scan".into(),
                o.root.as_os_str().into(),
                "--mode".into(),
                mode.into(),
                "--throttle".into(),
                o.throttle.clone().into(),
            ],
        }]
    }
}
/// Build a standalone watcher command for the overhead experiment.
pub fn watcher_command(workspace: &Path, root: &Path, config: &Path) -> Invocation {
    Invocation {
        program: workspace.join("target/release/tidemark").into_os_string(),
        args: vec![
            "--config".into(),
            config.as_os_str().into(),
            "--json".into(),
            "watch".into(),
            "--mount".into(),
            root.as_os_str().into(),
            "--events".into(),
            "close_write".into(),
        ],
    }
}
fn is_root() -> Result<bool> {
    let status = fs::read_to_string("/proc/self/status")?;
    let uid = status
        .lines()
        .find(|s| s.starts_with("Uid:"))
        .context("missing uid")?;
    Ok(uid.split_whitespace().nth(2) == Some("0"))
}
fn cold_preflight(force: bool) -> Result<()> {
    let root = is_root()?;
    let load: f64 = fs::read_to_string("/proc/loadavg")?
        .split_whitespace()
        .next()
        .context("missing load")?
        .parse()?;
    let cpus = std::thread::available_parallelism()?.get() as f64;
    let mut daemon = false;
    for e in fs::read_dir("/proc")? {
        let e = e?;
        if e.file_name().to_string_lossy().parse::<u32>().is_ok()
            && fs::read_to_string(e.path().join("comm")).is_ok_and(|s| s.trim() == "tidemarkd")
        {
            daemon = true;
        }
    }
    cold_guard(root, daemon, load, cpus, force)
}
fn cold_guard(root: bool, daemon: bool, load: f64, cpus: f64, force: bool) -> Result<()> {
    if !root {
        bail!("cold cache needs root");
    }
    if !force && (daemon || load > cpus) {
        bail!("refusing cold cache: tidemarkd active={daemon}, load={load}, CPUs={cpus}; staging-only --force overrides these guards");
    }
    Ok(())
}
fn drop_caches(force: bool) -> Result<()> {
    cold_preflight(force)?;
    eprintln!("HOST-WIDE: flushing writes and dropping page/dentry/inode caches on this host (--drop-caches-on-this-host)");
    if !Command::new("sync").status()?.success() {
        bail!("sync failed");
    }
    fs::write("/proc/sys/vm/drop_caches", b"3\n")?;
    Ok(())
}
fn leaves(root: &Path) -> Result<Vec<PathBuf>> {
    let dev = fs::metadata(root)?.dev();
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & 0xfffff000);
    let minor = (dev & 0xff) | ((dev >> 12) & 0xffffff00);
    let start = PathBuf::from(format!("/sys/dev/block/{major}:{minor}"));
    if !start.exists() {
        return Ok(Vec::new());
    }
    let mut todo = vec![fs::canonicalize(start)?];
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    while let Some(mut p) = todo.pop() {
        if !seen.insert(p.clone()) {
            continue;
        }
        if seen.len() > 256 {
            bail!("device graph exceeds cap");
        }
        if p.join("partition").exists() {
            p = p.parent().context("partition parent")?.into();
        }
        let slaves = fs::read_dir(p.join("slaves"))?.collect::<std::io::Result<Vec<_>>>()?;
        if slaves.is_empty() {
            if !result.contains(&p) {
                result.push(p);
            }
        } else {
            for s in slaves {
                todo.push(fs::canonicalize(s.path())?);
            }
        }
    }
    result.sort();
    Ok(result)
}
struct Disks {
    leaves: Vec<PathBuf>,
    previous: BTreeMap<PathBuf, (Instant, Vec<u64>)>,
}
impl Disks {
    fn sample(&mut self, phase: &str, out: &mut File) -> Result<()> {
        for p in &self.leaves {
            let stat: Vec<u64> = fs::read_to_string(p.join("stat"))?
                .split_whitespace()
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            if stat.len() < 11 {
                bail!("short block stat");
            }
            let now = Instant::now();
            let derived=self.previous.get(p).map(|(t,old)| { let ms=now.duration_since(*t).as_secs_f64()*1000.0; let ios=stat[0].saturating_sub(old[0])+stat[4].saturating_sub(old[4]); json!({"interval_ms":ms,"util_percent":100.0*stat[9].saturating_sub(old[9]) as f64/ms,"await_ms":if ios>0 { Some((stat[3].saturating_sub(old[3])+stat[7].saturating_sub(old[7])) as f64/ios as f64) } else { None }}) });
            writeln!(
                out,
                "{}",
                json!({"phase":phase,"unix_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),"device":p.file_name().map(|s|s.to_string_lossy()),"stat":stat,"derived":derived})
            )?;
            self.previous.insert(p.clone(), (now, stat));
        }
        Ok(())
    }
}
fn tree_rss(parent: u32) -> u64 {
    let mut todo = vec![parent];
    let mut seen = BTreeSet::new();
    let mut rss = 0;
    while let Some(pid) = todo.pop() {
        if !seen.insert(pid) || seen.len() > 4096 {
            continue;
        }
        if let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) {
            rss += status
                .lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
        }
        if let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) {
            for task in tasks.flatten() {
                if let Ok(children) = fs::read_to_string(task.path().join("children")) {
                    todo.extend(
                        children
                            .split_whitespace()
                            .filter_map(|s| s.parse::<u32>().ok()),
                    );
                }
            }
        }
    }
    rss
}
#[derive(Serialize)]
struct Measurement {
    command: Value,
    wall_seconds: f64,
    user_seconds: f64,
    system_seconds: f64,
    peak_rss_kib: u64,
    sampled_tree_peak_rss_kib: u64,
    measurement_source: String,
    stdout: String,
    stderr: String,
    result: Value,
}
fn bounded_read(p: &Path) -> Result<String> {
    if fs::metadata(p)?.len() > 16 * 1024 * 1024 {
        bail!(
            "command output exceeds 16MiB; raw output retained at {}",
            p.display()
        );
    }
    Ok(fs::read_to_string(p)?)
}
fn child_ticks() -> Result<(u64, u64)> {
    let text = fs::read_to_string("/proc/self/stat")?;
    let fields = text
        .rsplit_once(')')
        .context("invalid proc stat")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    Ok((
        fields.get(13).context("missing cutime")?.parse()?,
        fields.get(14).context("missing cstime")?.parse()?,
    ))
}
fn measure(
    inv: &Invocation,
    out: &Path,
    label: &str,
    disks: &mut Disks,
    series: &mut File,
) -> Result<Measurement> {
    let stdout = out.join(format!("{label}.stdout"));
    let stderr = out.join(format!("{label}.stderr"));
    let times = out.join(format!("{label}.time"));
    let gnu_time = Path::new("/usr/bin/time").is_file();
    let mut c = if gnu_time {
        let mut c = Command::new("/usr/bin/time");
        c.args(["-f", "%U %S %M", "-o"])
            .arg(&times)
            .arg("--")
            .arg(&inv.program)
            .args(&inv.args);
        c
    } else {
        inv.command()
    };
    c.stdout(File::create(&stdout)?)
        .stderr(File::create(&stderr)?);
    let hz = if gnu_time {
        1.0
    } else {
        let output = Command::new("getconf").arg("CLK_TCK").output()?;
        if !output.status.success() {
            bail!("getconf CLK_TCK failed");
        }
        let hz = String::from_utf8(output.stdout)?.trim().parse::<f64>()?;
        if hz <= 0.0 {
            bail!("invalid clock tick rate");
        }
        hz
    };
    let before = child_ticks()?;
    disks.sample(label, series)?;
    let started = Instant::now();
    let mut child = c.spawn()?;
    let mut peak = 0;
    let mut sample = Instant::now();
    let status = loop {
        peak = peak.max(tree_rss(child.id()));
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if sample.elapsed() >= Duration::from_secs(1) {
            disks.sample(label, series)?;
            sample = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let wall = started.elapsed().as_secs_f64();
    disks.sample(label, series)?;
    if !status.success() {
        bail!("{label} failed ({status}); inspect {}", stderr.display());
    }
    let (user, system, rss, source) = if gnu_time {
        let t = fs::read_to_string(&times)?;
        let t = t.split_whitespace().collect::<Vec<_>>();
        if t.len() != 3 {
            bail!("invalid GNU time output");
        }
        (
            t[0].parse()?,
            t[1].parse()?,
            t[2].parse()?,
            "gnu-time-rusage",
        )
    } else {
        let after = child_ticks()?;
        let user = after.0.saturating_sub(before.0) as f64 / hz;
        let system = after.1.saturating_sub(before.1) as f64 / hz;
        fs::write(times,format!("{user} {system} {peak}\nprocfs fallback: sampled RSS lower bound; CPU tick resolution\n"))?;
        (
            user,
            system,
            peak,
            "procfs-child-cpu-ticks-and-sampled-rss-lower-bound",
        )
    };
    let stdout = bounded_read(&stdout)?;
    let stderr = bounded_read(&stderr)?;
    let result = serde_json::from_str(&stdout).unwrap_or_else(
        |_| json!({"bytes":stdout.split_whitespace().next().and_then(|s|s.parse::<u64>().ok())}),
    );
    Ok(Measurement {
        command: inv.printable(),
        wall_seconds: wall,
        user_seconds: user,
        system_seconds: system,
        peak_rss_kib: rss,
        sampled_tree_peak_rss_kib: peak,
        measurement_source: source.into(),
        stdout,
        stderr,
        result,
    })
}
fn private_config(out: &Path) -> Result<PathBuf> {
    let config = out.join("bench.toml");
    let state = out.join("state");
    let socket = out.join("private.sock");
    fs::create_dir_all(&state)?;
    let string = |p: &Path| -> Result<String> {
        Ok(toml::Value::String(
            p.to_str()
                .context("private output path must be UTF-8 for TOML")?
                .into(),
        )
        .to_string())
    };
    fs::write(
        &config,
        format!(
            "[general]\nstate_dir = {}\nsocket = {}\n",
            string(&state)?,
            string(&socket)?
        ),
    )?;
    Ok(config)
}
fn ensure_binary(workspace: &Path) -> Result<()> {
    if !workspace.join("target/release/tidemark").is_file() {
        let status = Command::new("cargo")
            .current_dir(workspace)
            .args([
                "build",
                "--offline",
                "--locked",
                "--release",
                "-p",
                "tidemark-cli",
            ])
            .status()?;
        if !status.success() {
            bail!("release CLI build failed");
        }
    }
    Ok(())
}
/// Execute benchmark measurements; privileged paths require explicit flags.
pub fn run(workspace: &Path, args: impl Iterator<Item = OsString>) -> Result<()> {
    let (args, root_path, out_path) = crate::gen_fixture::os_args(args)?;
    let mut o = parse_args(args.into_iter())?;
    if let Some(root) = root_path {
        o.root = root;
    }
    if let Some(out) = out_path {
        o.out = out;
    }
    let root = crate::gen_fixture::safe_root(&o.root)?;
    o.root = root.original.clone();
    if o.watcher && !is_root()? {
        bail!("watcher overhead requires root on staging");
    }
    if o.caches.iter().any(|c| c == "cold") {
        cold_preflight(o.force)?;
    }
    fs::create_dir_all(&o.out)?;
    o.out = fs::canonicalize(&o.out)?;
    if o.out.starts_with(&o.root) {
        bail!("--out must be outside measured root");
    }
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let date = Command::new("date").args(["-u", "+%Y-%m-%d"]).output()?;
    if !date.status.success() {
        bail!("date failed");
    }
    let host = fs::read_to_string("/proc/sys/kernel/hostname")?;
    let host: String = host
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = format!("results-{}-{host}", String::from_utf8(date.stdout)?.trim());
    let run_dir = o
        .out
        .join(format!("{stem}-{seconds}-{}", std::process::id()));
    fs::create_dir(&run_dir)?;
    let config = private_config(&run_dir)?;
    if o.watcher || o.modes.iter().any(|m| m != "du") {
        ensure_binary(workspace)?;
    }
    let leaf = leaves(&o.root)?;
    let mut disks = Disks {
        leaves: leaf.clone(),
        previous: BTreeMap::new(),
    };
    let mut series = File::create(run_dir.join("block-stat.jsonl"))?;
    disks.sample("baseline", &mut series)?;
    for _ in 0..2 {
        std::thread::sleep(Duration::from_secs(1));
        disks.sample("baseline", &mut series)?;
    }
    let mut rows = Vec::new();
    if o.watcher {
        rows = watcher_experiment(workspace, &o, &config, &run_dir, &mut disks, &mut series)?;
    } else {
        for cache in &o.caches {
            for mode in &o.modes {
                let state = run_dir.join(format!("state-{cache}-{mode}"));
                fs::create_dir(&state)?;
                let invocations = commands(workspace, &o, &config, &state, mode);
                let mut measurements = Vec::new();
                for (index, inv) in invocations.iter().enumerate() {
                    if cache == "cold" {
                        drop_caches(o.force)?;
                    } else {
                        let warm = inv
                            .command()
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status()?;
                        if !warm.success() {
                            bail!("warm-up failed for {mode}");
                        }
                    }
                    measurements.push(measure(
                        inv,
                        &run_dir,
                        &format!("{cache}-{mode}-{index}"),
                        &mut disks,
                        &mut series,
                    )?);
                }
                rows.push(json!({"cache":cache,"mode":mode,"measurements":measurements,"gate_statistics":null,"gate_statistics_note":"current scan --json does not publish gate statistics"}));
            }
        }
    }
    let document = json!({"schema":"tidemark.bench.v1","root":o.root.to_string_lossy(),"throttle":o.throttle,"leaf_devices":leaf,"block_series":run_dir.join("block-stat.jsonl"),"rss_note":"Per-row source identifies GNU time rusage or procfs fallback; 20ms sampled aggregate tree RSS may miss brief peaks","rows":rows});
    let json_path = o.out.join(format!("{stem}.json"));
    fs::write(&json_path, serde_json::to_vec_pretty(&document)?)?;
    let mut md = File::create(o.out.join(format!("{stem}.md")))?;
    writeln!(md,"# Benchmark {stem}\n\nRoot: `{}`; throttle `{}`. Raw evidence: [{}]({}); per-command output and block series: `{}`.\n\n| Cache | Mode | Wall s | User s | System s | Peak RSS KiB | Result |\n|---|---|---:|---:|---:|---:|---|",o.root.display(),o.throttle,json_path.display(),json_path.display(),run_dir.display())?;
    for row in &rows {
        if let Some(ms) = row["measurements"].as_array() {
            for m in ms {
                writeln!(
                    md,
                    "| {} | {} | {} | {} | {} | {} | `{}` |",
                    row["cache"],
                    row["mode"],
                    m["wall_seconds"],
                    m["user_seconds"],
                    m["system_seconds"],
                    m["peak_rss_kib"],
                    m["result"].to_string().replace('|', "\\|")
                )?;
            }
        } else {
            writeln!(md, "\nWatcher result: `{row}`")?;
        }
    }
    writeln!(md,"\nGate statistics unavailable in current CLI JSON. RSS includes GNU time rusage maximum and a sampled aggregate tree peak in raw JSON. No performance gate is inferred automatically.")?;
    println!("{}", json_path.display());
    Ok(())
}
fn watcher_experiment(
    workspace: &Path,
    o: &Options,
    config: &Path,
    out: &Path,
    disks: &mut Disks,
    series: &mut File,
) -> Result<Vec<Value>> {
    let dir = crate::gen_fixture::safe_root(&o.root)?;
    let (count, seed, dense) = crate::gen_fixture::fixture_identity(&dir)?;
    let mut inv = Invocation {
        program: std::env::current_exe()?.into_os_string(),
        args: vec![
            "gen-fixture".into(),
            "--profile".into(),
            "reports".into(),
            "--inodes".into(),
            count.to_string().into(),
            "--seed".into(),
            seed.to_string().into(),
            "--root".into(),
            o.root.as_os_str().into(),
            "--mutate-rate".into(),
            o.rate.to_string().into(),
            "--duration".into(),
            o.duration.to_string().into(),
        ],
    };
    if dense {
        inv.args.push("--dense".into());
    }
    let baseline = measure(&inv, out, "writer-alone", disks, series)?;
    let watch = watcher_command(workspace, &o.root, config);
    let mut child = watch
        .command()
        .stdout(File::create(out.join("watcher.stdout"))?)
        .stderr(File::create(out.join("watcher.stderr"))?)
        .spawn()?;
    std::thread::sleep(Duration::from_secs(1));
    if child.try_wait()?.is_some() {
        bail!("watcher exited before workload; inspect watcher.stderr");
    }
    let watched = measure(&inv, out, "writer-watched", disks, series);
    // Killing only the private foreground child never affects a system daemon.
    let _ = child.kill();
    let _ = child.wait();
    let watched = watched?;
    let throughput = |m: &Measurement| -> Result<Value> {
        Ok(serde_json::from_str(
            m.stdout.lines().last().context("missing mutate summary")?,
        )?)
    };
    let baseline_throughput = throughput(&baseline)?;
    let watched_throughput = throughput(&watched)?;
    let b = baseline_throughput["operations_per_second"]
        .as_f64()
        .context("missing baseline throughput")?;
    let w = watched_throughput["operations_per_second"]
        .as_f64()
        .context("missing watched throughput")?;
    Ok(vec![
        json!({"throughput_loss_percent":100.0*(1.0-w/b),"experiment":"watcher-overhead","watcher_command":watch.printable(),"baseline":throughput(&baseline)?,"watched":throughput(&watched)?,"baseline_measurement":baseline,"watched_measurement":watched,"note":"foreground watcher startup is checked for liveness; verify event delivery from watcher.stdout; CPU budget requires separate watcher CPU evidence"}),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cold_guards() {
        assert!(cold_guard(false, false, 0.0, 4.0, true).is_err());
        assert!(cold_guard(true, true, 0.0, 4.0, false).is_err());
        assert!(cold_guard(true, false, 5.0, 4.0, false).is_err());
        assert!(cold_guard(true, true, 5.0, 4.0, true).is_ok());
        assert!(cold_guard(true, false, 1.0, 4.0, false).is_ok());
    }
    #[test]
    fn parsing_and_commands() {
        let o = parse_args(
            [
                "--root",
                "/tmp/fixture",
                "--modes",
                "du,walk",
                "--cache",
                "warm",
            ]
            .into_iter()
            .map(String::from),
        )
        .unwrap();
        let du = commands(
            Path::new("/repo"),
            &o,
            Path::new("/config"),
            Path::new("/state"),
            "du",
        );
        assert_eq!(du.len(), 2);
        assert_eq!(du[0].args[0], "-sx");
        assert!(du[1].args.contains(&"--apparent-size".into()));
        let scan = commands(
            Path::new("/repo"),
            &o,
            Path::new("/private/config"),
            Path::new("/private/state"),
            "walk",
        );
        assert_eq!(scan[0].program, "/repo/target/release/tidemark");
        assert!(scan[0].args.contains(&"--json".into()));
        assert!(parse_args(
            ["--root", "/tmp/f", "--cache", "cold"]
                .into_iter()
                .map(String::from)
        )
        .is_err());
        assert!(parse_args(
            ["--root", "/tmp/f", "--modes", "invalid"]
                .into_iter()
                .map(String::from)
        )
        .is_err());
        assert!(
            watcher_command(Path::new("/repo"), Path::new("/f"), Path::new("/c"))
                .args
                .contains(&"close_write".into())
        );
    }
}
