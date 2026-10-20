//! Owned by: WSNAP-writer2
//! Raw v2 layout, bounded streaming, publication and cleanup evidence.
#![allow(clippy::unwrap_used)]
use rusqlite::{params, Connection};
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};
use tidemark_core::{
    config::*,
    path::{RawName, RawPath},
    scan::{ScanMode, SnapshotStatus},
    snapshot::*,
};
use tidemark_store::{
    chunk::*,
    space_guard::SpaceVerdict,
    writer::SnapshotWriter,
    writer_v2::{SnapshotWriterV2, WriterOptions},
    StoreError,
};
fn path(bytes: &[u8]) -> RawPath {
    RawPath::new(bytes.to_vec()).unwrap()
}
fn meta() -> SnapshotMeta {
    SnapshotMeta {
        schema_version: 2,
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
        mtime: -900 + i64::from(id),
        dir_disk: u64::from(id) + 1,
        dir_apparent: u64::from(id) + 2,
        projid: Some(id),
        entries: 7,
        errors: id % 17,
        own_files: u64::from(id) + 3,
        own_symlinks: u64::from(id) + 4,
        own_charged: u64::from(id) + 5,
        own_apparent: u64::from(id) + 6,
        t_dirs: u64::from(id) + 7,
        t_files: u64::from(id) + 8,
        t_inodes: u64::from(id) + 9,
        t_disk: u64::from(id) + 4096,
        t_apparent: u64::from(id) + 10,
        t_charged: u64::from(id) + 11,
        t_reclaimable: u64::from(id) + 12,
        t_errors: u64::from(id) + 13,
        age0: u64::from(id) + 14,
        age1: u64::from(id) + 15,
        age2: u64::from(id) + 16,
        age3: u64::from(id) + 17,
        age4: u64::from(id) + 18,
        age5: u64::from(id) + 19,
        newest_mtime: -900 + i64::from(id),
        max_entries: id + 100,
    }
}
fn expected(r: &DirRow, file_lo: u32, file_count: u32) -> ChunkDir {
    ChunkDir {
        id: r.id,
        parent_id: r.parent_id,
        subtree_end: r.subtree_end,
        depth: r.depth,
        name: r.name.clone(),
        ino: r.ino,
        mtime: r.mtime,
        dir_disk: r.dir_disk,
        dir_apparent: r.dir_apparent,
        projid: r.projid,
        entries: r.entries,
        errors: r.errors,
        own_files: r.own_files,
        own_symlinks: r.own_symlinks,
        own_charged: r.own_charged,
        own_apparent: r.own_apparent,
        t_dirs: r.t_dirs,
        t_files: r.t_files,
        t_inodes: r.t_inodes,
        t_disk: r.t_disk,
        t_apparent: r.t_apparent,
        t_charged: r.t_charged,
        t_reclaimable: r.t_reclaimable,
        t_errors: r.t_errors,
        age0: r.age0,
        age1: r.age1,
        age2: r.age2,
        age3: r.age3,
        age4: r.age4,
        age5: r.age5,
        newest_mtime: r.newest_mtime,
        max_entries: r.max_entries,
        file_lo,
        file_count,
    }
}

