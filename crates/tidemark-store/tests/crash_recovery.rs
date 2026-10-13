//! Owned by: WCRASH-recovery
//! Abandoned scan cleanup preserves unrelated state and supports bound paths.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::os::unix::{fs::symlink, io::AsRawFd};
use tidemark_core::{path::RawPath, scan::ScanMode};
use tidemark_store::catalog::{cleanup_abandoned_scans, Catalog, ScanEntry};

#[test]
fn cleanup_is_selective_repairs_rows_and_is_idempotent() {
    let state = ::test_support::private_tempdir().unwrap();
    let root = state.path().join("snapshots/root");
    ::test_support::private_dir(root.join("nested")).unwrap();
    for path in [
        root.join("dead.db.tmp"),
        root.join("keep.db"),
        root.join("other.tmp"),
        root.join("nested/deep.db.tmp"),
    ] {
        test_support::private_write(path, b"preserved unless temporary").unwrap();
    }
    std::fs::create_dir(root.join("directory.db.tmp")).unwrap();
    let outside = ::test_support::private_tempdir().unwrap();
    test_support::private_write(outside.path().join("outside.db.tmp"), b"keep").unwrap();
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
    let state = ::test_support::private_tempdir().unwrap();
    cleanup_abandoned_scans(&state.path().join("new"), 1);
    assert!(Catalog::open(&state.path().join("new"), false)
        .unwrap()
        .list(None)
        .unwrap()
        .is_empty());
}

#[test]
fn file_cleanup_failure_does_not_prevent_catalog_repair() {
    let state = ::test_support::private_tempdir().unwrap();
    let outside = ::test_support::private_tempdir().unwrap();
    let target = outside.path().join("outside.db.tmp");
    test_support::private_write(&target, b"keep").unwrap();
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
    let state = ::test_support::private_tempdir().unwrap();
    let root = state.path().join("snapshots/root");
    ::test_support::private_dir(&root).unwrap();
    let tmp = root.join("dead.db.tmp");
    test_support::private_write(&tmp, b"dead").unwrap();
    test_support::private_write(state.path().join("timeseries.db"), b"not sqlite").unwrap();
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
    let dir = ::test_support::private_tempdir().unwrap();
    let database = dir.path().join("timeseries.db");
    test_support::private_write(&database, b"corrupt").unwrap();
    test_support::private_write(dir.path().join("scan.lock"), b"").unwrap();
    let held = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.path().join("scan.lock"))
        .unwrap();
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

#[test]
fn live_writer_child() {
    let Some(state) = std::env::var_os("TIDEMARK_LEASE_CHILD") else {
        return;
    };
    let state = std::path::PathBuf::from(state);
    let _series =
        tidemark_store::timeseries::TimeSeries::open(&state.join("timeseries.db"), true).unwrap();
    test_support::private_write(state.join("ready"), b"ready").unwrap();
    // Model persistent daemon connections before API bind: no socket or scan lock.
    std::thread::sleep(std::time::Duration::from_secs(30));
}

#[test]
fn recovery_excludes_live_writer_and_preserves_artifacts() {
    use std::{
        os::unix::fs::MetadataExt,
        time::{Duration, Instant},
    };
    let state = test_support::private_tempdir().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "live_writer_child"])
        .env("TIDEMARK_LEASE_CHILD", state.path())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !state.path().join("ready").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut originals = Vec::new();
    for name in ["timeseries.db", "timeseries.db-wal", "timeseries.db-shm"] {
        let path = state.path().join(name);
        test_support::private_write(&path, b"deliberate corruption").unwrap();
        originals.push((path.clone(), std::fs::metadata(path).unwrap().ino()));
    }
    let start = Instant::now();
    let error =
        tidemark_store::lease::RecoveryLease::acquire(state.path(), Duration::from_millis(60))
            .err()
            .unwrap();
    assert!(error
        .get_ref()
        .unwrap()
        .downcast_ref::<tidemark_store::lease::LeaseBusy>()
        .is_some());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(tidemark_store::catalog::recover_catalog(state.path()).is_err());
    for (path, inode) in originals {
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(std::fs::read(path).unwrap(), b"deliberate corruption");
    }
    assert!(!state.path().join("timeseries-recovery-0").exists());
    child.kill().unwrap();
    child.wait().unwrap();
    let catalog = tidemark_store::catalog::recover_catalog(state.path()).unwrap();
    assert_eq!(catalog.recovery_files().len(), 3);
}

#[test]
fn every_writer_holds_lease_and_readers_do_not() {
    use std::time::Duration;
    use tidemark_store::lease::{RecoveryLease, WriterLease};
    let state = test_support::private_tempdir().unwrap();
    let catalog = Catalog::open(state.path(), true).unwrap();
    assert!(RecoveryLease::acquire(state.path(), Duration::ZERO).is_err());
    let series =
        tidemark_store::timeseries::TimeSeries::open(&state.path().join("timeseries.db"), true)
            .unwrap();
    drop(catalog);
    assert!(RecoveryLease::acquire(state.path(), Duration::ZERO).is_err());
    drop(series);
    let reader = Catalog::open(state.path(), false).unwrap();
    let exclusive = RecoveryLease::acquire(state.path(), Duration::ZERO).unwrap();
    let started = std::time::Instant::now();
    let error = WriterLease::acquire(state.path()).err().unwrap();
    assert!(matches!(
        error
            .get_ref()
            .unwrap()
            .downcast_ref::<tidemark_store::lease::LeaseBusy>(),
        Some(tidemark_store::lease::LeaseBusy::Recovery)
    ));
    assert!(started.elapsed() < Duration::from_secs(3));
    drop(exclusive);
    drop(reader);
    assert!(Catalog::open(state.path(), true).is_ok());
}

#[test]
fn writer_lock_obeys_verified_leaf_rules() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let state = test_support::private_tempdir().unwrap();
    let lease = tidemark_store::lease::WriterLease::acquire(state.path()).unwrap();
    let lock = state.path().join("writer.lock");
    let meta = lock.metadata().unwrap();
    assert_eq!(meta.mode() & 0o777, 0o600);
    assert_eq!(meta.nlink(), 1);
    drop(lease);
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(tidemark_store::lease::WriterLease::acquire(state.path()).is_err());
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&lock, state.path().join("alias")).unwrap();
    assert!(tidemark_store::lease::WriterLease::acquire(state.path()).is_err());
    std::fs::remove_file(&lock).unwrap();
    symlink("alias", &lock).unwrap();
    assert!(
        tidemark_store::lease::RecoveryLease::acquire(state.path(), std::time::Duration::ZERO)
            .is_err()
    );
}
