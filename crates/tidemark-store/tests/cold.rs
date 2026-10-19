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
        ..Default::default()
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
            weekly: 0,
            ..Default::default()
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

#[test]
fn diff_and_explain_all_hot_cold_directions() {
    use tidemark_store::diff::{diff, explain, DiffMetric};
    let (_a, hot_a) = write_fixture(3);
    let (_b, hot_b) = write_fixture(3);
    let cold_a = compress_snapshot(&hot_a, 1, &|| false).unwrap();
    let cold_b = compress_snapshot(&hot_b, 1, &|| false).unwrap();
    for new_path in [&hot_a, &cold_a] {
        let new = opened(new_path);
        for old in [&hot_b, &cold_b] {
            let rows = diff(&new, old, &meta().root, DiffMetric::Disk, 2, 20).unwrap();
            assert_eq!(rows.len(), 3);
            assert!(rows.iter().all(|row| row.delta == 0));
            let explanation =
                explain(&new, old, &meta().root, DiffMetric::Disk, 0.8, 1, 20).unwrap();
            assert_eq!(explanation.target_delta, 0);
        }
    }
}

#[test]
fn valid_frame_with_wrong_payload_preserves_hot_and_catalog() {
    use std::io::Write;
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let bytes = fs::read(&cold).unwrap();
    let header_len = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize + 28;
    let length = fs::metadata(&hot).unwrap().len() as usize;
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).unwrap();
    encoder.include_checksum(true).unwrap();
    encoder.write_all(&vec![0; length]).unwrap();
    let mut bad = bytes[..header_len].to_vec();
    bad.extend(encoder.finish().unwrap());
    fs::write(&cold, bad).unwrap();
    validate_cold(&cold).unwrap();
    assert!(Snapshot::open(&cold).is_err());
    let transition = catalog.point_to_cold(&meta().scan_id.to_string(), &cold);
    cleanup_abandoned_scans(state.path(), 123);
    assert!(Snapshot::open(&hot).is_ok());
    assert!(!cold.exists());
    assert!(cold.with_extension("zst.bad").exists());
    assert!(transition.is_err());
    assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&hot));
}

#[test]
fn rebuild_prefers_hot_over_truncated_cold() {
    let (state, hot) = catalog_fixture();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let f = fs::OpenOptions::new().write(true).open(&cold).unwrap();
    f.set_len(f.metadata().unwrap().len() - 1).unwrap();
    let catalog = Catalog::open(state.path(), true).unwrap();
    assert_eq!(catalog.rebuild_from_files().unwrap(), 1);
    assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&hot));
}

#[test]
fn cleanup_repairs_cold_only_stale_hot_pointer() {
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    fs::remove_file(&hot).unwrap();
    cleanup_abandoned_scans(state.path(), 123);
    assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&cold));
    assert!(opened(&cold).root().is_ok());
}

#[test]
fn cleanup_sweeps_v2_staging_leaf() {
    let (state, hot) = catalog_fixture();
    let stage = hot.with_extension("db.stage.tmp");
    test_support::private_write(&stage, b"abandoned stage").unwrap();
    cleanup_abandoned_scans(state.path(), 123);
    assert!(!stage.exists());
    assert!(hot.exists());
}

#[test]
fn descriptor_exhaustion_never_leaks_inflate_leaf() {
    const CHILD: &str = "TIDEMARK_COLD_EMFILE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "descriptor_exhaustion_never_leaks_inflate_leaf",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let (tmp, hot) = write_fixture(3);
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let original = rustix::process::getrlimit(rustix::process::Resource::Nofile);
    let mut errors = 0;
    // Probe all setup descriptor boundaries, restoring the limit before inspection.
    for cap in 4..24 {
        rustix::process::setrlimit(
            rustix::process::Resource::Nofile,
            rustix::process::Rlimit {
                current: Some(cap),
                maximum: original.maximum,
            },
        )
        .unwrap();
        let result = Snapshot::open_with_space_guard(&cold, |_| Ok(()));
        rustix::process::setrlimit(rustix::process::Resource::Nofile, original).unwrap();
        errors += usize::from(result.is_err());
        drop(result);
        let inflate = tmp.path().join(".inflate");
        if inflate.exists() {
            assert_eq!(fs::read_dir(inflate).unwrap().count(), 0, "limit {cap}");
        }
    }
    assert!(errors > 0);
}

