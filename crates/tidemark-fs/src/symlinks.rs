//! Owned by: W3-B
//! Gated metadata-only symlink analysis without symlink traversal.

use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, RawDir, StatxFlags, CWD};
use std::{
    io,
    mem::MaybeUninit,
    os::fd::OwnedFd,
    sync::atomic::{AtomicBool, Ordering},
};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::Gate,
};

const TASK_CAP: usize = 65_536;
const BYTE_CAP: usize = 64 * 1024 * 1024;
const EXAMPLE_CAP: usize = 1_000_000;
/// Symlink target checks and bounded example output.
pub struct SymlinkOpts {
    /// Follow only the target with stat to diagnose dangling links, loops and other devices.
    pub check_targets: bool,
    /// Maximum number of examples; at most one million.
    pub limit: usize,
}
/// Target classification; lexical membership remains available without target checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymlinkClass {
    /// Target is lexically contained in the root.
    InsideRoot,
    /// Target is lexically outside the root.
    OutsideRoot,
    /// Target stat reports another filesystem.
    OtherFs,
    /// Target does not exist.
    Dangling,
    /// Target resolution encountered a symlink loop.
    Loop,
    /// Link text or target metadata could not be read.
    Unreadable,
}
const CLASSES: [SymlinkClass; 6] = [
    SymlinkClass::InsideRoot,
    SymlinkClass::OutsideRoot,
    SymlinkClass::OtherFs,
    SymlinkClass::Dangling,
    SymlinkClass::Loop,
    SymlinkClass::Unreadable,
];
/// Counts cover the full traversal; examples are deterministic and bounded.
pub struct SymlinkReport {
    /// All symlink entries encountered.
    pub total: u64,
    /// Counts in stable class order, including zero counts.
    pub by_class: Vec<(SymlinkClass, u64)>,
    /// Links with absolute target text.
    pub absolute: u64,
    /// Links with relative target text.
    pub relative: u64,
    /// Byte-preserving link paths and target text with classifications.
    pub examples: Vec<(RawPath, Vec<u8>, SymlinkClass)>,
}
fn sx(fd: &OwnedFd, gate: &dyn Gate) -> io::Result<fs::Statx> {
    gate.acquire(1);
    Ok(fs::statx(
        fd,
        &b""[..],
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )?)
}
fn open_one(fd: impl std::os::fd::AsFd, name: &[u8], gate: &dyn Gate) -> io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    gate.acquire(1);
    match fs::openat(fd.as_fd(), name, flags | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => {
            gate.acquire(1);
            Ok(fs::openat(fd, name, flags, Mode::empty())?)
        }
        r => Ok(r?),
    }
}
fn same_fs(a: &fs::Statx, b: &fs::Statx) -> bool {
    a.stx_dev_major == b.stx_dev_major
        && a.stx_dev_minor == b.stx_dev_minor
        && (a.stx_mnt_id == 0 || b.stx_mnt_id == a.stx_mnt_id)
}
// Reopen every path component securely. Revalidate each descendant component against
// the root mount, preventing races that replace a queued ancestor with a bind mount.
fn open_path(
    path: &RawPath,
    root: &RawPath,
    identity: Option<&fs::Statx>,
    gate: &dyn Gate,
) -> io::Result<OwnedFd> {
    let mut fd = open_one(CWD, b"/", gate)?;
    let mut bytes = vec![b'/'];
    if root.as_bytes() == b"/" {
        if let Some(id) = identity {
            if !same_fs(id, &sx(&fd, gate)?) {
                return Err(io::Error::other("root mount changed"));
            }
        }
    }
    for part in path
        .as_bytes()
        .split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
    {
        fd = open_one(&fd, part, gate)?;
        if bytes.len() > 1 {
            bytes.push(b'/');
        }
        bytes.extend_from_slice(part);
        if let Some(id) = identity {
            if bytes.len() >= root.as_bytes().len() && !same_fs(id, &sx(&fd, gate)?) {
                return Err(io::Error::other("mount changed during analysis"));
            }
        }
    }
    Ok(fd)
}
fn lexical(root: &RawPath, dir: &RawPath, target: &[u8]) -> io::Result<SymlinkClass> {
    let bytes = if target.starts_with(b"/") {
        target.to_vec()
    } else {
        let mut b = dir.as_bytes().to_vec();
        b.push(b'/');
        b.extend_from_slice(target);
        b
    };
    let path = RawPath::new(bytes).map_err(io::Error::other)?;
    Ok(if path.starts_with(root) {
        SymlinkClass::InsideRoot
    } else {
        SymlinkClass::OutsideRoot
    })
}
/// Analyze a tree on one filesystem without descending through any symlink.
///
/// Every open, getdents, stat and readlink is charged before execution. Directory
/// opens use O_NOATIME with the EPERM fallback. Mount discovery fails closed.
/// Root paths are capped at 1 MiB. Pending directories are capped at 65,536
/// and 64 MiB of paths, directory names
/// at one million entries and 64 MiB, and examples at one million and 64 MiB.
/// Cancellation returns Interrupted rather than a deceptively complete report.
pub fn analyze(
    root: &RawPath,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    opts: &SymlinkOpts,
) -> io::Result<SymlinkReport> {
    analyze_matching(root, gate, cancel, opts, |_, _, _| true)
}
/// Analyze all links while retaining only matching examples.
///
/// Filtering precedes the example limit and byte cap; all class totals still
/// cover the entire tree. This additive helper preserves the frozen `analyze`
/// and `SymlinkOpts` contracts while allowing bounded CLI filtering.
pub fn analyze_matching(
    root: &RawPath,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    opts: &SymlinkOpts,
    include: impl Fn(&RawPath, &[u8], SymlinkClass) -> bool,
) -> io::Result<SymlinkReport> {
    if root.as_bytes().len() > 1024 * 1024 {
        return Err(io::Error::other("root path exceeds 1 MiB cap"));
    }
    if opts.limit > EXAMPLE_CAP {
        return Err(io::Error::other("example limit exceeds one million"));
    }
    cancelled(cancel)?;
    let fd = open_path(root, root, None, gate)?;
    let identity = sx(&fd, gate)?;
    gate.acquire(1);
    let mounts = crate::mount::read_mountinfo()?;
    let mut mounts = crate::mount::submounts_under(&mounts, root);
    mounts.sort_unstable();
    drop(fd);
    let mut tasks = vec![(root.clone(), identity.stx_ino)];
    let mut task_bytes = root.as_bytes().len();
    let mut example_bytes = 0usize;
    let mut report = SymlinkReport {
        total: 0,
        by_class: CLASSES.into_iter().map(|c| (c, 0)).collect(),
        absolute: 0,
        relative: 0,
        examples: Vec::new(),
    };
    let mut buffer = vec![MaybeUninit::uninit(); 256 * 1024];
    while let Some((dir, ino)) = tasks.pop() {
        cancelled(cancel)?;
        task_bytes -= dir.as_bytes().len();
        let fd = open_path(&dir, root, Some(&identity), gate)?;
        let info = sx(&fd, gate)?;
        if info.stx_ino != ino || !same_fs(&identity, &info) {
            return Err(io::Error::other("directory changed during analysis"));
        }
        let mut iter = RawDir::new(&fd, &mut buffer);
        let mut names = Vec::new();
        let mut name_bytes = 0usize;
        loop {
            cancelled(cancel)?;
            if iter.is_buffer_empty() {
                gate.acquire(1);
            }
            let Some(entry) = iter.next() else { break };
            let entry = entry?;
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            name_bytes += bytes.len();
            if names.len() == EXAMPLE_CAP || name_bytes > BYTE_CAP {
                return Err(io::Error::other("directory name cap reached"));
            }
            names.push(RawName::new(bytes.to_vec()).map_err(io::Error::other)?);
        }
        names.sort_unstable();
        // All entries are lstat-ed: d_type may race and may be DT_UNKNOWN.
        let mut children = Vec::new();
        for name in names {
            cancelled(cancel)?;
            let path = dir.join(&name);
            if mounts.binary_search(&path).is_ok() {
                continue;
            }
            gate.acquire(1);
            let stat = fs::statx(
                &fd,
                name.as_bytes(),
                AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
                StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
            )?;
            match FileType::from_raw_mode(stat.stx_mode.into()) {
                FileType::Directory if same_fs(&identity, &stat) => {
                    task_bytes += path.as_bytes().len();
                    if tasks.len() + children.len() >= TASK_CAP || task_bytes > BYTE_CAP {
                        return Err(io::Error::other("directory task cap reached"));
                    }
                    children.push((path, stat.stx_ino));
                }
                FileType::Symlink => {
                    report.total += 1;
                    gate.acquire(1);
                    let (target, class) = match fs::readlinkat(&fd, name.as_bytes(), Vec::new()) {
                        Ok(text) => {
                            let target = text.into_bytes();
                            if target.starts_with(b"/") {
                                report.absolute += 1;
                            } else {
                                report.relative += 1;
                            }
                            let mut class = lexical(root, &dir, &target)?;
                            if opts.check_targets {
                                gate.acquire(1);
                                class = match fs::statat(&fd, target.as_slice(), AtFlags::empty()) {
                                    Ok(st)
                                        if st.st_dev
                                            != libc::makedev(
                                                identity.stx_dev_major,
                                                identity.stx_dev_minor,
                                            ) =>
                                    {
                                        SymlinkClass::OtherFs
                                    }
                                    Ok(_) => class,
                                    Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR) => {
                                        SymlinkClass::Dangling
                                    }
                                    Err(rustix::io::Errno::LOOP) => SymlinkClass::Loop,
                                    Err(_) => SymlinkClass::Unreadable,
                                };
                            }
                            (target, class)
                        }
                        Err(_) => (Vec::new(), SymlinkClass::Unreadable),
                    };
                    for (c, count) in &mut report.by_class {
                        if *c == class {
                            *count += 1;
                        }
                    }
                    if report.examples.len() < opts.limit && include(&path, &target, class) {
                        example_bytes += path.as_bytes().len() + target.len();
                        if example_bytes > BYTE_CAP {
                            return Err(io::Error::other("example byte cap reached"));
                        }
                        report.examples.push((path, target, class));
                    }
                }
                _ => {}
            }
        }
        tasks.extend(children.into_iter().rev());
    }
    report.examples.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(report)
}
fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "symlink analysis cancelled",
        ))
    } else {
        Ok(())
    }
}