fn values(c: &Connection, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let mut s = c.prepare(sql).unwrap();
    let n = s.column_count();
    s.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
        .unwrap()
        .map(Result::unwrap)
        .collect()
}
fn files() -> Vec<FileRow> {
    (0..37)
        .rev()
        .map(|i| FileRow {
            id: i,
            dir_id: i % 6,
            name: if i % 4 == 0 {
                None
            } else {
                Some(RawName::new(vec![b'a' + i as u8, 0xff]).unwrap())
            },
            ino: 100 + i as u64,
            disk: 4096 + i as u64,
            apparent: 10000,
            mtime: -10 + i as i64,
            nlink: 2,
        })
        .collect()
}
fn populate(w: &mut SnapshotWriterV2, dirs: &[DirRow]) {
    for rows in dirs.chunks(11) {
        w.put_dirs(rows).unwrap();
    }
    for rows in files().chunks(3) {
        w.put_files(rows).unwrap();
    }
    w.put_link_sets(&[LinkSetRow {
        ino: 100,
        nlink: 2,
        found: 3,
        complete: true,
        disk: 4096,
        apparent: 10000,
        mtime: -7,
        lca_dir_id: 0,
    }])
    .unwrap();
    for ino in (100..105).rev() {
        for id in (0..6).rev() {
            w.put_link_instances(&[LinkInstanceRow {
                ino,
                dir_id: id,
                name: if id % 2 == 0 {
                    None
                } else {
                    Some(RawName::new(vec![0xff]).unwrap())
                },
            }])
            .unwrap();
        }
    }
    w.put_skipped(&[SkipRow {
        dir_id: 1,
        name: RawName::new(b"mount".to_vec()).unwrap(),
        reason: tidemark_core::scan::SkipReason::Mount,
    }])
    .unwrap();
    w.put_errors(&[ErrorRow {
        dir_id: None,
        path: dirs[0].path.clone(),
        op: tidemark_core::scan::ErrOp::Statx,
        errno: 13,
    }])
    .unwrap();
    w.put_annotations(&[AnnotationRow {
        dir_id: 2,
        key: "app".into(),
        value: "database".into(),
    }])
    .unwrap();
}
fn populate_v1(w: &mut SnapshotWriter, dirs: &[DirRow]) {
    for rows in dirs.chunks(11) {
        w.put_dirs(rows).unwrap();
    }
    for rows in files().chunks(3) {
        w.put_files(rows).unwrap();
    }
    w.put_link_sets(&[LinkSetRow {
        ino: 100,
        nlink: 2,
        found: 3,
        complete: true,
        disk: 4096,
        apparent: 10000,
        mtime: -7,
        lca_dir_id: 0,
    }])
    .unwrap();
    for ino in (100..105).rev() {
        for id in (0..6).rev() {
            w.put_link_instances(&[LinkInstanceRow {
                ino,
                dir_id: id,
                name: if id % 2 == 0 {
                    None
                } else {
                    Some(RawName::new(vec![0xff]).unwrap())
                },
            }])
            .unwrap();
        }
    }
    w.put_skipped(&[SkipRow {
        dir_id: 1,
        name: RawName::new(b"mount".to_vec()).unwrap(),
        reason: tidemark_core::scan::SkipReason::Mount,
    }])
    .unwrap();
    w.put_errors(&[ErrorRow {
        dir_id: None,
        path: dirs[0].path.clone(),
        op: tidemark_core::scan::ErrOp::Statx,
        errno: 13,
    }])
    .unwrap();
    w.put_annotations(&[AnnotationRow {
        dir_id: 2,
        key: "app".into(),
        value: "database".into(),
    }])
    .unwrap();
}
fn check(c: &Connection, dirs: &[DirRow], width: u32) {
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        c.query_row("PRAGMA freelist_count", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        values(
            c,
            "SELECT name FROM sqlite_master WHERE type='index' AND sql IS NOT NULL ORDER BY name"
        ),
        vec![
            vec!["ls_lca".to_owned().into()],
            vec!["shallow_parent".to_owned().into()]
        ]
    );
    let mut fs = files();
    fs.sort_by(|a, b| (a.dir_id, &a.name).cmp(&(b.dir_id, &b.name)));
    let mut actual_files = vec![];
    let mut stmt = c
        .prepare(
            "SELECT first_row,last_row,first_dir,last_dir,data FROM file_chunks ORDER BY first_row",
        )
        .unwrap();
    for row in stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, Vec<u8>>(4)?,
            ))
        })
        .unwrap()
    {
        let (first, last, lo, hi, data) = row.unwrap();
        let decoded = decode_files(&data).unwrap();
        assert_eq!(first, actual_files.len() as u32);
        assert_eq!(last, first + decoded.len() as u32 - 1);
        assert_eq!(lo, decoded[0].dir_id);
        assert_eq!(hi, decoded.last().unwrap().dir_id);
        actual_files.extend(decoded);
    }
    assert_eq!(
        actual_files,
        fs.iter()
            .map(|r| ChunkFile {
                dir_id: r.dir_id,
                name: r.name.as_ref().map(|n| n.as_bytes().to_vec()),
                ino: r.ino,
                disk: r.disk,
                apparent: r.apparent,
                mtime: r.mtime,
                nlink: r.nlink
            })
            .collect::<Vec<_>>()
    );
    let mut stmt = c
        .prepare("SELECT first_id,last_id,data FROM dir_chunks ORDER BY first_id")
        .unwrap();
    let mut seen = 0;
    let mut reconstructed: Vec<RawPath> = vec![];
    for row in stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })
        .unwrap()
    {
        let (first, last, data) = row.unwrap();
        let decoded = decode_dirs(&data, first).unwrap();
        assert_eq!(last, decoded.last().unwrap().id);
        for d in decoded {
            let r = &dirs[seen];
            let lo = fs.partition_point(|f| f.dir_id < r.id) as u32;
            let n = fs.iter().filter(|f| f.dir_id == r.id).count() as u32;
            assert_eq!(d, expected(r, lo, n));
            let p = match d.parent_id {
                None => path(&d.name),
                Some(id) => reconstructed[id as usize].join(&RawName::new(d.name.clone()).unwrap()),
            };
            assert_eq!(p, r.path);
            reconstructed.push(p);
            assert_eq!(
                c.query_row(
                    "SELECT id FROM dir_lookup WHERE hash=? AND id=?",
                    params![
                        HashWidth::new(width).unwrap().key(r.path.hash64() as u64),
                        r.id
                    ],
                    |r| r.get::<_, u32>(0)
                )
                .unwrap(),
                r.id
            );
            seen += 1;
        }
    }
    assert_eq!(seen, dirs.len());
    let mut expected_li = vec![];
    for ino in 100..105 {
        for id in 0..6 {
            expected_li.push(ChunkLinkInstance {
                ino,
                dir_id: id,
                name: if id % 2 == 0 { None } else { Some(vec![0xff]) },
            })
        }
    }
    let mut actual_li = vec![];
    for row in values(
        c,
        "SELECT first_row,last_row,first_ino,last_ino,data FROM li_chunks ORDER BY first_row",
    ) {
        let rusqlite::types::Value::Blob(data) = &row[4] else {
            panic!()
        };
        let ds = decode_link_instances(data).unwrap();
        assert_eq!(
            row[0],
            rusqlite::types::Value::Integer(actual_li.len() as i64)
        );
        assert_eq!(
            row[1],
            rusqlite::types::Value::Integer((actual_li.len() + ds.len() - 1) as i64)
        );
        assert_eq!(row[2], rusqlite::types::Value::Integer(ds[0].ino as i64));
        assert_eq!(
            row[3],
            rusqlite::types::Value::Integer(ds.last().unwrap().ino as i64)
        );
        actual_li.extend(ds);
    }
    assert_eq!(actual_li, expected_li);
    assert_eq!(
        values(c, "SELECT * FROM link_sets"),
        vec![vec![
            100.into(),
            2.into(),
            3.into(),
            1.into(),
            4096.into(),
            10000.into(),
            (-7).into(),
            0.into()
        ]]
    );
    assert_eq!(
        values(c, "SELECT * FROM skipped"),
        vec![vec![
            1.into(),
            rusqlite::types::Value::Blob(b"mount".to_vec()),
            "mount".to_owned().into()
        ]]
    );
    assert_eq!(
        values(c, "SELECT * FROM errors"),
        vec![vec![
            rusqlite::types::Value::Null,
            rusqlite::types::Value::Blob(dirs[0].path.as_bytes().to_vec()),
            "statx".to_owned().into(),
            13.into()
        ]]
    );
    assert_eq!(
        values(c, "SELECT * FROM annotations"),
        vec![vec![
            2.into(),
            "app".to_owned().into(),
            "database".to_owned().into()
        ]]
    );
    let shallow = values(c, "SELECT * FROM shallow_dirs ORDER BY id");
    let columns = values(c, "PRAGMA table_info(shallow_dirs)")
        .iter()
        .map(|r| match &r[1] {
            rusqlite::types::Value::Text(s) => s.clone(),
            _ => panic!(),
        })
        .collect::<Vec<_>>()
        .join(",");
    let tmp = test_support::private_tempdir().unwrap();
    let mut m = meta();
    m.schema_version = 1;
    let mut v1 = SnapshotWriter::create(tmp.path(), &m).unwrap();
    populate_v1(&mut v1, dirs);
    let f = v1.finish(&m).unwrap();
    let old = Connection::open(&f).unwrap();
    assert_eq!(
        shallow,
        values(
            &old,
            &format!("SELECT {columns} FROM dirs WHERE depth<=2 ORDER BY id")
        )
    );
    assert_eq!(values(c,"SELECT key,value FROM meta WHERE key NOT IN ('schema_version','plain_depth','hash_width','chunk_dirs','chunk_files','chunk_li','zstd_level') ORDER BY key"),values(&old,"SELECT key,value FROM meta WHERE key!='schema_version' ORDER BY key"));
    let size1 = std::fs::metadata(f).unwrap().len();
    let size2 = c
        .query_row("PRAGMA page_count", [], |r| r.get::<_, i64>(0))
        .unwrap()
        * 4096;
    eprintln!("directories={} v1={} ({:.2} B/dir), v2={} ({:.2} B/dir); v1 comparison contains identical input rows",dirs.len(),size1,size1 as f64/dirs.len() as f64,size2,size2 as f64/dirs.len() as f64);
}
#[test]
fn generated_50k_raw_roundtrip() {
    let dirs = (0..50_000)
        .map(|id| {
            let mut d = dir(id);
            d.parent_id = if id == 0 {
                None
            } else if id < 4 {
                Some(id - 1)
            } else {
                Some(3)
            };
            d.depth = id.min(4);
            d.subtree_end = if id < 4 { 49_999 } else { id };
            d.name = if id == 0 {
                b"/data/\xff".to_vec()
            } else if id < 4 {
                vec![b'a' + id as u8 - 1]
            } else {
                format!("{id:08}").into_bytes()
            };
            d.path = if id == 0 {
                path(b"/data/\xff")
            } else {
                let mut bytes = b"/data/\xff".to_vec();
                for component in 0..id.min(3) {
                    bytes.push(b'/');
                    bytes.push(b'a' + component as u8);
                }
                if id >= 4 {
                    bytes.push(b'/');
                    bytes.extend_from_slice(&d.name);
                }
                path(&bytes)
            };
            d.path_hash = d.path.hash64();
            d
        })
        .collect::<Vec<_>>();
    let tmp = test_support::private_tempdir().unwrap();
    let m = meta();
    let mut w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
    populate(&mut w, &dirs);
    let f = w.finish(&m).unwrap();
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    check(&Connection::open(f).unwrap(), &dirs, 64);
}
#[test]
fn golden_fixture_small_chunks_and_32_bit_hashes() {
    let model = test_support::golden::link_semantics_basic();
    let paths = model.paths();
    let dirs = model
        .dirs
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut d = dir(i as u32);
            d.path = paths[i].clone();
            d.path_hash = d.path.hash64();
            d.name = m
                .name
                .as_ref()
                .map(|n| n.as_bytes().to_vec())
                .unwrap_or_else(|| d.path.as_bytes().to_vec());
            d.parent_id = m.parent.map(|p| p as u32);
            d.depth = d.path.as_bytes().iter().filter(|&&b| b == b'/').count() as u32 - 1;
            d.subtree_end = if i == 0 || i == 3 { 5 } else { i as u32 };
            d.ino = m.ino;
            d.projid = Some(i as u32);
            d
        })
        .collect::<Vec<_>>();
    let tmp = test_support::private_tempdir().unwrap();
    let m = meta();
    let options = WriterOptions {
        hash_width: 32,
        chunk_dirs: 2,
        chunk_files: 3,
        chunk_li: 4,
        ..Default::default()
    };
    let mut w = SnapshotWriterV2::create_with_options(tmp.path(), &m, options).unwrap();
    populate(&mut w, &dirs);
    let f = w.finish(&m).unwrap();
    check(&Connection::open(f).unwrap(), &dirs, 32);
}
#[test]
fn cancellation_at_every_checkpoint_cleans_both_leaves() {
    use std::cell::Cell;
    let dirs = (0..6).map(dir).collect::<Vec<_>>();
    let m = meta();
    let calls = Cell::new(0);
    let tmp = test_support::private_tempdir().unwrap();
    let mut w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
    populate(&mut w, &dirs);
    w.finish_with(&m, &|| {
        calls.set(calls.get() + 1);
        false
    })
    .unwrap();
    for stop in 1..=calls.get() {
        let tmp = test_support::private_tempdir().unwrap();
        let mut w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
        populate(&mut w, &dirs);
        let n = Cell::new(0);
        assert!(matches!(
            w.finish_with(&m, &|| {
                n.set(n.get() + 1);
                n.get() == stop
            }),
            Err(StoreError::Cancelled)
        ));
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().count(),
            0,
            "checkpoint {stop}"
        );
    }
}
#[test]
fn cleanup_errors_drop_abort_and_space_refusal() {
    let m = meta();
    for mode in 0..5 {
        let tmp = test_support::private_tempdir().unwrap();
        let mut w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
        for leaf in std::fs::read_dir(tmp.path()).unwrap() {
            assert_eq!(
                leaf.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        match mode {
            0 => drop(w),
            1 => w.abort(),
            2 => {
                w.space_check_every(
                    0,
                    Box::new(|| {
                        Ok(SpaceVerdict::Refuse {
                            reason: "test".into(),
                        })
                    }),
                );
                assert!(matches!(w.put_dirs(&[dir(0)]), Err(StoreError::Space(_))));
                assert!(w.finish(&m).is_err());
            }
            3 => {
                let mut bad = m.clone();
                bad.schema_version = 1;
                assert!(w.finish(&bad).is_err());
            }
            _ => {
                let mut bad = dir(0);
                bad.ino = u64::MAX;
                assert!(w.put_dirs(&[bad]).is_err());
                assert!(w.finish(&m).is_err());
            }
        }
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }
}
#[test]
fn options_metadata_identity_and_noreplace() {
    let tmp = test_support::private_tempdir().unwrap();
    let mut m = meta();
    m.schema_version = 1;
    assert!(SnapshotWriterV2::create(tmp.path(), &m).is_err());
    m.schema_version = 2;
    for options in [
        WriterOptions {
            hash_width: 16,
            ..Default::default()
        },
        WriterOptions {
            chunk_dirs: 0,
            ..Default::default()
        },
        WriterOptions {
            chunk_files: 8193,
            ..Default::default()
        },
        WriterOptions {
            zstd_level: 20,
            ..Default::default()
        },
    ] {
        assert!(SnapshotWriterV2::create_with_options(tmp.path(), &m, options).is_err());
    }
    let w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
    assert!(SnapshotWriterV2::create(tmp.path(), &m).is_err());
    let f = w.finish(&m).unwrap();
    let c = Connection::open(&f).unwrap();
    for (key, value) in [
        ("plain_depth", "2"),
        ("hash_width", "64"),
        ("chunk_dirs", "4096"),
        ("chunk_files", "8192"),
        ("chunk_li", "8192"),
        ("zstd_level", "3"),
    ] {
        assert_eq!(
            c.query_row("SELECT value FROM meta WHERE key=?", [key], |r| r
                .get::<_, String>(0))
                .unwrap(),
            value
        );
    }
    assert!(SnapshotWriterV2::create(tmp.path(), &m).is_err());
    let tmp = test_support::private_tempdir().unwrap();
    let w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
    let final_path = tmp.path().join(format!("{}.db", m.scan_id));
    std::fs::write(&final_path, b"winner").unwrap();
    assert!(w.finish(&m).is_err());
    assert_eq!(std::fs::read(final_path).unwrap(), b"winner");
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
}

#[test]
fn stage_trust_and_preexisting_leaves_are_preserved() {
    use std::os::unix::fs::symlink;
    for symlinked in [false, true] {
        let tmp = test_support::private_tempdir().unwrap();
        let m = meta();
        let stage = tmp.path().join(format!("{}.db.stage.tmp", m.scan_id));
        let target = tmp.path().join("target");
        test_support::private_write(&target, b"preserve").unwrap();
        if symlinked {
            symlink(&target, &stage).unwrap();
        } else {
            test_support::private_write(&stage, b"preserve").unwrap();
        }
        assert!(SnapshotWriterV2::create(tmp.path(), &m).is_err());
        assert_eq!(std::fs::read(&stage).unwrap(), b"preserve");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
    }
}

#[test]
fn streamed_50k_dirs_and_100k_instances() {
    let tmp = test_support::private_tempdir().unwrap();
    let m = meta();
    let mut w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
    for start in (0..50_000).step_by(512) {
        let rows = (start..(start + 512).min(50_000))
            .map(|id| {
                let mut d = dir(id);
                if id == 0 {
                    d.subtree_end = 49_999;
                }
                d
            })
            .collect::<Vec<_>>();
        w.put_dirs(&rows).unwrap();
    }
    for start in (0..100_000).step_by(512) {
        let rows = (start..(start + 512).min(100_000))
            .rev()
            .map(|id| LinkInstanceRow {
                ino: 100_000 - u64::from(id),
                dir_id: id % 50_000,
                name: None,
            })
            .collect::<Vec<_>>();
        w.put_link_instances(&rows).unwrap();
    }
    let f = w.finish(&m).unwrap();
    let c = Connection::open(f).unwrap();
    let mut count = 0;
    let mut last_ino = 0;
    for row in values(&c, "SELECT data FROM li_chunks ORDER BY first_row") {
        let rusqlite::types::Value::Blob(data) = &row[0] else {
            panic!()
        };
        for row in decode_link_instances(data).unwrap() {
            assert!(row.ino > last_ino);
            last_ino = row.ino;
            count += 1;
        }
    }
    assert_eq!(count, 100_000);
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
}

#[test]
fn incoming_file_ids_do_not_define_clustered_rows() {
    let tmp = test_support::private_tempdir().unwrap();
    let m = meta();
    let mut w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
    let mut rows = files();
    for row in &mut rows {
        row.id = 999;
    }
    w.put_files(&rows[..20]).unwrap();
    w.put_files(&rows[20..]).unwrap();
    let f = w.finish(&m).unwrap();
    let c = Connection::open(f).unwrap();
    let data = c
        .query_row("SELECT data FROM file_chunks WHERE first_row=0", [], |r| {
            r.get::<_, Vec<u8>>(0)
        })
        .unwrap();
    assert_eq!(decode_files(&data).unwrap().len(), 37);
    assert_eq!(
        c.query_row("SELECT last_row FROM file_chunks", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        36
    );
}

#[test]
fn finish_rechecks_space_after_input_and_each_conversion_checkpoint() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let m = meta();
    let options = WriterOptions {
        chunk_dirs: 2,
        chunk_files: 2,
        chunk_li: 2,
        ..Default::default()
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let tmp = test_support::private_tempdir().unwrap();
    let mut w = SnapshotWriterV2::create_with_options(tmp.path(), &m, options.clone()).unwrap();
    populate(&mut w, &(0..6).map(dir).collect::<Vec<_>>());
    let count = calls.clone();
    w.space_check_every(
        u64::MAX,
        Box::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(SpaceVerdict::Ok)
        }),
    );
    w.finish(&m).unwrap();
    assert!(calls.load(Ordering::SeqCst) >= 25);
    for stop in 0..calls.load(Ordering::SeqCst) {
        let tmp = test_support::private_tempdir().unwrap();
        let mut w = SnapshotWriterV2::create_with_options(tmp.path(), &m, options.clone()).unwrap();
        populate(&mut w, &(0..6).map(dir).collect::<Vec<_>>());
        let mut n = 0;
        w.space_check_every(
            u64::MAX,
            Box::new(move || {
                let refuse = n == stop;
                n += 1;
                if refuse {
                    Ok(SpaceVerdict::Refuse {
                        reason: "NoSpace test".into(),
                    })
                } else {
                    Ok(SpaceVerdict::Ok)
                }
            }),
        );
        assert!(
            matches!(w.finish(&m), Err(StoreError::Space(_))),
            "checkpoint {stop}"
        );
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }
}

// This child also supplies reproducible before/after scale measurements. No clock
// assertion: an external harness records elapsed time and peak RSS.
#[test]
fn conversion_scale_child() {
    let Ok(root) = std::env::var("TIDEMARK_WRITER_PROBE") else {
        return;
    };
    let m = meta();
    let mut w = SnapshotWriterV2::create(std::path::Path::new(&root), &m).unwrap();
    for start in (0..200_000).step_by(512) {
        let rows = (start..(start + 512).min(200_000))
            .map(|id| {
                let mut d = dir(id);
                if id == 0 {
                    d.subtree_end = 199_999;
                }
                d
            })
            .collect::<Vec<_>>();
        w.put_dirs(&rows).unwrap();
    }
    for start in (0..500_000).step_by(512) {
        let rows = (start..(start + 512).min(500_000))
            .rev()
            .map(|id| LinkInstanceRow {
                ino: 500_000 - u64::from(id),
                dir_id: id % 200_000,
                name: Some(RawName::new(format!("{id:0240}").into_bytes()).unwrap()),
            })
            .collect::<Vec<_>>();
        w.put_link_instances(&rows).unwrap();
    }
    if std::env::var_os("TIDEMARK_STAGE_ONLY").is_some() {
        // Model process death after committed input, without destructors.
        std::process::exit(0);
    }
    w.finish(&m).unwrap();
}

#[test]
fn staging_leaf_and_readback_plans() {
    let tmp = test_support::private_tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "conversion_scale_child", "--nocapture"])
        .env("TIDEMARK_WRITER_PROBE", tmp.path())
        .env("TIDEMARK_STAGE_ONLY", "1")
        .status()
        .unwrap();
    assert!(status.success());
    let stage = tmp.path().join(format!("{}.db.stage.tmp", meta().scan_id));
    assert!(stage.is_file());
    let c = Connection::open(stage).unwrap();
    for table in ["dirs", "files", "link_instances"] {
        let sql: String = c
            .query_row("SELECT sql FROM sqlite_master WHERE name=?", [table], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(sql.contains("WITHOUT ROWID"), "{sql}");
    }
    c.execute_batch("CREATE TABLE spans(dir_id INTEGER PRIMARY KEY,lo INTEGER NOT NULL,n INTEGER NOT NULL) WITHOUT ROWID").unwrap();
    // Keep this list synchronized with all build_output read-back statements.
    for sql in [
        "SELECT id,dir_id,CASE WHEN name_present=0 THEN NULL ELSE name END,ino,disk,apparent,mtime,nlink FROM files ORDER BY dir_id,name_present,name,id",
        "SELECT * FROM dirs ORDER BY id",
        "SELECT lo,n FROM spans WHERE dir_id=0",
        "SELECT ino,dir_id,CASE WHEN name_present=0 THEN NULL ELSE name END FROM link_instances ORDER BY ino,dir_id,name_present,name,seq",
        "SELECT * FROM link_sets", "SELECT * FROM skipped", "SELECT * FROM errors", "SELECT * FROM annotations",
        "SELECT * FROM dirs WHERE depth <= 2 ORDER BY id",
    ] {
        for row in values(&c, &format!("EXPLAIN QUERY PLAN {sql}")) {
            assert!(!format!("{row:?}").contains("USE TEMP B-TREE"), "{sql}: {row:?}");
        }
    }
}