#[test]
fn review_healthy_cold_corrupt_hot_deleted() {
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    fs::write(&hot, b"invalid SQLite header").unwrap();
    assert!(opened(&cold).root().is_ok());
    cleanup_abandoned_scans(state.path(), 123);
    assert!(cold.exists());
    assert!(!hot.exists());
    assert!(hot.with_extension("db.bad").exists());
    assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&cold));
    assert!(opened(&cold).root().is_ok());
}

#[test]
fn review_rebuild_hides_healthy_cold_behind_corrupt_hot() {
    let (state, hot) = catalog_fixture();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    fs::write(&hot, b"invalid SQLite header").unwrap();
    let catalog = Catalog::open(state.path(), true).unwrap();
    assert_eq!(catalog.rebuild_from_files().unwrap(), 1);
    assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&cold));
}

// Preserve the original frame while changing valid JSON. Optionally recompute the
// checksum to test semantic disagreement independently of accidental corruption.
fn wrong_header(cold: &Path, checksum: bool) {
    let bytes = fs::read(cold).unwrap();
    let n = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + n]).unwrap();
    json["root"]["t_disk"] = 5096.into();
    let json = serde_json::to_vec(&json).unwrap();
    assert_eq!(n, json.len());
    let mut changed = bytes.clone();
    changed[20..20 + n].copy_from_slice(&json);
    if checksum && &bytes[..8] == b"TMCOLD02" {
        changed[20 + n..28 + n].copy_from_slice(&xxhash_rust::xxh3::xxh3_64(&json).to_le_bytes());
    }
    fs::write(cold, changed).unwrap();
}

#[test]
fn review_matching_payload_wrong_header_survives_cleanup() {
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    wrong_header(&cold, true);
    assert!(catalog
        .point_to_cold(&meta().scan_id.to_string(), &cold)
        .is_err());
    cleanup_abandoned_scans(state.path(), 123);
    assert!(hot.exists());
    assert!(!cold.exists());
    assert!(cold.with_extension("zst.bad").exists());
    assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&hot));
    assert_eq!(catalog.rebuild_from_files().unwrap(), 1);
    assert_eq!(catalog.list(None).unwrap()[0].t_disk, Some(4096));
}

#[test]
fn cold_only_rebuild_rejects_header_corruption_and_old_magic() {
    for old_magic in [false, true] {
        let (state, hot) = catalog_fixture();
        let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
        fs::remove_file(&hot).unwrap();
        if old_magic {
            let mut bytes = fs::read(&cold).unwrap();
            bytes[..8].copy_from_slice(b"TMCOLD01");
            fs::write(&cold, bytes).unwrap();
        } else {
            wrong_header(&cold, false);
        }
        assert!(read_cold_header(&cold).is_err());
        let catalog = Catalog::open(state.path(), true).unwrap();
        assert_eq!(catalog.rebuild_from_files().unwrap(), 0);
        assert!(cold.exists());
    }
}

#[test]
fn inconclusive_pairs_preserve_both_bytes_and_catalog() {
    for both_valid in [false, true] {
        let (state, hot) = catalog_fixture();
        let catalog = Catalog::open(state.path(), true).unwrap();
        catalog.rebuild_from_files().unwrap();
        let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
        if both_valid {
            let conn = rusqlite::Connection::open(&hot).unwrap();
            conn.execute("UPDATE dirs SET t_disk=5096 WHERE id=0", [])
                .unwrap();
        } else {
            fs::write(&hot, b"invalid SQLite header").unwrap();
            fs::write(&cold, b"invalid cold header").unwrap();
        }
        let hot_bytes = fs::read(&hot).unwrap();
        let cold_bytes = fs::read(&cold).unwrap();
        cleanup_abandoned_scans(state.path(), 123);
        assert_eq!(fs::read(&hot).unwrap(), hot_bytes);
        assert_eq!(fs::read(&cold).unwrap(), cold_bytes);
        assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&hot));
    }
}

