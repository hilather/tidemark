//! Owned by: WSNAP-cold-store
//! Cold publication, reader parity, corruption and recovery integration evidence.
#![allow(clippy::unwrap_used)]
use std::{collections::BTreeMap, fs, path::Path};
use tidemark_core::{
    config::*,
    path::{RawName, RawPath},
    scan::{ScanMode, SnapshotStatus},
    snapshot::*,
};
use tidemark_store::{
    catalog::{cleanup_abandoned_scans, root_slug, Catalog, ScanEntry},
    cold::*,
    reader::Snapshot,
    writer::SnapshotWriter,
    StoreError,
};
fn path(bytes: &[u8]) -> RawPath {
    RawPath::new(bytes.to_vec()).unwrap()
}
fn meta() -> SnapshotMeta {
    SnapshotMeta {
        schema_version: 1,
        tool_version: "test".into(),
        scan_id: serde_json::from_str("\"20260101T000000Z-00000000\"").unwrap(),
        root: path(b"/data/\xff"),
        root_display: "display".into(),
        mode: ScanMode::Walk,
        mode_reason: String::new(),
        status: SnapshotStatus::Complete,
        degraded: false,
        exclude: vec![],
        link_instances: LinkInstances::All,
        started_at: 1000000,
        finished_at: 1000010,
        host: String::new(),
        kernel: String::new(),
        os_release: String::new(),
        fs_uuid: String::new(),
        fs_dev: 0,
        fs_blocksize: 0,
        threads: 0,
        throttle_profile: String::new(),
        dirs: 0,
        files: 0,
        inodes: 0,
        errors: 0,
        races: 0,
        skipped: 0,
        noatime_fallbacks: 0,
        phase_ms: BTreeMap::new(),
        gate_stats: Default::default(),
    }
}
fn dir(id: u32) -> DirRow {
    let p = if id == 0 {
        path(b"/data/\xff")
    } else {
        path(b"/data/\xff").join(&RawName::new(format!("{id:08}").into_bytes()).unwrap())
    };
    DirRow {
        id,
        parent_id: if id == 0 { None } else { Some(0) },
        subtree_end: if id == 0 { 99999 } else { id },
        depth: if id == 0 { 0 } else { 1 },
        name: if id == 0 {
            p.as_bytes().to_vec()
        } else {
            format!("{id:08}").into_bytes()
        },
        path: p.clone(),
        path_hash: p.hash64(),
        ino: u64::from(id) + 1,
        mtime: 999900,
        dir_disk: 0,
        dir_apparent: 0,
        projid: None,
        entries: 7,
        errors: 0,
        own_files: 0,
        own_symlinks: 0,
        own_charged: 0,
        own_apparent: 0,
        t_dirs: 1,
        t_files: 0,
        t_inodes: 0,
        t_disk: u64::from(id) + 4096,
        t_apparent: 0,
        t_charged: 0,
        t_reclaimable: 0,
        t_errors: 0,
        age0: 0,
        age1: 0,
        age2: 0,
        age3: 0,
        age4: 0,
        age5: 0,
        newest_mtime: 999900,
        max_entries: 0,
    }
}
fn write_fixture(n: u32) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = ::test_support::private_tempdir().unwrap();
    let m = meta();
    let mut w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    let mut rows: Vec<_> = (0..n).map(dir).collect();
    rows[0].subtree_end = n - 1;
    w.put_dirs(&rows).unwrap();
    w.put_files(&[
        FileRow {
            id: 0,
            dir_id: 0,
            name: None,
            ino: 77,
            disk: 8192,
            apparent: 9000,
            mtime: 999990,
            nlink: 1,
        },
        FileRow {
            id: 1,
            dir_id: 1,
            name: Some(RawName::new(vec![0xff]).unwrap()),
            ino: 78,
            disk: 4096,
            apparent: 5000,
            mtime: 999800,
            nlink: 2,
        },
    ])
    .unwrap();
    w.put_link_sets(&[LinkSetRow {
        ino: 78,
        nlink: 2,
        found: 2,
        complete: true,
        disk: 4096,
        apparent: 5000,
        mtime: 999800,
        lca_dir_id: 0,
    }])
    .unwrap();
    w.put_link_instances(&[
        LinkInstanceRow {
            ino: 78,
            dir_id: 1,
            name: None,
        },
        LinkInstanceRow {
            ino: 78,
            dir_id: 2,
            name: None,
        },
    ])
    .unwrap();
    w.put_skipped(&[SkipRow {
        dir_id: 1,
        name: RawName::new(b"mount".to_vec()).unwrap(),
        reason: tidemark_core::scan::SkipReason::Mount,
    }])
    .unwrap();
    w.put_errors(&[ErrorRow {
        dir_id: Some(1),
        path: dir(1).path,
        op: tidemark_core::scan::ErrOp::Statx,
        errno: 13,
    }])
    .unwrap();
    w.put_annotations(&[AnnotationRow {
        dir_id: 1,
        key: "app.database".into(),
        value: "app".into(),
    }])
    .unwrap();
    let file = w.finish(&m).unwrap();
    (tmp, file)
}

