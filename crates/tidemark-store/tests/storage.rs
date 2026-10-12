//! Owned by: W1-E
//! Snapshot, schema, compaction, retention, and space-policy integration evidence.
#![allow(clippy::unwrap_used)]
use rusqlite::Connection;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};
use tidemark_core::{
    config::*,
    path::{RawName, RawPath},
    scan::{ScanMode, SnapshotStatus},
    snapshot::*,
};
use tidemark_store::{
    catalog::{root_slug, Catalog, ScanEntry},
    reader::Snapshot,
    space_guard::{space_check_with, FsTotals, SpaceVerdict},
    timeseries::TimeSeries,
    writer::SnapshotWriter,
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
    let tmp = tempfile::tempdir().unwrap();
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
#[test]
fn roundtrip_100k_dirs_and_all_query_families() {
    let (_tmp, file) = write_fixture(100000);
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let s = Snapshot::open(&file).unwrap();
    assert_eq!(s.meta(), &meta());
    assert_eq!(s.root().unwrap().subtree_end, 99999);
    for id in [1, 17, 99999] {
        assert_eq!(s.dir(id).unwrap(), dir(id));
        assert_eq!(s.dir_by_path(&dir(id).path).unwrap(), Some(dir(id)));
    }
    let children = s.children(0, SortKey::Name, 100000).unwrap();
    assert_eq!(children.len(), 99999);
    assert_eq!(children[0], dir(1));
    assert_eq!(s.children(0, SortKey::Disk, 1).unwrap(), vec![dir(99999)]);
    assert_eq!(s.subtree_dirs(0, 0, SortKey::Name, 10).unwrap().len(), 1);
    assert_eq!(
        s.subtree_dirs(1, 20, SortKey::Name, 10).unwrap(),
        vec![dir(1)]
    );
    let hits = s.subtree_files(0, &FileFilter::default(), 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].path.is_none());
    assert_eq!(
        hits[1].path.as_ref().unwrap().as_bytes(),
        dir(1)
            .path
            .join(&RawName::new(vec![0xff]).unwrap())
            .as_bytes()
    );
    assert_eq!(
        s.subtree_files(
            0,
            &FileFilter {
                min_size: Some(6000),
                ..Default::default()
            },
            10
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        s.subtree_files(
            0,
            &FileFilter {
                older_than: Some(std::time::Duration::from_secs(100)),
                ..Default::default()
            },
            10
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(s.wide_dirs(0, 7, 10).unwrap().len(), 10);
    let links = s.link_sets_under(0, &LinkFilter::default(), 10).unwrap();
    assert_eq!(links[0].instance_count, 2);
    assert_eq!(s.link_instances(78).unwrap().len(), 2);
    assert_eq!(s.skipped_under(1).unwrap().len(), 1);
    assert_eq!(s.errors_under(1, 10).unwrap().len(), 1);
    assert_eq!(
        s.annotations(1).unwrap(),
        vec![("app.database".into(), "app".into())]
    );
    assert!(s
        .link_sets_under(
            0,
            &LinkFilter {
                boundary: vec!["*".into()],
                boundary_depth: Some(1),
                ..Default::default()
            },
            10
        )
        .is_err());
}
#[test]
fn drop_abort_collision_and_failed_write_never_publish() {
    let tmp = tempfile::tempdir().unwrap();
    let m = meta();
    {
        let _w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    }
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    let mut w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    let mut bad = dir(0);
    bad.ino = u64::MAX;
    assert!(w.put_dirs(&[bad]).is_err());
    assert!(w.finish(&m).is_err());
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    let mut w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    w.space_check_every(
        1,
        Box::new(|| SpaceVerdict::Refuse {
            reason: "test".into(),
        }),
    );
    assert!(w.put_dirs(&[dir(0)]).is_err());
    w.abort();
    let w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    let dest = tmp.path().join(format!("{}.db", m.scan_id));
    std::fs::write(&dest, b"existing").unwrap();
    assert!(w.finish(&m).is_err());
    assert_eq!(std::fs::read(&dest).unwrap(), b"existing");
    assert!(SnapshotWriter::create(tmp.path(), &m).is_err());
}
#[test]
fn schema_matches_document_sqlite_master() {
    let (_tmp, file) = write_fixture(3);
    let actual = Connection::open(file).unwrap();
    let expected = Connection::open_in_memory().unwrap();
    let doc = include_str!("../../../docs/plan/05-storage.md");
    let sql = doc
        .split("```sql\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    expected.execute_batch(sql).unwrap();
    let schema = |conn: &Connection| -> Vec<(String, String, String)> {
        conn.prepare("SELECT type,name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(schema(&actual), schema(&expected));
    let tmp = tempfile::tempdir().unwrap();
    TimeSeries::open(&tmp.path().join("timeseries.db"), true).unwrap();
    let actual = Connection::open(tmp.path().join("timeseries.db")).unwrap();
    let expected = Connection::open_in_memory().unwrap();
    let sql = doc
        .split("```sql\n")
        .find(|b| b.starts_with("CREATE TABLE kv"))
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    expected.execute_batch(sql).unwrap();
    assert_eq!(schema(&actual), schema(&expected));
}
#[test]
fn immutable_reader_ignores_exclusive_sqlite_lock_and_uri_special_bytes() {
    let (tmp, file) = write_fixture(3);
    let odd = tmp.path().join("a?#%.db");
    std::fs::rename(file, &odd).unwrap();
    let locker = Connection::open(&odd).unwrap();
    locker.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let s = Snapshot::open(&odd).unwrap();
    assert_eq!(s.root().unwrap().id, 0);
}
#[test]
fn reader_rejects_corrupt_unsigned_values_and_versions() {
    let (_tmp, file) = write_fixture(3);
    let conn = Connection::open(&file).unwrap();
    conn.execute("UPDATE dirs SET ino=-1 WHERE id=1", [])
        .unwrap();
    let s = Snapshot::open(&file).unwrap();
    assert!(s.dir(1).is_err());
    drop(s);
    conn.execute("UPDATE dirs SET depth=4294967296 WHERE id=1", [])
        .unwrap();
    assert!(Snapshot::open(&file).unwrap().dir(1).is_err());
    conn.pragma_update(None, "user_version", 99).unwrap();
    assert!(Snapshot::open(&file).is_err());
}
#[test]
fn root_slugs_are_injective_including_bytes() {
    let roots = [
        b"/".as_slice(),
        b"/data_reports",
        b"/data/reports",
        b"/data%reports",
        b"/a?b",
        b"/a\xff",
        b"/a_b",
        b"/a/b",
        b"/a%5Fb",
    ];
    let slugs: std::collections::BTreeSet<_> = roots.iter().map(|r| root_slug(&path(r))).collect();
    assert_eq!(slugs.len(), roots.len());
    assert_eq!(root_slug(&path(b"/")), "_");
}

fn entry(id: &str, root: &[u8], ts: i64, status: &str) -> ScanEntry {
    ScanEntry {
        scan_id: id.into(),
        root: path(root),
        mode: ScanMode::Walk,
        started_at: ts,
        finished_at: Some(ts + 10),
        status: status.into(),
        degraded: false,
        file: None,
        t_disk: Some(123),
        t_inodes: Some(7),
        t_dirs: Some(3),
        errors: Some(0),
        duration_ms: Some(10000),
    }
}
#[test]
fn catalog_selection_rebuild_and_nonutf8_file_paths() {
    use std::os::unix::ffi::OsStringExt;
    let state = tempfile::tempdir().unwrap();
    let c = Catalog::open(state.path(), true).unwrap();
    c.record_start(&entry("early", b"/data", 10, "running"))
        .unwrap();
    assert!(c
        .record_start(&entry("early", b"/data", 10, "running"))
        .is_err());
    assert!(c.latest(&path(b"/data")).unwrap().is_none());
    c.record_finish(&entry("early", b"/data", 10, "complete"))
        .unwrap();
    c.record_finish(&entry("late", b"/data", 30, "partial"))
        .unwrap();
    let mut e = entry("other", b"/other", 40, "complete");
    e.file = Some(
        state
            .path()
            .join(std::ffi::OsString::from_vec(b"\xff.db".to_vec())),
    );
    c.record_finish(&e).unwrap();
    assert_eq!(c.get("other").unwrap().unwrap().file, e.file);
    assert_eq!(c.list(None).unwrap().len(), 3);
    assert_eq!(c.list(Some(&path(b"/data"))).unwrap().len(), 2);
    assert_eq!(c.latest(&path(b"/data")).unwrap().unwrap().scan_id, "early");
    assert!(c.nearest_before(&path(b"/data"), 9).unwrap().is_none());
    assert_eq!(
        c.nearest_before(&path(b"/data"), 10)
            .unwrap()
            .unwrap()
            .scan_id,
        "early"
    );
    let (_tmp, file) = write_fixture(3);
    let snapshots = state.path().join("snapshots").join(root_slug(&meta().root));
    std::fs::create_dir_all(&snapshots).unwrap();
    std::fs::copy(file, snapshots.join("valid.db")).unwrap();
    std::fs::write(snapshots.join("bad.db"), b"invalid").unwrap();
    std::fs::write(snapshots.join("partial.db.tmp"), b"invalid").unwrap();
    assert_eq!(c.rebuild_from_files().unwrap(), 1);
    let catalog_sql = Connection::open(state.path().join("timeseries.db")).unwrap();
    let stored_version: String = catalog_sql
        .query_row("SELECT tool_version FROM scans", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored_version, meta().tool_version);

    assert_eq!(
        c.get(&meta().scan_id.to_string()).unwrap().unwrap().t_disk,
        Some(4096)
    );
    assert_eq!(c.rebuild_from_files().unwrap(), 1);
    let readonly = Catalog::open(state.path(), false).unwrap();
    assert_eq!(readonly.list(None).unwrap().len(), 1);
    assert!(readonly
        .record_start(&entry("readonly", b"/data", 1, "running"))
        .is_err());
}
#[test]
fn retention_utc_days_dst_and_iso_weeks_per_root() {
    use tidemark_store::retention::retention_plan;
    let ts = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp();
    let now = ts("2026-01-05T12:00:00Z"); // Monday, across the ISO year boundary.
    let dates = [
        ("new", "2026-01-05T10:00:00Z"),
        ("same", "2026-01-05T09:00:00Z"),
        ("sun", "2026-01-04T23:59:59Z"),
        ("oldweek", "2026-01-01T12:00:00Z"),
        ("older", "2025-12-28T12:00:00Z"),
    ];
    let mut entries: Vec<_> = dates
        .iter()
        .map(|(id, date)| entry(id, b"/data", ts(date), "complete"))
        .collect();
    entries.push(entry(
        "other",
        b"/other",
        ts("2020-01-01T00:00:00Z"),
        "complete",
    ));
    entries.push(entry("failed", b"/data", now - 86411, "failed"));
    entries.push(entry("partial", b"/data", now - 86400, "partial"));
    entries.push(entry("running", b"/data", 0, "running"));
    assert_eq!(
        retention_plan(
            &entries,
            &SnapshotRetention {
                recent: 1,
                daily: 2,
                weekly: 2
            },
            now
        ),
        vec!["failed", "older", "oldweek", "same"]
    );
    assert_eq!(
        retention_plan(
            &entries,
            &SnapshotRetention {
                recent: 0,
                daily: 0,
                weekly: 0
            },
            now
        )
        .len(),
        5
    );
    let dst_now = ts("2026-03-09T00:30:00Z");
    let dst = vec![
        entry("sun", b"/", ts("2026-03-08T23:30:00Z"), "complete"),
        entry("same", b"/", ts("2026-03-08T08:00:00Z"), "complete"),
        entry("sat", b"/", ts("2026-03-07T23:59:00Z"), "complete"),
    ];
    assert_eq!(
        retention_plan(
            &dst,
            &SnapshotRetention {
                recent: 0,
                daily: 2,
                weekly: 0
            },
            dst_now
        ),
        vec!["same", "sat"]
    );
}
#[test]
fn space_guard_decision_table_and_live_state_measurement() {
    use tidemark_store::space_guard::space_check;
    let mut policy = RetentionConfig::default();
    let gb = 1 << 30;
    let cases = [
        (10 * gb, 4 * gb, 0, gb, true),
        (10 * gb, 3 * gb, 0, gb, true),
        (10 * gb, 3 * gb, 0, gb + 1, false),
        (100 * gb, 5 * gb, 0, 1, false),
        (100 * gb, 6 * gb, 0, gb, true),
        (10 * gb, 10 * gb, 20 * gb, 1, false),
    ];
    for (size, avail, used, estimate, ok) in cases {
        assert_eq!(
            matches!(
                space_check_with(|| Ok(FsTotals { size, avail }), used, estimate, &policy),
                SpaceVerdict::Ok
            ),
            ok
        );
    }
    assert!(matches!(
        space_check_with(|| Err(std::io::Error::other("test")), 0, 0, &policy),
        SpaceVerdict::Refuse { .. }
    ));
    assert!(matches!(
        space_check_with(
            || Ok(FsTotals {
                size: u64::MAX,
                avail: u64::MAX
            }),
            u64::MAX,
            1,
            &policy
        ),
        SpaceVerdict::Refuse { .. }
    ));
    policy.min_free_pct = f64::NAN;
    assert!(matches!(
        space_check_with(|| Ok(FsTotals { size: 0, avail: 0 }), 0, 0, &policy),
        SpaceVerdict::Refuse { .. }
    ));
    let temp = tempfile::tempdir().unwrap();
    let policy = RetentionConfig {
        max_state_size: tidemark_core::units::ByteSize(1),
        ..Default::default()
    };
    std::fs::write(temp.path().join("allocated"), vec![1; 8192]).unwrap();
    std::os::unix::fs::symlink("/does/not/exist", temp.path().join("symlink")).unwrap();
    assert!(matches!(
        space_check(temp.path(), 0, &policy).unwrap(),
        SpaceVerdict::Refuse { .. }
    ));
}

fn fs_sample(ts: i64, value: u64) -> tidemark_core::live::FsSample {
    tidemark_core::live::FsSample {
        ts,
        mount: "/data".into(),
        size: 1000,
        used: value,
        avail: 1000 - value,
        inodes: 100,
        inodes_used: value,
    }
}
#[test]
fn samples_roundtrip_checked_inserts_and_readonly_opens() {
    use tidemark_core::live::*;
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("timeseries.db");
    assert!(TimeSeries::open(&db, false).is_err());
    let t = TimeSeries::open(&db, true).unwrap();
    assert_eq!(
        std::fs::metadata(&db).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let fs = fs_sample(10, 20);
    t.insert_fs(std::slice::from_ref(&fs)).unwrap();
    assert_eq!(t.query_fs("/data", 0, 20).unwrap(), vec![fs]);
    let lvm = LvmSample {
        ts: 11,
        vg: "vg".into(),
        lv: "lv".into(),
        lv_size: 1000,
        vg_free: 100,
        data_pct: Some(50.0),
        meta_pct: None,
    };
    t.insert_lvm(std::slice::from_ref(&lvm)).unwrap();
    assert_eq!(t.query_lvm("vg", "lv", 0, 20).unwrap(), vec![lvm]);
    let hot = HotMinute {
        mount: "/data".into(),
        scope: "dir".into(),
        key: vec![b'/', 0xff],
        ts: 13,
        events: 100,
        files: 8,
        bytes_delta: -100,
        lossy: true,
    };
    t.insert_hot_minutes(std::slice::from_ref(&hot)).unwrap();
    assert_eq!(t.query_hot("/data", "dir", 0, 20, 1).unwrap(), vec![hot]);
    let phantom = PhantomSample {
        ts: 14,
        mount: "/data".into(),
        pid: 123,
        comm: "process".into(),
        unit: None,
        path: vec![b'/', 0xff],
        dev: 1,
        ino: 2,
        bytes: 100,
    };
    t.insert_phantom(&[phantom]).unwrap();
    let writer = WriterSample {
        ts: 15,
        mount: "/data".into(),
        pid: 123,
        comm: "process".into(),
        unit: Some("app.service".into()),
        path: vec![b'/', 0xfe],
        dev: 1,
        ino: 2,
        size: 100,
        disk: 512,
    };
    t.insert_writers(&[writer]).unwrap();
    t.insert_alert(&AlertEvent {
        ts: 16,
        rule: "rule".into(),
        subject: "subject".into(),
        state: AlertState::Firing,
        value: Some(1.0),
        message: "test".into(),
    })
    .unwrap();
    t.insert_dir_series(&meta().root, "scan", 17, &[dir(0), dir(1)])
        .unwrap();
    assert_eq!(
        t.query_dir_series(&meta().root, &dir(1).path, 0, 20)
            .unwrap(),
        vec![(17, 4097)]
    );
    assert!(t
        .query_dir_series(&meta().root, &path(b"/missing"), 0, 20)
        .unwrap()
        .is_empty());
    let mut bad = fs_sample(20, 0);
    bad.size = u64::MAX;
    assert!(t.insert_fs(&[fs_sample(19, 1), bad]).is_err());
    assert!(t.query_fs("/data", 19, 20).unwrap().is_empty()); // whole batch rolls back
    let read = TimeSeries::open(&db, false).unwrap();
    assert_eq!(read.query_fs("/data", 0, 20).unwrap().len(), 1);
    assert!(read.insert_fs(&[fs_sample(21, 1)]).is_err());
    let sql = Connection::open(&db).unwrap();
    let data: Vec<u8> = sql
        .query_row("SELECT path FROM phantom_samples", [], |r| r.get(0))
        .unwrap();
    assert_eq!(data, vec![b'/', 0xff]);
    let data: Vec<u8> = sql
        .query_row("SELECT path FROM writer_samples", [], |r| r.get(0))
        .unwrap();
    assert_eq!(data, vec![b'/', 0xfe]);
    sql.execute("UPDATE fs_samples SET used=-1", []).unwrap();
    assert!(read.query_fs("/data", 0, 20).is_err());
}
#[test]
fn migrations_backup_existing_v0_and_refuse_future_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("timeseries.db");
    Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TABLE original (value TEXT);INSERT INTO original VALUES ('preserved');",
        )
        .unwrap();
    TimeSeries::open(&db, true).unwrap();
    let backup = Connection::open(tmp.path().join("timeseries.db.bak-v0")).unwrap();
    assert_eq!(
        backup
            .query_row("SELECT value FROM original", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
    assert_eq!(
        backup
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        std::fs::metadata(tmp.path().join("timeseries.db.bak-v0"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let sql = Connection::open(&db).unwrap();
    sql.pragma_update(None, "user_version", 999).unwrap();
    assert!(TimeSeries::open(&db, true).is_err());
    assert!(TimeSeries::open(&db, false).is_err());
}
#[test]
fn compaction_last_values_idempotence_and_incomplete_buckets() {
    use tidemark_core::live::LvmSample;
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("timeseries.db");
    let t = TimeSeries::open(&db, true).unwrap();
    let policy = SampleRetention::default();
    let now = 10_000_000;
    // Older than five-minute retention: both tiers should be created.
    let old = now - 4_000_000;
    let bucket = old - old % 3600;
    t.insert_fs(&[
        fs_sample(bucket + 10, 1),
        fs_sample(bucket + 299, 2),
        fs_sample(bucket + 301, 3),
    ])
    .unwrap();
    let lvm = |ts, value| LvmSample {
        ts,
        vg: "vg".into(),
        lv: "lv".into(),
        lv_size: value,
        vg_free: 1,
        data_pct: None,
        meta_pct: Some(1.0),
    };
    t.insert_lvm(&[
        lvm(bucket + 10, 1),
        lvm(bucket + 299, 2),
        lvm(bucket + 301, 3),
    ])
    .unwrap();
    // A partially expired five-minute bucket must not be frozen by OR IGNORE.
    let cutoff = now - i64::try_from(policy.raw.as_secs()).unwrap();
    let partial = cutoff - cutoff % 300;
    t.insert_fs(&[
        fs_sample(partial + 1, 4),
        fs_sample(partial + 250, 5),
        fs_sample(now, 6),
    ])
    .unwrap();
    let changed = t.compact(now, &policy).unwrap();
    assert!(changed.inserted >= 6);
    assert!(changed.deleted >= 6);
    let sql = Connection::open(&db).unwrap();
    assert_eq!(
        sql.query_row(
            "SELECT used FROM fs_samples WHERE res=2 AND ts=?",
            [bucket],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    assert_eq!(
        sql.query_row(
            "SELECT lv_size FROM lvm_samples WHERE res=2 AND ts=?",
            [bucket],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    assert_eq!(
        t.compact(now, &policy).unwrap(),
        tidemark_store::timeseries::CompactStats::default()
    );
    assert_eq!(t.query_fs("/data", 0, now).unwrap().last().unwrap().used, 6);
    t.compact(now + 600, &policy).unwrap();
    assert_eq!(
        sql.query_row(
            "SELECT used FROM fs_samples WHERE res=1 AND ts=?",
            [partial],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        5
    );
    t.compact(now + 40_000_000, &policy).unwrap();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM fs_samples", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn depth_boundary_selects_crossing_sets_and_no_instances_is_honest() {
    let (_tmp, file) = write_fixture(3);
    let s = Snapshot::open(&file).unwrap();
    assert_eq!(
        s.link_sets_under(
            0,
            &LinkFilter {
                boundary_depth: Some(1),
                ..Default::default()
            },
            10
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        s.link_sets_under(
            0,
            &LinkFilter {
                boundary_depth: Some(0),
                ..Default::default()
            },
            10
        )
        .unwrap()
        .len(),
        0
    );
    drop(s);
    let sql = Connection::open(&file).unwrap();
    sql.execute("DELETE FROM link_instances", []).unwrap();
    let s = Snapshot::open(&file).unwrap();
    assert_eq!(
        s.link_sets_under(0, &LinkFilter::default(), 10).unwrap()[0].instance_count,
        0
    );
}
#[test]
#[ignore = "scale"]
fn insert_2m_directory_rows_under_60_seconds() {
    let tmp = tempfile::tempdir().unwrap();
    let m = meta();
    let mut w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    let start = std::time::Instant::now();
    for batch in 0..200 {
        let rows: Vec<_> = (batch * 10000..(batch + 1) * 10000)
            .map(|i| {
                let mut d = dir(i);
                if i == 0 {
                    d.subtree_end = 1999999;
                }
                d
            })
            .collect();
        w.put_dirs(&rows).unwrap();
    }
    let elapsed = start.elapsed();
    eprintln!("2M directory insert duration: {elapsed:?}");
    assert!(elapsed.as_secs_f64() < 60.0, "2M insert: {elapsed:?}");
    w.finish(&m).unwrap();
}
#[cfg(feature = "privileged")]
#[test]
fn snapshot_xfs_publication_owner_permissions_and_sync() {
    use std::os::unix::fs::MetadataExt;
    let xfs = test_support::xfs::LoopXfs::new(1 << 30, &[], "");
    let m = meta();
    let mut w = SnapshotWriter::create(&xfs.mountpoint, &m).unwrap();
    w.put_dirs(&[dir(0)]).unwrap();
    let file = w.finish(&m).unwrap();
    let md = std::fs::metadata(&file).unwrap();
    assert_eq!(md.uid(), 0);
    assert_eq!(md.gid(), 0);
    assert_eq!(md.mode() & 0o777, 0o600);
    assert_eq!(Snapshot::open(&file).unwrap().root().unwrap().id, 0);
}

#[test]
fn series_keeps_largest_20000_rows_with_byte_path_ties() {
    let tmp = tempfile::tempdir().unwrap();
    let t = TimeSeries::open(&tmp.path().join("timeseries.db"), true).unwrap();
    let rows: Vec<_> = (0..30000).map(dir).collect();
    t.insert_dir_series(&meta().root, "scale-series", 1, &rows)
        .unwrap();
    assert!(t
        .query_dir_series(&meta().root, &dir(9999).path, 0, 2)
        .unwrap()
        .is_empty());
    assert_eq!(
        t.query_dir_series(&meta().root, &dir(10000).path, 0, 2)
            .unwrap(),
        vec![(1, 14096)]
    );
    let sql = Connection::open(tmp.path().join("timeseries.db")).unwrap();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM dir_series", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        20000
    );
}
#[test]
fn mixed_sample_resolutions_keep_old_and_recent_history_for_short_ranges() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("timeseries.db");
    let t = TimeSeries::open(&db, true).unwrap();
    let sql = Connection::open(db).unwrap();
    sql.execute(
        "INSERT INTO fs_samples VALUES ('/data',1,0,100,10,90,100,10)",
        [],
    )
    .unwrap();
    t.insert_fs(&[fs_sample(400, 20)]).unwrap();
    let values = t.query_fs("/data", 0, 500).unwrap();
    assert_eq!(
        values.iter().map(|s| s.used).collect::<Vec<_>>(),
        vec![10, 20]
    );
}

#[test]
fn exclusive_lock_holder_child() {
    if let Some(file) = std::env::var_os("TIDEMARK_STORE_LOCK_PATH") {
        let conn = Connection::open(std::path::PathBuf::from(file)).unwrap();
        conn.execute_batch("BEGIN EXCLUSIVE").unwrap();
        println!("READY");
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
#[test]
fn immutable_reader_opens_under_another_process_exclusive_lock() {
    use std::{
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
    };
    struct Holder(Child);
    impl Drop for Holder {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (_tmp, file) = write_fixture(3);
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "exclusive_lock_holder_child", "--nocapture"])
        .env("TIDEMARK_STORE_LOCK_PATH", &file)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut holder = Holder(child);
    let output = holder.0.stdout.take().unwrap();
    let ready = BufReader::new(output)
        .lines()
        .any(|line| line.unwrap() == "READY");
    assert!(ready);
    assert_eq!(Snapshot::open(&file).unwrap().root().unwrap().id, 0);
}
#[cfg(feature = "privileged")]
#[test]
fn state_usage_skips_same_device_bind_mount() {
    use std::{os::unix::fs::MetadataExt, process::Command};
    use tidemark_store::space_guard::space_check;
    let xfs = test_support::xfs::LoopXfs::new(1 << 30, &[], "");
    let state = xfs.mountpoint.join("state");
    let outside = xfs.mountpoint.join("outside");
    std::fs::create_dir(&state).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let bind = state.join("bind");
    std::fs::create_dir(&bind).unwrap();
    std::fs::write(outside.join("large"), vec![1; 1024 * 1024]).unwrap();
    assert!(Command::new("mount")
        .arg("--bind")
        .arg(&outside)
        .arg(&bind)
        .status()
        .unwrap()
        .success());
    // If the bind mount were traversed, state allocation would exceed this cap.
    let cap = std::fs::metadata(&state).unwrap().blocks() * 512 + 4096;
    let policy = RetentionConfig {
        min_free_pct: 0.0,
        max_state_size: tidemark_core::units::ByteSize(cap),
        ..Default::default()
    };
    let result = space_check(&state, 0, &policy);
    assert!(Command::new("umount")
        .arg(&bind)
        .status()
        .unwrap()
        .success());
    // The small loop filesystem necessarily violates the mandatory 2-GiB floor;
    // ensure it did not instead violate the state-allocation cap.
    assert!(
        matches!(result.unwrap(),SpaceVerdict::Refuse{reason} if reason.contains("free-space floor"))
    );
}

#[test]
fn retention_preserves_newest_with_expired_windows_for_each_root() {
    let entries = vec![
        entry("a", b"/a", 0, "complete"),
        entry("b", b"/b", 1, "complete"),
        entry("older", b"/b", 0, "complete"),
    ];
    for policy in [
        SnapshotRetention {
            recent: 0,
            daily: 14,
            weekly: 8,
        },
        SnapshotRetention {
            recent: 0,
            daily: 0,
            weekly: 0,
        },
    ] {
        assert_eq!(
            tidemark_store::retention::retention_plan(&entries, &policy, 100 * 86400),
            vec!["older"]
        );
    }
}

#[test]
fn glob_boundaries_match_raw_bytes_and_nested_regions() {
    let (_tmp, file) = write_fixture(3);
    let check = |patterns: &[&str], expected| {
        let snapshot = Snapshot::open(&file).unwrap();
        let filter = LinkFilter {
            boundary: patterns.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        assert_eq!(
            snapshot.link_sets_under(0, &filter, 10).unwrap().len(),
            expected
        );
    };
    check(&["/data/*/*"], 1); // invalid UTF-8 root component matches as bytes
    check(&["/missing/*"], 0);
    check(&["/data/�/*"], 0); // replacement characters must not match invalid bytes
    check(&["/data/*"], 0); // both instances share the same region
    check(&["/data/*", "/data/*/00000001"], 1); // nested region and parent
    check(&["/data/*/*", "/data/*/00000001"], 1); // overlapping globs
    check(&["/data/*/00000001"], 1); // matched versus outside
    let snapshot = Snapshot::open(&file).unwrap();
    assert!(snapshot
        .link_sets_under(
            0,
            &LinkFilter {
                boundary: vec!["[".into()],
                ..Default::default()
            },
            10
        )
        .is_err());
    assert!(snapshot
        .link_sets_under(
            0,
            &LinkFilter {
                boundary: vec!["*".into()],
                boundary_depth: Some(1),
                ..Default::default()
            },
            10
        )
        .is_err());
    assert!(snapshot
        .link_sets_under(
            0,
            &LinkFilter {
                boundary: vec!["/data/*/*".into()],
                ..Default::default()
            },
            0
        )
        .unwrap()
        .is_empty());
}

#[test]
fn recover_corrupt_catalog_preserves_database_and_sidecars() {
    let state = tempfile::tempdir().unwrap();
    let snapshots = state.path().join("snapshots").join(root_slug(&meta().root));
    let mut writer = SnapshotWriter::create(&snapshots, &meta()).unwrap();
    let mut root = dir(0);
    root.subtree_end = 0;
    writer.put_dirs(&[root]).unwrap();
    writer.finish(&meta()).unwrap();
    for name in ["timeseries.db", "timeseries.db-wal", "timeseries.db-shm"] {
        std::fs::write(state.path().join(name), name.as_bytes()).unwrap();
    }
    for rw in [false, true] {
        for error in [
            Catalog::open(state.path(), rw).err().unwrap(),
            TimeSeries::open(&state.path().join("timeseries.db"), rw)
                .err()
                .unwrap(),
        ] {
            assert!(matches!(
                error,
                tidemark_store::StoreError::Sql(rusqlite::Error::SqliteFailure(e, _))
                    if e.code == rusqlite::ErrorCode::NotADatabase
            ));
        }
        for name in ["timeseries.db", "timeseries.db-wal", "timeseries.db-shm"] {
            assert_eq!(
                std::fs::read(state.path().join(name)).unwrap(),
                name.as_bytes()
            );
        }
    }
    let catalog = tidemark_store::catalog::recover_catalog(state.path()).unwrap();
    assert_eq!(catalog.list(None).unwrap().len(), 1);
    assert_eq!(catalog.recovery_files().len(), 3);
    for file in catalog.recovery_files() {
        assert_eq!(
            std::fs::read(file).unwrap(),
            file.file_name().unwrap().as_encoded_bytes()
        );
    }
    drop(catalog);
    let second = tidemark_store::catalog::recover_catalog(state.path()).unwrap();
    assert!(second.recovery_files()[0]
        .parent()
        .unwrap()
        .ends_with("timeseries-recovery-1"));
}

#[test]
fn space_floor_does_not_round_down_large_sizes() {
    let mut policy = RetentionConfig {
        min_free_pct: 100.0,
        ..Default::default()
    };
    policy.max_state_size.0 = u64::MAX;
    assert!(matches!(
        space_check_with(
            || Ok(FsTotals {
                size: (1 << 62) + 512,
                avail: 1 << 62
            }),
            0,
            0,
            &policy
        ),
        SpaceVerdict::Refuse { .. }
    ));
    policy.min_free_pct = 12.5;
    assert!(matches!(
        space_check_with(
            || Ok(FsTotals {
                size: (1 << 62) + 1,
                avail: 1 << 59
            }),
            0,
            0,
            &policy
        ),
        SpaceVerdict::Refuse { .. }
    ));
}

#[test]
fn retained_400_day_history_returns_all_tiers() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("timeseries.db");
    let store = TimeSeries::open(&db, true).unwrap();
    let mut sql = Connection::open(&db).unwrap();
    let tx = sql.transaction().unwrap();
    let mut expected = Vec::new();
    for (res, start, count, step) in [
        (2, 0, 8760, 3600),
        (1, 365 * 86400, 9504, 300),
        (0, 398 * 86400, 5760, 30),
    ] {
        for index in 0..count {
            let ts = start + index * step;
            expected.push(ts);
            tx.execute(
                "INSERT INTO fs_samples VALUES ('/data',?,?,100,10,90,100,10)",
                rusqlite::params![res, ts],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO lvm_samples VALUES ('vg','lv',?,?,100,10,NULL,NULL)",
                rusqlite::params![res, ts],
            )
            .unwrap();
        }
    }
    tx.commit().unwrap();
    assert_eq!(expected.len(), 24024);
    assert_eq!(
        store
            .query_fs("/data", 0, 400 * 86400)
            .unwrap()
            .iter()
            .map(|r| r.ts)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        store
            .query_lvm("vg", "lv", 0, 400 * 86400)
            .unwrap()
            .iter()
            .map(|r| r.ts)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn overlapping_sample_tiers_preserve_selection_for_all_query_families() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("timeseries.db");
    let store = TimeSeries::open(&db, true).unwrap();
    let sql = Connection::open(db).unwrap();
    for (res, ts) in [
        (0, 20),
        (0, 400),
        (1, 0),
        (1, 300),
        (1, 600),
        (2, 0),
        (2, 3600),
    ] {
        sql.execute(
            "INSERT INTO fs_samples VALUES ('/data',?,?,100,10,90,100,10)",
            rusqlite::params![res, ts],
        )
        .unwrap();
        sql.execute(
            "INSERT INTO lvm_samples VALUES ('vg','lv',?,?,100,10,NULL,NULL)",
            rusqlite::params![res, ts],
        )
        .unwrap();
    }
    for (to, expected) in [
        (4000, vec![20, 400, 600, 3600]),
        (3 * 86400, vec![0, 300, 600, 3600]),
        (40 * 86400, vec![0, 3600]),
    ] {
        assert_eq!(
            store
                .query_fs("/data", 0, to)
                .unwrap()
                .iter()
                .map(|r| r.ts)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            store
                .query_lvm("vg", "lv", 0, to)
                .unwrap()
                .iter()
                .map(|r| r.ts)
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn timeseries_header_preflight_handles_empty_short_and_valid_headers() {
    let state = tempfile::tempdir().unwrap();
    let db = state.path().join("timeseries.db");
    std::fs::write(&db, []).unwrap();
    drop(TimeSeries::open(&db, true).unwrap());
    drop(TimeSeries::open(&db, false).unwrap());

    for bytes in [
        b"S".as_slice(),
        b"SQLite format 3".as_slice(),
        b"garbage database bytes".as_slice(),
    ] {
        std::fs::write(&db, bytes).unwrap();
        assert!(matches!(
            TimeSeries::open(&db, true).err().unwrap(),
            tidemark_store::StoreError::Sql(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::NotADatabase
        ));
        assert_eq!(std::fs::read(&db).unwrap(), bytes);
    }

    // A valid header with a truncated first page must reach SQLite.
    let db = state.path().join("damaged.db");
    let mut damaged = vec![0; 100];
    damaged[..16].copy_from_slice(b"SQLite format 3\0");
    damaged[16..18].copy_from_slice(&4096_u16.to_be_bytes());
    damaged[18] = 1;
    damaged[19] = 1;
    damaged[21..24].copy_from_slice(&[64, 32, 32]);
    damaged[28..32].copy_from_slice(&1_u32.to_be_bytes());
    std::fs::write(&db, &damaged).unwrap();
    let direct = Connection::open(&db)
        .unwrap()
        .execute_batch("SELECT name FROM sqlite_master;")
        .unwrap_err();
    let actual = TimeSeries::open(&db, true).err().unwrap();
    match (actual, direct) {
        (
            tidemark_store::StoreError::Sql(rusqlite::Error::SqliteFailure(actual, _)),
            rusqlite::Error::SqliteFailure(expected, _),
        ) => {
            assert_eq!(actual, expected);
            assert_eq!(actual.code, rusqlite::ErrorCode::DatabaseCorrupt);
        }
        errors => panic!("unexpected errors: {errors:?}"),
    }
}

#[test]
fn finish_cancels_at_every_publication_checkpoint() {
    use std::cell::Cell;
    // One before indexes, seven after individual indexes, one before metadata,
    // and one immediately before rename. Successful finish must not check afterwards.
    const CHECKPOINTS: usize = 10;
    for cancel_at in 1..=CHECKPOINTS + 1 {
        let tmp = tempfile::tempdir().unwrap();
        let old_meta = meta();
        let existing = SnapshotWriter::create(tmp.path(), &old_meta)
            .unwrap()
            .finish(&old_meta)
            .unwrap();
        let old_bytes = std::fs::read(&existing).unwrap();
        let mut m = old_meta.clone();
        m.scan_id = serde_json::from_str("\"20260101T000001Z-00000000\"").unwrap();
        let writer = SnapshotWriter::create(tmp.path(), &m).unwrap();
        let published = tmp.path().join(format!("{}.db", m.scan_id));
        let temporary = tmp.path().join(format!("{}.db.tmp", m.scan_id));
        let calls = Cell::new(0);
        let result = writer.finish_with(&m, &|| {
            calls.set(calls.get() + 1);
            calls.get() == cancel_at
        });
        if cancel_at <= CHECKPOINTS {
            assert!(matches!(result, Err(tidemark_store::StoreError::Cancelled)));
            assert!(!published.exists());
            assert_eq!(calls.get(), cancel_at);
        } else {
            assert_eq!(result.unwrap(), published);
            assert_eq!(calls.get(), CHECKPOINTS);
            assert_eq!(
                Snapshot::open(&published).unwrap().meta().status,
                SnapshotStatus::Complete
            );
        }
        assert!(!temporary.exists());
        assert_eq!(std::fs::read(&existing).unwrap(), old_bytes);
    }
}

#[test]
fn catalog_paths_are_relative_and_resolve_for_each_reader() {
    use std::os::{
        fd::AsRawFd,
        unix::ffi::{OsStrExt, OsStringExt},
    };
    let state = tempfile::tempdir().unwrap();
    let handle = std::fs::File::open(state.path()).unwrap();
    let anchored = std::path::PathBuf::from(format!("/proc/self/fd/{}/.", handle.as_raw_fd()));
    let plain = Catalog::open(state.path(), true).unwrap();
    let bound = Catalog::open(&anchored, true).unwrap();
    let tail = std::path::PathBuf::from("snapshots")
        .join(std::ffi::OsString::from_vec(b"root-\xff".to_vec()))
        .join(std::ffi::OsString::from_vec(b"scan-\xfe.db".to_vec()));
    std::fs::create_dir_all(state.path().join(&tail).parent().unwrap()).unwrap();
    let (_fixture, snapshot) = write_fixture(3);
    std::fs::copy(snapshot, state.path().join(&tail)).unwrap();
    let sql = Connection::open(state.path().join("timeseries.db")).unwrap();
    for (id, writer, base) in [
        ("plain", &plain, state.path()),
        ("bound", &bound, anchored.as_path()),
    ] {
        let mut e = entry(id, b"/data", 10, "complete");
        e.file = Some(base.join(".").join(&tail));
        writer.record_start(&e).unwrap();
        writer.record_finish(&e).unwrap();
        let raw: Vec<u8> = sql
            .query_row(
                "SELECT CAST(file AS BLOB) FROM scans WHERE scan_id=?",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, tail.as_os_str().as_bytes());
        for (reader, base) in [(&plain, state.path()), (&bound, anchored.as_path())] {
            for read in [
                reader.get(id).unwrap().unwrap(),
                reader.latest(&e.root).unwrap().unwrap(),
                reader.nearest_before(&e.root, 10).unwrap().unwrap(),
            ] {
                assert_eq!(read.file, Some(base.join(&tail)));
                let file = read.file.unwrap();
                assert!(file.exists());
                assert!(Snapshot::open(&file).is_ok());
            }
            assert!(reader
                .list(None)
                .unwrap()
                .iter()
                .all(|e| e.file.as_ref() == Some(&base.join(&tail))));
        }
    }
}

#[test]
fn catalog_rejects_outside_and_parent_paths() {
    let state = tempfile::tempdir().unwrap();
    let c = Catalog::open(state.path(), true).unwrap();
    for file in [
        state.path().with_extension("outside").join("scan.db"),
        state.path().join("snapshots/../scan.db"),
        std::path::PathBuf::from("snapshots/scan.db"),
    ] {
        let mut e = entry("bad", b"/data", 10, "complete");
        e.file = Some(file);
        assert!(matches!(
            c.record_start(&e),
            Err(tidemark_store::StoreError::Invalid(_))
        ));
        assert!(matches!(
            c.record_finish(&e),
            Err(tidemark_store::StoreError::Invalid(_))
        ));
    }
    assert!(c.list(None).unwrap().is_empty());
}

#[test]
fn catalog_legacy_paths_and_rebuild_repair() {
    use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};
    let state = tempfile::tempdir().unwrap();
    let c = Catalog::open(state.path(), true).unwrap();
    let (_fixture, snapshot) = write_fixture(3);
    let tail = std::path::PathBuf::from("snapshots/root/valid.db");
    let published = state.path().join(&tail);
    std::fs::create_dir_all(published.parent().unwrap()).unwrap();
    std::fs::copy(snapshot, &published).unwrap();
    let sql = Connection::open(state.path().join("timeseries.db")).unwrap();
    let existing = state.path().join("legacy.db");
    std::fs::copy(&published, &existing).unwrap();
    for (id, legacy, expected) in [
        ("existing", existing.clone(), existing),
        (
            "dead",
            std::path::PathBuf::from("/proc/self/fd/999999/./snapshots/root/valid.db"),
            published.clone(),
        ),
        (
            "last",
            std::path::PathBuf::from("/missing/snapshots/unused/snapshots/root/valid.db"),
            published.clone(),
        ),
        (
            "missing",
            std::path::PathBuf::from("/missing/snapshots/root/absent.db"),
            std::path::PathBuf::from("/missing/snapshots/root/absent.db"),
        ),
    ] {
        c.record_start(&entry(id, b"/data", 10, "complete"))
            .unwrap();
        sql.execute(
            "UPDATE scans SET file=CAST(? AS TEXT) WHERE scan_id=?",
            rusqlite::params![legacy.as_os_str().as_bytes(), id],
        )
        .unwrap();
        assert_eq!(c.get(id).unwrap().unwrap().file, Some(expected.clone()));
        assert_eq!(
            c.latest(&path(b"/data")).unwrap().unwrap().file,
            Some(expected.clone())
        );
        assert_eq!(
            c.nearest_before(&path(b"/data"), 10).unwrap().unwrap().file,
            Some(expected.clone())
        );
        assert_eq!(c.list(None).unwrap()[0].file, Some(expected));
        sql.execute("DELETE FROM scans", []).unwrap();
    }
    let handle = std::fs::File::open(state.path()).unwrap();
    let anchored = std::path::PathBuf::from(format!("/proc/self/fd/{}/.", handle.as_raw_fd()));
    c.record_start(&entry(
        &meta().scan_id.to_string(),
        b"/data",
        10,
        "complete",
    ))
    .unwrap();
    sql.execute(
        "UPDATE scans SET file='/proc/self/fd/999999/./snapshots/root/valid.db'",
        [],
    )
    .unwrap();
    let bound = Catalog::open(&anchored, true).unwrap();
    assert_eq!(bound.rebuild_from_files().unwrap(), 1);
    let raw: Vec<u8> = sql
        .query_row("SELECT CAST(file AS BLOB) FROM scans", [], |r| r.get(0))
        .unwrap();
    assert_eq!(raw, tail.as_os_str().as_bytes());
    assert!(Snapshot::open(c.list(None).unwrap()[0].file.as_ref().unwrap()).is_ok());
}
