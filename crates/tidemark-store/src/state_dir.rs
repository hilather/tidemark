//! Owned by: WSTATE-trust-A
//! Retained trusted directories and private storage leaves.
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::fd::{AsFd, OwnedFd},
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
};

fn error(path: &Path, reason: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!(
        "state directory {}: {reason}",
        path.as_os_str().to_string_lossy()
    ))
}
fn directory(fd: impl AsFd, name: &Path) -> io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match fs::openat(&fd, name, flags | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => Ok(fs::openat(&fd, name, flags, Mode::empty())?),
        result => Ok(result?),
    }
}
fn private(path: &Path, stat: &fs::Stat) -> io::Result<()> {
    if stat.st_uid != rustix::process::geteuid().as_raw() {
        return Err(error(path, format!("owned by uid {}", stat.st_uid)));
    }
    if stat.st_mode & 0o077 != 0 {
        return Err(error(path, "has group or world permission bits"));
    }
    Ok(())
}
fn ancestor(path: &Path, fd: &OwnedFd) -> io::Result<bool> {
    let stat = fs::fstat(fd)?;
    if stat.st_uid != 0
        && stat.st_uid != rustix::process::geteuid().as_raw()
        && !fs::fstatvfs(fd)?
            .f_flag
            .contains(fs::StatVfsMountFlags::RDONLY)
    {
        return Err(error(
            path,
            format!("ancestor is owned by uid {}", stat.st_uid),
        ));
    }
    let writable = stat.st_mode & 0o022 != 0;
    if writable && stat.st_mode & 0o1000 == 0 {
        return Err(error(
            path,
            "ancestor is writable by group or others without sticky bit",
        ));
    }
    Ok(writable)
}
fn retained(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    bytes
        .strip_prefix(b"/proc/self/fd/")
        .and_then(|s| s.strip_suffix(b"/."))
        .is_some_and(|s| !s.is_empty() && s.iter().all(u8::is_ascii_digit))
}
/// Walk absolute directory ancestry without following links, retaining the final fd.
/// Trusted owners are root/euid (or any owner on a read-only mount). Writable
/// ancestors must be sticky and their next child must be euid-owned/private.
pub fn trusted_directory(path: &Path) -> io::Result<OwnedFd> {
    if !path.is_absolute() {
        return Err(error(path, "path must be absolute"));
    }
    let mut fd = directory(rustix::fs::CWD, Path::new("/"))?;
    let mut traversed = PathBuf::from("/");
    for component in path.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => return Err(error(path, "unsafe directory component")),
        };
        let sticky = ancestor(&traversed, &fd)?;
        traversed.push(name);
        fd = directory(&fd, Path::new(name)).map_err(|e| error(&traversed, e))?;
        if sticky {
            private(&traversed, &fs::fstat(&fd)?)?;
        }
    }
    ancestor(&traversed, &fd)?;
    Ok(fd)
}
/// A verified euid-owned directory with no group/world permissions.
pub struct StateDir {
    fd: OwnedFd,
    path: PathBuf,
}
impl StateDir {
    /// Verify an existing state directory, or create its final component privately.
    /// The exact daemon `/proc/self/fd/<n>/.` form reopens the retained descriptor
    /// without walking procfs ancestry; all directory and leaf rules still apply.
    pub fn open(path: &Path, create: bool) -> io::Result<Self> {
        if retained(path) {
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
            let fd = match fs::open(path, flags | OFlags::NOATIME, Mode::empty()) {
                Err(rustix::io::Errno::PERM) => fs::open(path, flags, Mode::empty()),
                result => result,
            }
            .map_err(|e| error(path, e))?;
            private(path, &fs::fstat(&fd)?)?;
            return Ok(Self {
                fd,
                path: path.to_owned(),
            });
        }
        // Reject parent components before any creation, including lexical '..'.
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(error(path, "unsafe directory component"));
        }
        let parent = path.parent().ok_or_else(|| error(path, "missing parent"))?;
        let name = path
            .file_name()
            .ok_or_else(|| error(path, "missing directory name"))?;
        let parent_fd = trusted_directory(parent)?;
        let fd = match directory(&parent_fd, Path::new(name)) {
            Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                fs::mkdirat(&parent_fd, name, Mode::from_raw_mode(0o700))
                    .map_err(|e| error(path, e))?;
                directory(&parent_fd, Path::new(name))?
            }
            result => result.map_err(|e| error(path, e))?,
        };
        private(path, &fs::fstat(&fd)?)?;
        Ok(Self {
            fd,
            path: path.to_owned(),
        })
    }
    /// Verify a snapshot directory hierarchy, creating private children if requested.
    /// Paths beneath `snapshots` validate every child of the enclosing state root.
    /// Standalone snapshot directories retain the same private-directory policy.
    pub fn snapshot_dir(path: &Path, create: bool) -> io::Result<Self> {
        let mut base = PathBuf::new();
        let mut relative = Vec::new();
        let mut found = false;
        for component in path.components() {
            if found {
                match component {
                    Component::Normal(n) => relative.push(n),
                    _ => return Err(error(path, "unsafe snapshot component")),
                }
            } else if component.as_os_str() == "snapshots" {
                found = true;
                relative.push(component.as_os_str());
            } else {
                base.push(component.as_os_str());
            }
        }
        if found {
            let mut dir = Self::open(&base, create)?;
            for name in relative {
                dir = dir.subdir(name, create)?;
            }
            Ok(dir)
        } else {
            Self::open(path, create)
        }
    }
    fn name(&self, name: &OsStr) -> io::Result<()> {
        let b = name.as_bytes();
        if b.is_empty() || b == b"." || b == b".." || b.contains(&b'/') || b.contains(&0) {
            return Err(error(&self.path, "invalid leaf name"));
        }
        Ok(())
    }
    fn leaf(&self, name: &OsStr, stat: &fs::Stat) -> io::Result<()> {
        let path = self.path.join(name);
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
            return Err(error(
                &path,
                "leaf is not a regular file (symbolic links are refused)",
            ));
        }
        private(&path, stat)?;
        if stat.st_nlink != 1 {
            return Err(error(&path, "leaf has multiple hard links"));
        }
        Ok(())
    }
    /// Check a leaf without following it; absence is accepted and returns false.
    pub fn check_leaf(&self, name: &OsStr) -> io::Result<bool> {
        self.name(name)?;
        match fs::statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => {
                self.leaf(name, &stat)?;
                Ok(true)
            }
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(e) => Err(error(&self.path.join(name), e)),
        }
    }
    /// Open a leaf without following links or blocking on special files.
    /// Creation is exclusive when absent; existing files are never repaired.
    pub fn open_leaf(&self, name: &OsStr, write: bool, create: bool) -> io::Result<File> {
        let exists = self.check_leaf(name)?;
        let mut flags = OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        flags |= if write { OFlags::RDWR } else { OFlags::RDONLY };
        if create && !exists {
            flags |= OFlags::CREATE | OFlags::EXCL;
        }
        let fd = fs::openat(&self.fd, name, flags, Mode::from_raw_mode(0o600))
            .map_err(|e| error(&self.path.join(name), e))?;
        self.leaf(name, &fs::fstat(&fd)?)?;
        Ok(File::from(fd))
    }
    /// Open or create a private child relative to the verified descriptor.
    pub fn subdir(&self, name: &OsStr, create: bool) -> io::Result<Self> {
        self.name(name)?;
        let path = self.path.join(name);
        let fd = match directory(&self.fd, Path::new(name)) {
            Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                fs::mkdirat(&self.fd, name, Mode::from_raw_mode(0o700))?;
                directory(&self.fd, Path::new(name))?
            }
            result => result.map_err(|e| error(&path, e))?,
        };
        private(&path, &fs::fstat(&fd)?)?;
        Ok(Self { fd, path })
    }
    /// Unlink a checked regular leaf relative to the retained descriptor.
    pub fn unlink_leaf(&self, name: &OsStr) -> io::Result<()> {
        if self.check_leaf(name)? {
            fs::unlinkat(&self.fd, name, AtFlags::empty())?;
        }
        Ok(())
    }
    /// Borrow the retained descriptor for descriptor-relative operations.
    pub fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn directory_policy_and_retained_fd() {
        let temp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let child = temp.path().join("state");
        assert!(StateDir::open(&child, false).is_err());
        assert!(!child.exists());
        let state = StateDir::open(&child, true).unwrap();
        use std::os::fd::AsRawFd;
        assert!(StateDir::open(
            Path::new(&format!("/proc/self/fd/{}/.", state.fd.as_raw_fd())),
            false
        )
        .is_ok());
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(StateDir::open(&child, true).is_err());
        symlink(&child, temp.path().join("alias")).unwrap();
        assert!(StateDir::open(&temp.path().join("alias"), true).is_err());
        assert!(StateDir::open(&temp.path().join("alias/nested"), true).is_err());
    }
    #[test]
    fn hostile_leaves_are_unchanged() {
        let temp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let state = StateDir::open(temp.path(), false).unwrap();
        let victim = temp.path().join("victim");
        std::fs::write(&victim, b"sentinel").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o600)).unwrap();
        for name in [
            "timeseries.db",
            "timeseries.db-wal",
            "scan.lock",
            "snapshot.db",
        ] {
            let path = temp.path().join(name);
            symlink(&victim, &path).unwrap();
            assert!(state.open_leaf(OsStr::new(name), true, true).is_err());
            std::fs::remove_file(&path).unwrap();
            std::fs::hard_link(&victim, &path).unwrap();
            assert!(state.open_leaf(OsStr::new(name), true, true).is_err());
            std::fs::remove_file(&path).unwrap();
            fs::mknodat(
                state.fd(),
                name,
                FileType::Fifo,
                Mode::from_raw_mode(0o600),
                0,
            )
            .unwrap();
            assert!(state.open_leaf(OsStr::new(name), false, false).is_err());
            std::fs::remove_file(&path).unwrap();
            std::fs::write(&path, b"private").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o620)).unwrap();
            assert!(state.check_leaf(OsStr::new(name)).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"private");
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o620
            );
            std::fs::remove_file(&path).unwrap();
            assert_eq!(std::fs::read(&victim).unwrap(), b"sentinel");
            assert_eq!(std::fs::metadata(&victim).unwrap().len(), 8);
            assert_eq!(
                std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn sticky_and_writable_ancestors() {
        let temp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let parent = temp.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o1777)).unwrap();
        let child = parent.join("child");
        StateDir::open(&child, true).unwrap();
        assert!(trusted_directory(&child).is_ok());
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(trusted_directory(&child).is_err());
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(StateDir::open(&child, false).is_err());
    }
}
