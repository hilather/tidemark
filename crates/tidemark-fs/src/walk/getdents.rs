//! Owned by: W1-B
//! Safe byte-oriented directory access and metadata classification.
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, RawDir, StatxFlags, CWD};
use std::{
    io,
    mem::MaybeUninit,
    os::fd::OwnedFd,
    sync::atomic::{AtomicBool, Ordering},
};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::{DevId, FileKind, Gate, Ino},
};

pub(super) fn charge(gate: &dyn Gate, cancel: &AtomicBool) -> io::Result<()> {
    gate.acquire(1);
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"))
    } else {
        Ok(())
    }
}

pub(super) const MAX_ENTRIES: usize = 1_000_000;
pub(super) fn kind(t: FileType) -> FileKind {
    match t {
        FileType::Directory => FileKind::Dir,
        FileType::RegularFile => FileKind::Regular,
        FileType::Symlink => FileKind::Symlink,
        _ => FileKind::Other,
    }
}

/// Open an absolute directory component by component from / without following links.
/// Charges each attempt, including NOATIME retries, and checks cancellation before I/O.
/// At most two descriptors are live. Ancestors use O_PATH; the final directory uses
/// O_RDONLY with O_NOATIME, falling back only on EPERM and counting that retry.
pub fn open_directory(
    path: &RawPath,
    gate: &dyn Gate,
    fallbacks: &mut u64,
    cancel: &AtomicBool,
) -> io::Result<OwnedFd> {
    open_flags(path, gate, fallbacks, false, cancel)
}

pub(super) fn open_path(
    path: &RawPath,
    gate: &dyn Gate,
    cancel: &AtomicBool,
) -> io::Result<OwnedFd> {
    open_flags(path, gate, &mut 0, true, cancel)
}

fn open_attempts<T>(
    gate: &dyn Gate,
    cancel: &AtomicBool,
    falls: &mut u64,
    mut attempt: impl FnMut(bool) -> rustix::io::Result<T>,
) -> io::Result<T> {
    charge(gate, cancel)?;
    match attempt(true) {
        Err(rustix::io::Errno::PERM) => {
            *falls += 1;
            charge(gate, cancel)?;
            Ok(attempt(false)?)
        }
        result => Ok(result?),
    }
}

fn open_flags(
    path: &RawPath,
    gate: &dyn Gate,
    fallbacks: &mut u64,
    path_only: bool,
    cancel: &AtomicBool,
) -> io::Result<OwnedFd> {
    if !path.as_bytes().starts_with(b"/") {
        return Err(io::Error::other("absolute path required"));
    }
    static OPEN_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    let _opening = OPEN_LOCK.lock();
    if path.as_bytes().len() > 1024 * 1024 {
        return Err(io::Error::other("path exceeds 1 MiB cap"));
    }
    let flags = (if path_only {
        OFlags::PATH
    } else {
        OFlags::RDONLY
    }) | OFlags::DIRECTORY
        | OFlags::NOFOLLOW
        | OFlags::CLOEXEC;
    fn one(
        fd: impl std::os::fd::AsFd,
        name: &[u8],
        flags: OFlags,
        gate: &dyn Gate,
        falls: &mut u64,
        cancel: &AtomicBool,
    ) -> io::Result<OwnedFd> {
        open_attempts(gate, cancel, falls, |noatime| {
            fs::openat(
                fd.as_fd(),
                name,
                flags
                    | if noatime {
                        OFlags::NOATIME
                    } else {
                        OFlags::empty()
                    },
                Mode::empty(),
            )
        })
    }
    let mut parts = path
        .as_bytes()
        .split(|b| *b == b'/')
        .filter(|p| !p.is_empty())
        .peekable();
    let ancestor_flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut fd = one(
        CWD,
        b"/",
        if parts.peek().is_none() {
            flags
        } else {
            ancestor_flags
        },
        gate,
        fallbacks,
        cancel,
    )?;
    while let Some(part) = parts.next() {
        fd = one(
            &fd,
            part,
            if parts.peek().is_none() {
                flags
            } else {
                ancestor_flags
            },
            gate,
            fallbacks,
            cancel,
        )?;
    }
    Ok(fd)
}

pub(super) fn stat(
    fd: impl std::os::fd::AsFd,
    name: &[u8],
    gate: &dyn Gate,
    cancel: &AtomicBool,
) -> io::Result<fs::Statx> {
    charge(gate, cancel)?;
    Ok(fs::statx(
        fd,
        name,
        AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )?)
}

