//! Owned by: W3-B
//! Gated metadata-only symlink analysis with mount-contained target resolution.

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
    /// Resolve targets component by component within the selected mount.
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
    /// Target leaves the selected mount; its state beyond the boundary is not checked.
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
// One charged syscall per attempt; at most 64 KiB retained and reused across links.
fn read_target(
    fd: &OwnedFd,
    name: &[u8],
    gate: &dyn Gate,
    cancel: &AtomicBool,
    buffer: &mut Vec<u8>,
) -> io::Result<Vec<u8>> {
    loop {
        gate.acquire(1);
        cancelled(cancel)?;
        let len = fs::readlinkat_raw(fd, name, buffer.as_mut_slice())?;
        if len < buffer.len() {
            return Ok(buffer[..len].to_vec());
        }
        if buffer.len() == 65_536 {
            return Err(io::Error::other("symlink target cap reached"));
        }
        buffer.resize((buffer.len() * 2).min(65_536), 0);
    }
}
// Pin the enumerated link before reading its text, so a concurrent replacement
// cannot redirect readlink into an unvalidated mount.
fn read_entry_target(
    fd: &OwnedFd,
    name: &[u8],
    expected: &fs::Statx,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    buffer: &mut Vec<u8>,
) -> io::Result<Vec<u8>> {
    gate.acquire(1);
    cancelled(cancel)?;
    let pinned = fs::openat(
        fd,
        name,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let current = sx(&pinned, gate, cancel)?;
    if !same_fs(expected, &current)
        || expected.stx_ino != current.stx_ino
        || FileType::from_raw_mode(current.stx_mode.into()) != FileType::Symlink
    {
        return Err(io::Error::other("link changed during analysis"));
    }
    read_target(&pinned, b"", gate, cancel, buffer)
}
fn sx(fd: &OwnedFd, gate: &dyn Gate, cancel: &AtomicBool) -> io::Result<fs::Statx> {
    gate.acquire(1);
    cancelled(cancel)?;
    Ok(fs::statx(
        fd,
        &b""[..],
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )?)
}
fn open_one(
    fd: impl std::os::fd::AsFd,
    name: &[u8],
    gate: &dyn Gate,
    cancel: &AtomicBool,
) -> io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    gate.acquire(1);
    cancelled(cancel)?;
    match fs::openat(fd.as_fd(), name, flags | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => {
            gate.acquire(1);
            cancelled(cancel)?;
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
    cancel: &AtomicBool,
) -> io::Result<OwnedFd> {
    let mut fd = open_one(CWD, b"/", gate, cancel)?;
    let mut bytes = vec![b'/'];
    if root.as_bytes() == b"/" {
        if let Some(id) = identity {
            if !same_fs(id, &sx(&fd, gate, cancel)?) {
                return Err(io::Error::other("root mount changed"));
            }
        }
    }
    for part in path
        .as_bytes()
        .split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
    {
        fd = open_one(&fd, part, gate, cancel)?;
        if bytes.len() > 1 {
            bytes.push(b'/');
        }
        bytes.extend_from_slice(part);
        if let Some(id) = identity {
            if bytes.len() >= root.as_bytes().len() && !same_fs(id, &sx(&fd, gate, cancel)?) {
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
// Linux follows at most 40 symbolic links; pending text and directory handles are
// independently bounded, including chains that repeatedly expand their targets.
const CHAIN_CAP: usize = 40;
const DEPTH_CAP: usize = 4096;
const RESOLVE_BYTES: usize = 1024 * 1024;

struct Resolver<'a> {
    root: &'a RawPath,
    mount: &'a RawPath,
    mount_fd: &'a OwnedFd,
    identity: &'a fs::Statx,
    mounts: &'a [RawPath],
    gate: &'a dyn Gate,
    cancel: &'a AtomicBool,
}
impl Resolver<'_> {
    fn check(
        &self,
        dir: &RawPath,
        target: &[u8],
        buffer: &mut Vec<u8>,
    ) -> io::Result<SymlinkClass> {
        let mut pending = if target.starts_with(b"/") {
            match self.absolute(target) {
                Some(p) => p,
                None => return Ok(SymlinkClass::OtherFs),
            }
        } else {
            let mut p = dir.as_bytes()[self.mount.as_bytes().len()..].to_vec();
            p.push(b'/');
            p.extend_from_slice(target);
            p
        };
        let fd = open_one(self.mount_fd, b".", self.gate, self.cancel)?;
        if !same_fs(self.identity, &sx(&fd, self.gate, self.cancel)?) {
            return Ok(SymlinkClass::OtherFs);
        }
        let mut dirs = vec![(fd, self.mount.clone())];
        let mut links = 0;
        loop {
            cancelled(self.cancel)?;
            if pending.len() > RESOLVE_BYTES {
                return Ok(SymlinkClass::Unreadable);
            }
            let split = pending
                .iter()
                .position(|b| *b == b'/')
                .unwrap_or(pending.len());
            let name = pending[..split].to_vec();
            let rest = if split < pending.len() {
                pending[split + 1..].to_vec()
            } else {
                Vec::new()
            };
            let (parent, path) = &dirs[dirs.len() - 1];
            if name.is_empty() || name == b"." {
                if rest.is_empty() {
                    return lexical(self.root, path, b".");
                }
                pending = rest;
                continue;
            }
            if name == b".." {
                if dirs.len() == 1 {
                    if self.mount.as_bytes() != b"/" {
                        return Ok(SymlinkClass::OtherFs);
                    }
                } else {
                    dirs.pop();
                }
                pending = rest;
                continue;
            }
            let next = path.join(&RawName::new(name.clone()).map_err(io::Error::other)?);
            if self.mounts.binary_search(&next).is_ok() {
                return Ok(SymlinkClass::OtherFs);
            }
            #[cfg(test)]
            record_resolution(&next, "stat");
            self.gate.acquire(1);
            cancelled(self.cancel)?;
            let stat = match fs::statx(
                parent,
                name.as_slice(),
                AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
                StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
            ) {
                Ok(s) => s,
                Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR) => {
                    return Ok(SymlinkClass::Dangling)
                }
                Err(e) => return Err(e.into()),
            };
            if !same_fs(self.identity, &stat) {
                return Ok(SymlinkClass::OtherFs);
            }
            // Pin the entry without following it, then check again before reading
            // text or opening a directory. A replacement can never induce following.
            self.gate.acquire(1);
            cancelled(self.cancel)?;
            #[cfg(test)]
            record_resolution(&next, "open");
            let pinned = fs::openat(
                parent,
                name.as_slice(),
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            let pinned_stat = sx(&pinned, self.gate, self.cancel)?;
            if !same_fs(self.identity, &pinned_stat) {
                return Ok(SymlinkClass::OtherFs);
            }
            if stat.stx_ino != pinned_stat.stx_ino || stat.stx_mode != pinned_stat.stx_mode {
                return Ok(SymlinkClass::Unreadable);
            }
            match FileType::from_raw_mode(pinned_stat.stx_mode.into()) {
                FileType::Symlink => {
                    links += 1;
                    if links > CHAIN_CAP {
                        return Ok(SymlinkClass::Loop);
                    }
                    #[cfg(test)]
                    record_resolution(&next, "readlink");
                    let text = read_target(&pinned, b"", self.gate, self.cancel, buffer)?;
                    let mut replacement = if text.starts_with(b"/") {
                        let Some(p) = self.absolute(&text) else {
                            return Ok(SymlinkClass::OtherFs);
                        };
                        dirs.truncate(1);
                        p
                    } else {
                        text
                    };
                    if split < pending.len() {
                        replacement.push(b'/');
                        replacement.extend_from_slice(&rest);
                    }
                    pending = replacement;
                }
                FileType::Directory => {
                    if dirs.len() == DEPTH_CAP
                        || dirs.iter().map(|(_, p)| p.as_bytes().len()).sum::<usize>()
                            + next.as_bytes().len()
                            > RESOLVE_BYTES
                    {
                        return Ok(SymlinkClass::Unreadable);
                    }
                    let fd = open_one(&pinned, b".", self.gate, self.cancel)?;
                    if !same_fs(self.identity, &sx(&fd, self.gate, self.cancel)?) {
                        return Ok(SymlinkClass::OtherFs);
                    }
                    dirs.push((fd, next));
                    pending = rest;
                }
                _ if split < pending.len() => return Ok(SymlinkClass::Dangling),
                _ => return lexical(self.root, &next, b"."),
            }
        }
    }
    // Do not normalize dot-dot before resolving preceding symlinks.
    fn absolute(&self, mut text: &[u8]) -> Option<Vec<u8>> {
        while text.starts_with(b"//") {
            text = &text[1..];
        }
        let mount = self.mount.as_bytes();
        if mount == b"/" {
            return Some(text[1..].to_vec());
        }
        if text == mount {
            return Some(Vec::new());
        }
        text.strip_prefix(mount)
            .filter(|p| p.starts_with(b"/"))
            .map(|p| p[1..].to_vec())
    }
}

#[cfg(test)]
thread_local! {
    static RESOLUTION_CALLS: std::cell::RefCell<Option<Vec<(RawPath, &'static str)>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn record_resolution(path: &RawPath, operation: &'static str) {
    RESOLUTION_CALLS.with(|calls| {
        if let Some(calls) = calls.borrow_mut().as_mut() {
            assert!(calls.len() < 10_000, "test instrumentation cap reached");
            calls.push((path.clone(), operation));
        }
    });
}

/// Analyze a tree on one filesystem without descending through any symlink.
///
/// Every open, getdents, stat and readlink is charged before execution. Directory
/// opens use O_NOATIME with the EPERM fallback. Mount discovery fails closed.
/// Root paths are capped at 1 MiB. Pending directories are capped at 65,536
/// and 64 MiB of paths, directory names
/// at one million entries and 64 MiB, and examples at one million and 64 MiB.
/// The reusable target buffer grows from 256 bytes to at most 64 KiB.
/// Target checking follows at most 40 links, retains at most 4096 directory
/// handles and 1 MiB of directory paths, and caps pending text at 1 MiB.
/// Leaving the selected mount produces OtherFs (state beyond it not checked).
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
    let fd = open_path(root, root, None, gate, cancel)?;
    let identity = sx(&fd, gate, cancel)?;
    gate.acquire(1);
    cancelled(cancel)?;
    let mounts = crate::mount::read_mountinfo()?;
    let mount_root = crate::mount::mount_for_path(&mounts, root)
        .ok_or_else(|| io::Error::other("selected mount not found"))?
        .mount_point
        .clone();
    let mut target_mounts = crate::mount::submounts_under(&mounts, &mount_root);
    target_mounts.sort_unstable();
    let mount_fd = open_path(&mount_root, &mount_root, Some(&identity), gate, cancel)?;
    let resolver = Resolver {
        root,
        mount: &mount_root,
        mount_fd: &mount_fd,
        identity: &identity,
        mounts: &target_mounts,
        gate,
        cancel,
    };
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
    let mut target_buffer = vec![0; 256];
    let mut buffer = vec![MaybeUninit::uninit(); 256 * 1024];
    while let Some((dir, ino)) = tasks.pop() {
        cancelled(cancel)?;
        task_bytes -= dir.as_bytes().len();
        let fd = open_path(&dir, root, Some(&identity), gate, cancel)?;
        let info = sx(&fd, gate, cancel)?;
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
                cancelled(cancel)?;
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
            cancelled(cancel)?;
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
                FileType::Symlink if same_fs(&identity, &stat) => {
                    report.total += 1;
                    let text = if opts.check_targets {
                        read_entry_target(
                            &fd,
                            name.as_bytes(),
                            &stat,
                            gate,
                            cancel,
                            &mut target_buffer,
                        )
                    } else {
                        read_target(&fd, name.as_bytes(), gate, cancel, &mut target_buffer)
                    };
                    let (target, class) = match text {
                        Ok(target) => {
                            if target.starts_with(b"/") {
                                report.absolute += 1;
                            } else {
                                report.relative += 1;
                            }
                            let mut class = lexical(root, &dir, &target)?;
                            if opts.check_targets {
                                class = match resolver.check(&dir, &target, &mut target_buffer) {
                                    Ok(class) => class,
                                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                                        return Err(e)
                                    }
                                    Err(_) => SymlinkClass::Unreadable,
                                };
                            }
                            (target, class)
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
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

// Integration tests compile this module privately to substitute a temporary
// directory for a mount boundary without adding a production API or feature.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn test_check_target(
    root: &RawPath,
    target: &[u8],
    gate: &dyn Gate,
) -> (SymlinkClass, Vec<(RawPath, &'static str)>) {
    let cancel = AtomicBool::new(false);
    let fd = open_path(root, root, None, gate, &cancel).unwrap();
    let identity = sx(&fd, gate, &cancel).unwrap();
    RESOLUTION_CALLS.with(|calls| *calls.borrow_mut() = Some(Vec::new()));
    let class = Resolver {
        root,
        mount: root,
        mount_fd: &fd,
        identity: &identity,
        mounts: &[],
        gate,
        cancel: &cancel,
    }
    .check(root, target, &mut vec![0; 256])
    .unwrap();
    let calls = RESOLUTION_CALLS.with(|calls| calls.borrow_mut().take().unwrap());
    (class, calls)
}