fn opened(path: &Path) -> Snapshot {
    Snapshot::open_with_space_guard(path, |_| Ok(())).unwrap()
}
fn equal<T: serde::Serialize>(a: T, b: T) {
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
}
#[test]
fn roundtrip_all_reader_methods_and_drop() {
    let (tmp, hot) = write_fixture(10);
    let cold = compress_snapshot(&hot, 3, &|| false).unwrap();
    assert!(hot.exists());
    let a = Snapshot::open(&hot).unwrap();
    let b = opened(&cold);
    equal(a.meta(), b.meta());
    equal(a.root().unwrap(), b.root().unwrap());
    equal(a.dir(1).unwrap(), b.dir(1).unwrap());
    equal(
        a.dir_by_path(&dir(1).path).unwrap(),
        b.dir_by_path(&dir(1).path).unwrap(),
    );
    for sort in [
        SortKey::Name,
        SortKey::Disk,
        SortKey::Apparent,
        SortKey::Charged,
        SortKey::Reclaimable,
        SortKey::Files,
        SortKey::Inodes,
        SortKey::Newest,
    ] {
        equal(
            a.children(0, sort, 20).unwrap(),
            b.children(0, sort, 20).unwrap(),
        );
        equal(
            a.children_page(0, sort, true, None, false, 20, b"")
                .unwrap(),
            b.children_page(0, sort, true, None, false, 20, b"")
                .unwrap(),
        );
        equal(
            a.subtree_dirs(0, 2, sort, 20).unwrap(),
            b.subtree_dirs(0, 2, sort, 20).unwrap(),
        );
    }
    assert_eq!(
        a.children_count(0, b"").unwrap(),
        b.children_count(0, b"").unwrap()
    );
    equal(
        a.subtree_files(0, &FileFilter::default(), 20).unwrap(),
        b.subtree_files(0, &FileFilter::default(), 20).unwrap(),
    );
    equal(
        a.wide_dirs(0, 0, 20).unwrap(),
        b.wide_dirs(0, 0, 20).unwrap(),
    );
    equal(
        a.link_sets_under(0, &LinkFilter::default(), 20).unwrap(),
        b.link_sets_under(0, &LinkFilter::default(), 20).unwrap(),
    );
    equal(a.link_instances(78).unwrap(), b.link_instances(78).unwrap());
    equal(
        a.link_instances_page(78, None, false, 20).unwrap(),
        b.link_instances_page(78, None, false, 20).unwrap(),
    );
    equal(a.skipped_under(0).unwrap(), b.skipped_under(0).unwrap());
    equal(
        a.errors_under(0, 20).unwrap(),
        b.errors_under(0, 20).unwrap(),
    );
    equal(a.annotations(1).unwrap(), b.annotations(1).unwrap());
    let (ad, ar, an, _) = a.metrics_candidates(2, 20).unwrap();
    let (bd, br, bn, file) = b.metrics_candidates(2, 20).unwrap();
    equal(ad, bd);
    equal(ar, br);
    assert_eq!(an, bn);
    assert_eq!(file, cold);
    assert_eq!(
        fs::read_dir(tmp.path().join(".inflate")).unwrap().count(),
        1
    );
    drop(b);
    assert_eq!(
        fs::read_dir(tmp.path().join(".inflate")).unwrap().count(),
        0
    );
    let dest = tmp.path().join("restored.db");
    inflate_to(&cold, &dest).unwrap();
    assert_eq!(fs::read(dest).unwrap(), fs::read(hot).unwrap());
}
#[test]
fn corrupt_headers_frames_lengths_and_destination_collisions() {
    let (tmp, hot) = write_fixture(3);
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let original = fs::read(&cold).unwrap();
    let dest = tmp.path().join("out.db");
    for mutation in 0..6 {
        let mut bytes = original.clone();
        match mutation {
            0 => bytes[0] ^= 1,
            1 => bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes()),
            2 => {
                bytes.truncate(bytes.len() - 3);
            }
            3 => {
                let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                bytes[8..16].copy_from_slice(&(n + 1).to_le_bytes());
            }
            4 => bytes.push(1),
            _ => {
                let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                bytes[8..16].copy_from_slice(&(n - 1).to_le_bytes());
            }
        }
        fs::write(&cold, bytes).unwrap();
        assert!(inflate_to(&cold, &dest).is_err(), "mutation {mutation}");
        assert!(!dest.exists());
    }
    fs::write(&cold, original).unwrap();
    inflate_to(&cold, &dest).unwrap();
    assert!(inflate_to(&cold, &dest).is_err());
    assert!(compress_snapshot(&hot, 1, &|| false).is_err());
}
#[test]
fn injected_space_refusal_precedes_leaf_creation() {
    let (tmp, hot) = write_fixture(3);
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let length = fs::metadata(hot).unwrap().len();
    let result = Snapshot::open_with_space_guard(&cold, |n| {
        assert_eq!(n, length);
        Err(StoreError::Space("injected".into()))
    });
    assert!(matches!(result, Err(StoreError::Space(_))));
    assert!(!tmp.path().join(".inflate").exists());
}
fn catalog_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let (source, hot) = write_fixture(3);
    let state = test_support::private_tempdir().unwrap();
    let root = state.path().join("snapshots").join(root_slug(&meta().root));
    test_support::private_dir(&root).unwrap();
    let dest = root.join(hot.file_name().unwrap());
    fs::rename(hot, &dest).unwrap();
    drop(source);
    (state, dest)
}
#[test]
fn cold_only_rebuild_and_crash_cleanup_keep_live_inflate() {
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    assert_eq!(catalog.rebuild_from_files().unwrap(), 1);
    let before = catalog.list(None).unwrap().remove(0);
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let reader = opened(&cold);
    let stale = cold.parent().unwrap().join(".inflate/stale.999999.0.db");
    test_support::private_write(&stale, b"stale").unwrap();
    let temporary = hot.with_extension("db.zst.tmp");
    test_support::private_write(&temporary, b"partial").unwrap();
    drop(catalog);
    cleanup_abandoned_scans(state.path(), 123);
    assert!(!hot.exists());
    assert!(!temporary.exists());
    assert!(!stale.exists());
    equal(
        reader.root().unwrap(),
        read_cold_header(&cold).unwrap().root,
    );
    assert_eq!(
        fs::read_dir(cold.parent().unwrap().join(".inflate"))
            .unwrap()
            .count(),
        1
    );
    drop(reader);
    let catalog = Catalog::open(state.path(), true).unwrap();
    assert_eq!(catalog.rebuild_from_files().unwrap(), 1);
    let after = catalog.list(None).unwrap().remove(0);
    assert_eq!(before.scan_id, after.scan_id);
    assert_eq!(before.root, after.root);
    equal(before.mode, after.mode);
    assert_eq!(before.started_at, after.started_at);
    assert_eq!(before.finished_at, after.finished_at);
    assert_eq!(before.status, after.status);
    assert_eq!(before.degraded, after.degraded);
    assert_eq!(before.t_disk, after.t_disk);
    assert_eq!(before.t_inodes, after.t_inodes);
    assert_eq!(before.t_dirs, after.t_dirs);
    assert_eq!(before.errors, after.errors);
    assert_eq!(before.duration_ms, after.duration_ms);
    assert_eq!(after.file, Some(cold));
}
#[test]
fn cancellation_leaves_only_repairable_temporary() {
    let (state, hot) = catalog_fixture();
    let calls = std::cell::Cell::new(0);
    let error = compress_snapshot(&hot, 1, &|| {
        let n = calls.get() + 1;
        calls.set(n);
        n >= 3
    })
    .unwrap_err();
    assert!(matches!(error, StoreError::Cancelled));
    assert!(hot.exists());
    assert!(!hot.with_extension("db.zst").exists());
    cleanup_abandoned_scans(state.path(), 0);
    assert!(!hot.with_extension("db.zst.tmp").exists());
    assert!(hot.exists());
}