#[test]
fn hot_identity_and_root_are_required_for_rebuild() {
    for wrong_identity in [false, true] {
        let (state, hot) = catalog_fixture();
        let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
        fs::write(cold, b"invalid cold header").unwrap();
        let conn = rusqlite::Connection::open(&hot).unwrap();
        if wrong_identity {
            conn.execute(
                "UPDATE meta SET value='20260101T000000Z-00000001' WHERE key='scan_id'",
                [],
            )
            .unwrap();
        } else {
            // A structurally readable snapshot with a missing root is invalid.
            conn.execute("DELETE FROM dirs WHERE id=0", []).unwrap();
        }
        drop(conn);
        let catalog = Catalog::open(state.path(), true).unwrap();
        assert_eq!(catalog.rebuild_from_files().unwrap(), 0);
    }
}

#[test]
fn cold_frame_must_include_checksum() {
    use std::io::Write;
    let (_tmp, hot) = write_fixture(3);
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let bytes = fs::read(&cold).unwrap();
    let n = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let offset = 20 + n + if &bytes[..8] == b"TMCOLD02" { 8 } else { 0 };
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).unwrap();
    encoder.include_checksum(false).unwrap();
    encoder.write_all(&fs::read(&hot).unwrap()).unwrap();
    let mut bad = bytes[..offset].to_vec();
    bad.extend(encoder.finish().unwrap());
    fs::write(&cold, bad).unwrap();
    assert!(validate_cold(&cold).is_err());
    assert!(Snapshot::open(&cold).is_err());
}

#[test]
fn quick_check_rejects_corruption_outside_meta_and_root() {
    let (state, hot) = catalog_fixture();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    fs::write(cold, b"invalid cold header").unwrap();
    let conn = rusqlite::Connection::open(&hot).unwrap();
    let page: u32 = conn
        .query_row(
            "SELECT rootpage FROM sqlite_master WHERE name='annotations'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let size: u32 = conn
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .unwrap();
    drop(conn);
    let mut bytes = fs::read(&hot).unwrap();
    bytes[((page - 1) * size) as usize] = 0x7f; // invalid b-tree page type
    fs::write(&hot, bytes).unwrap();
    // Opening metadata/root alone misses corruption in an unrelated table.
    assert!(Snapshot::open(&hot).unwrap().root().is_ok());
    let catalog = Catalog::open(state.path(), true).unwrap();
    assert_eq!(catalog.rebuild_from_files().unwrap(), 0);
}

#[test]
fn quarantine_collision_never_overwrites_previous_evidence() {
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    wrong_header(&cold, true);
    let bad = cold.with_extension("zst.bad");
    test_support::private_write(&bad, b"previous evidence").unwrap();
    cleanup_abandoned_scans(state.path(), 123);
    assert!(hot.exists());
    assert!(cold.exists());
    assert_eq!(fs::read(&bad).unwrap(), b"previous evidence");
    assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&hot));
}

#[test]
fn space_refusal_is_inconclusive_for_pair_cleanup() {
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let mut bytes = fs::read(&cold).unwrap();
    // Valid signed-domain length that cannot fit on the fixture filesystem.
    bytes[8..16].copy_from_slice(&(i64::MAX as u64 - 1).to_le_bytes());
    fs::write(&cold, &bytes).unwrap();
    assert!(matches!(Snapshot::open(&cold), Err(StoreError::Space(_))));
    cleanup_abandoned_scans(state.path(), 123);
    assert!(hot.exists());
    assert_eq!(fs::read(&cold).unwrap(), bytes);
    assert!(!cold.with_extension("zst.bad").exists());
    assert_eq!(catalog.list(None).unwrap()[0].file.as_ref(), Some(&hot));
}

