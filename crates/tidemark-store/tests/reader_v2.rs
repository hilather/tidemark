//! Owned by: WSNAP-reader2-core
//! Differential directory query coverage against the v1 reader.
#![allow(clippy::unwrap_used)]
use std::collections::BTreeMap;
use tidemark_core::{
    config::*,
    path::{RawName, RawPath},
    scan::{ScanMode, SnapshotStatus},
    snapshot::*,
};
use tidemark_store::{
    writer::SnapshotWriter,
    writer_v2::{SnapshotWriterV2, WriterOptions},
    Snapshot, StoreError,
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
fn tree(width: u32, depth: u32) -> Vec<DirRow> {
    let mut rows = vec![dir(0)];
    // A deep first branch, followed by a wide root. Prefix siblings deliberately
    // make preorder differ from raw full-path ordering.
    for level in 1..=depth {
        let mut row = dir(level);
        row.parent_id = Some(level - 1);
        row.depth = level;
        row.name = vec![b'a'];
        row.path = rows[(level - 1) as usize]
            .path
            .join(&RawName::new(row.name.clone()).unwrap());
        row.path_hash = row.path.hash64();
        row.subtree_end = depth;
        rows.push(row);
    }
    for n in 0..width {
        let id = rows.len() as u32;
        let mut row = dir(id);
        row.name = if n == 0 {
            b"a-".to_vec()
        } else {
            format!("b{n:06}").into_bytes()
        };
        if n == width - 1 {
            row.name.push(0xff);
        }
        row.path = rows[0].path.join(&RawName::new(row.name.clone()).unwrap());
        row.path_hash = row.path.hash64();
        row.subtree_end = id;
        rows.push(row);
    }
    rows[0].subtree_end = rows.len() as u32 - 1;
    for row in &mut rows {
        row.t_disk = u64::from(row.id % 7);
        row.t_reclaimable = u64::from(row.id % 3);
        row.t_files = u64::from(row.id % 5);
        row.entries = row.id % 11;
        row.newest_mtime = i64::from(row.id % 9) - 5;
    }
    rows
}
struct Pair {
    _one: tempfile::TempDir,
    _two: tempfile::TempDir,
    one: Snapshot,
    two: Snapshot,
}
fn pair(rows: &[DirRow], plain_depth: u32, hash_width: u32, chunk_dirs: usize) -> Pair {
    let one = test_support::private_tempdir().unwrap();
    let two = test_support::private_tempdir().unwrap();
    let mut m = meta();
    m.root = rows[0].path.clone();
    m.dirs = rows.len() as u64;
    m.schema_version = 1;
    let mut w = SnapshotWriter::create(one.path(), &m).unwrap();
    w.put_dirs(rows).unwrap();
    let f1 = w.finish(&m).unwrap();
    m.schema_version = 2;
    let mut w = SnapshotWriterV2::create_with_options(
        two.path(),
        &m,
        WriterOptions {
            plain_depth,
            hash_width,
            chunk_dirs,
            ..Default::default()
        },
    )
    .unwrap();
    w.put_dirs(rows).unwrap();
    let f2 = w.finish(&m).unwrap();
    Pair {
        _one: one,
        _two: two,
        one: Snapshot::open(&f1).unwrap(),
        two: Snapshot::open(&f2).unwrap(),
    }
}
fn eq_rows(a: &[DirRow], b: &[DirRow]) {
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
}
const SORTS: [SortKey; 8] = [
    SortKey::Disk,
    SortKey::Apparent,
    SortKey::Charged,
    SortKey::Reclaimable,
    SortKey::Files,
    SortKey::Inodes,
    SortKey::Newest,
    SortKey::Name,
];
fn compare(p: &Pair, rows: &[DirRow], exhaustive: bool) {
    let a = &p.one;
    let b = &p.two;
    eq_rows(&[a.root().unwrap()], &[b.root().unwrap()]);
    let mut m = a.meta().clone();
    m.schema_version = 2;
    assert_eq!(
        serde_json::to_value(m).unwrap(),
        serde_json::to_value(b.meta()).unwrap()
    );
    for row in rows.iter().step_by(if exhaustive { 1 } else { 997 }) {
        eq_rows(&[a.dir(row.id).unwrap()], &[b.dir(row.id).unwrap()]);
        eq_rows(
            &[a.dir_by_path(&row.path).unwrap().unwrap()],
            &[b.dir_by_path(&row.path).unwrap().unwrap()],
        );
    }
    assert!(b.dir_by_path(&path(b"/absent")).unwrap().is_none());
    for id in [0, 1, rows.len() as u32 - 1, 999999] {
        for filter in [b"".as_slice(), b"a", b"b000", b"\xff", b"missing"] {
            assert_eq!(
                a.children_count(id, filter).unwrap(),
                b.children_count(id, filter).unwrap()
            );
            for sort in SORTS {
                for limit in [0, 1, 7, 1000000] {
                    eq_rows(
                        &a.children(id, sort, limit).unwrap(),
                        &b.children(id, sort, limit).unwrap(),
                    );
                    for desc in [false, true] {
                        for rev in [false, true] {
                            let x = a
                                .children_page(id, sort, desc, None, rev, limit, filter)
                                .unwrap();
                            let y = b
                                .children_page(id, sort, desc, None, rev, limit, filter)
                                .unwrap();
                            eq_rows(&x, &y);
                            if let Some(cursor) = x.first() {
                                eq_rows(
                                    &a.children_page(id, sort, desc, Some(cursor), rev, 3, filter)
                                        .unwrap(),
                                    &b.children_page(id, sort, desc, Some(cursor), rev, 3, filter)
                                        .unwrap(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    for depth in 0..=if exhaustive { 14 } else { 2 } {
        for limit in [0, 1, 13, i64::MAX as usize] {
            for sort in SORTS {
                eq_rows(
                    &a.subtree_dirs(0, depth, sort, limit).unwrap(),
                    &b.subtree_dirs(0, depth, sort, limit).unwrap(),
                );
            }
            if limit <= 1000000 {
                let (x, y, n, _) = a.metrics_candidates(depth, limit).unwrap();
                let (u, v, m, file) = b.metrics_candidates(depth, limit).unwrap();
                eq_rows(&x, &u);
                eq_rows(&y, &v);
                assert_eq!(n, m);
                assert!(file.starts_with(p._two.path()));
            }
            for min in [0, 5, 100] {
                eq_rows(
                    &a.wide_dirs(0, min, limit).unwrap(),
                    &b.wide_dirs(0, min, limit).unwrap(),
                );
            }
        }
    }
}
#[test]
fn golden_and_deep_tree_all_query_variants() {
    let model = test_support::golden::link_semantics_basic();
    let paths = model.paths();
    let mut golden = tree(5, 0);
    for (i, row) in golden.iter_mut().enumerate() {
        let g = &test_support::golden::LINK_SEMANTICS_BASIC[i];
        row.path = paths[i].clone();
        row.path_hash = row.path.hash64();
        row.name = model.dirs[i]
            .name
            .as_ref()
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_else(|| row.path.as_bytes().to_vec());
        row.parent_id = model.dirs[i].parent.map(|p| p as u32);
        row.depth = if i == 0 {
            0
        } else if i < 4 {
            1
        } else {
            2
        };
        row.subtree_end = if i == 0 || i == 3 { 5 } else { i as u32 };
        row.t_disk = g.values[0];
        row.t_charged = g.values[1];
        row.t_reclaimable = g.values[2];
        row.t_files = g.values[4];
        row.t_inodes = g.values[5];
        row.t_dirs = g.values[6];
    }
    for plain in [0, 2, 4] {
        for hash in [32, 64] {
            compare(&pair(&golden, plain, hash, 2), &golden, true);
            let rows = tree(17, 12);
            compare(&pair(&rows, plain, hash, 7), &rows, true);
        }
    }
}
#[test]
fn fifty_thousand_directories_small_chunks_and_wide_children() {
    let rows = tree(50000, 12);
    for (plain, hash, chunk) in [(0, 32, 2), (2, 64, 7), (4, 32, 5)] {
        let p = pair(&rows, plain, hash, chunk);
        // Keep full-scale work bounded while checking crossings and top-N selection.
        for sort in SORTS {
            eq_rows(
                &p.one.children(0, sort, 51).unwrap(),
                &p.two.children(0, sort, 51).unwrap(),
            );
            eq_rows(
                &p.one.subtree_dirs(0, 12, sort, 17).unwrap(),
                &p.two.subtree_dirs(0, 12, sort, 17).unwrap(),
            );
        }
        assert_eq!(p.two.children_count(0, b"").unwrap(), 50001);
        for row in rows.iter().step_by(997) {
            eq_rows(
                &[p.one.dir(row.id).unwrap()],
                &[p.two.dir_by_path(&row.path).unwrap().unwrap()],
            );
        }
        eq_rows(
            &p.one.wide_dirs(0, 5, 31).unwrap(),
            &p.two.wide_dirs(0, 5, 31).unwrap(),
        );
        for depth in [0, 2, 4, 12] {
            let (a, b, n, _) = p.one.metrics_candidates(depth, 17).unwrap();
            let (x, y, m, _) = p.two.metrics_candidates(depth, 17).unwrap();
            eq_rows(&a, &x);
            eq_rows(&b, &y);
            assert_eq!(n, m);
        }
        for desc in [false, true] {
            eq_rows(
                &p.one
                    .children_page(0, SortKey::Disk, desc, None, false, 50, b"b")
                    .unwrap(),
                &p.two
                    .children_page(0, SortKey::Disk, desc, None, false, 50, b"b")
                    .unwrap(),
            );
        }
    }
}
#[test]
fn cold_v2_and_hash_collision_verify_full_path() {
    let rows = tree(8, 12);
    for hash in [32, 64] {
        let p = pair(&rows, 2, hash, 2);
        let hot = std::fs::read_dir(p._two.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        drop(p.two);
        let conn = rusqlite::Connection::open(&hot).unwrap();
        let key = tidemark_store::chunk::HashWidth::new(hash)
            .unwrap()
            .key(rows[12].path.hash64() as u64);
        // An earlier candidate has the requested hash but different path.
        conn.execute(
            "INSERT OR REPLACE INTO dir_lookup(hash,id) VALUES (?,0)",
            [key],
        )
        .unwrap();
        drop(conn);
        let hot_snap = Snapshot::open(&hot).unwrap();
        eq_rows(
            &[rows[12].clone()],
            &[hot_snap.dir_by_path(&rows[12].path).unwrap().unwrap()],
        );
        drop(hot_snap);
        let cold = tidemark_store::cold::compress_snapshot(&hot, 3, &|| false).unwrap();
        let snap = Snapshot::open_with_space_guard(&cold, |_| Ok(())).unwrap();
        eq_rows(
            &p.one
                .children_page(0, SortKey::Disk, true, None, false, 50, b"")
                .unwrap(),
            &snap
                .children_page(0, SortKey::Disk, true, None, false, 50, b"")
                .unwrap(),
        );
        eq_rows(
            &[rows[12].clone()],
            &[snap.dir_by_path(&rows[12].path).unwrap().unwrap()],
        );
    }
}
#[test]
fn corrupt_chunks_and_unsupported_versions_are_invalid() {
    let rows = tree(5, 12);
    let p = pair(&rows, 0, 64, 2);
    let file = std::fs::read_dir(p._two.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    drop(p.two);
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute("UPDATE dir_chunks SET data=x'00' WHERE first_id=12", [])
        .unwrap();
    drop(conn);
    let snap = Snapshot::open(&file).unwrap();
    assert!(matches!(snap.dir(12), Err(StoreError::Invalid(_))));
    drop(snap);
    for version in [0, 3] {
        let conn = rusqlite::Connection::open(&file).unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
        drop(conn);
        assert!(matches!(Snapshot::open(&file), Err(StoreError::Invalid(_))));
    }
}
#[test]
fn legacy_queries_return_not_yet_implemented() {
    let rows = tree(4, 3);
    let p = pair(&rows, 2, 64, 2);
    let s = &p.two;
    macro_rules! nyi {
        ($e:expr) => {
            assert!(matches!($e, Err(StoreError::NotYetImplemented)));
        };
    }
    nyi!(s.subtree_files(0, &FileFilter::default(), 1));
    nyi!(s.link_sets_under(0, &LinkFilter::default(), 1));
    nyi!(s.link_instances(1));
    nyi!(s.link_instances_page(1, None, false, 1));
    nyi!(s.skipped_under(0));
    nyi!(s.errors_under(0, 1));
    nyi!(s.annotations(0));
    let one = std::fs::read_dir(p._one.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let two = std::fs::read_dir(p._two.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    nyi!(tidemark_store::diff::diff(
        s,
        &one,
        &rows[0].path,
        tidemark_store::diff::DiffMetric::Disk,
        2,
        10
    ));
    nyi!(tidemark_store::diff::diff(
        &p.one,
        &two,
        &rows[0].path,
        tidemark_store::diff::DiffMetric::Disk,
        2,
        10
    ));
    nyi!(tidemark_store::export_ncdu(
        s,
        &rows[0].path,
        &mut Vec::new(),
        &tidemark_store::NcduOptions {
            utf8_lossy: true,
            max_depth: None
        }
    ));
    nyi!(tidemark_store::links::Regions::build(
        s,
        &rows[0].path,
        &tidemark_store::links::RegionSpec::Depth(1)
    ));
}
#[test]
fn deep_paths_exceed_path_max_and_snapshot_remains_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Snapshot>();
    let rows = tree(0, 2200);
    let p = pair(&rows, 2, 64, 7);
    let target = rows.last().unwrap();
    assert!(target.path.as_bytes().len() > 4096);
    eq_rows(
        std::slice::from_ref(target),
        &[p.two.dir_by_path(&target.path).unwrap().unwrap()],
    );
}
#[test]
fn sibling_hopping_and_path_lookup_skip_unvisited_chunks() {
    let rows = tree(20, 12);
    let p = pair(&rows, 0, 64, 2);
    let file = std::fs::read_dir(p._two.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    drop(p.two);
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute("UPDATE dir_chunks SET data=x'00' WHERE first_id=4", [])
        .unwrap();
    drop(conn);
    let snap = Snapshot::open(&file).unwrap();
    eq_rows(
        &p.one.children(0, SortKey::Name, 100).unwrap(),
        &snap.children(0, SortKey::Name, 100).unwrap(),
    );
    drop(snap);
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute("UPDATE dir_chunks SET data=x'00' WHERE first_id=16", [])
        .unwrap();
    // Restore the damaged ancestor chunk directly from the pure codec so this
    // lookup tests an unrelated corrupt chunk, rather than a required ancestor.
    let mut chunk_rows = Vec::new();
    for r in &rows[4..6] {
        chunk_rows.push(tidemark_store::chunk::ChunkDir {
            id: r.id,
            parent_id: r.parent_id,
            subtree_end: r.subtree_end,
            depth: r.depth,
            name: r.name.clone(),
            ..Default::default()
        });
    }
    let blob = tidemark_store::chunk::encode_dirs(&chunk_rows).unwrap();
    conn.execute("UPDATE dir_chunks SET data=? WHERE first_id=4", [blob])
        .unwrap();
    drop(conn);
    let snap = Snapshot::open(&file).unwrap();
    eq_rows(
        &[rows[12].clone()],
        &[snap.dir_by_path(&rows[12].path).unwrap().unwrap()],
    );
}
#[test]
fn thousands_of_deep_children_and_paging_edges() {
    let mut rows = tree(3000, 12);
    for row in &mut rows[..=12] {
        row.subtree_end = 3012;
    }
    let parent = rows[12].path.clone();
    for row in &mut rows[13..] {
        row.parent_id = Some(12);
        row.depth = 13;
        row.path = parent.join(&RawName::new(row.name.clone()).unwrap());
        row.path_hash = row.path.hash64();
    }
    for plain in [0, 2, 4] {
        let p = pair(&rows, plain, 64, 3);
        assert_eq!(p.two.children_count(12, b"").unwrap(), 3000);
        for sort in SORTS {
            eq_rows(
                &p.one.children(12, sort, 7).unwrap(),
                &p.two.children(12, sort, 7).unwrap(),
            );
            for desc in [false, true] {
                for reverse in [false, true] {
                    let page = p
                        .one
                        .children_page(12, sort, desc, None, reverse, 7, b"")
                        .unwrap();
                    eq_rows(
                        &page,
                        &p.two
                            .children_page(12, sort, desc, None, reverse, 7, b"")
                            .unwrap(),
                    );
                    for cursor in [page.first().unwrap(), page.last().unwrap()] {
                        eq_rows(
                            &p.one
                                .children_page(12, sort, desc, Some(cursor), reverse, 7, b"")
                                .unwrap(),
                            &p.two
                                .children_page(12, sort, desc, Some(cursor), reverse, 7, b"")
                                .unwrap(),
                        );
                    }
                }
            }
        }
        assert!(matches!(
            p.two
                .children_page(12, SortKey::Name, false, None, false, 1000001, b""),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            p.two.metrics_candidates(2, 1000001),
            Err(StoreError::Invalid(_))
        ));
    }
}
#[test]
fn missing_directory_and_limit_errors_match_v1() {
    let rows = tree(5, 12);
    let p = pair(&rows, 2, 64, 2);
    for id in [rows.len() as u32, u32::MAX] {
        assert_eq!(
            p.one.dir(id).unwrap_err().to_string(),
            p.two.dir(id).unwrap_err().to_string()
        );
        assert_eq!(
            p.one
                .subtree_dirs(id, 2, SortKey::Disk, 1)
                .unwrap_err()
                .to_string(),
            p.two
                .subtree_dirs(id, 2, SortKey::Disk, 1)
                .unwrap_err()
                .to_string()
        );
        assert_eq!(
            p.one.wide_dirs(id, 0, 1).unwrap_err().to_string(),
            p.two.wide_dirs(id, 0, 1).unwrap_err().to_string()
        );
    }
    assert_eq!(
        p.one
            .metrics_candidates(2, 1000001)
            .unwrap_err()
            .to_string(),
        p.two
            .metrics_candidates(2, 1000001)
            .unwrap_err()
            .to_string()
    );
    assert_eq!(
        p.one
            .children(0, SortKey::Disk, usize::MAX)
            .unwrap_err()
            .to_string(),
        p.two
            .children(0, SortKey::Disk, usize::MAX)
            .unwrap_err()
            .to_string()
    );
}

// ---- SNAP-reader2-fix regressions ----

/// Rewrites the v2 file with `sql` applied, then reopens it.
fn tampered(p: Pair, sql: &str) -> (tempfile::TempDir, Snapshot, Snapshot) {
    let Pair {
        _one,
        _two,
        one: _,
        two,
    } = p;
    drop(two);
    let file = std::fs::read_dir(_two.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute_batch(sql).unwrap();
    drop(conn);
    let snap = Snapshot::open(&file).unwrap();
    // Keep a second, independent reader for warmed-cache orderings.
    let snap2 = Snapshot::open(&file).unwrap();
    drop(_one);
    (_two, snap, snap2)
}
fn is_invalid<T: std::fmt::Debug>(r: Result<T, StoreError>) -> bool {
    matches!(r, Err(StoreError::Invalid(_)))
}

#[test]
fn b1_overlapping_chunks_are_invalid_cold_warm_and_range() {
    let rows = tree(5, 12);
    // chunk 4..=5 re-registered as 3..=5, overlapping chunks 2..=3 and 4..=5.
    let sql = "INSERT INTO dir_chunks(first_id,last_id,data) SELECT 3,5,data FROM dir_chunks WHERE first_id=4;";
    let (_d, cold, warm) = tampered(pair(&rows, 0, 64, 2), sql);
    for id in 2..=7 {
        assert!(is_invalid(cold.dir(id)), "cold {id}");
    }
    // Warm the cache with an unaffected chunk, then query in both orders.
    assert!(warm.dir(16).is_ok());
    for id in (0..rows.len() as u32).rev() {
        if (2..=7).contains(&id) {
            assert!(is_invalid(warm.dir(id)), "warm reversed {id}");
        }
    }
    for id in 0..rows.len() as u32 {
        if (2..=7).contains(&id) {
            assert!(is_invalid(warm.dir(id)), "warm forward {id}");
        }
    }
    assert!(is_invalid(cold.subtree_dirs(0, 20, SortKey::Name, 100)));
    assert!(is_invalid(cold.subtree_dirs(0, 20, SortKey::Disk, 100)));
    assert!(is_invalid(cold.wide_dirs(0, 1, 100)));
}

#[test]
fn b1_gaps_between_chunks_are_invalid() {
    let rows = tree(5, 12);
    let (_d, cold, warm) = tampered(
        pair(&rows, 0, 64, 2),
        "DELETE FROM dir_chunks WHERE first_id=4;",
    );
    assert!(is_invalid(cold.dir(2)));
    assert!(is_invalid(cold.dir(6)));
    assert!(warm.dir(16).is_ok());
    assert!(is_invalid(warm.dir(6)));
    assert!(is_invalid(warm.dir(2)));
    assert!(is_invalid(cold.subtree_dirs(0, 20, SortKey::Name, 100)));
    assert!(is_invalid(cold.subtree_dirs(0, 20, SortKey::Disk, 100)));
    assert!(is_invalid(cold.wide_dirs(0, 1, 100)));
}

#[test]
fn s1_shallow_subtree_does_not_decode_deep_chunks() {
    let rows = tree(5, 12);
    let p = pair(&rows, 2, 64, 2);
    let expected: Vec<Vec<DirRow>> = SORTS
        .iter()
        .map(|s| p.one.subtree_dirs(0, 2, *s, 100).unwrap())
        .collect();
    let expected_top: Vec<Vec<DirRow>> = SORTS
        .iter()
        .map(|s| p.one.subtree_dirs(1, 1, *s, 100).unwrap())
        .collect();
    let metrics = p.one.metrics_candidates(2, 100).unwrap();
    let (_d, snap, _) = tampered(p, "UPDATE dir_chunks SET data=x'00' WHERE first_id=8;");
    for (s, want) in SORTS.iter().zip(&expected) {
        eq_rows(want, &snap.subtree_dirs(0, 2, *s, 100).unwrap());
    }
    for (s, want) in SORTS.iter().zip(&expected_top) {
        eq_rows(want, &snap.subtree_dirs(1, 1, *s, 100).unwrap());
    }
    let got = snap.metrics_candidates(2, 100).unwrap();
    eq_rows(&metrics.0, &got.0);
    eq_rows(&metrics.1, &got.1);
    // A query that does need the deep chunk still reports corruption.
    assert!(is_invalid(snap.subtree_dirs(0, 12, SortKey::Name, 100)));
}

#[test]
fn s2_oversized_blob_is_rejected_by_length() {
    let rows = tree(5, 12);
    let sql = "UPDATE dir_chunks SET data=zeroblob(67108864) WHERE first_id=0;";
    let (_d, snap, _) = tampered(pair(&rows, 0, 64, 2), sql);
    let err = snap.root().unwrap_err();
    assert!(
        matches!(&err, StoreError::Invalid(m) if m.contains("dir_chunks.data")),
        "{err:?}"
    );
    // Projected path (wide_dirs with min>0) and a subtree scan.
    assert!(is_invalid(snap.wide_dirs(0, 1, 10)));
    assert!(is_invalid(snap.subtree_dirs(0, 20, SortKey::Disk, 10)));
}

#[test]
fn s3_corrupt_coordinates_and_meta_are_invalid() {
    let rows = tree(5, 12);
    let (_d, snap, _) = tampered(
        pair(&rows, 0, 64, 2),
        "UPDATE dir_chunks SET last_id=-1 WHERE first_id=0;",
    );
    let err = snap.root().unwrap_err();
    assert!(
        matches!(&err, StoreError::Invalid(m) if m.contains("dir_chunks.last_id")),
        "{err:?}"
    );
    let (_d, snap, _) = tampered(
        pair(&rows, 0, 64, 2),
        "UPDATE dir_chunks SET last_id='x' WHERE first_id=4;",
    );
    assert!(is_invalid(snap.dir(5)));
    let (_d, snap, _) = tampered(
        pair(&rows, 0, 64, 2),
        "UPDATE dir_chunks SET last_id=4294967296 WHERE first_id=0;",
    );
    assert!(is_invalid(snap.root()));
    let p = pair(&rows, 0, 64, 2);
    let file = std::fs::read_dir(p._two.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    drop(p.two);
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute("DELETE FROM meta WHERE key='plain_depth'", [])
        .unwrap();
    drop(conn);
    match Snapshot::open(&file) {
        Err(StoreError::Invalid(m)) => assert!(m.contains("plain_depth"), "{m}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    // A legitimately missing id keeps the v1-compatible error.
    let p = pair(&rows, 0, 64, 2);
    assert_eq!(
        p.one.dir(999).unwrap_err().to_string(),
        p.two.dir(999).unwrap_err().to_string()
    );
}
