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
fn existing_ancestor(path: &Path) -> io::Result<&Path> {
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
        super::atomic_write_owned(path, bytes, rustix::process::geteuid().is_root())
    }
    fn private_dir(&self, path: &Path) -> io::Result<()> {
        super::private_dir(path)?;
        if rustix::process::geteuid().is_root() {
            let fd = rustix::fs::open(
                path,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )?;
            rustix::fs::fchown(
                fd,
                Some(rustix::process::Uid::ROOT),
                Some(rustix::process::Gid::ROOT),
            )?;
        }
        Ok(())
    }
    fn now(&self) -> i64 {
        self.now
    }
    fn installation(&self, cfg: &Config) -> io::Result<()> {
        for path in [
            "/usr/bin/tidemark",
            "/usr/bin/tidemarkd",
            "/usr/lib/systemd/system/tidemarkd.service",
            "/usr/bin/systemctl",
        ] {
            if !std::fs::metadata(path)?.is_file() {
                return Err(io::Error::other(format!("missing installed file: {path}")));
            }
        }
        ensure_writable(
            self.installed_config
                .parent()
                .ok_or_else(|| io::Error::other("config path has no parent"))?,
        )?;
        ensure_writable(&cfg.general.state_dir)
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
        // Fail closed for unknown and pseudo filesystems rather than guessing local semantics.
        if [
            "xfs", "ext2", "ext3", "ext4", "btrfs", "tmpfs", "zfs", "f2fs", "jfs", "reiserfs",
        ]
        .contains(&entry.fs_type.as_str())
        {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "not a supported local filesystem: {}",
                entry.fs_type
            )))
        }
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