#[test]
fn conversion_never_uses_external_sqlite_scratch() {
    let tmp = test_support::private_tempdir().unwrap();
    let scratch = test_support::private_tempdir().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "conversion_scale_child", "--nocapture"])
        .env("TIDEMARK_WRITER_PROBE", tmp.path())
        .env("SQLITE_TMPDIR", scratch.path())
        .spawn()
        .unwrap();
    let mut observed_scratch = false;
    loop {
        observed_scratch |= std::fs::read_dir(scratch.path()).unwrap().count() != 0;
        // SQLite unlinks scratch immediately: inspect open descriptors too.
        if let Ok(fds) = std::fs::read_dir(format!("/proc/{}/fd", child.id())) {
            for fd in fds.flatten() {
                if let Ok(target) = std::fs::read_link(fd.path()) {
                    observed_scratch |= target.starts_with(scratch.path());
                }
            }
        }
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // Reap the child before asserting, including when the regression reappears.
    assert!(
        !observed_scratch,
        "SQLite used external scratch during conversion"
    );
    assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
}

#[test]
fn clustered_staging_preserves_null_names_and_duplicate_instances() {
    let tmp = test_support::private_tempdir().unwrap();
    let m = meta();
    let mut w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
    let mut f = files()[0].clone();
    f.name = None;
    w.put_files(&[f.clone(), f]).unwrap();
    let li = LinkInstanceRow {
        ino: 9,
        dir_id: 0,
        name: None,
    };
    w.put_link_instances(&[li.clone(), li]).unwrap();
    w.put_link_instances(&[LinkInstanceRow {
        ino: 9,
        dir_id: 0,
        name: Some(RawName::new(b"a".to_vec()).unwrap()),
    }])
    .unwrap();
    let c = Connection::open(w.finish(&m).unwrap()).unwrap();
    let file_data: Vec<u8> = c
        .query_row("SELECT data FROM file_chunks", [], |r| r.get(0))
        .unwrap();
    let rows = decode_files(&file_data).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.name.is_none()));
    let li_data: Vec<u8> = c
        .query_row("SELECT data FROM li_chunks", [], |r| r.get(0))
        .unwrap();
    let rows = decode_link_instances(&li_data).unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].name.is_none() && rows[1].name.is_none());
    assert_eq!(rows[2].name.as_deref(), Some(&b"a"[..]));
}

#[test]
fn space_guard_turns_to_refusal_after_last_input_row() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let tmp = test_support::private_tempdir().unwrap();
    let m = meta();
    let mut w = SnapshotWriterV2::create(tmp.path(), &m).unwrap();
    let refuse = Arc::new(AtomicBool::new(false));
    let flag = refuse.clone();
    w.space_check_every(
        0,
        Box::new(move || {
            if flag.load(Ordering::SeqCst) {
                Ok(SpaceVerdict::Refuse {
                    reason: "NoSpace after input".into(),
                })
            } else {
                Ok(SpaceVerdict::Ok)
            }
        }),
    );
    populate(&mut w, &(0..6).map(dir).collect::<Vec<_>>());
    refuse.store(true, Ordering::SeqCst);
    assert!(matches!(w.finish(&m), Err(StoreError::Space(_))));
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}