pub(super) fn read(dir: &RawPath, gate: &dyn Gate) -> io::Result<Vec<(RawName, Ino, FileKind)>> {
    let cancel = &AtomicBool::new(false);
    let fd = open_directory(dir, gate, &mut 0, cancel)?;
    read_fd(&fd, gate, cancel)
}

fn read_fd(
    fd: &OwnedFd,
    gate: &dyn Gate,
    cancel: &AtomicBool,
) -> io::Result<Vec<(RawName, Ino, FileKind)>> {
    let mut buffer = vec![MaybeUninit::uninit(); 256 * 1024];
    let mut iter = RawDir::new(fd, &mut buffer);
    let mut entries = Vec::new();
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"));
        }
        // Charge before each potential getdents refill, including EOF.
        if iter.is_buffer_empty() {
            charge(gate, cancel)?;
        }
        let Some(entry) = iter.next() else { break };
        let entry = entry?;
        let bytes = entry.file_name().to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        if entries.len() == MAX_ENTRIES {
            return Err(io::Error::other("directory entry cap reached"));
        }
        let name = RawName::new(bytes.to_vec()).map_err(io::Error::other)?;
        let ty = if entry.file_type() == FileType::Unknown {
            kind(FileType::from_raw_mode(
                stat(fd, bytes, gate, cancel)?.stx_mode.into(),
            ))
        } else {
            kind(entry.file_type())
        };
        entries.push((name, entry.ino(), ty));
    }
    entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Like `read_dir_entries`, but checks every opened component below `root` on
/// the descriptor used for reading. Device or mount changes, an unexpected final
/// inode, and paths outside the root return `CrossesDevices` before enumeration.
pub fn read_dir_entries_checked_cancellable(
    root: &RawPath,
    dev: DevId,
    path: &RawPath,
    expect_ino: Option<Ino>,
    gate: &dyn Gate,
    cancel: &AtomicBool,
) -> io::Result<Vec<(RawName, Ino, FileKind)>> {
    fn mismatch() -> io::Error {
        io::Error::new(
            io::ErrorKind::CrossesDevices,
            "directory identity or mount changed",
        )
    }
    fn identity(fd: &OwnedFd, gate: &dyn Gate, cancel: &AtomicBool) -> io::Result<fs::Statx> {
        charge(gate, cancel)?;
        Ok(fs::statx(
            fd,
            &b""[..],
            AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
        )?)
    }
    if !path.starts_with(root) {
        return Err(mismatch());
    }
    let mut fd = open_directory(root, gate, &mut 0, cancel)?;
    let root_stat = identity(&fd, gate, cancel)?;
    if libc::makedev(root_stat.stx_dev_major, root_stat.stx_dev_minor) != dev {
        return Err(mismatch());
    }
    for component in path.as_bytes()[root.as_bytes().len()..]
        .split(|b| *b == b'/')
        .filter(|p| !p.is_empty())
    {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        charge(gate, cancel)?;
        fd = match fs::openat(&fd, component, flags | OFlags::NOATIME, Mode::empty()) {
            Err(rustix::io::Errno::PERM) => {
                charge(gate, cancel)?;
                fs::openat(&fd, component, flags, Mode::empty())?
            }
            result => result?,
        };
        let st = identity(&fd, gate, cancel)?;
        if libc::makedev(st.stx_dev_major, st.stx_dev_minor) != dev
            || (root_stat.stx_mnt_id != 0 && st.stx_mnt_id != root_stat.stx_mnt_id)
        {
            return Err(mismatch());
        }
    }
    let final_stat = identity(&fd, gate, cancel)?;
    if expect_ino.is_some_and(|ino| ino != final_stat.stx_ino) {
        return Err(mismatch());
    }
    read_fd(&fd, gate, cancel)
}

/// Checked directory enumeration without a cancellation flag.
pub fn read_dir_entries_checked(
    root: &RawPath,
    dev: DevId,
    path: &RawPath,
    expect_ino: Option<Ino>,
    gate: &dyn Gate,
) -> io::Result<Vec<(RawName, Ino, FileKind)>> {
    read_dir_entries_checked_cancellable(root, dev, path, expect_ino, gate, &AtomicBool::new(false))
}

#[cfg(test)]
mod checked_tests {
    use std::{fs, os::unix::fs::MetadataExt};
    use tidemark_core::{path::RawPath, scan::NoopGate};

