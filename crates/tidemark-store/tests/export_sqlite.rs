//! Owned by: SNAP-export-sqlite
//! Export of v1, v2 and cold snapshots as v1-schema SQLite databases.
#![allow(clippy::unwrap_used)]
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path};
use tidemark_core::{
    config::*,
    path::{RawName, RawPath},
    scan::{ErrOp, ScanMode, SkipReason, SnapshotStatus},
    snapshot::*,
};
use tidemark_store::{
    cold::compress_snapshot,
    export_sqlite,
    writer::SnapshotWriter,
    writer_v2::{SnapshotWriterV2, WriterOptions},
    Snapshot, StoreError,
};

fn path(bytes: &[u8]) -> RawPath {
    RawPath::new(bytes.to_vec()).unwrap()
}
fn name(bytes: &[u8]) -> RawName {
    RawName::new(bytes.to_vec()).unwrap()
}
fn meta(version: u32) -> SnapshotMeta {
    SnapshotMeta {
        schema_version: version,
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
        host: "h".into(),
        kernel: String::new(),
        os_release: String::new(),
        fs_uuid: String::new(),
        fs_dev: 7,
        fs_blocksize: 4096,
        threads: 2,
        throttle_profile: String::new(),
        dirs: 6,
        files: 4,
        inodes: 9,
        errors: 1,
        races: 0,
        skipped: 1,
        noatime_fallbacks: 0,
        phase_ms: BTreeMap::new(),
        gate_stats: Default::default(),
    }
}
const DIRS: u32 = 6;
fn dir(id: u32) -> DirRow {
    let root = path(b"/data/\xff");
    let p = if id == 0 {
        root.clone()
    } else {
        root.join(&name(format!("d{id:03}").as_bytes()))
    };
    let n = u64::from(id);
    DirRow {
        id,
        parent_id: (id != 0).then_some(0),
        subtree_end: if id == 0 { DIRS - 1 } else { id },
        depth: u32::from(id != 0),
        name: if id == 0 {
            p.as_bytes().to_vec()
        } else {
            format!("d{id:03}").into_bytes()
        },
        path_hash: p.hash64(),
        path: p,
        ino: n + 100,
        mtime: 5 + i64::from(id),
        dir_disk: 4096,
        dir_apparent: 6,
        projid: id.is_multiple_of(2).then_some(id),
        entries: 3,
        errors: id % 2,
        own_files: n + 1,
        own_symlinks: 0,
        own_charged: n + 4096,
        own_apparent: n + 6,
        t_dirs: 6,
        t_files: n + 8,
        t_inodes: n + 9,
        t_disk: 4096 * (n + 1),
        t_apparent: n + 10,
        t_charged: n + 11,
        t_reclaimable: n + 12,
        t_errors: 1,
        age0: 1,
        age1: 2,
        age2: 3,
        age3: 4,
        age4: 5,
        age5: 6,
        newest_mtime: 50 + i64::from(id),
        max_entries: 3,
    }
}
fn files() -> Vec<FileRow> {
    let f = |id, dir_id, n: Option<&[u8]>, ino| FileRow {
        id,
        dir_id,
        name: n.map(name),
        ino,
        disk: 8192,
        apparent: 100,
        mtime: 9,
        nlink: 1,
    };
    vec![
        f(0, 0, Some(b"a"), 1000),
        f(1, 1, Some(b"b\xfe"), 1001),
        f(2, 1, Some(b"c"), 1002),
        f(3, 3, None, 1003),
    ]
}
fn link_sets() -> Vec<LinkSetRow> {
    vec![
        LinkSetRow {
            ino: 2000,
            nlink: 2,
            found: 2,
            complete: true,
            disk: 4096,
            apparent: 10,
            mtime: 3,
            lca_dir_id: 0,
        },
        LinkSetRow {
            ino: 2001,
            nlink: 3,
            found: 2,
            complete: false,
            disk: 4096,
            apparent: 10,
            mtime: 3,
            lca_dir_id: 2,
        },
    ]
}
fn instances() -> Vec<LinkInstanceRow> {
    let i = |ino, dir_id, n: Option<&[u8]>| LinkInstanceRow {
        ino,
        dir_id,
        name: n.map(name),
    };
    vec![
        i(2000, 1, Some(b"x")),
        i(2000, 2, Some(b"y")),
        i(2001, 2, Some(b"p")),
        i(2001, 4, Some(b"q")),
    ]
}
fn skipped() -> Vec<SkipRow> {
    vec![SkipRow {
        dir_id: 2,
        name: name(b"mnt"),
        reason: SkipReason::Mount,
    }]
}
fn errors() -> Vec<ErrorRow> {
    vec![ErrorRow {
        dir_id: Some(1),
        path: path(b"/data/\xff/d001/bad"),
        op: ErrOp::Statx,
        errno: 13,
    }]
}
fn annotations() -> Vec<AnnotationRow> {
    vec![AnnotationRow {
        dir_id: 1,
        key: "k".into(),
        value: "v".into(),
    }]
}
fn dirs() -> Vec<DirRow> {
    (0..DIRS).map(dir).collect()
}
fn write_v1(dir: &Path) -> std::path::PathBuf {
    let m = meta(1);
    let mut w = SnapshotWriter::create(dir, &m).unwrap();
    w.put_dirs(&dirs()).unwrap();
    w.put_files(&files()).unwrap();
    w.put_link_sets(&link_sets()).unwrap();
    w.put_link_instances(&instances()).unwrap();
    w.put_skipped(&skipped()).unwrap();
    w.put_errors(&errors()).unwrap();
    w.put_annotations(&annotations()).unwrap();
    w.finish(&m).unwrap()
}
fn write_v2(dir: &Path) -> std::path::PathBuf {
    let m = meta(2);
    let mut w = SnapshotWriterV2::create_with_options(
        dir,
        &m,
        WriterOptions {
            plain_depth: 1,
            chunk_dirs: 2,
            ..Default::default()
        },
    )
    .unwrap();
    w.put_dirs(&dirs()).unwrap();
    w.put_files(&files()).unwrap();
    w.put_link_sets(&link_sets()).unwrap();
    w.put_link_instances(&instances()).unwrap();
    w.put_skipped(&skipped()).unwrap();
    w.put_errors(&errors()).unwrap();
    w.put_annotations(&annotations()).unwrap();
    w.finish(&m).unwrap()
}

