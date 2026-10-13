//! Owned by: W4-B-fix
//! No-follow descriptor-relative deployment file operations.
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use std::{
    io::{self, Read, Write},
    os::fd::OwnedFd,
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
};
const MARKER: &str = ".tidemark-state.json";
const FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
pub(crate) fn directory(path: &Path, create: bool) -> io::Result<OwnedFd> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(io::Error::other(format!(
            "unsafe deployment directory {}",
            display(path)
        )));
    }
    let mut ancestor = path;
    let mut missing = Vec::new();
    let mut fd = loop {
        match tidemark_store::state_dir::trusted_directory(ancestor) {
            Ok(fd) => break fd,
            Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                if missing.len() >= 4096 {
                    return Err(io::Error::other("deployment directory depth exceeds 4096"));
                }
                missing.push(
                    ancestor
                        .file_name()
                        .ok_or_else(|| io::Error::other("missing name"))?,
                );
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| io::Error::other("missing parent"))?;
            }
            Err(e) => return Err(directory_error(path, e)),
        }
    };
    for name in missing.into_iter().rev() {
        fd = directory_at(&fd, name, true)?;
        eligible(&fd, Path::new(name), false)?;
    }
    Ok(fd)
}

pub(crate) fn directory_at(
    parent: &OwnedFd,
    name: &std::ffi::OsStr,
    create: bool,
) -> io::Result<OwnedFd> {
    let next = match fs::openat(parent, name, FLAGS | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => fs::openat(parent, name, FLAGS, Mode::empty()),
        result => result,
    };
    match next {
        Ok(fd) => Ok(fd),
        Err(rustix::io::Errno::NOENT) if create => {
            fs::mkdirat(parent, name, Mode::from_raw_mode(0o700))?;
            let fd = match fs::openat(parent, name, FLAGS | OFlags::NOATIME, Mode::empty()) {
                Err(rustix::io::Errno::PERM) => fs::openat(parent, name, FLAGS, Mode::empty())?,
                result => result?,
            };
            eligible(&fd, Path::new(name), false)?;
            // Normalize only the directory just created, through its no-follow fd.
            fs::fchown(
                &fd,
                Some(rustix::process::geteuid()),
                Some(rustix::process::getegid()),
            )?;
            fs::fchmod(&fd, Mode::from_raw_mode(0o700))?;
            Ok(fd)
        }
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn validate_state(
    path: &Path,
    roots: impl Iterator<Item = impl AsRef<Path>>,
) -> io::Result<()> {
    if !path.is_absolute()
        || path == Path::new("/")
        || path.components().any(|p| matches!(p, Component::ParentDir))
    {
        return Err(io::Error::other("unsafe state_dir"));
    }
    for protected in [
        "/etc", "/usr", "/var", "/var/lib", "/run", "/home", "/root", "/boot", "/dev", "/proc",
        "/sys", "/tmp",
    ]
    .iter()
    .map(Path::new)
    {
        if protected.starts_with(path) {
            return Err(io::Error::other("state_dir contains a system directory"));
        }
    }
    for root in roots {
        let resolved =
            std::fs::canonicalize(root.as_ref()).unwrap_or_else(|_| root.as_ref().to_path_buf());
        if root.as_ref().starts_with(path) || resolved.starts_with(path) {
            return Err(io::Error::other("state_dir contains a tracked root"));
        }
    }
    let mut ancestor = path;
    loop {
        match directory(ancestor, false) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| io::Error::other("missing parent"))?;
            }
            Err(e) => return Err(e),
        }
    }
    if let Ok(fd) = directory(path, false) {
        use std::os::fd::AsRawFd;
        let parent = directory(
            path.parent()
                .ok_or_else(|| io::Error::other("missing state parent"))?,
            false,
        )?;
        let mount_id = |fd: &OwnedFd| -> io::Result<String> {
            let mut text = String::new();
            std::fs::File::open(format!("/proc/self/fdinfo/{}", fd.as_raw_fd()))?
                .take(4097)
                .read_to_string(&mut text)?;
            if text.len() > 4096 {
                return Err(io::Error::other("fdinfo exceeds bound"));
            }
            text.lines()
                .find_map(|l| l.strip_prefix("mnt_id:").map(|v| v.trim().to_owned()))
                .ok_or_else(|| io::Error::other("mount id unavailable"))
        };
        if mount_id(&fd)? != mount_id(&parent)? {
            return Err(io::Error::other("state_dir is a mount point"));
        }
    }
    Ok(())
}
pub(crate) fn validate_marker_roots_at(path: &Path, state: Option<&OwnedFd>) -> io::Result<()> {
    let bytes = match state.map_or_else(
        || Err(io::ErrorKind::NotFound.into()),
        |fd| read_state_at(fd, std::ffi::OsStr::new(MARKER), &path.join(MARKER)),
    ) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let cfg = tidemark_core::config::Config::default();
            return validate_state(
                path,
                cfg.roots
                    .iter()
                    .map(|r| &r.path)
                    .chain(cfg.sampler.mounts.iter())
                    .chain(cfg.watch.mounts.iter()),
            );
        }
        Err(e) => return Err(e),
    };
    let marker: serde_json::Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if marker.get("format").and_then(|v| v.as_str()) != Some("tidemark-state-v1") {
        return Err(io::Error::other("invalid state marker"));
    }
    let marked_path: tidemark_core::path::RawPath = serde_json::from_value(
        marker
            .get("state_dir")
            .cloned()
            .ok_or_else(|| io::Error::other("state marker path missing"))?,
    )
    .map_err(io::Error::other)?;
    if marked_path.as_path() != path {
        return Err(io::Error::other("state marker path mismatch"));
    }
    let config_path: tidemark_core::path::RawPath = serde_json::from_value(
        marker
            .get("config_path")
            .cloned()
            .ok_or_else(|| io::Error::other("state marker config missing"))?,
    )
    .map_err(io::Error::other)?;
    let bytes = read(config_path.as_path())?;
    let cfg = tidemark_core::config::Config::from_toml_bytes(&bytes).map_err(io::Error::other)?;
    validate_state(
        path,
        cfg.roots
            .iter()
            .map(|r| &r.path)
            .chain(cfg.sampler.mounts.iter())
            .chain(cfg.watch.mounts.iter()),
    )?;

    Ok(())
}
pub(crate) fn read(path: &Path) -> io::Result<Vec<u8>> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let path = absolute.as_path();
    let parent = directory(
        path.parent()
            .ok_or_else(|| io::Error::other("missing parent"))?,
        false,
    )?;
    read_at(
        &parent,
        path.file_name()
            .ok_or_else(|| io::Error::other("missing name"))?,
    )
}
pub(crate) fn read_at(parent: &OwnedFd, name: &std::ffi::OsStr) -> io::Result<Vec<u8>> {
    let fd = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    if FileType::from_raw_mode(fs::fstat(&fd)?.st_mode) != FileType::RegularFile {
        return Err(io::Error::other("deployment file is not regular"));
    }
    let mut bytes = Vec::new();
    std::fs::File::from(fd)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other("deployment file exceeds 1 MiB"));
    }
    Ok(bytes)
}
pub(crate) fn inspect(path: &Path) -> io::Result<()> {
    match read(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
pub(crate) fn private_dir(path: &Path, _root: bool) -> io::Result<()> {
    let (state, deploy) = state_handles(path, true)?;
    let (fd, _) = complete_handles(path, state, deploy)?;
    eligible(&fd, path, false)
}

pub(crate) fn remove(path: &Path) -> io::Result<()> {
    let parent = match directory(
        path.parent()
            .ok_or_else(|| io::Error::other("missing parent"))?,
        false,
    ) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        result => result?,
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing name"))?;
    match fs::unlinkat(&parent, name, AtFlags::empty()) {
        Err(rustix::io::Errno::NOENT) => Ok(()),
        result => result.map_err(Into::into),
    }
}
pub(crate) fn write(path: &Path, bytes: &[u8], root: bool) -> io::Result<()> {
    let parent = directory(
        path.parent()
            .ok_or_else(|| io::Error::other("missing parent"))?,
        true,
    )?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing name"))?;
    write_at(&parent, name, bytes, root)
}
pub(crate) fn write_at(
    parent: &OwnedFd,
    name: &std::ffi::OsStr,
    bytes: &[u8],
    root: bool,
) -> io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    match fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(s) if FileType::from_raw_mode(s.st_mode) != FileType::RegularFile => {
            return Err(io::Error::other("unsafe deployment destination"))
        }
        Err(e) if e != rustix::io::Errno::NOENT => return Err(e.into()),
        _ => {}
    }
    let temp = format!(
        ".tidemark.{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let fd = fs::openat(
        parent,
        temp.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?;
    let result = (|| {
        if root {
            fs::fchown(
                &fd,
                Some(rustix::process::Uid::ROOT),
                Some(rustix::process::Gid::ROOT),
            )?;
        }
        let mut file = std::fs::File::from(fd);
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::renameat(parent, temp.as_str(), parent, name)?;
        std::fs::File::from(rustix::io::dup(parent)?).sync_all()
    })();
    if result.is_err() {
        let _ = fs::unlinkat(parent, temp.as_str(), AtFlags::empty());
    }
    result
}

// Only a marker identifying this exact state root authorizes normalization.
pub(crate) fn state_handles(
    path: &Path,
    create: bool,
) -> io::Result<(Option<OwnedFd>, Option<OwnedFd>)> {
    let state = match directory(path, false) {
        Ok(fd) => fd,
        Err(e) if e.kind() == io::ErrorKind::NotFound && !create => return Ok((None, None)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => directory(path, true)?,
        Err(e) => return Err(e),
    };
    eligible(&state, path, true)?;
    let marked = marker_authorizes(&state, path)?;
    eligible(&state, path, marked)?;
    let mut deploy = match directory_at(&state, std::ffi::OsStr::new("deploy"), false) {
        Ok(fd) => {
            eligible(&fd, &path.join("deploy"), false)?;
            Some(fd)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(leaf_error(
                &state,
                std::ffi::OsStr::new("deploy"),
                &path.join("deploy"),
                e,
            ))
        }
    };
    check_content(&state, deploy.as_ref(), path)?;
    if create && deploy.is_none() {
        deploy = Some(directory_at(&state, std::ffi::OsStr::new("deploy"), true)?);
    }
    Ok((Some(state), deploy))
}
fn marker_authorizes(state: &OwnedFd, path: &Path) -> io::Result<bool> {
    check_leaf_at(state, std::ffi::OsStr::new(MARKER), &path.join(MARKER))?;
    match read_state_at(state, std::ffi::OsStr::new(MARKER), &path.join(MARKER)) {
        Ok(bytes) => {
            let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            Ok(
                v.get("format").and_then(|v| v.as_str()) == Some("tidemark-state-v1")
                    && v.get("state_dir")
                        .and_then(|v| {
                            serde_json::from_value::<tidemark_core::path::RawPath>(v.clone()).ok()
                        })
                        .is_some_and(|p| p.as_path() == path),
            )
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
fn eligible(fd: &OwnedFd, path: &Path, marked: bool) -> io::Result<()> {
    let stat = fs::fstat(fd)?;
    #[cfg(test)]
    let stat = FOREIGN_OWNER.with(|owner| {
        let mut stat = stat;
        if owner.borrow().as_deref() == Some(path) {
            stat.st_uid = rustix::process::geteuid().as_raw().wrapping_add(1);
        }
        stat
    });
    if stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o022 != 0
        || (!marked && stat.st_mode & 0o077 != 0)
    {
        return Err(io::Error::other(format!("unsafe directory {}: owner {}, mode {:04o}, links {}; requires effective-uid ownership and no group/world permissions", display(path), stat.st_uid, stat.st_mode & 0o7777, stat.st_nlink)));
    }
    Ok(())
}

pub(crate) fn complete_handles(
    path: &Path,
    state: Option<OwnedFd>,
    deploy: Option<OwnedFd>,
) -> io::Result<(OwnedFd, OwnedFd)> {
    let state = match state {
        Some(fd) => fd,
        None => {
            let fd = directory(path, true)?;
            eligible(&fd, path, false)?;
            fd
        }
    };
    let marked = marker_authorizes(&state, path)?;
    eligible(&state, path, marked)?;
    check_content(&state, deploy.as_ref(), path)?;
    let deploy = match deploy {
        Some(fd) => fd,
        None => directory_at(&state, std::ffi::OsStr::new("deploy"), true)?,
    };
    eligible(&deploy, &path.join("deploy"), false)?;
    // Existing directories are normalized only when their marker authorizes it.
    if marked {
        fs::fchmod(&state, Mode::from_raw_mode(0o700))?;
    }
    Ok((state, deploy))
}
pub(crate) fn optional_at(parent: Option<&OwnedFd>, name: &str) -> io::Result<Option<Vec<u8>>> {
    let Some(parent) = parent else {
        return Ok(None);
    };
    match read_state_at(parent, std::ffi::OsStr::new(name), Path::new(name)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

// Test-only installation metadata injection; default metadata is untouched.
#[cfg(test)]
thread_local! {
    pub(crate) static FOREIGN_OWNER: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn display(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    tidemark_core::path::display_lossy_bytes(path.as_os_str().as_bytes())
}

pub(crate) fn check_leaf_at(
    parent: &OwnedFd,
    name: &std::ffi::OsStr,
    path: &Path,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let dir = tidemark_store::state_dir::StateDir::open(
        Path::new(&format!("/proc/self/fd/{}/.", parent.as_raw_fd())),
        false,
    )?;
    let result = dir.check_leaf(name);
    #[cfg(test)]
    let result = FOREIGN_OWNER.with(|owner| {
        if owner.borrow().as_deref() == Some(path) {
            Err(io::Error::other("untrusted owner (test injection)"))
        } else {
            result
        }
    });
    result.map_err(|e| leaf_error(parent, name, path, e))?;
    Ok(())
}

fn check_content(state: &OwnedFd, deploy: Option<&OwnedFd>, path: &Path) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let dir = tidemark_store::state_dir::StateDir::open(
        Path::new(&format!("/proc/self/fd/{}/.", state.as_raw_fd())),
        false,
    )?;
    for name in [
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-shm",
        "timeseries.db-journal",
        "timeseries.db.bak-v0",
        "timeseries.db.bak-v0-wal",
        "timeseries.db.bak-v0-shm",
        "timeseries.db.bak-v0-journal",
        "scan.lock",
        MARKER,
    ] {
        check_leaf_at(state, std::ffi::OsStr::new(name), &path.join(name))?;
    }
    for name in ["snapshots", "tmp", "deploy"] {
        match dir.subdir(std::ffi::OsStr::new(name), false) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(leaf_error(
                    state,
                    std::ffi::OsStr::new(name),
                    &path.join(name),
                    e,
                ))
            }
        }
    }
    if let Some(deploy) = deploy {
        for name in ["staged.json", "firstboot.json"] {
            check_leaf_at(
                deploy,
                std::ffi::OsStr::new(name),
                &path.join("deploy").join(name),
            )?;
        }
    }
    Ok(())
}

pub(crate) fn check_purge_root(fd: &OwnedFd, path: &Path) -> io::Result<()> {
    eligible(fd, path, true)
}

pub(crate) fn optional_name_at(
    parent: Option<&OwnedFd>,
    name: &std::ffi::OsStr,
) -> io::Result<Option<Vec<u8>>> {
    let Some(parent) = parent else {
        return Ok(None);
    };
    match read_at(parent, name) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
pub(crate) fn remove_at(parent: &OwnedFd, name: &std::ffi::OsStr) -> io::Result<()> {
    match fs::unlinkat(parent, name, AtFlags::empty()) {
        Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn leaf_error(
    parent: &OwnedFd,
    name: &std::ffi::OsStr,
    path: &Path,
    error: io::Error,
) -> io::Error {
    let metadata = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map(|s| {
            format!(
                "owner {}, mode {:04o}, links {}",
                s.st_uid,
                s.st_mode & 0o7777,
                s.st_nlink
            )
        })
        .unwrap_or_else(|e| format!("metadata unavailable: {e}"));
    io::Error::new(
        error.kind(),
        format!("{}: {error}; {metadata}", display(path)),
    )
}
pub(crate) fn read_state_at(
    parent: &OwnedFd,
    name: &std::ffi::OsStr,
    path: &Path,
) -> io::Result<Vec<u8>> {
    use std::os::fd::AsRawFd;
    check_leaf_at(parent, name, path)?;
    let dir = tidemark_store::state_dir::StateDir::open(
        Path::new(&format!("/proc/self/fd/{}/.", parent.as_raw_fd())),
        false,
    )?;
    let mut bytes = Vec::new();
    dir.open_leaf(name, false, false)
        .map_err(|e| leaf_error(parent, name, path, e))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other("deployment file exceeds 1 MiB"));
    }
    Ok(bytes)
}

pub(crate) fn read_state(path: &Path) -> io::Result<Vec<u8>> {
    let parent = tidemark_store::state_dir::StateDir::open(
        &tidemark_store::state_dir::file_parent(path)?,
        false,
    )?;
    let fd = rustix::io::dup(parent.fd())?;
    read_state_at(
        &fd,
        path.file_name()
            .ok_or_else(|| io::Error::other("missing state name"))?,
        path,
    )
}

fn directory_error(path: &Path, error: io::Error) -> io::Error {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)
        .map(|s| {
            format!(
                "owner {}, mode {:04o}, links {}",
                s.uid(),
                s.mode() & 0o7777,
                s.nlink()
            )
        })
        .unwrap_or_else(|e| format!("metadata unavailable: {e}"));
    io::Error::new(
        error.kind(),
        format!("{}: {error}; {metadata}", display(path)),
    )
}