    #[test]
    fn noatime_fallback_charges_each_attempt_and_checks_cancel() {
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        struct Count(AtomicU32);
        impl tidemark_core::scan::Gate for Count {
            fn acquire(&self, cost: u32) {
                self.0.fetch_add(cost, Ordering::Relaxed);
            }
            fn current_rate(&self) -> f64 {
                0.0
            }
        }
        let gate = Count(AtomicU32::new(0));
        let cancel = AtomicBool::new(false);
        let mut falls = 0;
        let mut attempts = 0;
        super::open_attempts(&gate, &cancel, &mut falls, |noatime| {
            attempts += 1;
            if noatime {
                Err(rustix::io::Errno::PERM)
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!((attempts, falls, gate.0.load(Ordering::Relaxed)), (2, 1, 2));
        let result = super::open_attempts(&gate, &cancel, &mut falls, |_| {
            cancel.store(true, Ordering::Release);
            Err::<(), _>(rustix::io::Errno::PERM)
        });
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(gate.0.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn checked_read_rejects_symlink_replacement_and_outside_root() {
        use super::read_dir_entries_checked;
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("file"), []).unwrap();
        let root = RawPath::from_path(dir.path()).unwrap();
        let path = RawPath::from_path(&child).unwrap();
        let m = fs::metadata(&child).unwrap();
        assert_eq!(
            read_dir_entries_checked(&root, m.dev(), &path, Some(m.ino()), &NoopGate)
                .unwrap()
                .len(),
            1
        );
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(
            read_dir_entries_checked(
                &root,
                m.dev(),
                &RawPath::from_path(outside.path()).unwrap(),
                None,
                &NoopGate
            )
            .unwrap_err()
            .kind(),
            std::io::ErrorKind::CrossesDevices
        );
        fs::rename(&child, dir.path().join("old")).unwrap();
        fs::create_dir(&child).unwrap();
        assert_eq!(
            read_dir_entries_checked(&root, m.dev(), &path, Some(m.ino()), &NoopGate)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::CrossesDevices
        );
        fs::remove_dir(&child).unwrap();
        std::os::unix::fs::symlink("old", &child).unwrap();
        let descendant = RawPath::from_path(&child.join("nested")).unwrap();
        assert!(read_dir_entries_checked(&root, m.dev(), &descendant, None, &NoopGate).is_err());
        assert_eq!(
            read_dir_entries_checked(&root, m.dev().wrapping_add(1), &root, None, &NoopGate)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::CrossesDevices
        );
    }

    #[cfg(feature = "privileged")]
    #[test]
    fn checked_read_refuses_mounts_added_after_namespace() {
        use super::read_dir_entries_checked;
        use std::process::Command;
        use test_support::xfs::LoopXfs;
        struct BindGuard(std::path::PathBuf);
        impl Drop for BindGuard {
            fn drop(&mut self) {
                assert!(Command::new("umount")
                    .arg(&self.0)
                    .status()
                    .unwrap()
                    .success());
            }
        }
        let xfs = LoopXfs::new(512 * 1024 * 1024, &[], "noatime");
        let other = LoopXfs::new(512 * 1024 * 1024, &[], "noatime");
        let source = xfs.mountpoint.join("source");
        let target = xfs.mountpoint.join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("file"), []).unwrap();
        let root = RawPath::from_path(&xfs.mountpoint).unwrap();
        let path = RawPath::from_path(&target).unwrap();
        let m = fs::metadata(&target).unwrap();
        let fd = fs::File::open(&xfs.mountpoint).unwrap();
        let root_stat = rustix::fs::statx(
            &fd,
            &b""[..],
            rustix::fs::AtFlags::EMPTY_PATH,
            rustix::fs::StatxFlags::BASIC_STATS | rustix::fs::StatxFlags::MNT_ID,
        )
        .unwrap();
        for mount_source in [&source, &other.mountpoint] {
            assert!(Command::new("mount")
                .arg("--bind")
                .arg(mount_source)
                .arg(&target)
                .status()
                .unwrap()
                .success());
            let guard = BindGuard(target.clone());
            // Mount-only rejection is promised when the kernel reports mount IDs.
            if root_stat.stx_mnt_id != 0 || mount_source == &other.mountpoint {
                assert_eq!(
                    read_dir_entries_checked(&root, m.dev(), &path, None, &NoopGate)
                        .unwrap_err()
                        .kind(),
                    std::io::ErrorKind::CrossesDevices
                );
            } else {
                eprintln!("mount-ID-only regression pending kernel mount-ID support");
            }
            assert_eq!(
                read_dir_entries_checked(&root, m.dev(), &path, Some(m.ino()), &NoopGate)
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::CrossesDevices
            );
            drop(guard);
        }
    }
}
