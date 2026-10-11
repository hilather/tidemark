//! Owned by: W1-B
//! Safe byte-oriented directory access and metadata classification.
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, RawDir, StatxFlags, CWD};
use std::{io, mem::MaybeUninit, os::fd::OwnedFd};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::{FileKind, Gate, Ino},
};

pub(super) const MAX_ENTRIES: usize = 1_000_000;
pub(super) fn kind(t: FileType) -> FileKind {
    match t {
        FileType::Directory => FileKind::Dir,
        FileType::RegularFile => FileKind::Regular,
        FileType::Symlink => FileKind::Symlink,
        _ => FileKind::Other,
    }
}

// Open every component, including ancestors, without ever following a symlink.
// Two descriptors are briefly live while replacing the current ancestor.
pub(super) fn open(path: &RawPath, gate: &dyn Gate, fallbacks: &mut u64) -> io::Result<OwnedFd> {
    open_flags(path, gate, fallbacks, false)
}

pub(super) fn open_path(path: &RawPath, gate: &dyn Gate) -> io::Result<OwnedFd> {
    open_flags(path, gate, &mut 0, true)
}

fn open_flags(
    path: &RawPath,
    gate: &dyn Gate,
    fallbacks: &mut u64,
    path_only: bool,
) -> io::Result<OwnedFd> {
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
    ) -> io::Result<OwnedFd> {
        gate.acquire(1);
        match fs::openat(fd.as_fd(), name, flags | OFlags::NOATIME, Mode::empty()) {
            Err(rustix::io::Errno::PERM) => {
                *falls += 1;
                gate.acquire(1);
                Ok(fs::openat(fd, name, flags, Mode::empty())?)
            }
            result => Ok(result?),
        }
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
        )?;
    }
    Ok(fd)
}

pub(super) fn stat(
    fd: impl std::os::fd::AsFd,
    name: &[u8],
    gate: &dyn Gate,
) -> io::Result<fs::Statx> {
    gate.acquire(1);
    Ok(fs::statx(
        fd,
        name,
        AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )?)
}

pub(super) fn read(dir: &RawPath, gate: &dyn Gate) -> io::Result<Vec<(RawName, Ino, FileKind)>> {
    let fd = open(dir, gate, &mut 0)?;
    let mut buffer = vec![MaybeUninit::uninit(); 256 * 1024];
    let mut iter = RawDir::new(&fd, &mut buffer);
    let mut entries = Vec::new();
    loop {
        // Charge before each potential getdents refill, including EOF.
        if iter.is_buffer_empty() {
            gate.acquire(1);
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
                stat(&fd, bytes, gate)?.stx_mode.into(),
            ))
        } else {
            kind(entry.file_type())
        };
        entries.push((name, entry.ino(), ty));
    }
    entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}
