//! Owned by: W7-B1
//! Deterministic, streamed report fixtures and bounded mutation workloads.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::{
        fs::{symlink, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Component, Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const NOFOLLOW: i32 = 0x20000;
const DIRECTORY: i32 = 0x10000;
const NOATIME: i32 = 0x40000;
const MARKER: &str = ".tidemark-fixture";

/// Generator options; inode count includes root, directories and control files.
#[derive(Debug, Clone)]
pub struct Options {
    /// Existing empty or marked fixture directory.
    pub root: PathBuf,
    /// Target unique-inode count including control files.
    pub inodes: u64,
    /// Deterministic SplitMix64 seed.
    pub seed: u64,
    /// Write large-file contents instead of creating holes.
    pub dense: bool,
    /// Mutation logical operations per second; zero disables mutation.
    pub rate: u64,
    /// Optional mutation duration in seconds.
    pub duration: Option<u64>,
}
/// Parse generator arguments without performing filesystem operations.
pub fn parse_args(args: impl Iterator<Item = String>) -> Result<Options> {
    let mut o = Options {
        root: PathBuf::new(),
        inodes: 0,
        seed: 1,
        dense: false,
        rate: 0,
        duration: None,
    };
    let mut args = args;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(a) = args.next() {
        if !seen.insert(a.clone()) {
            bail!("duplicate argument {a}");
        }
        if a == "--dense" {
            o.dense = true;
            continue;
        }
        let v = args.next().with_context(|| format!("{a} needs a value"))?;
        match a.as_str() {
            "--root" => o.root = v.into(),
            "--profile" if v == "reports" => {}
            "--inodes" => o.inodes = v.parse()?,
            "--seed" => o.seed = v.parse()?,
            "--mutate-rate" => o.rate = v.parse()?,
            "--duration" => o.duration = Some(v.parse()?),
            _ => bail!("unknown argument or profile: {a} {v}"),
        }
    }
    if o.root.as_os_str().is_empty() || o.inodes < 32 || !seen.contains("--profile") {
        bail!("usage: gen-fixture --profile reports --inodes N (>=32) --root DIR [--seed S] [--dense] [--mutate-rate R] [--duration seconds]");
    }
    if o.inodes > 100_000_000 || o.rate > 1_000_000 || o.duration == Some(0) {
        bail!("count/rate/duration outside supported bounds");
    }
    if o.duration.is_some() && o.rate == 0 {
        bail!("--duration requires --mutate-rate");
    }
    Ok(o)
}

/// A no-follow directory capability, addressed through Linux procfs.
pub struct Dir {
    file: File,
    /// Validated absolute path for operator reporting.
    pub original: PathBuf,
}
impl Dir {
    /// Open a directory without following its final component.
    pub fn open(path: &Path) -> Result<Self> {
        let open = |flags| OpenOptions::new().read(true).custom_flags(flags).open(path);
        let file = match open(DIRECTORY | NOFOLLOW | NOATIME) {
            Err(e) if e.raw_os_error() == Some(1) => open(DIRECTORY | NOFOLLOW)?,
            other => other?,
        };
        Ok(Self {
            file,
            original: path.into(),
        })
    }
    /// Anchored path valid while this capability remains alive.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
    }
}
/// Validate every root component and mount topology before opening a capability.
pub fn safe_root(path: &Path) -> Result<Dir> {
    let absolute = if path.is_absolute() {
        path.into()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::from("/");
    let mut capability = Dir::open(Path::new("/"))?;
    for c in absolute.components() {
        match c {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(n) => {
                current.push(n);
                let child = capability.path().join(n);
                if fs::symlink_metadata(&child)?.file_type().is_symlink() {
                    bail!("symbolic link in root: {}", current.display());
                }
                capability = Dir::open(&child)?;
            }
            _ => bail!("root cannot contain parent components"),
        }
    }
    let canonical = current;
    if canonical == Path::new("/") {
        bail!("refusing filesystem root /");
    }
    let mounts = mount_points()?;
    validate_mounts(&canonical, &mounts)?;
    capability.original = canonical;
    Ok(capability)
}
fn validate_mounts(canonical: &Path, mounts: &[PathBuf]) -> Result<()> {
    if !mounts.iter().any(|m| canonical.starts_with(m)) {
        bail!("cannot resolve root mount parent chain");
    }
    if mounts
        .iter()
        .any(|m| m.starts_with(canonical) && m != canonical)
    {
        bail!("nested mount below fixture root");
    }
    Ok(())
}
/// Decode mountinfo's byte escapes and reject malformed topology.
pub fn mount_points() -> Result<Vec<PathBuf>> {
    use std::os::unix::ffi::OsStringExt;
    let contents = fs::read("/proc/self/mountinfo")?;
    let mut mounts = Vec::new();
    for line in contents.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let field = line
            .split(|b| *b == b' ')
            .nth(4)
            .context("invalid mountinfo")?;
        let mut decoded = Vec::new();
        let mut i = 0;
        while i < field.len() {
            if field[i] == b'\\' {
                let s = field.get(i + 1..i + 4).context("invalid mount escape")?;
                if s[0] > b'3' || !s.iter().all(|b| (b'0'..=b'7').contains(b)) {
                    bail!("invalid mount escape");
                }
                decoded.push((s[0] - b'0') * 64 + (s[1] - b'0') * 8 + s[2] - b'0');
                i += 4;
            } else {
                decoded.push(field[i]);
                i += 1;
            }
        }
        mounts.push(PathBuf::from(std::ffi::OsString::from_vec(decoded)));
        if mounts.len() > 65536 {
            bail!("mount table exceeds cap");
        }
    }
    Ok(mounts)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Marker {
    profile: String,
    seed: u64,
    count: u64,
    dense: bool,
}
/// Unique-inode totals (hard-link aliases do not add bytes or inodes).
#[derive(Default, Serialize, Deserialize, Debug)]
pub struct Summary {
    /// Snapshot phase; mutation starts after this summary is written.
    pub state: String,
    /// Directory inodes, including the root.
    pub directories: u64,
    /// Unique regular data-file inodes.
    pub regular: u64,
    /// Unique symlink inodes.
    pub symlinks: u64,
    /// Marker and summary regular-file inodes.
    pub control_files: u64,
    /// Unique inode apparent bytes including directories and controls.
    pub apparent_bytes: u64,
    /// Unique inode allocated bytes including directories and controls.
    pub allocated_bytes: u64,
    /// Data-file inodes with aliases.
    pub link_sets: u64,
    /// Additional hard-link names.
    pub hard_link_entries: u64,
}
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        ((self.next() >> 11) as f64 + 0.5) / ((1u64 << 53) as f64)
    }
    fn size(&mut self) -> u64 {
        let normal = (-2.0 * self.unit().ln()).sqrt() * (std::f64::consts::TAU * self.unit()).cos();
        (32768.0 * (normal * 2.0).exp()).clamp(1.0, 2147483648.0) as u64
    }
}
fn create(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(NOFOLLOW)
        .open(path)?)
}
/// Flush the fixture filesystem after closing writers, outside measured runs.
pub fn flush_fixture(dir: &Dir) -> Result<()> {
    // xtask has no rustix dependency. The child cannot use /proc/self/fd:
    // Rust closes these fds on exec, so address the parent capability instead.
    if !Command::new("sync")
        .arg("-f")
        .arg(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            dir.file.as_raw_fd()
        ))
        .status()?
        .success()
    {
        bail!("fixture filesystem flush failed");
    }
    Ok(())
}

