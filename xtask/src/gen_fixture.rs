//! Owned by: W7-B1
//! Deterministic, streamed report fixtures and bounded mutation workloads.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{symlink, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const NOFOLLOW: i32 = 0x20000;
const DIRECTORY: i32 = 0x10000;
const NOATIME: i32 = 0x40000;
const MARKER: &str = ".tidemark-fixture";

/// Generator options; inode count includes root, directories and control files.
#[derive(Debug, Clone)]
pub struct Options {
    pub root: PathBuf,
    pub inodes: u64,
    pub seed: u64,
    pub dense: bool,
    pub rate: u64,
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
    if !mounts.iter().any(|m| canonical.starts_with(m)) {
        bail!("cannot resolve root mount parent chain");
    }
    if mounts
        .iter()
        .any(|m| m.starts_with(&canonical) && m != &canonical)
    {
        bail!("nested mount below fixture root");
    }
    capability.original = canonical;
    Ok(capability)
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
                if !s.iter().all(|b| (b'0'..=b'7').contains(b)) {
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
    pub directories: u64,
    pub regular: u64,
    pub symlinks: u64,
    pub control_files: u64,
    pub apparent_bytes: u64,
    pub allocated_bytes: u64,
    pub link_sets: u64,
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
fn add_bytes(s: &mut Summary, path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    s.apparent_bytes += m.len();
    s.allocated_bytes += m.blocks() * 512;
    Ok(())
}
fn marker(dir: &Dir) -> Result<Marker> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW)
        .open(dir.path().join(MARKER))?;
    if !f.metadata()?.is_file() || f.metadata()?.len() > 4096 {
        bail!("invalid fixture marker");
    }
    let mut data = String::new();
    f.read_to_string(&mut data)?;
    Ok(serde_json::from_str(&data)?)
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
/// Generate a fixture using constant memory, then optionally mutate it.
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
    let mut s = Summary {
        directories: dirs + 1,
        regular,
        symlinks: syms,
        control_files: 2,
        ..Summary::default()
    };
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
        add_bytes(&mut s, &p)?;
        if i % 5 == 0 {
            let links = 2 + rng.next() % 3;
            s.link_sets += 1;
            for n in 1..=links {
                let target = (d + n) % dirs;
                let dest = Dir::open(&dir.path().join(format!("report-{target:04}")))?;
                fs::hard_link(&p, dest.path().join(format!("link-{i:012}")))?;
                s.hard_link_entries += 1;
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
        add_bytes(&mut s, &p)?;
    }
    let root_metadata = dir.file.metadata()?;
    s.apparent_bytes += root_metadata.len();
    s.allocated_bytes += root_metadata.blocks() * 512;
    for i in 0..dirs {
        add_bytes(&mut s, &dir.path().join(format!("report-{i:04}")))?;
    }
    // Control-file byte totals excluded: summary cannot include its own serialized size.
    serde_json::to_writer_pretty(create(&dir.path().join(".tidemark-fixture.json"))?, &s)?;
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
pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    generate(&parse_args(args)?)?;
    Ok(())
}
