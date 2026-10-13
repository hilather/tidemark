//! Owned by: WCRASH-recovery
//! Abandoned scan cleanup preserves unrelated state and supports bound paths.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::os::unix::{fs::symlink, io::AsRawFd};
use tidemark_core::{path::RawPath, scan::ScanMode};
use tidemark_store::catalog::{cleanup_abandoned_scans, Catalog, ScanEntry};

#[test]
fn cleanup_is_selective_repairs_rows_and_is_idempotent() {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("snapshots/root");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    for path in [
        root.join("dead.db.tmp"),
        root.join("keep.db"),
        root.join("other.tmp"),
        root.join("nested/deep.db.tmp"),
    ] {
        std::fs::write(path, b"preserved unless temporary").unwrap();
    }
    std::fs::create_dir(root.join("directory.db.tmp")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("outside.db.tmp"), b"keep").unwrap();
    symlink(outside.path(), state.path().join("snapshots/outside")).unwrap();
    symlink("keep.db", root.join("x.db.tmp")).unwrap();
    symlink(&root, state.path().join("snapshots/linked")).unwrap();
    let catalog = Catalog::open(state.path(), true).unwrap();
    for (id, status) in [("dead", "running"), ("done", "complete")] {
        catalog
            .record_start(&ScanEntry {
                scan_id: id.into(),
                root: RawPath::new(b"/fixture".to_vec()).unwrap(),
                mode: ScanMode::Walk,
                started_at: 1,
                finished_at: if status == "complete" { Some(2) } else { None },
                status: status.into(),
                degraded: false,
                file: Some(root.join("keep.db")),
                t_disk: None,
                t_inodes: None,
                t_dirs: None,
                errors: None,
                duration_ms: None,
            })
            .unwrap();
    }
    let fd = std::fs::File::open(state.path()).unwrap();
    let bound = std::path::PathBuf::from(format!("/proc/self/fd/{}/.", fd.as_raw_fd()));
    cleanup_abandoned_scans(&bound, 50);
    cleanup_abandoned_scans(state.path(), 60);
    assert!(!root.join("dead.db.tmp").exists());
    assert!(root.join("directory.db.tmp").is_dir());
    assert!(outside.path().join("outside.db.tmp").exists());
    for name in ["keep.db", "other.tmp", "nested/deep.db.tmp", "x.db.tmp"] {
        assert!(root.join(name).exists());
    }
    assert!(root
        .join("x.db.tmp")
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    let dead = catalog.get("dead").unwrap().unwrap();
    assert_eq!(dead.status, "failed");
    assert_eq!(dead.finished_at, Some(50));
    assert!(dead.file.is_none());
    let done = catalog.get("done").unwrap().unwrap();
    assert_eq!(done.status, "complete");
    assert_eq!(done.finished_at, Some(2));
    assert!(done.file.is_some());
}

#[test]
fn empty_and_missing_snapshot_directories_are_supported() {
    let state = tempfile::tempdir().unwrap();
    cleanup_abandoned_scans(&state.path().join("new"), 1);
    assert!(Catalog::open(&state.path().join("new"), false)
        .unwrap()
        .list(None)
        .unwrap()
        .is_empty());
}

#[test]
fn file_cleanup_failure_does_not_prevent_catalog_repair() {
    let state = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("outside.db.tmp");
    std::fs::write(&target, b"keep").unwrap();
    symlink(outside.path(), state.path().join("snapshots")).unwrap();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog
        .record_start(&ScanEntry {
            scan_id: "failed-cleanup".into(),
            root: RawPath::new(b"/fixture".to_vec()).unwrap(),
            mode: ScanMode::Walk,
            started_at: 1,
            finished_at: None,
            status: "running".into(),
            degraded: false,
            file: None,
            t_disk: None,
            t_inodes: None,
            t_dirs: None,
            errors: None,
            duration_ms: None,
        })
        .unwrap();
    cleanup_abandoned_scans(state.path(), 2);
    assert!(target.exists());
    assert_eq!(
        catalog.get("failed-cleanup").unwrap().unwrap().status,
        "failed"
    );
}

#[test]
fn catalog_failure_does_not_prevent_file_cleanup() {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("snapshots/root");
    std::fs::create_dir_all(&root).unwrap();
    let tmp = root.join("dead.db.tmp");
    std::fs::write(&tmp, b"dead").unwrap();
    std::fs::write(state.path().join("timeseries.db"), b"not sqlite").unwrap();
    cleanup_abandoned_scans(state.path(), 3);
    assert!(!tmp.exists());
    assert_eq!(
        std::fs::read(state.path().join("timeseries.db")).unwrap(),
        b"not sqlite"
    );
}

#[test]
fn checked_recovery_refuses_wrong_or_busy_scan_lock() {
    use tidemark_store::catalog::recover_catalog_with_lock;
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("timeseries.db");
    std::fs::write(&database, b"corrupt").unwrap();
    let held = std::fs::File::create(dir.path().join("scan.lock")).unwrap();
    rustix::fs::flock(&held, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    let other = std::fs::File::open(dir.path().join("scan.lock")).unwrap();
    assert!(recover_catalog_with_lock(dir.path(), &other).is_err());
    assert!(tidemark_store::catalog::recover_catalog(dir.path()).is_err());
    let wrong = std::fs::File::create(dir.path().join("wrong.lock")).unwrap();
    assert!(recover_catalog_with_lock(dir.path(), &wrong).is_err());
    assert_eq!(std::fs::read(&database).unwrap(), b"corrupt");
    assert!(!dir.path().join("timeseries-recovery-0").exists());
    let catalog = recover_catalog_with_lock(dir.path(), &held).unwrap();
    assert_eq!(catalog.recovery_files().len(), 1);
}