fn finished_summary(dir: &Dir) -> Result<Summary> {
    let mut s = Summary {
        state: "before_mutation".into(),
        ..Summary::default()
    };
    // Only multi-link non-directory inodes are retained. At 20M inodes the
    // reports profile has <=4M link sets: a HashSet<(u64,u64)> uses about
    // 136 MiB (8M buckets, 16-byte keys + control bytes), <204 MiB while growing.
    // Cap at 20M sets for the supported 100M-inode profile; never retain paths.
    let mut linked = HashSet::new();
    let device = dir.file.metadata()?.dev();
    let mut account = |path: &Path, m: &fs::Metadata| -> Result<bool> {
        if m.dev() != device {
            bail!("fixture changed filesystem");
        }
        if !m.is_dir() && m.nlink() > 1 {
            let key = (m.dev(), m.ino());
            if linked.contains(&key) {
                s.hard_link_entries += 1;
                return Ok(false);
            }
            if linked.len() >= 20_000_000 {
                bail!("fixture exceeds multi-link inode accounting cap");
            }
            linked.insert(key);
            s.link_sets += 1;
        }
        s.apparent_bytes += m.len();
        s.allocated_bytes += m.blocks() * 512;
        if m.is_dir() {
            s.directories += 1;
        } else if m.file_type().is_symlink() {
            s.symlinks += 1;
        } else if path
            .file_name()
            .is_some_and(|n| n == MARKER || n == ".tidemark-fixture.json")
        {
            s.control_files += 1;
        } else {
            s.regular += 1;
        }
        Ok(m.is_dir())
    };
    // The procfs capability path is itself a symlink: fstat the held root.
    account(&dir.original, &dir.file.metadata()?)?;
    // Generated fixtures are flat: stream each report directory without a
    // width-dependent queue or recursion. Dir::open preserves no-follow/noatime.
    for entry in fs::read_dir(dir.path())? {
        let path = entry?.path();
        // lstat includes symlink inodes without following their targets.
        if account(&path, &fs::symlink_metadata(&path)?)? {
            let child = Dir::open(&path)?;
            for entry in fs::read_dir(child.path())? {
                let path = entry?.path();
                if account(&path, &fs::symlink_metadata(&path)?)? {
                    bail!("unexpected nested directory in fixture summary");
                }
            }
        }
    }
    Ok(s)
}
fn marker(dir: &Dir) -> Result<Marker> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW | 0x800)
        .open(dir.path().join(MARKER))?;
    if !f.metadata()?.is_file() || f.metadata()?.len() > 4096 {
        bail!("invalid fixture marker");
    }
    let mut data = String::new();
    f.read_to_string(&mut data)?;
    Ok(serde_json::from_str(&data)?)
}
/// Read bounded, no-follow fixture identity for the watcher workload.
pub fn fixture_identity(dir: &Dir) -> Result<(u64, u64, bool)> {
    let m = marker(dir)?;
    if m.profile != "reports" || m.count < 32 || m.count > 100_000_000 {
        bail!("invalid fixture identity");
    }
    Ok((m.count, m.seed, m.dense))
}
fn clear(dir: &Dir) -> Result<()> {
    // Two passes: validate all directories first, so unexpected subtrees are refused.
    for entry in fs::read_dir(dir.path())? {
        let e = entry?;
        let m = e.metadata()?;
        if m.is_dir() && !e.file_type()?.is_symlink() {
            let child = Dir::open(&e.path())?;
            for item in fs::read_dir(child.path())? {
                if item?.file_type()?.is_dir() {
                    bail!("unexpected nested directory in fixture");
                }
            }
        }
    }
    for entry in fs::read_dir(dir.path())? {
        let e = entry?;
        if e.file_type()?.is_dir() {
            let child = Dir::open(&e.path())?;
            for item in fs::read_dir(child.path())? {
                fs::remove_file(item?.path())?;
            }
            fs::remove_dir(e.path())?;
        } else {
            fs::remove_file(e.path())?;
        }
    }
    Ok(())
}
/// Generate a fixture with bounded link accounting, then optionally mutate it.
pub fn generate(o: &Options) -> Result<Summary> {
    let dir = safe_root(&o.root)?;
    let populated = fs::read_dir(dir.path())?.next().is_some();
    if populated {
        let m =
            marker(&dir).context("root must be empty or have a valid .tidemark-fixture marker")?;
        if m.profile != "reports" {
            bail!("unknown marker profile");
        }
        clear(&dir)?;
    }
    let m = Marker {
        profile: "reports".into(),
        seed: o.seed,
        count: o.inodes,
        dense: o.dense,
    };
    serde_json::to_writer(create(&dir.path().join(MARKER))?, &m)?;
    // Three wide dirs plus >= five report dirs; at scale reports average 35k.
    let scale = (o.inodes as f64 / 5_000_000.0).min(1.0);
    let wide = ((100_000.0 * scale) as u64).max(1);
    let remaining = o.inodes.saturating_sub(3 * wide + 3);
    let reports = (remaining / 35_000).clamp(5, 3000);
    let dirs = reports + 3;
    let available = o.inodes - dirs - 3; // root and two control files
    let syms = available / 10;
    let regular = available - syms;
    for i in 0..dirs {
        fs::create_dir(dir.path().join(format!("report-{i:04}")))?;
    }
    let mut rng = Random(o.seed);
    let report_for = |i: u64| {
        if i < 3 * wide {
            i / wide
        } else {
            3 + (i - 3 * wide) % reports
        }
    };
    for i in 0..regular {
        let d = report_for(i);
        let child = Dir::open(&dir.path().join(format!("report-{d:04}")))?;
        let p = child.path().join(format!("file-{i:012}"));
        let mut f = create(&p)?;
        let size = rng.size();
        if o.dense || size <= 1_048_576 {
            let buffer = [0u8; 65536];
            let mut left = size;
            while left > 0 {
                let n = left.min(buffer.len() as u64) as usize;
                f.write_all(&buffer[..n])?;
                left -= n as u64;
            }
        } else {
            f.set_len(size)?;
        }
        drop(f);
        if i % 5 == 0 {
            let links = 2 + rng.next() % 3;
            for n in 1..=links {
                let target = (d + n) % dirs;
                let dest = Dir::open(&dir.path().join(format!("report-{target:04}")))?;
                fs::hard_link(&p, dest.path().join(format!("link-{i:012}")))?;
            }
        }
    }
    for i in 0..syms {
        let d = i % dirs;
        let child = Dir::open(&dir.path().join(format!("report-{d:04}")))?;
        let p = child.path().join(format!("sym-{i:012}"));
        let target = if i % 2 == 0 {
            format!("../report-{:04}", (d + 1) % dirs)
        } else {
            format!("../report-0000/file-{:012}", i % wide)
        };
        symlink(target, p.clone())?;
    }
    // Fixed-size JSON control files make their own size/blocks non-circular.
    let summary_path = dir.path().join(".tidemark-fixture.json");
    let mut summary_file = create(&summary_path)?;
    summary_file.write_all(&[b' '; 4096])?;
    drop(summary_file);
    flush_fixture(&dir)?;
    let s = finished_summary(&dir)?;
    let mut summary_file = OpenOptions::new()
        .write(true)
        .custom_flags(NOFOLLOW)
        .open(&summary_path)?;
    let encoded = serde_json::to_vec_pretty(&s)?;
    if encoded.len() > 4096 {
        bail!("summary exceeds fixed control-file size");
    }
    summary_file.seek(SeekFrom::Start(0))?;
    summary_file.write_all(&encoded)?;
    drop(summary_file);
    flush_fixture(&dir)?;
    println!("{}", serde_json::to_string(&s)?);
    if o.rate > 0 {
        mutate(&dir, o.rate, o.duration)?;
    }
    Ok(s)
}
/// Run fixed create/append/link/delete proportions with a bounded 1024-slot ring.
pub fn mutate(dir: &Dir, rate: u64, duration: Option<u64>) -> Result<u64> {
    marker(dir).context("mutation requires marker")?;
    let p = dir.path().join("mutations");
    if !p.exists() {
        fs::create_dir(&p)?;
    }
    let child = Dir::open(&p)?;
    let started = Instant::now();
    let mut ops = 0u64;
    loop {
        if duration.is_some_and(|d| started.elapsed() >= Duration::from_secs(d)) {
            break;
        }
        let slot = (ops / 4) % 1024;
        let base = child.path().join(format!("slot-{slot}"));
        let alias = child.path().join(format!("alias-{slot}"));
        match ops % 4 {
            0 => {
                create(&base)?.write_all(&[0u8; 4096])?;
            }
            1 => {
                OpenOptions::new()
                    .append(true)
                    .custom_flags(NOFOLLOW)
                    .open(&base)?
                    .write_all(&[0u8; 4096])?;
            }
            2 => fs::hard_link(&base, &alias)?,
            _ => {
                fs::remove_file(&alias)?;
                fs::remove_file(&base)?;
            }
        }
        ops += 1;
        if rate > 0 {
            let deadline = Duration::from_secs_f64(ops as f64 / rate as f64);
            if let Some(wait) = deadline.checked_sub(started.elapsed()) {
                std::thread::sleep(wait.min(Duration::from_secs(1)));
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"mutation_operations":ops,"seconds":started.elapsed().as_secs_f64(),"operations_per_second":ops as f64/started.elapsed().as_secs_f64()})
    );
    Ok(ops)
}
/// Dispatch the fixture command.
pub fn run(args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let (args, root, _) = os_args(args)?;
    let mut options = parse_args(args.into_iter())?;
    if let Some(root) = root {
        options.root = root;
    }
    generate(&options)?;
    Ok(())
}

