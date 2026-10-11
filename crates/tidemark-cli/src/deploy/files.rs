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
    if !path.is_absolute() {
        return Err(io::Error::other("deployment path must be absolute"));
    }
    let mut fd = fs::open("/", FLAGS, Mode::empty())?;
    for part in path.components() {
        let name = match part {
            Component::RootDir => continue,
            Component::Normal(n) => n,
            _ => return Err(io::Error::other("unsafe deployment path component")),
        };
        fd = directory_at(&fd, name, create)?;
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
            match fs::openat(parent, name, FLAGS | OFlags::NOATIME, Mode::empty()) {
                Err(rustix::io::Errno::PERM) => Ok(fs::openat(parent, name, FLAGS, Mode::empty())?),
                result => result.map_err(Into::into),
            }
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
pub(crate) fn validate_marker_roots(path: &Path) -> io::Result<()> {
    let bytes = match read(&path.join(MARKER)) {
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
    inspect(config_path.as_path())?;
    let cfg =
        tidemark_core::config::Config::load(config_path.as_path()).map_err(io::Error::other)?;
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
    let fd = fs::openat(
        &parent,
        path.file_name()
            .ok_or_else(|| io::Error::other("missing name"))?,
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
pub(crate) fn private_dir(path: &Path, root: bool) -> io::Result<()> {
    let fd = match directory(path, false) {
        Ok(fd) => {
            let marker = fs::openat(
                &fd,
                MARKER,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            );
            let marked = marker
                .ok()
                .and_then(|m| {
                    if FileType::from_raw_mode(fs::fstat(&m).ok()?.st_mode) != FileType::RegularFile
                    {
                        return None;
                    }
                    let mut bytes = Vec::new();
                    std::fs::File::from(m)
                        .take(1024 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .ok()?;
                    if bytes.len() > 1024 * 1024 {
                        return None;
                    }
                    serde_json::from_slice::<serde_json::Value>(&bytes).ok()
                })
                .is_some_and(|v| {
                    v.get("format").and_then(|v| v.as_str()) == Some("tidemark-state-v1")
                        && v.get("state_dir")
                            .and_then(|v| {
                                serde_json::from_value::<tidemark_core::path::RawPath>(v.clone())
                                    .ok()
                            })
                            .is_some_and(|p| p.as_path() == path)
                });
            if !marked {
                return Ok(());
            }
            fd
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => directory(path, true)?,
        Err(e) => return Err(e),
    };
    fs::fchmod(&fd, Mode::from_raw_mode(0o700))?;
    if root {
        fs::fchown(
            &fd,
            Some(rustix::process::Uid::ROOT),
            Some(rustix::process::Gid::ROOT),
        )?;
    }
    Ok(())
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
