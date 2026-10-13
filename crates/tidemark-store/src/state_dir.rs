//! Owned by: WSTATE-trust-A
//! Retained trusted directories and integrity-checked storage leaves.
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
        tidemark_core::path::display_lossy_bytes(path.as_os_str().as_bytes())
    ))
}
fn context(path: &Path, source: impl Into<io::Error>) -> io::Error {
    let source = source.into();
    io::Error::new(source.kind(), error(path, &source).to_string())
}
fn directory(fd: impl AsFd, name: &Path) -> io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match fs::openat(&fd, name, flags | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => Ok(fs::openat(&fd, name, flags, Mode::empty())?),
        result => Ok(result?),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Ancestor,
    StickyChild,
    StateDirectory,
    Subdirectory,
    Leaf,
}

#[derive(Clone, Copy, Debug)]
struct PolicyInput {
    owner: u32,
    mode: u32,
    effective_uid: u32,
    user_namespace: bool,
    overflow_uid: u32,
    read_only_mount: bool,
    role: Role,
    links: u64,
    file_type: FileType,
}

// Pure integrity policy; Ok(true) requests the process-wide readability warning.
fn policy(input: PolicyInput) -> Result<bool, &'static str> {
    let PolicyInput {
        owner,
        mode,
        effective_uid,
        user_namespace,
        overflow_uid,
        read_only_mount,
        role,
        links,
        file_type,
    } = input;
    if file_type
        != if role == Role::Leaf {
            FileType::RegularFile
        } else {
            FileType::Directory
        }
    {
        return Err(if role == Role::Leaf {
            "leaf is not a regular file (symbolic links are refused)"
        } else {
            "not a directory (symbolic links are refused)"
        });
    }
    let ancestor_owner =
        owner == 0 || owner == effective_uid || (user_namespace && owner == overflow_uid);
    let owner_ok = match role {
        Role::Ancestor => ancestor_owner || read_only_mount,
        Role::StickyChild => owner == 0 || owner == effective_uid,
        _ => owner == effective_uid,
    };
    if !owner_ok {
        return Err("untrusted owner");
    }
    if mode & 0o022 != 0
        && !(matches!(role, Role::Ancestor | Role::StickyChild) && mode & 0o1000 != 0)
    {
        return Err("writable by group or others");
    }
    if role == Role::Leaf && links != 1 {
        return Err("leaf has multiple hard links");
    }
    Ok(matches!(role, Role::StateDirectory | Role::Subdirectory | Role::Leaf) && mode & 0o044 != 0)
}

fn namespace() -> io::Result<(bool, u32)> {
    static CONTEXT: std::sync::OnceLock<Result<(bool, u32), String>> = std::sync::OnceLock::new();
    CONTEXT
        .get_or_init(|| {
            let map = std::fs::read_to_string("/proc/self/uid_map").map_err(|e| e.to_string())?;
            let identity = map.split_whitespace().collect::<Vec<_>>() == ["0", "0", "4294967295"];
            let overflow = std::fs::read_to_string("/proc/sys/kernel/overflowuid")
                .map_err(|e| e.to_string())?
                .trim()
                .parse::<u32>()
                .map_err(|e| e.to_string())?;
            Ok((!identity, overflow))
        })
        .as_ref()
        .copied()
        .map_err(|e| io::Error::other(e.clone()))
}