/// Retain raw path bytes while validating text-only flags and numeric values.
pub fn os_args(
    args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(Vec<String>, Option<PathBuf>, Option<PathBuf>)> {
    let mut args = args;
    let mut text = Vec::new();
    let mut root = None;
    let mut out = None;
    while let Some(arg) = args.next() {
        let flag = arg.to_str().context("flag/value must be UTF-8")?.to_owned();
        text.push(flag.clone());
        if flag == "--root" || flag == "--out" {
            let path = PathBuf::from(args.next().context("path flag needs a value")?);
            if flag == "--root" {
                root = Some(path);
            } else {
                out = Some(path);
            }
            text.push("raw-path-placeholder".into());
        }
    }
    Ok((text, root, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finished_summary_counts_three_links_once() {
        let root = tempfile::tempdir().unwrap();
        let p = root.path();
        fs::create_dir(p.join("report")).unwrap();
        fs::write(p.join("one"), [0u8; 4096]).unwrap();
        fs::write(p.join("two"), [0u8; 8192]).unwrap();
        fs::hard_link(p.join("one"), p.join("alias")).unwrap();
        fs::hard_link(p.join("one"), p.join("report/alias")).unwrap();
        symlink("one", p.join("sym")).unwrap();
        let dir = safe_root(p).unwrap();
        flush_fixture(&dir).unwrap();
        let s = finished_summary(&dir).unwrap();
        assert_eq!((s.directories, s.regular, s.symlinks), (2, 2, 1));
        assert_eq!((s.link_sets, s.hard_link_entries), (1, 2));
        let unique = [
            p.to_path_buf(),
            p.join("report"),
            p.join("one"),
            p.join("two"),
            p.join("sym"),
        ];
        let metadata: Vec<_> = unique
            .iter()
            .map(|p| fs::symlink_metadata(p).unwrap())
            .collect();
        assert_eq!(
            s.allocated_bytes,
            metadata.iter().map(|m| m.blocks() * 512).sum::<u64>()
        );
        assert_eq!(
            s.apparent_bytes,
            metadata.iter().map(MetadataExt::size).sum::<u64>()
        );
        assert_eq!(s.allocated_bytes, test_support::oracle::du_disk(p));
        assert_eq!(
            s.apparent_bytes,
            test_support::oracle::du_apparent_normalized(p).unwrap()
        );
    }
    #[test]
    fn mutation_requires_marker() {
        let root = tempfile::tempdir().unwrap();
        let dir = safe_root(root.path()).unwrap();
        assert!(mutate(&dir, 10, Some(1)).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[test]
    fn random_reproducible_and_bounded() {
        let mut a = Random(42);
        let mut b = Random(42);
        for _ in 0..1000 {
            let size = a.size();
            assert_eq!(size, b.size());
            assert!((1..=2147483648).contains(&size));
        }
    }
    #[test]
    fn mount_refusals() {
        assert!(validate_mounts(Path::new("/tmp/fixture"), &[]).is_err());
        assert!(validate_mounts(
            Path::new("/tmp/fixture"),
            &["/".into(), "/tmp/fixture/nested".into()]
        )
        .is_err());
        assert!(validate_mounts(Path::new("/tmp/fixture"), &["/".into()]).is_ok());
    }
    #[test]
    fn raw_paths_retained() {
        use std::os::unix::ffi::OsStringExt;
        let raw = std::ffi::OsString::from_vec(b"/tmp/raw-\xff".to_vec());
        let (_, root, _) = os_args(["--root".into(), raw.clone()].into_iter()).unwrap();
        assert_eq!(root.unwrap().as_os_str(), raw);
    }
}
