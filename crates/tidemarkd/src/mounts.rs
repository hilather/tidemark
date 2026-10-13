//! Owned by: W6-mount-presence
//! Exact configured mount presence and bounded transition decisions.

use crate::DaemonError;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tidemark_core::{config::Config, path::RawPath};
use tidemark_fs::mount::{self, MountInfo};

/// Identity of the topmost mount at a configured path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Namespace mount identifier.
    pub id: u32,
    /// Device major number.
    pub major: u32,
    /// Device minor number.
    pub minor: u32,
}

/// Parse injected mountinfo using the shared byte-preserving parser.
pub fn parse(bytes: &[u8]) -> Result<Vec<MountInfo>, DaemonError> {
    mount::parse_mountinfo(bytes).map_err(|e| DaemonError::Invalid(e.to_string()))
}

/// Read the bounded process mount table once for a service pass.
pub fn read() -> Result<Vec<MountInfo>, DaemonError> {
    mount::read_mountinfo().map_err(|source| DaemonError::Io {
        context: "read mountinfo".into(),
        source,
    })
}

fn normalized(path: &Path) -> Result<RawPath, DaemonError> {
    // Match deploy preflight canonicalization when the path exists. A missing
    // path remains absent; lexical normalization allows injected tables too.
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    RawPath::from_path(&canonical).map_err(|e| DaemonError::Config(e.to_string()))
}

/// Resolve only an exact mount point; never accept its containing filesystem.
pub fn identity(mounts: &[MountInfo], path: &Path) -> Result<Option<Identity>, DaemonError> {
    let raw = normalized(path)?;
    Ok(mount::mount_for_path(mounts, &raw)
        .filter(|m| m.mount_point == raw)
        .map(|m| Identity {
            id: m.id,
            major: m.major,
            minor: m.minor,
        }))
}

/// Refuse a scan covered by any absent configured sampler or watcher mount.
pub fn check_scan(
    config: &Config,
    root: &RawPath,
    mounts: &[MountInfo],
) -> Result<(), DaemonError> {
    let root = normalized(root.as_path())?;
    for path in config.sampler.mounts.iter().chain(&config.watch.mounts) {
        if root.starts_with(&normalized(path)?) && identity(mounts, path)?.is_none() {
            return Err(DaemonError::Invalid(format!(
                "mount not present: {}",
                RawPath::from_path(path)
                    .map_err(|e| DaemonError::Config(e.to_string()))?
                    .display_lossy()
            )));
        }
    }
    Ok(())
}

/// A presence transition; None as the previous value also denotes initial absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// Configured path.
    pub path: PathBuf,
    /// Previously observed identity.
    pub previous: Option<Identity>,
    /// Currently observed identity.
    pub current: Option<Identity>,
}

/// Reconcile configured entries with one table, emitting each transition once.
/// Memory is bounded by the validated configured mount inventory.
pub fn reconcile(
    paths: &[PathBuf],
    previous: &mut BTreeMap<PathBuf, Option<Identity>>,
    mounts: &[MountInfo],
) -> Result<Vec<Transition>, DaemonError> {
    previous.retain(|p, _| paths.contains(p));
    let mut changes = Vec::new();
    for path in paths {
        let current = identity(mounts, path)?;
        if previous.get(path) != Some(&current) {
            changes.push(Transition {
                path: path.clone(),
                previous: previous.get(path).copied().flatten(),
                current,
            });
            previous.insert(path.clone(), current);
        }
    }
    Ok(changes)
}

/// Log each reconciled transition at its specified level.
pub fn log(changes: &[Transition], service: &str) {
    for change in changes {
        let path = change.path.display();
        match (change.previous, change.current) {
            (_, None) => tracing::warn!(%path, service, "not mounted"),
            (None, Some(identity)) => {
                tracing::info!(%path, service, mount_id=identity.id, "mounted")
            }
            (Some(_), Some(identity)) => {
                tracing::warn!(%path, service, mount_id=identity.id, "mount identity changed")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table(id: u32) -> Vec<MountInfo> {
        parse(
            format!(
                "1 0 8:0 / / rw - xfs /dev/root rw\n{id} 1 8:1 / /data rw - xfs /dev/data rw\n"
            )
            .as_bytes(),
        )
        .unwrap()
    }
    #[test]
    fn exact_presence_overmount_and_transitions_once() {
        let paths = vec!["/data".into(), "/other".into()];
        let mut previous = BTreeMap::new();
        let absent = parse(b"1 0 8:0 / / rw - xfs /dev/root rw\n").unwrap();
        assert_eq!(reconcile(&paths, &mut previous, &absent).unwrap().len(), 2);
        assert!(reconcile(&paths, &mut previous, &absent)
            .unwrap()
            .is_empty());
        let changes = reconcile(&paths, &mut previous, &table(2)).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].previous, None);
        assert_eq!(changes[0].current.unwrap().id, 2);
        assert!(reconcile(&paths, &mut previous, &table(2))
            .unwrap()
            .is_empty());
        let changes = reconcile(&paths, &mut previous, &table(3)).unwrap();
        assert_eq!(changes[0].previous.unwrap().id, 2);
        assert_eq!(changes[0].current.unwrap().id, 3);
        assert_eq!(reconcile(&paths, &mut previous, &absent).unwrap().len(), 1);
        let mut over = table(2);
        over.extend(table(3).into_iter().skip(1));
        assert_eq!(
            identity(&over, Path::new("/data/./")).unwrap().unwrap().id,
            3
        );
        assert!(identity(&over, Path::new("/data/child")).unwrap().is_none());
    }
    #[test]
    fn scan_coverage_is_component_wise_and_includes_disabled_watch_entries() {
        let mut config = Config::default();
        config.sampler.mounts = vec!["/data".into()];
        config.watch.mounts = vec!["/other".into()];
        let mounts = table(2);
        assert!(check_scan(
            &config,
            &RawPath::new(b"/data/child".to_vec()).unwrap(),
            &mounts
        )
        .is_ok());
        let error = check_scan(
            &config,
            &RawPath::new(b"/other/child".to_vec()).unwrap(),
            &mounts,
        )
        .unwrap_err();
        assert!(error.to_string().contains("mount not present: /other"));
        assert!(check_scan(
            &config,
            &RawPath::new(b"/otherwise".to_vec()).unwrap(),
            &mounts
        )
        .is_ok());
    }
}