fn entry(id: &str, root: &[u8], ts: i64, status: &str, cold: bool) -> ScanEntry {
    ScanEntry {
        scan_id: id.into(),
        root: path(root),
        mode: ScanMode::Walk,
        started_at: ts,
        finished_at: Some(ts + 1),
        status: status.into(),
        degraded: false,
        file: Some(format!("{id}.db{}", if cold { ".zst" } else { "" }).into()),
        t_disk: None,
        t_inodes: None,
        t_dirs: None,
        errors: None,
        duration_ms: None,
    }
}
#[test]
fn cold_selection_respects_recent_newest_calendar_status_and_deleted() {
    use tidemark_store::retention::cold_plan;
    let now = 100 * 86400 + 100;
    let entries = vec![
        entry("new", b"/a", now, "complete", false),
        entry("recent", b"/a", now - 1, "complete", false),
        entry("daily", b"/a", now - 86400, "complete", false),
        entry("weekly", b"/a", now - 7 * 86400, "complete", false),
        entry("deleted", b"/a", 0, "complete", false),
        entry("partial", b"/a", now - 2, "partial", false),
        entry("failed", b"/a", now - 3, "failed", false),
        entry("cold", b"/a", now - 2 * 86400, "complete", true),
        entry("other", b"/b", now - 86400, "complete", false),
    ];
    let policy = SnapshotRetention {
        recent: 2,
        daily: 7,
        weekly: 8,
    };
    assert_eq!(
        cold_plan(&entries, &policy, 2, now),
        vec!["daily", "weekly"]
    );
    assert_eq!(cold_plan(&entries, &policy, 3, now), vec!["weekly"]);
    assert!(cold_plan(
        &entries,
        &SnapshotRetention {
            recent: 0,
            daily: 0,
            weekly: 0
        },
        0,
        now
    )
    .is_empty());
}

