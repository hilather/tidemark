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
    let raw = RawPath::from_path(path).map_err(|e| DaemonError::Config(e.to_string()))?;
    Ok(mount::mount_for_path(mounts, &raw)
        .filter(|m| m.mount_point == raw)
        .map(|m| Identity {
            id: m.id,
            major: m.major,
            minor: m.minor,
        }))
}

pub(crate) fn live_identity(
    mounts: &[MountInfo],
    path: &Path,
) -> Result<Option<Identity>, DaemonError> {
    identity(mounts, normalized(path)?.as_path())
}

/// Refuse a scan covered by any absent configured sampler or watcher mount.
pub fn check_scan(
    config: &Config,
    root: &RawPath,
    mounts: &[MountInfo],
) -> Result<(), DaemonError> {
    let root = normalized(root.as_path())?;
    for path in config.sampler.mounts.iter().chain(&config.watch.mounts) {
        if root.starts_with(&normalized(path)?) && live_identity(mounts, path)?.is_none() {
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

/// Pure reconciliation over configured paths, previous identities and one table.
/// Emits each presence transition once; None-to-Some starts, Some-to-None stops,
/// and Some-to-different-Some stops before starting a replacement.
/// Memory is bounded by the validated configured mount inventory.
pub fn reconcile(
    paths: &[PathBuf],
    previous: &mut BTreeMap<PathBuf, Option<Identity>>,
    mounts: &[MountInfo],
) -> Result<Vec<Transition>, DaemonError> {
    reconcile_with(paths, previous, mounts, identity)
}

pub(crate) fn reconcile_live(
    paths: &[PathBuf],
    previous: &mut BTreeMap<PathBuf, Option<Identity>>,
    mounts: &[MountInfo],
) -> Result<Vec<Transition>, DaemonError> {
    reconcile_with(paths, previous, mounts, live_identity)
}

fn reconcile_with(
    paths: &[PathBuf],
    previous: &mut BTreeMap<PathBuf, Option<Identity>>,
    mounts: &[MountInfo],
    resolve: impl Fn(&[MountInfo], &Path) -> Result<Option<Identity>, DaemonError>,
) -> Result<Vec<Transition>, DaemonError> {
    previous.retain(|p, _| paths.contains(p));
    let mut changes = Vec::new();
    for path in paths {
        let current = resolve(mounts, path)?;
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
        let path = RawPath::from_path(&change.path)
            .map(|p| p.display_lossy())
            .unwrap_or_else(|_| "invalid configured mount path".into());
        match (change.previous, change.current) {
            (_, None) => tracing::warn!(service = %service, "mount not present: {path}"),
            (None, Some(identity)) => {
                tracing::info!(
                    service = %service,
                    "mount present: {path} ({}:{})",
                    identity.major,
                    identity.minor
                )
            }
            (Some(_), Some(identity)) => {
                tracing::warn!(service = %service, mount_id = identity.id, "mount changed: {path}")
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
    fn logger_emits_one_line_per_transition_and_device_changes_restart() {
        use std::{io::Write, sync::Arc};
        #[derive(Clone)]
        struct Sink(Arc<parking_lot::Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let bytes = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = Sink(bytes.clone());
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let paths = vec!["/data".into()];
            let mut previous = BTreeMap::new();
            for table in [
                vec![],
                vec![],
                table(2),
                table(2),
                parse(b"2 1 8:2 / /data rw - xfs /dev/new rw\n").unwrap(),
                vec![],
                vec![],
            ] {
                log(
                    &reconcile(&paths, &mut previous, &table).unwrap(),
                    "watcher",
                );
            }
        });
        let log = String::from_utf8(bytes.lock().clone()).unwrap();
        assert_eq!(log.lines().count(), 4, "{log}");
        assert_eq!(log.matches("mount not present: /data").count(), 2);
        assert_eq!(log.matches("mount changed: /data").count(), 1);
        assert_eq!(log.matches("INFO").count(), 1);
        assert!(log.contains("mount present: /data (8:1)"));
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