fn check(path: &Path, stat: &fs::Stat, role: Role, read_only_mount: bool) -> io::Result<()> {
    let (user_namespace, overflow_uid) = namespace()?;
    let warn = policy(PolicyInput {
        owner: stat.st_uid,
        mode: stat.st_mode,
        effective_uid: rustix::process::geteuid().as_raw(),
        user_namespace,
        overflow_uid,
        read_only_mount,
        role,
        links: stat.st_nlink,
        file_type: FileType::from_raw_mode(stat.st_mode),
    })
    .map_err(|reason| {
        error(
            path,
            format!(
                "{reason}; owner uid {}, mode {:o}",
                stat.st_uid,
                stat.st_mode & 0o7777
            ),
        )
    })?;
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if warn && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::warn!(
            "state file readable by other users: {}",
            tidemark_core::path::display_lossy_bytes(path.as_os_str().as_bytes())
        );
    }
    Ok(())
}
fn private(path: &Path, stat: &fs::Stat) -> io::Result<()> {
    check(path, stat, Role::StateDirectory, false)
}
fn ancestor(path: &Path, fd: &OwnedFd) -> io::Result<bool> {
    let stat = fs::fstat(fd)?;
    let read_only = fs::fstatvfs(fd)?
        .f_flag
        .contains(fs::StatVfsMountFlags::RDONLY);
    check(path, &stat, Role::Ancestor, read_only)?;
    Ok(stat.st_mode & 0o022 != 0)
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
/// ancestors must be sticky; their children must be root/euid-owned and safe from writes.
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
        fd = directory(&fd, Path::new(name)).map_err(|e| context(&traversed, e))?;
        if sticky {
            check(&traversed, &fs::fstat(&fd)?, Role::StickyChild, false)?;
        }
    }
    ancestor(&traversed, &fd)?;
    Ok(fd)
}
/// Obtain a file's parent without normalizing the retained-fd `/.` suffix.
pub fn file_parent(path: &Path) -> io::Result<PathBuf> {
    let bytes = path.as_os_str().as_bytes();
    let split = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .ok_or_else(|| error(path, "file parent missing"))?;
    Ok(PathBuf::from(OsStr::from_bytes(if split == 0 {
        b"/"
    } else {
        &bytes[..split]
    })))
}