#[test]
fn corrupt_cold_copy_never_removes_hot_source() {
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    drop(catalog);
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let file = fs::OpenOptions::new().write(true).open(&cold).unwrap();
    file.set_len(file.metadata().unwrap().len() - 1).unwrap();
    cleanup_abandoned_scans(state.path(), 123);
    assert!(hot.exists());
    assert!(Snapshot::open(&hot).is_ok());
}

#[test]
fn nofollow_private_modes_and_default_open() {
    use std::os::unix::fs::PermissionsExt;
    let (tmp, hot) = write_fixture(3);
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    // This also exercises the real default policy against the private fixture state.
    let reader = Snapshot::open(&cold).unwrap();
    let inflate = tmp.path().join(".inflate");
    assert_eq!(
        fs::metadata(&inflate).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let leaf = fs::read_dir(&inflate)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::metadata(leaf).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(reader);
    let alias = tmp.path().join("alias.db.zst");
    std::os::unix::fs::symlink(&cold, &alias).unwrap();
    assert!(read_cold_header(&alias).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&cold, &alias).unwrap();
    assert!(read_cold_header(&alias).is_err());
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&inflate, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Snapshot::open_with_space_guard(&cold, |_| Ok(())).is_err());
}

#[cfg(feature = "privileged")]
#[test]
fn cold_open_and_cleanup_refuse_same_device_inflate_bind_mount() {
    use std::process::Command;
    let xfs = test_support::xfs::LoopXfs::new(1 << 30, &[], "");
    let state = xfs.mountpoint.join("state");
    let root = state.join("snapshots").join(root_slug(&meta().root));
    test_support::private_dir(&root).unwrap();
    let (_source, original) = write_fixture(3);
    let hot = root.join(original.file_name().unwrap());
    fs::copy(original, &hot).unwrap();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let outside = xfs.mountpoint.join("outside");
    let inflate = root.join(".inflate");
    test_support::private_dir(&outside).unwrap();
    test_support::private_dir(&inflate).unwrap();
    let sentinel = outside.join("sentinel.999999.0.db");
    test_support::private_write(&sentinel, b"unchanged").unwrap();
    assert!(Command::new("mount")
        .arg("--bind")
        .arg(&outside)
        .arg(&inflate)
        .status()
        .unwrap()
        .success());
    let result = Snapshot::open_with_space_guard(&cold, |_| Ok(()));
    cleanup_abandoned_scans(&state, 123);
    let preserved = fs::read(&sentinel);
    assert!(Command::new("umount")
        .arg(&inflate)
        .status()
        .unwrap()
        .success());
    assert!(result.is_err());
    assert_eq!(preserved.unwrap(), b"unchanged");
}
