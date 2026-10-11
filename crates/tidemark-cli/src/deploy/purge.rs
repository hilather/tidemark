//! Owned by: W4-B
//! Descriptor-relative state removal with configuration, marker and mount guards.

use super::{DeployError, Marker, MARKER, MAX_RECORD_BYTES};
use rustix::fs::{self, AtFlags, Dir, FileType, Mode, OFlags};
use std::{
    ffi::CString,
    io::{self, Read},
    os::fd::{AsRawFd, OwnedFd},
    path::{Component, Path},
};

// Limit simultaneous open directories; arbitrary width is streamed rather than collected.
const MAX_PURGE_DEPTH: usize = 256;
type GuardedRoot = (OwnedFd, CString, OwnedFd);
fn directory_at(
    parent: impl std::os::fd::AsFd,
    name: impl rustix::path::Arg + Copy,
) -> io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match fs::openat(&parent, name, flags | OFlags::NOATIME, Mode::empty()) {
        Ok(fd) => Ok(fd),
        Err(rustix::io::Errno::PERM) => Ok(fs::openat(parent, name, flags, Mode::empty())?),
        Err(e) => Err(e.into()),
    }
}
fn open_absolute(path: &Path) -> io::Result<OwnedFd> {
    if !path.is_absolute() {
        return Err(io::Error::other("purge path must be absolute"));
    }
    let mut fd = fs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                fd = directory_at(&fd, name)?;
            }
            _ => {
                return Err(io::Error::other(
                    "purge path must have only normal components",
                ))
            }
        }
    }
    Ok(fd)
}
fn read_at(fd: &OwnedFd, name: &str) -> io::Result<Vec<u8>> {
    let file = fs::openat(
        fd,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    if FileType::from_raw_mode(fs::fstat(&file)?.st_mode) != FileType::RegularFile {
        return Err(io::Error::other("marker is not a regular file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::from(file)
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(io::Error::other("marker exceeds 1 MiB"));
    }
    Ok(bytes)
}
fn mount_id(fd: &OwnedFd) -> io::Result<u64> {
    // fdinfo exposes mount ids on EL8, including same-device bind mounts (statx MNT_ID is newer).
    let mut text = String::new();
    std::fs::File::open(format!("/proc/self/fdinfo/{}", fd.as_raw_fd()))?
        .take(4097)
        .read_to_string(&mut text)?;
    if text.len() > 4096 {
        return Err(io::Error::other("fdinfo exceeds bound"));
    }
    text.lines()
        .find_map(|line| line.strip_prefix("mnt_id:")?.trim().parse().ok())
        .ok_or_else(|| io::Error::other("mount id unavailable; refusing purge"))
}
pub(super) fn purge_guard(state_dir: &Path) -> Result<GuardedRoot, DeployError> {
    let parent_path = state_dir
        .parent()
        .filter(|p| *p != state_dir)
        .ok_or_else(|| DeployError::Guard("cannot purge filesystem root".into()))?;
    let name = CString::new(
        state_dir
            .file_name()
            .ok_or_else(|| DeployError::Guard("state path has no final component".into()))?
            .as_encoded_bytes(),
    )
    .map_err(|_| DeployError::Guard("invalid state name".into()))?;
    let parent =
        open_absolute(parent_path).map_err(|error| DeployError::Guard(super::io_detail(&error)))?;
    let root = directory_at(&parent, &name)
        .map_err(|error| DeployError::Guard(super::io_detail(&error)))?;
    let bytes = read_at(&root, MARKER).map_err(|error| {
        DeployError::Guard(format!(
            "missing or unsafe state marker: {}",
            super::io_detail(&error)
        ))
    })?;
    let marker: Marker = serde_json::from_slice(&bytes)
        .map_err(|error| DeployError::Guard(format!("invalid state marker: {error}")))?;
    if marker.format != "tidemark-state-v1" || marker.state_dir.as_path() != state_dir {
        return Err(DeployError::Guard("state marker path mismatch".into()));
    }
    super::reject_symlink_ancestors(marker.config_path.as_path())?;
    let cfg =
        tidemark_core::config::Config::load(marker.config_path.as_path()).map_err(|error| {
            DeployError::Guard(format!("cannot validate installed configuration: {error}"))
        })?;
    if cfg.general.state_dir != state_dir || !cfg.validate().is_empty() {
        return Err(DeployError::Guard(
            "path differs from configured state_dir".into(),
        ));
    }
    let mount = mount_id(&root)?;
    if mount_id(&parent)? != mount {
        return Err(DeployError::Guard(
            "state directory is a mount point".into(),
        ));
    }
    walk(&root, mount, false)?;
    Ok((parent, name, root))
}
struct Frame {
    fd: OwnedFd,
    entries: Dir,
    name: Option<CString>,
}
fn walk(root: &OwnedFd, mount: u64, remove: bool) -> io::Result<()> {
    let fd = rustix::io::dup(root)?;
    fs::seek(&fd, rustix::fs::SeekFrom::Start(0))?;
    let entries = Dir::new(rustix::io::dup(&fd)?)?;
    let mut stack = vec![Frame {
        fd,
        entries,
        name: None,
    }];
    while let Some(frame) = stack.last_mut() {
        match frame.entries.read() {
            Some(entry) => {
                let entry = entry?;
                let name = entry.file_name();
                if name.to_bytes() == b"." || name.to_bytes() == b".." {
                    continue;
                }
                let stat = fs::statat(&frame.fd, name, AtFlags::SYMLINK_NOFOLLOW)?;
                if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
                    let child = directory_at(&frame.fd, name)?;
                    if mount_id(&child)? != mount {
                        return Err(io::Error::other(
                            "mount boundary in state directory; refusing purge",
                        ));
                    }
                    if stack.len() >= MAX_PURGE_DEPTH {
                        return Err(io::Error::other(
                            "state directory exceeds purge depth cap (256)",
                        ));
                    }
                    let entries = Dir::new(rustix::io::dup(&child)?)?;
                    stack.push(Frame {
                        fd: child,
                        entries,
                        name: Some(name.to_owned()),
                    });
                } else if remove {
                    fs::unlinkat(&frame.fd, name, AtFlags::empty())?;
                }
            }
            None => {
                if let Some(completed) = stack.pop() {
                    if remove {
                        if let (Some(parent), Some(name)) = (stack.last(), completed.name) {
                            let current = fs::statat(&parent.fd, &name, AtFlags::SYMLINK_NOFOLLOW)?;
                            let opened = fs::fstat(&completed.fd)?;
                            if current.st_dev != opened.st_dev || current.st_ino != opened.st_ino {
                                return Err(io::Error::other(
                                    "state subtree replaced during purge",
                                ));
                            }
                            fs::unlinkat(&parent.fd, &name, AtFlags::REMOVEDIR)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
pub(super) fn purge_directory(parent: OwnedFd, name: CString, root: OwnedFd) -> io::Result<()> {
    let current = fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)?;
    let opened = fs::fstat(&root)?;
    if current.st_dev != opened.st_dev || current.st_ino != opened.st_ino {
        return Err(io::Error::other("state directory replaced before purge"));
    }
    walk(&root, mount_id(&root)?, true)?;
    let current = fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)?;
    if current.st_dev != opened.st_dev || current.st_ino != opened.st_ino {
        return Err(io::Error::other("state directory replaced during purge"));
    }
    fs::unlinkat(&parent, &name, AtFlags::REMOVEDIR)?;
    Ok(())
}