/// Every row of every table, in a deterministic order, as debug strings.
fn dump(db: &Path) -> BTreeMap<&'static str, Vec<String>> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut out = BTreeMap::new();
    for (table, order) in [
        ("meta", "key"),
        ("dirs", "id"),
        ("files", "id"),
        ("link_sets", "ino"),
        ("link_instances", "ino,dir_id,name"),
        ("skipped", "dir_id,name,reason"),
        ("errors", "path,op,errno"),
        ("annotations", "dir_id,key,value"),
    ] {
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
            .unwrap();
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |r| {
                Ok(format!(
                    "{:?}",
                    (0..columns)
                        .map(|i| r.get::<_, rusqlite::types::Value>(i).unwrap())
                        .collect::<Vec<_>>()
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        out.insert(table, rows);
    }
    out
}
fn user_version(db: &Path) -> i64 {
    rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}
fn indexes(db: &Path) -> Vec<String> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn v1_and_v2_exports_have_identical_rows_and_v1_shape() {
    let a = test_support::private_tempdir().unwrap();
    let b = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let v1 = Snapshot::open(&write_v1(a.path())).unwrap();
    let v2 = Snapshot::open(&write_v2(b.path())).unwrap();
    let (o1, o2) = (out.path().join("one.db"), out.path().join("two.db"));
    let s1 = export_sqlite(&v1, &o1).unwrap();
    let s2 = export_sqlite(&v2, &o2).unwrap();
    assert_eq!(s1, s2);
    assert_eq!(
        (s1.dirs, s1.files, s1.link_sets, s1.link_instances),
        (6, 4, 2, 4)
    );
    assert_eq!((s1.skipped, s1.errors, s1.annotations), (1, 1, 1));
    for o in [&o1, &o2] {
        assert_eq!(user_version(o), 1);
        assert_eq!(fs::metadata(o).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(indexes(o).contains(&"dirs_path".to_owned()));
        assert!(indexes(o).contains(&"li_ino".to_owned()));
    }
    let (d1, d2) = (dump(&o1), dump(&o2));
    assert_eq!(d1, d2);
    // The v1 export is also the v1 source's own content (meta differs only in nothing).
    assert_eq!(d1, dump(&a.path().join("20260101T000000Z-00000000.db")));
    // The exported file is a normal v1 snapshot with paths.
    let reopened = Snapshot::open(&o2).unwrap();
    assert_eq!(reopened.meta().schema_version, 1);
    assert_eq!(reopened.dir(2).unwrap(), dir(2));
    // Only the output remains in the destination directory.
    let mut left: Vec<_> = fs::read_dir(out.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    left.sort();
    assert_eq!(left, ["one.db", "two.db"]);
}

#[test]
fn existing_output_is_refused_and_untouched() {
    let a = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let snap = Snapshot::open(&write_v2(a.path())).unwrap();
    let target = out.path().join("exists.db");
    fs::write(&target, b"precious").unwrap();
    match export_sqlite(&snap, &target) {
        Err(StoreError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists),
        other => panic!("expected AlreadyExists, got {other:?}"),
    }
    assert_eq!(fs::read(&target).unwrap(), b"precious");
    // A dangling symlink also counts as existing and is not followed.
    let link = out.path().join("link.db");
    std::os::unix::fs::symlink(out.path().join("nowhere"), &link).unwrap();
    assert!(export_sqlite(&snap, &link).is_err());
    assert!(!out.path().join("nowhere").exists());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 2);
}

#[test]
fn error_mid_export_leaves_nothing_behind() {
    let a = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let file = write_v1(a.path());
    // An undecodable reason in the last copied table fails after dirs and files were written.
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute("INSERT INTO skipped VALUES (1, x'61', 'bogus')", [])
        .unwrap();
    let snap = Snapshot::open(&file).unwrap();
    let target = out.path().join("never.db");
    assert!(export_sqlite(&snap, &target).is_err());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn cold_snapshot_exports_like_its_hot_source() {
    let a = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let hot = write_v2(a.path());
    let cold = compress_snapshot(&hot, 3, &|| false).unwrap();
    let snap = Snapshot::open_with_space_guard(&cold, |_| Ok(())).unwrap();
    let o = out.path().join("cold.db");
    export_sqlite(&snap, &o).unwrap();
    let hot_out = out.path().join("hot.db");
    export_sqlite(&Snapshot::open(&hot).unwrap(), &hot_out).unwrap();
    assert_eq!(dump(&o), dump(&hot_out));
    assert_eq!(user_version(&o), 1);
}

fn rows(db: &Path, sql: &str) -> i64 {
    rusqlite::Connection::open(db)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

#[test]
fn b1_staging_never_touches_a_preexisting_entry() {
    let a = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let snap = Snapshot::open(&write_v2(a.path())).unwrap();
    // The names the previous implementation used (and similar guesses) must stay untouched.
    let old = out.path().join(format!(
        ".tidemark-export-{}-{}",
        std::process::id(),
        snap.meta().scan_id
    ));
    fs::create_dir(&old).unwrap();
    fs::write(old.join("preexisting-sentinel"), b"x").unwrap();
    export_sqlite(&snap, &out.path().join("new.db")).unwrap();
    assert!(old.join("preexisting-sentinel").exists());
    assert!(out.path().join("new.db").exists());
    // Only the sentinel directory and the output remain: staging cleaned up.
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 2);
}

#[test]
fn s2_cancellation_stops_and_leaves_nothing() {
    use std::cell::Cell;
    let a = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let snap = Snapshot::open(&write_v1(a.path())).unwrap();
    let polls = Cell::new(0);
    let result = tidemark_store::export_sqlite::export_sqlite_cancellable(
        &snap,
        &out.path().join("never.db"),
        &|| {
            polls.set(polls.get() + 1);
            polls.get() > 2
        },
    );
    match result {
        Err(StoreError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::Interrupted),
        other => panic!("expected Interrupted, got {other:?}"),
    }
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn s4_v2_instances_without_link_sets_are_exported() {
    let a = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let m = meta(2);
    let mut w = SnapshotWriterV2::create_with_options(
        a.path(),
        &m,
        WriterOptions {
            plain_depth: 1,
            chunk_dirs: 2,
            ..Default::default()
        },
    )
    .unwrap();
    w.put_dirs(&dirs()).unwrap();
    w.put_files(&files()).unwrap();
    w.put_link_instances(&instances()).unwrap();
    let file = w.finish(&m).unwrap();
    let snap = Snapshot::open(&file).unwrap();
    let target = out.path().join("o.db");
    let stats = export_sqlite(&snap, &target).unwrap();
    assert_eq!(stats.link_instances, 4);
    assert_eq!(rows(&target, "SELECT count(*) FROM link_instances"), 4);
    assert_eq!(rows(&target, "SELECT count(*) FROM link_sets"), 0);
}

#[test]
fn s5_export_is_not_limited_by_the_query_cap() {
    let a = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let m = meta(1);
    let mut w = SnapshotWriter::create(a.path(), &m).unwrap();
    w.put_dirs(&dirs()).unwrap();
    const N: u32 = 1_000_001;
    w.put_link_sets(&[LinkSetRow {
        ino: 3000,
        nlink: N,
        found: N,
        complete: true,
        disk: 4096,
        apparent: 10,
        mtime: 3,
        lca_dir_id: 0,
    }])
    .unwrap();
    let mut batch = Vec::new();
    for n in 0..N {
        batch.push(LinkInstanceRow {
            ino: 3000,
            dir_id: 1,
            name: Some(name(format!("n{n:08}").as_bytes())),
        });
        if batch.len() == 50_000 {
            w.put_link_instances(&batch).unwrap();
            batch.clear();
        }
    }
    w.put_link_instances(&batch).unwrap();
    let file = w.finish(&m).unwrap();
    let snap = Snapshot::open(&file).unwrap();
    assert!(
        snap.link_instances(3000).is_err(),
        "public query stays capped"
    );
    let target = out.path().join("o.db");
    let stats = export_sqlite(&snap, &target).unwrap();
    assert_eq!(stats.link_instances, u64::from(N));
    assert_eq!(
        rows(&target, "SELECT count(*) FROM link_instances"),
        i64::from(N)
    );
}

#[test]
fn s7_non_increasing_directory_ids_are_an_error_not_a_panic() {
    let a = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let file = write_v1(a.path());
    {
        let c = rusqlite::Connection::open(&file).unwrap();
        c.execute_batch(
            "ALTER TABLE dirs RENAME TO old_dirs;
             CREATE TABLE dirs AS SELECT * FROM old_dirs;
             INSERT INTO dirs SELECT * FROM old_dirs WHERE id=0;
             DROP TABLE old_dirs;",
        )
        .unwrap();
    }
    let snap = Snapshot::open(&file).unwrap();
    match export_sqlite(&snap, &out.path().join("o.db")) {
        Err(StoreError::Invalid(_)) => {}
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn s7_writer_rejects_non_increasing_directory_ids() {
    let a = test_support::private_tempdir().unwrap();
    let mut w = SnapshotWriter::create(a.path(), &meta(1)).unwrap();
    let mut rows = dirs();
    rows.push(rows[0].clone());
    assert!(matches!(w.put_dirs(&rows), Err(StoreError::Invalid(_))));
}

/// Files in the staging directory, for driving cancellation at a precise export stage.
fn staged(out: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    for e in fs::read_dir(out).unwrap().flatten() {
        if e.file_name()
            .to_string_lossy()
            .starts_with(".tidemark-export-")
        {
            for f in fs::read_dir(e.path()).unwrap().flatten() {
                paths.push(f.path());
            }
        }
    }
    paths
}

/// Run an export whose cancel callback trips once `stage(staged files)` holds.
fn export_cancelled_at(snap: &Snapshot, stage: &dyn Fn(&[std::path::PathBuf]) -> bool) {
    let out = test_support::private_tempdir().unwrap();
    let target = out.path().join("never.db");
    let result = tidemark_store::export_sqlite::export_sqlite_cancellable(snap, &target, &|| {
        stage(&staged(out.path()))
    });
    match result {
        Err(StoreError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::Interrupted),
        other => panic!("expected Interrupted, got {other:?}"),
    }
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0, "leftovers");
}

#[test]
fn s1_cancel_during_finalization_before_publication() {
    let a = test_support::private_tempdir().unwrap();
    let hot1 = write_v1(a.path());
    let b = test_support::private_tempdir().unwrap();
    let hot2 = write_v2(b.path());
    let cold = compress_snapshot(&hot2, 3, &|| false).unwrap();
    let sources = [
        Snapshot::open(&hot1).unwrap(),
        Snapshot::open(&hot2).unwrap(),
        Snapshot::open_with_space_guard(&cold, |_| Ok(())).unwrap(),
    ];
    for snap in &sources {
        // The last index exists: only `finish_with`'s own checkpoints can stop the export.
        export_cancelled_at(snap, &|files| {
            files.iter().any(|p| {
                p.to_string_lossy().ends_with(".db.tmp")
                    && rusqlite::Connection::open_with_flags(
                        p,
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )
                    .and_then(|c| {
                        c.busy_timeout(std::time::Duration::ZERO)?;
                        c.query_row(
                            "SELECT count(*) FROM sqlite_master WHERE name='li_dir'",
                            [],
                            |r| r.get::<_, i64>(0),
                        )
                    })
                    .is_ok_and(|n| n == 1)
            })
        });
        // The writer has already renamed its database into the staging directory: only
        // the check right before the outer publication rename can stop the export.
        export_cancelled_at(snap, &|files| {
            files
                .iter()
                .any(|p| p.extension().is_some_and(|e| e == "db"))
        });
    }
}

fn sqlite_failure(extended: i32) -> StoreError {
    StoreError::Sql(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(extended),
        None,
    ))
}

/// Export `snap` with `fault` failing `stage`; `probe` is the staging-directory write probe.
fn export_with_fault(
    snap: &Snapshot,
    stage: &'static str,
    error: fn() -> StoreError,
    probe: fn(&Path) -> Option<i32>,
) -> (
    Result<tidemark_store::export_sqlite::SqliteExportStats, StoreError>,
    usize,
) {
    let out = test_support::private_tempdir().unwrap();
    let faults = tidemark_store::export_sqlite::ExportFaults {
        probe: Some(std::sync::Arc::new(probe)),
        fault: Some(std::sync::Arc::new(move |s| (s == stage).then(error))),
    };
    let result = tidemark_store::export_sqlite::export_sqlite_with_faults(
        snap,
        &out.path().join("never.db"),
        &|| false,
        &faults,
    );
    (result, fs::read_dir(out.path()).unwrap().count())
}

/// A quota that stops the database but would admit a small probe once the failed
/// database's space is released: the probe must run while the temporary file exists.
fn quota_probe(dir: &Path) -> Option<i32> {
    let tmp_exists = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().ends_with(".db.tmp"));
    tmp_exists.then(|| rustix::io::Errno::DQUOT.raw_os_error())
}

#[test]
fn fix3_s1_write_failure_is_classified_before_cleanup_frees_space() {
    let a = test_support::private_tempdir().unwrap();
    let hot = write_v1(a.path());
    let snap = Snapshot::open(&hot).unwrap();
    // `copy` fails while the writer is live; `finish` fails inside `finish_with`, which
    // consumes the writer (its drop unlinks the database).
    for stage in ["copy", "finish"] {
        let (result, leftovers) =
            export_with_fault(&snap, stage, || sqlite_failure(778), quota_probe);
        assert!(
            matches!(result, Err(StoreError::Space(_))),
            "{stage}: {result:?}"
        );
        assert_eq!(leftovers, 0, "{stage}");
    }
    // An unrelated write error (probe finds the directory healthy) keeps its class.
    let (result, _) = export_with_fault(&snap, "finish", || sqlite_failure(778), |_| None);
    assert!(matches!(result, Err(StoreError::Sql(_))), "{result:?}");
}

#[test]
fn fix3_s2_space_failure_during_writer_creation_is_space() {
    let a = test_support::private_tempdir().unwrap();
    let hot = write_v1(a.path());
    let snap = Snapshot::open(&hot).unwrap();
    let (result, leftovers) = export_with_fault(&snap, "create", || sqlite_failure(13), |_| None);
    assert!(matches!(result, Err(StoreError::Space(_))), "{result:?}");
    assert_eq!(leftovers, 0);
    let (result, leftovers) =
        export_with_fault(&snap, "create", || sqlite_failure(778), quota_probe);
    assert!(matches!(result, Err(StoreError::Space(_))), "{result:?}");
    assert_eq!(leftovers, 0);
}
