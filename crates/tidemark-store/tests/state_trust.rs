//! Owned by: WSTATE-trust-A
//! Hostile state leaves are refused before SQLite can read or mutate them.
#![allow(clippy::unwrap_used)]
use std::{
    ffi::OsStr,
    os::{
        fd::AsRawFd,
        unix::fs::{symlink, PermissionsExt},
    },
    path::Path,
};
use tidemark_store::{
    catalog::Catalog, reader::Snapshot, state_dir::StateDir, timeseries::TimeSeries,
};

fn plant(dir: &Path, name: &str, kind: &str, victim: &Path) {
    let path = dir.join(name);
    match kind {
        "symlink" => symlink(victim, path).unwrap(),
        "hardlink" => std::fs::hard_link(victim, path).unwrap(),
        "fifo" => rustix::fs::mknodat(
            rustix::fs::CWD,
            &path,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap(),
        "mode" => {
            test_support::private_write(&path, b"unsafe").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o620)).unwrap();
        }
        _ => panic!("unknown fixture"),
    }
}
#[test]
fn catalog_and_series_refuse_every_hostile_database_leaf() {
    for name in [
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-shm",
        "timeseries.db-journal",
        "timeseries.db.bak-v0",
        "timeseries.db.bak-v0-wal",
        "timeseries.db.bak-v0-shm",
        "timeseries.db.bak-v0-journal",
    ] {
        for kind in ["symlink", "hardlink", "fifo", "mode"] {
            let state = test_support::private_tempdir().unwrap();
            let outside = test_support::private_tempdir().unwrap();
            let victim = outside.path().join("victim");
            test_support::private_write(&victim, b"").unwrap();
            plant(state.path(), name, kind, &victim);
            let before = std::fs::read_dir(state.path()).unwrap().count();
            let error = Catalog::open(state.path(), true).err().unwrap();
            assert!(error.to_string().contains(name), "{name} {kind}: {error}");
            assert!(TimeSeries::open(&state.path().join("timeseries.db"), true).is_err());
            assert_eq!(std::fs::read_dir(state.path()).unwrap().count(), before);
            assert_eq!(std::fs::read(&victim).unwrap(), b"");
            let metadata = std::fs::metadata(&victim).unwrap();
            assert_eq!(metadata.len(), 0);
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        }
    }
}
#[test]
fn readonly_missing_state_creates_nothing() {
    let parent = test_support::private_tempdir().unwrap();
    let state = parent.path().join("missing");
    assert!(Catalog::open(&state, false).is_err());
    assert!(!state.exists());
    assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
}
#[test]
fn snapshot_reads_refuse_hostile_leaves_and_hierarchy() {
    for kind in ["symlink", "hardlink", "fifo", "mode"] {
        let state = test_support::private_tempdir().unwrap();
        let outside = test_support::private_tempdir().unwrap();
        let victim = outside.path().join("victim");
        test_support::private_write(&victim, b"sentinel").unwrap();
        let snapshots = StateDir::open(state.path(), false)
            .unwrap()
            .subdir(OsStr::new("snapshots"), true)
            .unwrap();
        snapshots.subdir(OsStr::new("root"), true).unwrap();
        let catalog = Catalog::open(state.path(), true).unwrap();
        let before_entries = std::fs::read_dir(state.path()).unwrap().count();
        plant(
            &state.path().join("snapshots/root"),
            "snapshot.db",
            kind,
            &victim,
        );
        assert!(Snapshot::open(&state.path().join("snapshots/root/snapshot.db")).is_err());
        assert!(catalog.rebuild_from_files().is_err());
        assert_eq!(
            std::fs::read_dir(state.path()).unwrap().count(),
            before_entries
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"sentinel");
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::read_dir(state.path().join("snapshots/root"))
                .unwrap()
                .count(),
            1
        );
    }
    let state = test_support::private_tempdir().unwrap();
    let outside = test_support::private_tempdir().unwrap();
    symlink(outside.path(), state.path().join("snapshots")).unwrap();
    assert!(StateDir::snapshot_dir(&state.path().join("snapshots/root"), true).is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
#[test]
fn retained_directory_survives_rename_and_sqlite_nofollow_refuses_proc_path() {
    let parent = test_support::private_tempdir().unwrap();
    let original = parent.path().join("state");
    test_support::private_dir(&original).unwrap();
    let retained = std::fs::File::open(&original).unwrap();
    let anchor = std::path::PathBuf::from(format!("/proc/self/fd/{}/.", retained.as_raw_fd()));
    std::fs::rename(&original, parent.path().join("renamed")).unwrap();
    Catalog::open(&anchor, true).unwrap();
    assert!(!original.exists());
    assert!(parent.path().join("renamed/timeseries.db").is_file());
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW;
    assert!(rusqlite::Connection::open_with_flags(anchor.join("timeseries.db"), flags).is_err());
    assert!(rusqlite::Connection::open_with_flags(
        parent.path().join("renamed/timeseries.db"),
        flags
    )
    .is_ok());
    StateDir::snapshot_dir(&anchor.join("snapshots/root"), true).unwrap();
    assert!(parent.path().join("renamed/snapshots/root").is_dir());
}