#[test]
fn quarantine_collision_does_not_stop_later_cold_pointer_repair() {
    let (state, hot) = catalog_fixture();
    let mut other = meta();
    other.scan_id = serde_json::from_str("\"20260102T000000Z-00000000\"").unwrap();
    let mut writer = SnapshotWriter::create(hot.parent().unwrap(), &other).unwrap();
    let mut root = dir(0);
    root.subtree_end = 0;
    writer.put_dirs(&[root]).unwrap();
    let second = writer.finish(&other).unwrap();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    compress_snapshot(&hot, 1, &|| false).unwrap();
    compress_snapshot(&second, 1, &|| false).unwrap();
    // Pick the collision from the actual directory iteration order, independent of filesystem ordering.
    let cold: Vec<_> = fs::read_dir(hot.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.as_os_str().as_encoded_bytes().ends_with(b".db.zst"))
        .collect();
    assert_eq!(cold.len(), 2);
    wrong_header(&cold[0], true);
    let bad = cold[0].with_extension("zst.bad");
    test_support::private_write(&bad, b"earlier quarantine").unwrap();
    let repair_hot = cold[1].with_extension("");
    fs::remove_file(&repair_hot).unwrap();
    cleanup_abandoned_scans(state.path(), 123);
    assert!(cold[0].exists());
    assert!(cold[0].with_extension("").exists());
    assert_eq!(fs::read(&bad).unwrap(), b"earlier quarantine");
    let repaired = catalog
        .list(None)
        .unwrap()
        .into_iter()
        .find(|e| e.file.as_ref() == Some(&cold[1]));
    assert!(
        repaired.is_some(),
        "later cold-only entry must be repaired in the same cleanup"
    );
}

fn unlimited(pct: f64) -> RetentionConfig {
    RetentionConfig {
        min_free_pct: pct,
        max_state_size: tidemark_core::units::ByteSize(u64::MAX),
        ..RetentionConfig::default()
    }
}

fn floor_reason(state: &Path, length: u64, pct: f64) -> Option<String> {
    match tidemark_store::space_guard::space_check(state, length, &unlimited(pct)).unwrap() {
        tidemark_store::space_guard::SpaceVerdict::Ok => None,
        tidemark_store::space_guard::SpaceVerdict::Refuse { reason }
            if reason.contains("free-space floor") =>
        {
            Some(reason)
        }
        tidemark_store::space_guard::SpaceVerdict::Refuse { reason } => {
            panic!("space check at {pct}% refused {length} bytes for another reason: {reason}")
        }
    }
}

fn filesystem_totals(state: &Path) -> tidemark_store::space_guard::FsTotals {
    let file = fs::File::open(state).unwrap();
    let stat = rustix::fs::fstatvfs(&file).unwrap();
    tidemark_store::space_guard::FsTotals {
        size: stat.f_blocks.saturating_mul(stat.f_frsize),
        avail: stat.f_bavail.saturating_mul(stat.f_frsize),
    }
}

fn refuses_frozen(totals: tidemark_store::space_guard::FsTotals, length: u64, pct: f64) -> bool {
    match tidemark_store::space_guard::space_check_with(|| Ok(totals), 0, length, &unlimited(pct)) {
        tidemark_store::space_guard::SpaceVerdict::Ok => false,
        tidemark_store::space_guard::SpaceVerdict::Refuse { reason }
            if reason.contains("free-space floor") =>
        {
            true
        }
        tidemark_store::space_guard::SpaceVerdict::Refuse { reason } => {
            panic!(
                "frozen space check at {pct}% refused {length} bytes for another reason: {reason}"
            )
        }
    }
}

fn first_floor_refusal(totals: tidemark_store::space_guard::FsTotals, pct: f64) -> Option<u64> {
    let max = i64::MAX as u64 - 1;
    if !refuses_frozen(totals, max, pct) {
        return None;
    }
    let mut lo = 1u64;
    let mut hi = max;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if refuses_frozen(totals, mid, pct) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    refuses_frozen(totals, lo, pct).then_some(lo)
}

/// A declared inflate that `pct` refuses and the 5% default still allows.
///
/// The boundary comes from one filesystem snapshot. Searching the live free
/// space lets a parallel test move the floor and collapse the two policies.
fn length_between(state: &Path, pct: f64) -> Option<u64> {
    for _ in 0..4 {
        let totals = filesystem_totals(state);
        let strict = first_floor_refusal(totals, pct)?;
        let loose = first_floor_refusal(totals, 5.0)?;
        if strict >= loose {
            return None;
        }
        let length = strict + (loose - strict) / 2;
        if floor_reason(state, length, pct).is_some() && floor_reason(state, length, 5.0).is_none()
        {
            return Some(length);
        }
    }
    None
}