/// A verified euid-owned directory with no group/world write permissions.
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
            .map_err(|e| context(path, e))?;
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
        let parent_fd = trusted_directory(parent).map_err(|e| context(path, e))?;
        let fd = match directory(&parent_fd, Path::new(name)) {
            Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                fs::mkdirat(&parent_fd, name, Mode::from_raw_mode(0o700))
                    .map_err(|e| context(path, e))?;
                directory(&parent_fd, Path::new(name))?
            }
            result => result.map_err(|e| context(path, e))?,
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
        let boundary = path
            .components()
            .enumerate()
            .fold(None, |last, (index, component)| {
                if component.as_os_str() == "snapshots" {
                    Some(index)
                } else {
                    last
                }
            });
        let mut base = PathBuf::new();
        let mut relative = Vec::new();
        let mut found = false;
        for (index, component) in path.components().enumerate() {
            if found {
                match component {
                    Component::Normal(n) => {
                        if relative.len() >= 4096 {
                            return Err(error(path, "snapshot directory depth exceeds 4096"));
                        }
                        relative.push(n);
                    }
                    _ => return Err(error(path, "unsafe snapshot component")),
                }
            } else if Some(index) == boundary {
                found = true;
                relative.push(component.as_os_str());
            } else {
                base.push(component.as_os_str());
            }
        }
        if found {
            if base.as_os_str().as_bytes().starts_with(b"/proc/self/fd/") {
                base.push(".");
            }
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
        check(&path, stat, Role::Leaf, false)?;
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
            Err(e) => Err(context(&self.path.join(name), e)),
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
            .map_err(|e| context(&self.path.join(name), e))?;
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
            result => result.map_err(|e| context(&path, e))?,
        };
        check(&path, &fs::fstat(&fd)?, Role::Subdirectory, false)?;
        Ok(Self { fd, path })
    }
    /// Exclusively create a private child directory relative to this descriptor.
    pub fn create_subdir(&self, name: &OsStr) -> io::Result<Self> {
        self.name(name)?;
        fs::mkdirat(&self.fd, name, Mode::from_raw_mode(0o700))?;
        self.subdir(name, false)
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
    fn readable_storage_warns_once() {
        const CHILD: &str = "TIDEMARK_READABLE_WARNING_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "state_dir::tests::readable_storage_warns_once",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        #[derive(Clone)]
        struct Capture(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl tracing::Subscriber for Capture {
            fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                tracing::span::Id::from_u64(1)
            }
            fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
            fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
            fn event(&self, event: &tracing::Event<'_>) {
                struct Visitor(String);
                impl tracing::field::Visit for Visitor {
                    fn record_debug(
                        &mut self,
                        _: &tracing::field::Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        self.0.push_str(&format!("{value:?}"));
                    }
                }
                let mut visitor = Visitor(String::new());
                event.record(&mut visitor);
                self.0.lock().unwrap().push(visitor.0);
            }
            fn enter(&self, _: &tracing::span::Id) {}
            fn exit(&self, _: &tracing::span::Id) {}
        }
        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        tracing::subscriber::with_default(Capture(messages.clone()), || {
            let temp = ::test_support::private_tempdir().unwrap();
            let state = StateDir::open(temp.path(), false).unwrap();
            let leaf = temp.path().join("readable.db");
            ::test_support::private_write(&leaf, b"sentinel").unwrap();
            std::fs::set_permissions(&leaf, std::fs::Permissions::from_mode(0o644)).unwrap();
            state
                .open_leaf(OsStr::new("readable.db"), false, false)
                .unwrap();
            state.check_leaf(OsStr::new("readable.db")).unwrap();
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
            StateDir::open(temp.path(), false).unwrap();
            let sub = state.subdir(OsStr::new("child"), true).unwrap();
            std::fs::set_permissions(&sub.path, std::fs::Permissions::from_mode(0o750)).unwrap();
            state.subdir(OsStr::new("child"), false).unwrap();
            let captured = messages.lock().unwrap();
            assert_eq!(captured.len(), 1);
            assert!(captured[0].contains(&format!(
                "state file readable by other users: {}",
                leaf.display()
            )));
            assert_eq!(std::fs::read(&leaf).unwrap(), b"sentinel");
            assert_eq!(
                std::fs::metadata(&leaf).unwrap().permissions().mode() & 0o777,
                0o644
            );
        });
    }

    #[test]
    fn policy_table() {
        let base = PolicyInput {
            owner: 1000,
            mode: 0o700,
            effective_uid: 1000,
            user_namespace: false,
            overflow_uid: 65534,
            read_only_mount: false,
            role: Role::Ancestor,
            links: 1,
            file_type: FileType::Directory,
        };
        let rows = [
            ("ancestor root", PolicyInput { owner: 0, ..base }, Ok(false)),
            ("ancestor self", base, Ok(false)),
            (
                "foreign ancestor",
                PolicyInput {
                    owner: 2000,
                    ..base
                },
                Err("untrusted owner"),
            ),
            (
                "readonly foreign",
                PolicyInput {
                    owner: 2000,
                    read_only_mount: true,
                    ..base
                },
                Ok(false),
            ),
            (
                "initial overflow",
                PolicyInput {
                    owner: 65534,
                    ..base
                },
                Err("untrusted owner"),
            ),
            (
                "namespace overflow",
                PolicyInput {
                    owner: 65534,
                    user_namespace: true,
                    ..base
                },
                Ok(false),
            ),
            (
                "namespace foreign",
                PolicyInput {
                    owner: 2000,
                    user_namespace: true,
                    ..base
                },
                Err("untrusted owner"),
            ),
            (
                "writable ancestor",
                PolicyInput {
                    mode: 0o775,
                    ..base
                },
                Err("writable by group or others"),
            ),
            (
                "readonly writable",
                PolicyInput {
                    mode: 0o777,
                    read_only_mount: true,
                    ..base
                },
                Err("writable by group or others"),
            ),
            (
                "sticky ancestor",
                PolicyInput {
                    mode: 0o1777,
                    ..base
                },
                Ok(false),
            ),
            (
                "readable child",
                PolicyInput {
                    role: Role::StickyChild,
                    mode: 0o755,
                    ..base
                },
                Ok(false),
            ),
            (
                "root child",
                PolicyInput {
                    role: Role::StickyChild,
                    owner: 0,
                    ..base
                },
                Ok(false),
            ),
            (
                "sticky child",
                PolicyInput {
                    role: Role::StickyChild,
                    mode: 0o1777,
                    ..base
                },
                Ok(false),
            ),
            (
                "writable child",
                PolicyInput {
                    role: Role::StickyChild,
                    mode: 0o777,
                    ..base
                },
                Err("writable by group or others"),
            ),
            (
                "overflow child",
                PolicyInput {
                    role: Role::StickyChild,
                    owner: 65534,
                    user_namespace: true,
                    ..base
                },
                Err("untrusted owner"),
            ),
        ];
        for (name, input, expected) in rows {
            assert_eq!(policy(input), expected, "{name}");
        }
        for role in [Role::StateDirectory, Role::Subdirectory, Role::Leaf] {
            let storage = PolicyInput {
                role,
                file_type: if role == Role::Leaf {
                    FileType::RegularFile
                } else {
                    FileType::Directory
                },
                mode: 0o600,
                ..base
            };
            assert_eq!(policy(storage), Ok(false));
            for mode in [0o640, 0o644, 0o755] {
                assert_eq!(policy(PolicyInput { mode, ..storage }), Ok(true));
            }
            assert_eq!(
                policy(PolicyInput {
                    mode: 0o711,
                    ..storage
                }),
                Ok(false)
            );
            for mode in [0o620, 0o602, 0o1777] {
                assert_eq!(
                    policy(PolicyInput { mode, ..storage }),
                    Err("writable by group or others")
                );
            }
            for owner in [0, 2000, 65534] {
                assert_eq!(
                    policy(PolicyInput {
                        owner,
                        user_namespace: true,
                        read_only_mount: true,
                        ..storage
                    }),
                    Err("untrusted owner")
                );
            }
            for file_type in [FileType::Symlink, FileType::Fifo, FileType::Socket] {
                assert!(policy(PolicyInput {
                    file_type,
                    ..storage
                })
                .is_err());
            }
            assert_eq!(
                policy(PolicyInput {
                    links: 2,
                    ..storage
                })
                .is_err(),
                role == Role::Leaf
            );
        }
    }

    #[test]
    fn directory_policy_and_retained_fd() {
        let temp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let child = temp.path().join("state");
        assert_eq!(
            StateDir::open(&child, false).err().unwrap().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!child.exists());
        let state = StateDir::open(&child, true).unwrap();
        assert_eq!(
            state
                .subdir(OsStr::new("missing"), false)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::NotFound
        );
        use std::os::fd::AsRawFd;
        assert!(StateDir::open(
            Path::new(&format!("/proc/self/fd/{}/.", state.fd.as_raw_fd())),
            false
        )
        .is_ok());
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(StateDir::open(&child, true).is_ok());
        symlink(&child, temp.path().join("alias")).unwrap();
        assert!(StateDir::open(&temp.path().join("alias"), true).is_err());
        assert!(StateDir::open(&temp.path().join("alias/nested"), true).is_err());
    }
    #[test]
    fn snapshot_ancestor_names_do_not_change_state_boundary() {
        let temp = ::test_support::private_tempdir().unwrap();
        let ancestor = temp.path().join("snapshots");
        std::fs::create_dir(&ancestor).unwrap();
        std::fs::set_permissions(&ancestor, std::fs::Permissions::from_mode(0o755)).unwrap();
        let state = ancestor.join("state");
        StateDir::open(&state, true).unwrap();
        StateDir::snapshot_dir(&state.join("snapshots/root"), true).unwrap();
        assert_eq!(
            std::fs::metadata(&ancestor).unwrap().permissions().mode() & 0o777,
            0o755
        );
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
        assert!(trusted_directory(&child).is_ok());
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o770)).unwrap();
        assert_eq!(
            StateDir::open(&child, false).err().unwrap().kind(),
            io::ErrorKind::Other
        );
    }
}
