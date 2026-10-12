//! Owned by: W4-B
//! Read-only production pre-flight probes and a status socket adapter.

use super::{ApiPing, Env};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};
use tidemark_core::{config::Config, path::RawPath};
use tidemark_fs::{mount, probe};

/// Production environment; destinations may be replaced by callers using temporary paths.
pub struct RealEnv {
    /// Installed configuration destination.
    pub installed_config: PathBuf,
    /// Injected Unix timestamp.
    pub now: i64,
}
pub(super) fn existing_ancestor(path: &Path) -> io::Result<&Path> {
    for parent in path.ancestors() {
        match std::fs::metadata(parent) {
            Ok(_) => return Ok(parent),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no existing deployment parent"))
}
fn ensure_writable(path: &Path) -> io::Result<()> {
    super::reject_symlink_ancestors(path)?;
    let existing = existing_ancestor(path)?;
    rustix::fs::access(
        existing,
        rustix::fs::Access::WRITE_OK | rustix::fs::Access::EXEC_OK,
    )
    .map_err(Into::into)
}
fn available(fact: Option<&probe::Fact>, reason: &str) -> io::Result<()> {
    if fact.and_then(|f| f.value.as_deref()) == Some("true") {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{reason}: {}",
            fact.and_then(|f| f.why.as_deref()).unwrap_or("unavailable")
        )))
    }
}
impl Env for RealEnv {
    fn config_path(&self) -> &Path {
        &self.installed_config
    }
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        super::atomic_write_owned(path, bytes, true)
    }
    fn state_write(
        &self,
        parent: &std::os::fd::OwnedFd,
        name: &std::ffi::OsStr,
        bytes: &[u8],
    ) -> io::Result<()> {
        super::files::write_at(parent, name, bytes, true)
    }
    fn private_dir(&self, path: &Path) -> io::Result<()> {
        super::files::private_dir(path, true)
    }
    fn now(&self) -> i64 {
        self.now
    }
    fn installation(&self, cfg: &Config) -> io::Result<()> {
        installation(
            cfg,
            &self.installed_config,
            &[
                Path::new("/usr/bin/tidemark"),
                Path::new("/usr/bin/tidemarkd"),
                Path::new("/usr/lib/systemd/system/tidemarkd.service"),
                Path::new("/usr/bin/systemctl"),
            ],
        )
    }

    fn local_directory(&self, path: &Path) -> io::Result<()> {
        if !std::fs::metadata(path)?.is_dir() {
            return Err(io::Error::other("not a directory"));
        }
        let canonical = std::fs::canonicalize(path)?;
        let raw = RawPath::from_path(&canonical).map_err(io::Error::other)?;
        let mounts = mount::read_mountinfo()?;
        let entry = mount::mount_for_path(&mounts, &raw)
            .ok_or_else(|| io::Error::other("mount not found"))?;
        local_filesystem(&entry.fs_type)
    }
    fn mount_point(&self, path: &Path) -> io::Result<()> {
        let canonical = std::fs::canonicalize(path)?;
        let mounts = mount::read_mountinfo()?;
        if mounts.iter().any(|m| m.mount_point.as_path() == canonical) {
            Ok(())
        } else {
            Err(io::Error::other("not a mount point"))
        }
    }
    fn capacity(&self, state_dir: &Path) -> io::Result<(u64, u64)> {
        let stat = rustix::fs::statvfs(existing_ancestor(state_dir)?)?;
        let available = stat
            .f_bavail
            .checked_mul(stat.f_frsize)
            .ok_or_else(|| io::Error::other("capacity overflow"))?;
        let total = stat
            .f_blocks
            .checked_mul(stat.f_frsize)
            .ok_or_else(|| io::Error::other("capacity overflow"))?;
        Ok((available, total))
    }
    fn fanotify(&self, cfg: &Config) -> io::Result<()> {
        if !cfg.watch.enabled {
            return Ok(());
        }
        let roots = cfg
            .watch
            .mounts
            .iter()
            .map(|p| RawPath::from_path(p).map_err(io::Error::other))
            .collect::<io::Result<Vec<_>>>()?;
        let report = probe::probe(cfg, &roots);
        available(
            report.host.facts.get("mount_namespace_equal"),
            "mount namespace mismatch; watcher will be disabled",
        )?;
        for path in roots {
            let root = report
                .roots
                .iter()
                .find(|r| r.root == path)
                .ok_or_else(|| io::Error::other("watch mount probe missing"))?;
            available(
                root.facts.get("fanotify_available"),
                "fanotify unavailable; watcher will be disabled",
            )?;
        }
        Ok(())
    }
    fn bulkstat(&self, cfg: &Config) -> io::Result<()> {
        let raw = RawPath::new(b"/data".to_vec()).map_err(io::Error::other)?;
        let report = probe::probe(cfg, std::slice::from_ref(&raw));
        let root = report
            .roots
            .iter()
            .find(|r| r.root == raw)
            .ok_or_else(|| io::Error::other("/data probe missing; walk mode will be used"))?;
        available(
            root.facts.get("bulkstat_v5"),
            "XFS v5 bulkstat unavailable; walk mode will be used",
        )
    }
    fn lvm(&self, cfg: &Config) -> io::Result<()> {
        if !cfg.lvm.enabled {
            return Ok(());
        }
        tidemark_fs::lvm::collect(&cfg.lvm, self.now)
            .map(|_| ())
            .map_err(io::Error::other)
    }
}
/// A status ping creates a connection for each verification poll.
pub struct SocketPing {
    /// Configured daemon socket.
    pub socket: PathBuf,
    /// Per-call deadline.
    pub timeout: Duration,
}
impl ApiPing for SocketPing {
    fn ping(&self) -> io::Result<()> {
        let mut client =
            crate::client::Client::connect(&self.socket, self.timeout).map_err(io::Error::other)?;
        client
            .call::<_, tidemark_core::api::StatusResult>(
                "status",
                &tidemark_core::api::StatusParams {},
            )
            .map(|_| ())
            .map_err(io::Error::other)
    }
}

// Shared installation probe; tests replace only the required installation paths.
pub(super) fn installation(cfg: &Config, installed: &Path, required: &[&Path]) -> io::Result<()> {
    for path in required {
        if !std::fs::metadata(path)?.is_file() {
            return Err(io::Error::other(format!(
                "missing installed file: {}",
                path.display()
            )));
        }
    }
    ensure_writable(
        installed
            .parent()
            .ok_or_else(|| io::Error::other("config path has no parent"))?,
    )?;
    ensure_writable(&cfg.general.state_dir)?;
    super::files::state_handles(&cfg.general.state_dir, false)?;
    Ok(())
}

pub(super) fn local_filesystem(kind: &str) -> io::Result<()> {
    if [
        "xfs", "ext2", "ext3", "ext4", "btrfs", "tmpfs", "zfs", "f2fs", "jfs", "reiserfs",
    ]
    .contains(&kind)
    {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "not a supported local filesystem: {kind}"
        )))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_local_filesystems_and_unknown_types() {
        for kind in [
            "xfs", "ext2", "ext3", "ext4", "btrfs", "tmpfs", "zfs", "f2fs", "jfs", "reiserfs",
        ] {
            assert!(local_filesystem(kind).is_ok());
        }
        for kind in ["nfs", "cifs", "overlay", "fuse", "unknown"] {
            assert!(local_filesystem(kind)
                .unwrap_err()
                .to_string()
                .contains(kind));
        }
    }
}