fn declare_length(cold: &Path, length: u64) {
    let mut bytes = fs::read(cold).unwrap();
    bytes[8..16].copy_from_slice(&length.to_le_bytes());
    fs::write(cold, bytes).unwrap();
}

fn must_err<T>(result: Result<T, StoreError>) -> StoreError {
    match result {
        Ok(_) => panic!("expected the cold inflate to fail"),
        Err(error) => error,
    }
}

#[test]
fn snapshot_open_cold_path_enforces_configured_min_free_pct() {
    use tidemark_store::diff::{diff_with_retention, explain_with_retention, DiffMetric};
    use tidemark_store::space_guard::{space_check_with, FsTotals, SpaceVerdict};
    let size = 100 << 30;
    let avail = 40 << 30;
    assert!(matches!(
        space_check_with(
            || Ok(FsTotals { size, avail }),
            0,
            1,
            &unlimited(50.0)
        ),
        SpaceVerdict::Refuse { reason } if reason.contains("free-space floor")
    ));
    assert!(matches!(
        space_check_with(|| Ok(FsTotals { size, avail }), 0, 1, &unlimited(5.0)),
        SpaceVerdict::Ok
    ));

    let (state, hot) = catalog_fixture();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let length = length_between(state.path(), 50.0).expect(
        "the 2 GiB absolute floor hides 50% on this filesystem; 50% cannot be separated from 5%",
    );
    declare_length(&cold, length);
    let inflate = cold.parent().unwrap().join(".inflate");
    let strict = unlimited(50.0);
    let loose = unlimited(5.0);
    let expected = floor_reason(state.path(), length, 50.0).unwrap();
    assert!(floor_reason(state.path(), length, 5.0).is_none());

    match tidemark_store::space_guard::inflate_check(&cold, length, &strict) {
        Err(StoreError::Space(reason)) => assert_eq!(reason, expected),
        other => panic!("inflate_check ignored 50%: {other:?}"),
    }
    assert!(tidemark_store::space_guard::inflate_check(&cold, length, &loose).is_ok());

    match must_err(Snapshot::open_with_retention(&cold, &strict)) {
        StoreError::Space(reason) => assert_eq!(reason, expected),
        other => panic!("Snapshot::open cold path ignored 50%: {other}"),
    }
    assert!(
        !inflate.exists(),
        "refusing the free-space floor must not create an inflate leaf"
    );
    match must_err(Snapshot::open_with_retention(&cold, &loose)) {
        StoreError::Invalid(reason) => assert!(reason.contains("length"), "{reason}"),
        other => panic!("5% should pass the guard and reject the declared length: {other}"),
    }

    let new = Snapshot::open(&hot).unwrap();
    let root = meta().root;
    match must_err(diff_with_retention(
        &new,
        &cold,
        &root,
        DiffMetric::Disk,
        1,
        10,
        &strict,
    )) {
        StoreError::Space(reason) => assert_eq!(reason, expected),
        other => panic!("diff inflate ignored 50%: {other}"),
    }
    match must_err(explain_with_retention(
        &new,
        &cold,
        &root,
        DiffMetric::Disk,
        0.5,
        1,
        5,
        &strict,
    )) {
        StoreError::Space(reason) => assert_eq!(reason, expected),
        other => panic!("explain inflate ignored 50%: {other}"),
    }
}

#[test]
fn cleanup_inflate_enforces_configured_min_free_pct() {
    let (state, hot) = catalog_fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let cold = compress_snapshot(&hot, 1, &|| false).unwrap();
    let length = length_between(state.path(), 50.0).expect(
        "the 2 GiB absolute floor hides 50% on this filesystem; 50% cannot be separated from 5%",
    );
    declare_length(&cold, length);
    drop(catalog);
    tidemark_store::catalog::cleanup_abandoned_scans_with_policy(
        state.path(),
        123,
        &unlimited(50.0),
    );
    assert!(hot.exists() && cold.exists());
    assert!(!cold.with_extension("zst.bad").exists());
    tidemark_store::catalog::cleanup_abandoned_scans_with_policy(
        state.path(),
        123,
        &unlimited(5.0),
    );
    assert!(hot.exists());
    assert!(!cold.exists());
    assert!(cold.with_extension("zst.bad").exists());
}
