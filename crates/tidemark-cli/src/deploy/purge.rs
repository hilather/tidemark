//! Owned by: W4-B
//! Descriptor-relative state removal with configuration, marker and mount guards.

use super::{DeployError, Marker, MARKER};
use rustix::fs::{self, AtFlags, Dir, FileType, Mode, OFlags};
use std::os::unix::ffi::OsStrExt;
use std::{
    ffi::CString,
    io::{self, Read},
    os::fd::{AsRawFd, OwnedFd},
    path::{Path, PathBuf},
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
    tidemark_store::state_dir::trusted_directory(path)
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
    super::files::check_purge_root(&root, state_dir)?;
    super::files::check_leaf_at(&root, std::ffi::OsStr::new(MARKER), &state_dir.join(MARKER))?;
    let bytes =
        super::files::read_state_at(&root, std::ffi::OsStr::new(MARKER), &state_dir.join(MARKER))
            .map_err(|error| {
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
    let config_bytes = super::files::read(marker.config_path.as_path())?;
    let cfg = tidemark_core::config::Config::from_toml_bytes(&config_bytes).map_err(|error| {
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
    walk(&root, state_dir, mount, false, &mut false)?;
    Ok((parent, name, root))
}
struct Frame {
    fd: OwnedFd,
    entries: Dir,
    name: Option<CString>,
    path: PathBuf,
}
fn walk(
    root: &OwnedFd,
    path: &Path,
    mount: u64,
    remove: bool,
    removed: &mut bool,
) -> io::Result<()> {
    let fd = rustix::io::dup(root)?;
    fs::seek(&fd, rustix::fs::SeekFrom::Start(0))?;
    verify_directory(&fd, path, mount)?;
    let entries = Dir::new(rustix::io::dup(&fd)?)?;
    let mut stack = vec![Frame {
        fd,
        entries,
        name: None,
        path: path.to_owned(),
    }];
    while !stack.is_empty() {
        // Recheck attachment before each read/unlink, not only after deleting contents.
        for pair in stack.windows(2) {
            if let Some(name) = &pair[1].name {
                verify_identity(&pair[0].fd, name, &pair[1].fd, &pair[1].path)?;
            }
        }
        let Some(frame) = stack.last_mut() else {
            break;
        };
        match frame.entries.read() {
            Some(entry) => {
                let entry = entry?;
                let name = entry.file_name();
                if name.to_bytes() == b"." || name.to_bytes() == b".." {
                    continue;
                }
                // Root exclusion sentinels remain linked until all data is removed.
                if remove
                    && frame.name.is_none()
                    && matches!(name.to_bytes(), b"writer.lock" | b"scan.lock")
                {
                    continue;
                }
                let stat = fs::statat(&frame.fd, name, AtFlags::SYMLINK_NOFOLLOW)?;
                if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
                    let child = directory_at(&frame.fd, name)?;
                    let path = frame
                        .path
                        .join(std::ffi::OsStr::from_bytes(name.to_bytes()));
                    #[cfg(test)]
                    AFTER_OPEN.with(|hook| {
                        let mut hook = hook.borrow_mut();
                        if hook.as_ref().is_some_and(|(target, _)| target == &path) {
                            if let Some((_, hook)) = hook.take() {
                                hook(&path);
                            }
                        }
                    });
                    verify_directory(&child, &path, mount)?;
                    verify_identity(&frame.fd, name, &child, &path)?;
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
                        path,
                    });
                } else if remove {
                    fs::unlinkat(&frame.fd, name, AtFlags::empty())?;
                    *removed = true;
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
                            *removed = true;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
pub(super) fn purge_directory(
    parent: OwnedFd,
    name: CString,
    root: OwnedFd,
    removed: &mut bool,
) -> io::Result<()> {
    let current = fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)?;
    let opened = fs::fstat(&root)?;
    if current.st_dev != opened.st_dev || current.st_ino != opened.st_ino {
        return Err(io::Error::other("state directory replaced before purge"));
    }
    let path = PathBuf::from(format!("/proc/self/fd/{}/.", root.as_raw_fd()));
    walk(&root, &path, mount_id(&root)?, true, removed)?;
    let current = fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)?;
    if current.st_dev != opened.st_dev || current.st_ino != opened.st_ino {
        return Err(io::Error::other("state directory replaced during purge"));
    }
    #[cfg(test)]
    BEFORE_LOCK_UNLINK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    for lock in ["scan.lock", "writer.lock"] {
        fs::unlinkat(&root, lock, AtFlags::empty())?;
        *removed = true;
    }
    #[cfg(test)]
    BEFORE_RMDIR.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    // A replacement or new entry is a refusal, never a reason to traverse again.
    let current = fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)?;
    if current.st_dev != opened.st_dev || current.st_ino != opened.st_ino {
        return Err(io::Error::other(
            "state directory replaced during purge finalization",
        ));
    }
    fs::unlinkat(&parent, &name, AtFlags::REMOVEDIR)?;
    *removed = true;
    Ok(())
}

fn verify_directory(fd: &OwnedFd, path: &Path, mount: u64) -> io::Result<()> {
    let stat = fs::fstat(fd)?;
    #[cfg(test)]
    let stat = super::files::FOREIGN_OWNER.with(|owner| {
        let mut stat = stat;
        if owner.borrow().as_deref() == Some(path) {
            stat.st_uid = rustix::process::geteuid().as_raw().wrapping_add(1);
        }
        stat
    });
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o022 != 0
        || mount_id(fd)? != mount
    {
        return Err(io::Error::other(format!("unsafe purge directory {}: requires effective-uid ownership, no group/world write and the state mount", tidemark_core::path::display_lossy_bytes(path.as_os_str().as_bytes()))));
    }
    Ok(())
}
fn verify_identity(
    parent: &OwnedFd,
    name: &std::ffi::CStr,
    child: &OwnedFd,
    path: &Path,
) -> io::Result<()> {
    let current = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|e| {
        io::Error::other(format!(
            "state subtree unavailable during purge {}: {e}",
            tidemark_core::path::display_lossy_bytes(path.as_os_str().as_bytes())
        ))
    })?;
    let opened = fs::fstat(child)?;
    if current.st_dev != opened.st_dev || current.st_ino != opened.st_ino {
        return Err(io::Error::other(format!(
            "state subtree replaced during purge: {}",
            path.display()
        )));
    }
    Ok(())
}
#[cfg(test)]
type OpenHook = Box<dyn FnOnce(&Path)>;
#[cfg(test)]
thread_local! {
    pub(super) static BEFORE_LOCK_UNLINK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    pub(super) static BEFORE_RMDIR: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    pub(super) static AFTER_OPEN: std::cell::RefCell<Option<(PathBuf, OpenHook)>> = const { std::cell::RefCell::new(None) };
}
