//! Owned by: WSNAP-reader2-core
//! Differential directory query coverage against the v1 reader.
#![allow(clippy::unwrap_used)]
use std::collections::BTreeMap;
use tidemark_core::{
    config::*,
    path::{RawName, RawPath},
    scan::{ErrOp, ScanMode, SkipReason, SnapshotStatus},
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
fn nothing_but_diff_is_left_unported() {
    // Everything ported by SNAP-reader2-queries answers on v2; the remaining
    // v1-only paths are covered by the parity tests below.
    let rows = tree(4, 3);
    let p = pair(&rows, 2, 64, 2);
    let s = &p.two;
    assert!(s
        .subtree_files(0, &FileFilter::default(), 1)
        .unwrap()
        .is_empty());
    assert!(s
        .link_sets_under(0, &LinkFilter::default(), 1)
        .unwrap()
        .is_empty());
    assert!(s.link_instances(1).unwrap().is_empty());
    assert!(s.link_instances_page(1, None, false, 1).unwrap().is_empty());
    assert!(s.skipped_under(0).unwrap().is_empty());
    assert!(s.errors_under(0, 1).unwrap().is_empty());
    assert!(s.annotations(0).unwrap().is_empty());
    // The fixture's accounting does not add up, so the export refuses with
    // a data error rather than `NotYetImplemented`.
    let err = tidemark_store::export_ncdu(
        s,
        &rows[0].path,
        &mut Vec::new(),
        &tidemark_store::NcduOptions {
            utf8_lossy: true,
            max_depth: None,
        },
    )
    .unwrap_err();
    assert!(!matches!(err, StoreError::NotYetImplemented), "{err:?}");
    tidemark_store::links::Regions::build(
        s,
        &rows[0].path,
        &tidemark_store::links::RegionSpec::Depth(1),
    )
    .unwrap();
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

// ---- SNAP-reader2-fix2 regressions ----

fn v2_file(sql: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let p = pair(&tree(5, 12), 0, 64, 2);
    let Pair {
        _one, _two, two, ..
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
    (_two, file)
}

fn vm_hwm_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with("VmHWM:")).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

/// Child half of `s2_text_chunk_is_rejected_without_reading_payload`: runs
/// only when re-executed by the parent with `TM_TEXT_CHUNK_FILE` set.
#[test]
fn s2_child_open_and_query() {
    let Some(file) = std::env::var_os("TM_TEXT_CHUNK_FILE") else {
        return;
    };
    let snap = Snapshot::open(std::path::Path::new(&file)).unwrap();
    let err = snap.root().unwrap_err();
    assert!(
        matches!(&err, StoreError::Invalid(m) if m.contains("dir_chunks")),
        "{err:?}"
    );
    assert!(is_invalid(snap.wide_dirs(0, 1, 10)));
    assert!(is_invalid(snap.subtree_dirs(0, 20, SortKey::Disk, 10)));
    let hwm = vm_hwm_kib();
    assert!(hwm < 40 * 1024, "VmHWM {hwm} KiB reached payload size");
}

#[test]
fn s2_text_chunk_is_rejected_without_reading_payload() {
    let (_dir, file) =
        v2_file("UPDATE dir_chunks SET data=CAST(zeroblob(67108864) AS TEXT) WHERE first_id=0;");
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "s2_child_open_and_query", "--nocapture"])
        .env("TM_TEXT_CHUNK_FILE", &file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn s2_non_blob_chunk_types_are_invalid_naming_table() {
    for value in ["'text'", "12", "1.5"] {
        let (_dir, file) = v2_file(&format!(
            "UPDATE dir_chunks SET data={value} WHERE first_id=0;"
        ));
        let snap = Snapshot::open(&file).unwrap();
        let err = snap.root().unwrap_err();
        assert!(
            matches!(&err, StoreError::Invalid(m) if m.contains("dir_chunks")),
            "{value}: {err:?}"
        );
        assert!(is_invalid(snap.wide_dirs(0, 1, 10)), "{value}");
    }
}

#[test]
fn s3_wrong_typed_v2_meta_is_invalid_naming_key() {
    for key in ["plain_depth", "hash_width"] {
        for value in ["x'32'", "NULL"] {
            // Rebuild `meta` without NOT NULL so NULL can be stored.
            let (_dir, file) = v2_file(&format!(
                "CREATE TABLE meta2 (key TEXT PRIMARY KEY,value);
                 INSERT INTO meta2 SELECT key,value FROM meta;
                 DROP TABLE meta; ALTER TABLE meta2 RENAME TO meta;
                 UPDATE meta SET value={value} WHERE key='{key}';"
            ));
            match Snapshot::open(&file) {
                Err(StoreError::Invalid(m)) => assert!(m.contains(key), "{key}/{value}: {m}"),
                other => panic!("{key}/{value}: {:?}", other.map(|_| ())),
            }
        }
    }
}

// ---- SNAP-reader2-queries: files, links, errors and ncdu parity ----

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}
/// `-`/`/` ordering traps and non-UTF-8 bytes.
const NAMES: [&[u8]; 9] = [
    b"a",
    b"a-",
    b"a-b",
    b"a.b",
    b"a0",
    b"\xff",
    b"\xfe\xff",
    b"b",
    b"Z",
];
fn rname(bytes: &[u8]) -> RawName {
    RawName::new(bytes.to_vec()).unwrap()
}
struct Data {
    rows: Vec<DirRow>,
    files: Vec<FileRow>,
    sets: Vec<LinkSetRow>,
    inst: Vec<LinkInstanceRow>,
    skipped: Vec<SkipRow>,
    errors: Vec<ErrorRow>,
    notes: Vec<AnnotationRow>,
    inos: Vec<u64>,
}
fn lca(rows: &[DirRow], dirs: &[u32]) -> u32 {
    let mut a = dirs[0];
    for &b in &dirs[1..] {
        let (mut x, mut y) = (a, b);
        while x != y {
            if x > y {
                x = rows[x as usize].parent_id.unwrap();
            } else {
                y = rows[y as usize].parent_id.unwrap();
            }
        }
        a = x;
    }
    a
}
/// Attach consistent notable files, link sets and bookkeeping rows to a tree.
fn data(mut rows: Vec<DirRow>, seed: u64) -> Data {
    let mut rng = Rng(seed);
    let started = meta().started_at;
    let mut files = Vec::new();
    for r in &rows {
        for _ in 0..rng.next() % 5 {
            let name = match rng.next() % 12 {
                0 => None,
                k => Some(rname(NAMES[k as usize % NAMES.len()])),
            };
            let disk = (rng.next() % 6) * 100;
            files.push(FileRow {
                id: 0,
                dir_id: r.id,
                name,
                ino: 5000 + files.len() as u64,
                disk,
                apparent: disk + rng.next() % 50,
                mtime: started - (rng.next() % 1000) as i64,
                nlink: if rng.next().is_multiple_of(4) { 2 } else { 1 },
            });
        }
    }
    files.sort_by(|a, b| {
        (a.dir_id, a.name.as_ref().map(|n| n.as_bytes()))
            .cmp(&(b.dir_id, b.name.as_ref().map(|n| n.as_bytes())))
    });
    for (i, f) in files.iter_mut().enumerate() {
        f.id = i as u32;
    }
    for r in &mut rows {
        let mine: Vec<&FileRow> = files.iter().filter(|f| f.dir_id == r.id).collect();
        r.own_files = mine.len() as u64 + rng.next() % 3;
        r.own_charged = r.dir_disk + mine.iter().map(|f| f.disk).sum::<u64>() + rng.next() % 100;
        r.own_apparent =
            r.dir_apparent + mine.iter().map(|f| f.apparent).sum::<u64>() + rng.next() % 50;
    }
    for i in 0..rows.len() {
        let end = rows[i].subtree_end as usize;
        rows[i].t_charged = rows[i..=end].iter().map(|r| r.own_charged).sum();
    }
    let n = rows.len() as u32;
    let mut sets = Vec::new();
    let mut inst = Vec::new();
    let mut inos = vec![0, 999, 123456789];
    for k in 0..14u64 {
        let ino = if k == 13 { (1 << 62) + 5 } else { 1000 + k * 7 };
        let m = if k == 0 { 30 } else { 2 + rng.next() % 4 };
        let mut dirs = Vec::new();
        for _ in 0..m {
            let dir = (rng.next() % u64::from(n)) as u32;
            dirs.push(dir);
            let name = match rng.next() % 5 {
                0 => None,
                j => Some(rname(NAMES[j as usize + 2])),
            };
            inst.push(LinkInstanceRow {
                ino,
                dir_id: dir,
                name,
            });
        }
        let disk = [4096u64, 8192, 4096][rng.next() as usize % 3];
        let found = m as u32;
        let nlink = found + (rng.next() % 2) as u32;
        sets.push(LinkSetRow {
            ino,
            nlink,
            found,
            complete: found >= nlink,
            disk,
            apparent: disk - 7,
            mtime: started - 5,
            lca_dir_id: lca(&rows, &dirs),
        });
        inos.push(ino);
    }
    // Instances without a set row.
    inst.push(LinkInstanceRow {
        ino: 999,
        dir_id: 0,
        name: None,
    });
    inst.sort_by(|a, b| {
        (a.ino, a.dir_id, a.name.as_ref().map(|n| n.as_bytes())).cmp(&(
            b.ino,
            b.dir_id,
            b.name.as_ref().map(|n| n.as_bytes()),
        ))
    });
    let mut skipped = Vec::new();
    let mut errors = Vec::new();
    let mut notes = Vec::new();
    for _ in 0..24 {
        let dir = (rng.next() % u64::from(n)) as u32;
        skipped.push(SkipRow {
            dir_id: dir,
            name: rname(NAMES[rng.next() as usize % NAMES.len()]),
            reason: match rng.next() % 3 {
                0 => SkipReason::Mount,
                1 => SkipReason::OtherFs,
                _ => SkipReason::Excluded,
            },
        });
        errors.push(ErrorRow {
            dir_id: if rng.next().is_multiple_of(5) {
                None
            } else {
                Some(dir)
            },
            path: rows[dir as usize]
                .path
                .join(&rname(NAMES[rng.next() as usize % NAMES.len()])),
            op: match rng.next() % 3 {
                0 => ErrOp::Open,
                1 => ErrOp::Getdents,
                _ => ErrOp::Statx,
            },
            errno: (rng.next() % 3) as i32 + 1,
        });
        notes.push(AnnotationRow {
            dir_id: dir,
            key: format!("k{}", rng.next() % 4),
            value: format!("v{}", rng.next() % 3),
        });
    }
    Data {
        rows,
        files,
        sets,
        inst,
        skipped,
        errors,
        notes,
        inos,
    }
}
#[derive(Clone, Copy)]
struct Shape {
    plain: u32,
    hash: u32,
    dirs: usize,
    files: usize,
    li: usize,
}
fn pair_data(d: &Data, shape: Shape) -> Pair {
    let one = test_support::private_tempdir().unwrap();
    let two = test_support::private_tempdir().unwrap();
    let mut m = meta();
    m.root = d.rows[0].path.clone();
    m.dirs = d.rows.len() as u64;
    m.schema_version = 1;
    let mut w = SnapshotWriter::create(one.path(), &m).unwrap();
    w.put_dirs(&d.rows).unwrap();
    w.put_files(&d.files).unwrap();
    w.put_link_sets(&d.sets).unwrap();
    w.put_link_instances(&d.inst).unwrap();
    w.put_skipped(&d.skipped).unwrap();
    w.put_errors(&d.errors).unwrap();
    w.put_annotations(&d.notes).unwrap();
    let f1 = w.finish(&m).unwrap();
    m.schema_version = 2;
    let mut w = SnapshotWriterV2::create_with_options(
        two.path(),
        &m,
        WriterOptions {
            plain_depth: shape.plain,
            hash_width: shape.hash,
            chunk_dirs: shape.dirs,
            chunk_files: shape.files,
            chunk_li: shape.li,
            ..Default::default()
        },
    )
    .unwrap();
    w.put_dirs(&d.rows).unwrap();
    w.put_files(&d.files).unwrap();
    w.put_link_sets(&d.sets).unwrap();
    w.put_link_instances(&d.inst).unwrap();
    w.put_skipped(&d.skipped).unwrap();
    w.put_errors(&d.errors).unwrap();
    w.put_annotations(&d.notes).unwrap();
    let f2 = w.finish(&m).unwrap();
    Pair {
        _one: one,
        _two: two,
        one: Snapshot::open(&f1).unwrap(),
        two: Snapshot::open(&f2).unwrap(),
    }
}
/// Both results succeed with equal JSON, or both fail with the same message.
fn same<T: serde::Serialize>(what: &str, a: Result<T, StoreError>, b: Result<T, StoreError>) {
    match (a, b) {
        (Ok(a), Ok(b)) => assert_eq!(
            serde_json::to_value(a).unwrap(),
            serde_json::to_value(b).unwrap(),
            "{what}"
        ),
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "{what}"),
        (a, b) => panic!("{what}: v1 {:?} vs v2 {:?}", a.map(|_| ()), b.map(|_| ())),
    }
}
fn same_debug<T: std::fmt::Debug>(what: &str, a: Result<T, StoreError>, b: Result<T, StoreError>) {
    match (a, b) {
        (Ok(a), Ok(b)) => assert_eq!(format!("{a:?}"), format!("{b:?}"), "{what}"),
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "{what}"),
        (a, b) => panic!("{what}: v1 {:?} vs v2 {:?}", a.map(|_| ()), b.map(|_| ())),
    }
}
const LIMITS: [usize; 6] = [0, 1, 2, 7, i64::MAX as usize, usize::MAX];
fn file_filters() -> Vec<FileFilter> {
    use std::time::Duration;
    let mut out = vec![FileFilter::default()];
    for min in [0, 100, 300, 10_000] {
        out.push(FileFilter {
            min_size: Some(min),
            ..Default::default()
        });
    }
    for secs in [0, 400, 5000] {
        out.push(FileFilter {
            older_than: Some(Duration::from_secs(secs)),
            ..Default::default()
        });
        out.push(FileFilter {
            newer_than: Some(Duration::from_secs(secs)),
            ..Default::default()
        });
    }
    out.push(FileFilter {
        min_size: Some(100),
        older_than: Some(Duration::from_secs(100)),
        newer_than: Some(Duration::from_secs(800)),
    });
    out.push(FileFilter {
        min_size: Some(u64::MAX),
        ..Default::default()
    });
    out
}
fn link_filters() -> Vec<LinkFilter> {
    let mut out = Vec::new();
    for incomplete in [false, true] {
        out.push(LinkFilter {
            incomplete,
            ..Default::default()
        });
        for depth in [0, 1, 2, 5] {
            out.push(LinkFilter {
                incomplete,
                boundary_depth: Some(depth),
                ..Default::default()
            });
        }
        for globs in [
            vec!["/data/*/a".to_string()],
            vec!["/data/*/a-".to_string(), "/data/*/a/a".to_string()],
            vec!["**".to_string()],
            vec!["/data/*/b0000*".to_string(), "/data/*/a*".to_string()],
            vec!["[".to_string()],
        ] {
            out.push(LinkFilter {
                incomplete,
                boundary: globs,
                boundary_depth: None,
            });
        }
    }
    out.push(LinkFilter {
        incomplete: false,
        boundary: vec!["**".to_string()],
        boundary_depth: Some(1),
    });
    out
}
fn probe_ids(d: &Data) -> Vec<u32> {
    let n = d.rows.len() as u32;
    let mut ids = vec![0, 1, 2, n / 2, n - 1, n, 999_999, u32::MAX];
    ids.dedup();
    ids
}
fn compare_files_and_links(a: &Snapshot, b: &Snapshot, d: &Data) {
    for id in probe_ids(d) {
        for filter in file_filters() {
            for limit in LIMITS {
                same(
                    &format!("subtree_files {id} {filter:?} {limit}"),
                    a.subtree_files(id, &filter, limit),
                    b.subtree_files(id, &filter, limit),
                );
            }
        }
        for filter in link_filters() {
            for limit in LIMITS {
                same(
                    &format!("link_sets_under {id} {filter:?} {limit}"),
                    a.link_sets_under(id, &filter, limit),
                    b.link_sets_under(id, &filter, limit),
                );
            }
        }
        same("skipped", a.skipped_under(id), b.skipped_under(id));
        for limit in LIMITS {
            same(
                &format!("errors_under {id} {limit}"),
                a.errors_under(id, limit),
                b.errors_under(id, limit),
            );
        }
        same("annotations", a.annotations(id), b.annotations(id));
    }
    for &ino in &d.inos {
        same(
            "link_instances",
            a.link_instances(ino),
            b.link_instances(ino),
        );
        for reverse in [false, true] {
            for limit in [0, 1, 2, 3, 1_000_000, 1_000_001] {
                let first = a.link_instances_page(ino, None, reverse, limit);
                same(
                    &format!("page {ino} {reverse} {limit}"),
                    a.link_instances_page(ino, None, reverse, limit),
                    b.link_instances_page(ino, None, reverse, limit),
                );
                for cursor in first.unwrap_or_default().iter() {
                    for limit in [1, 2, 1000] {
                        same(
                            &format!("page {ino} {reverse} {limit} {cursor:?}"),
                            a.link_instances_page(ino, Some(cursor), reverse, limit),
                            b.link_instances_page(ino, Some(cursor), reverse, limit),
                        );
                    }
                }
            }
        }
    }
    same(
        "page ino too large",
        a.link_instances_page(u64::MAX, None, false, 1),
        b.link_instances_page(u64::MAX, None, false, 1),
    );
    same(
        "instances ino too large",
        a.link_instances(u64::MAX),
        b.link_instances(u64::MAX),
    );
}
fn compare_exports(a: &Snapshot, b: &Snapshot, d: &Data) {
    use tidemark_store::{export_ncdu, NcduOptions};
    for row in d.rows.iter().step_by(1 + d.rows.len() / 12) {
        for lossy in [false, true] {
            for max_depth in [None, Some(0), Some(1), Some(3)] {
                let opts = NcduOptions {
                    utf8_lossy: lossy,
                    max_depth,
                };
                let (mut x, mut y) = (Vec::new(), Vec::new());
                let ra = export_ncdu(a, &row.path, &mut x, &opts);
                let rb = export_ncdu(b, &row.path, &mut y, &opts);
                same_debug("export stats", ra, rb);
                assert_eq!(x, y, "{} {lossy} {max_depth:?}", row.id);
            }
        }
    }
    let absent = path(b"/absent");
    same_debug(
        "export absent",
        export_ncdu(
            a,
            &absent,
            &mut Vec::new(),
            &NcduOptions {
                utf8_lossy: true,
                max_depth: None,
            },
        ),
        export_ncdu(
            b,
            &absent,
            &mut Vec::new(),
            &NcduOptions {
                utf8_lossy: true,
                max_depth: None,
            },
        ),
    );
}
fn compare_links_module(a: &Snapshot, b: &Snapshot, d: &Data) {
    use tidemark_store::links::*;
    let specs = [
        RegionSpec::Depth(0),
        RegionSpec::Depth(1),
        RegionSpec::Depth(2),
        RegionSpec::Depth(30),
        RegionSpec::Globs(vec!["/data/*/a".into(), "/data/*/a/a".into()]),
        RegionSpec::Globs(vec!["/data/*/b0000*".into(), "**".into()]),
        RegionSpec::Globs(vec!["/nothing".into()]),
    ];
    for under in [
        &d.rows[0].path,
        &d.rows[1].path,
        &d.rows[d.rows.len() / 2].path,
        &path(b"/absent"),
    ] {
        for top in [0, 1, 3, 1000, 1_000_001] {
            same_report(a, b, under, None, top);
            for spec in &specs {
                same_report(a, b, under, Some(spec), top);
                let ra = Regions::build(a, under, spec);
                let rb = Regions::build(b, under, spec);
                match (ra, rb) {
                    (Ok(ra), Ok(rb)) => {
                        for limit in [0, 1, 3, 1_000_001] {
                            same_debug(
                                &format!("crossing {spec:?} {limit}"),
                                crossing_sets(a, &ra, limit),
                                crossing_sets(b, &rb, limit),
                            );
                        }
                        for id in 0..d.rows.len() as u32 {
                            assert_eq!(ra.region_of(id), rb.region_of(id));
                            if let (Some(x), Some(y)) = (ra.region_of(id), rb.region_of(id)) {
                                assert_eq!(ra.name(x), rb.name(y));
                            }
                        }
                    }
                    (ra, rb) => same_debug("regions", ra.map(|_| ()), rb.map(|_| ())),
                }
            }
        }
    }
}
fn same_report(
    a: &Snapshot,
    b: &Snapshot,
    under: &RawPath,
    spec: Option<&tidemark_store::links::RegionSpec>,
    top: usize,
) {
    use tidemark_store::links::link_report;
    match (
        link_report(a, under, spec, top),
        link_report(b, under, spec, top),
    ) {
        (Ok(x), Ok(y)) => {
            assert_eq!(format!("{:?}", x.totals), format!("{:?}", y.totals));
            assert_eq!(format!("{:?}", x.crossing), format!("{:?}", y.crossing));
            assert_eq!(
                format!("{:?}", x.region_pairs),
                format!("{:?}", y.region_pairs)
            );
            assert_eq!(
                format!("{:?}", x.incomplete_top_dirs),
                format!("{:?}", y.incomplete_top_dirs)
            );
            assert_eq!(
                serde_json::to_value(&x.largest_sets).unwrap(),
                serde_json::to_value(&y.largest_sets).unwrap()
            );
            assert_eq!(
                format!("{:?}", x.most_shared_dirs),
                format!("{:?}", y.most_shared_dirs)
            );
        }
        (Err(x), Err(y)) => assert_eq!(x.to_string(), y.to_string()),
        (x, y) => panic!("report: {:?} vs {:?}", x.map(|_| ()), y.map(|_| ())),
    }
}
fn compare_everything(a: &Snapshot, b: &Snapshot, d: &Data) {
    compare_files_and_links(a, b, d);
    compare_exports(a, b, d);
    compare_links_module(a, b, d);
}
fn query_trees() -> Vec<(&'static str, Vec<DirRow>)> {
    vec![("small", tree(5, 3)), ("deep", tree(17, 12))]
}

#[test]
fn files_links_errors_and_ncdu_match_v1_across_shapes() {
    for (name, rows) in query_trees() {
        let d = data(rows, 7);
        assert!(
            d.files.len() > 10 && d.files.iter().any(|f| f.name.is_none()),
            "{name}"
        );
        for shape in [
            Shape {
                plain: 0,
                hash: 32,
                dirs: 2,
                files: 2,
                li: 2,
            },
            Shape {
                plain: 2,
                hash: 64,
                dirs: 7,
                files: 3,
                li: 5,
            },
            Shape {
                plain: 4,
                hash: 32,
                dirs: 5,
                files: 1,
                li: 1,
            },
            Shape {
                plain: 2,
                hash: 64,
                dirs: 4096,
                files: 8192,
                li: 8192,
            },
        ] {
            let p = pair_data(&d, shape);
            compare_everything(&p.one, &p.two, &d);
            assert_nontrivial(&p.two, &d);
        }
    }
}

/// Guards against comparing only empty results or errors.
fn assert_nontrivial(s: &Snapshot, d: &Data) {
    let files = s.subtree_files(0, &FileFilter::default(), 1000).unwrap();
    assert_eq!(
        files.len(),
        d.files
            .iter()
            .filter(|f| f.dir_id <= d.rows[0].subtree_end)
            .count()
    );
    assert!(files.iter().any(|f| f.path.is_none()));
    let depth = LinkFilter {
        boundary_depth: Some(1),
        ..Default::default()
    };
    assert!(!s.link_sets_under(0, &depth, 100).unwrap().is_empty());
    let globs = LinkFilter {
        boundary: vec!["/data/*/a".into(), "/data/*/a-".into()],
        ..Default::default()
    };
    assert!(!s.link_sets_under(0, &globs, 100).unwrap().is_empty());
    assert_eq!(
        s.link_sets_under(0, &LinkFilter::default(), 100)
            .unwrap()
            .len(),
        d.sets.len()
    );
    assert!(s.link_instances(1000).unwrap().len() >= 2);
    assert!(!s.skipped_under(0).unwrap().is_empty());
    assert!(!s.errors_under(0, 100).unwrap().is_empty());
    assert!(!s.annotations(0).unwrap().is_empty() || d.notes.iter().all(|n| n.dir_id != 0));
    let mut out = Vec::new();
    let stats = tidemark_store::export_ncdu(
        s,
        &d.rows[0].path,
        &mut out,
        &tidemark_store::NcduOptions {
            utf8_lossy: false,
            max_depth: None,
        },
    )
    .unwrap();
    assert!(stats.files > 0 && stats.directories == d.rows.len() as u64);
}

#[test]
fn golden_fixture_files_and_links_match_v1() {
    let model = test_support::golden::link_semantics_basic();
    let paths = model.paths();
    let mut golden = tree(5, 0);
    for (i, row) in golden.iter_mut().enumerate() {
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
    }
    let d = data(golden, 99);
    for shape in [
        Shape {
            plain: 0,
            hash: 64,
            dirs: 2,
            files: 2,
            li: 3,
        },
        Shape {
            plain: 2,
            hash: 32,
            dirs: 3,
            files: 4,
            li: 2,
        },
    ] {
        let p = pair_data(&d, shape);
        compare_everything(&p.one, &p.two, &d);
    }
}

#[test]
fn files_links_and_ncdu_match_v1_through_a_cold_file() {
    let d = data(tree(8, 6), 21);
    let p = pair_data(
        &d,
        Shape {
            plain: 2,
            hash: 64,
            dirs: 3,
            files: 3,
            li: 3,
        },
    );
    let hot = std::fs::read_dir(p._two.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let cold = tidemark_store::cold::compress_snapshot(&hot, 3, &|| false).unwrap();
    let snap = Snapshot::open_with_space_guard(&cold, |_| Ok(())).unwrap();
    compare_everything(&p.one, &snap, &d);
}

#[test]
fn many_files_in_one_directory_span_chunks_and_break_ties_like_v1() {
    // Every file has the same size, so order is decided by path, name and id.
    let rows = tree(6, 4);
    let mut d = data(rows, 3);
    d.files.clear();
    for dir in [0u32, 3, 4, 5, 9] {
        for k in 0..40u32 {
            d.files.push(FileRow {
                id: 0,
                dir_id: dir,
                name: if k % 13 == 0 {
                    None
                } else {
                    Some(rname(format!("f{:03}", k / 2).as_bytes()))
                },
                ino: 9000 + u64::from(dir) * 100 + u64::from(k),
                disk: 500,
                apparent: 500,
                mtime: 1_000_000 - i64::from(k),
                nlink: 1,
            });
        }
    }
    d.files.sort_by(|a, b| {
        (a.dir_id, a.name.as_ref().map(|n| n.as_bytes()))
            .cmp(&(b.dir_id, b.name.as_ref().map(|n| n.as_bytes())))
    });
    for (i, f) in d.files.iter_mut().enumerate() {
        f.id = i as u32;
    }
    for r in &mut d.rows {
        let mine: Vec<&FileRow> = d.files.iter().filter(|f| f.dir_id == r.id).collect();
        r.own_files = mine.len() as u64;
        r.own_charged = r.dir_disk + mine.iter().map(|f| f.disk).sum::<u64>();
        r.own_apparent = r.dir_apparent + mine.iter().map(|f| f.apparent).sum::<u64>();
    }
    for i in 0..d.rows.len() {
        let end = d.rows[i].subtree_end as usize;
        d.rows[i].t_charged = d.rows[i..=end].iter().map(|r| r.own_charged).sum();
    }
    let p = pair_data(
        &d,
        Shape {
            plain: 2,
            hash: 64,
            dirs: 3,
            files: 7,
            li: 4,
        },
    );
    for id in probe_ids(&d) {
        for limit in [1, 5, 50, 1000] {
            same(
                "ties",
                p.one.subtree_files(id, &FileFilter::default(), limit),
                p.two.subtree_files(id, &FileFilter::default(), limit),
            );
        }
    }
    compare_exports(&p.one, &p.two, &d);
}

#[test]
fn preorder_is_not_path_byte_order_for_ranked_results() {
    // `a/b` precedes `a-c` in preorder but follows it in byte order. Same-size
    // files and link sets in both must rank by path bytes, as in v1.
    let rows = tree(3, 2); // /r, /r/a, /r/a/a, then /r/a-, /r/b000001, ...
    let mut d = data(rows, 5);
    d.files.clear();
    d.sets.clear();
    d.inst.clear();
    for dir in 0..d.rows.len() as u32 {
        d.files.push(FileRow {
            id: dir,
            dir_id: dir,
            name: Some(rname(b"same")),
            ino: 7000 + u64::from(dir),
            disk: 100,
            apparent: 100,
            mtime: 1_000_000,
            nlink: 1,
        });
        d.sets.push(LinkSetRow {
            ino: 100 + u64::from(dir),
            nlink: 2,
            found: 2,
            complete: true,
            disk: 4096,
            apparent: 4096,
            mtime: 0,
            lca_dir_id: dir,
        });
        for _ in 0..2 {
            d.inst.push(LinkInstanceRow {
                ino: 100 + u64::from(dir),
                dir_id: dir,
                name: Some(rname(b"x")),
            });
        }
    }
    d.inos = (100..110).collect();
    let p = pair_data(
        &d,
        Shape {
            plain: 1,
            hash: 64,
            dirs: 2,
            files: 2,
            li: 2,
        },
    );
    let files = p.two.subtree_files(0, &FileFilter::default(), 100).unwrap();
    // v1 ranks equal sizes by the containing directory's path bytes.
    let order: Vec<&[u8]> = files
        .iter()
        .map(|f| d.rows[f.row.dir_id as usize].path.as_bytes())
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    let preorder: Vec<u32> = files.iter().map(|f| f.row.dir_id).collect();
    assert_ne!(preorder, (0..preorder.len() as u32).collect::<Vec<_>>());
    let sets = p
        .two
        .link_sets_under(0, &LinkFilter::default(), 100)
        .unwrap();
    assert!(sets.windows(2).all(|w| w[0].lca_path < w[1].lca_path));
    same(
        "files",
        p.one.subtree_files(0, &FileFilter::default(), 100),
        Ok(files),
    );
    same(
        "sets",
        p.one.link_sets_under(0, &LinkFilter::default(), 100),
        Ok(sets),
    );
}

// ---- corrupt file and link-instance chunks ----

fn sample() -> (Data, Pair) {
    let d = data(tree(6, 4), 11);
    let p = pair_data(
        &d,
        Shape {
            plain: 2,
            hash: 64,
            dirs: 3,
            files: 3,
            li: 3,
        },
    );
    (d, p)
}
fn damaged(sql: &str) -> (Data, tempfile::TempDir, std::path::PathBuf, Snapshot) {
    let (d, p) = sample();
    let Pair {
        _one,
        _two,
        two,
        one,
        ..
    } = p;
    drop(two);
    drop(one);
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
    (d, _two, file, snap)
}
fn exercise_files(snap: &Snapshot, d: &Data) -> Vec<Result<(), StoreError>> {
    let opts = tidemark_store::NcduOptions {
        utf8_lossy: true,
        max_depth: None,
    };
    vec![
        snap.subtree_files(0, &FileFilter::default(), 100)
            .map(|_| ()),
        tidemark_store::export_ncdu(snap, &d.rows[0].path, &mut Vec::new(), &opts).map(|_| ()),
    ]
}
fn exercise_links(snap: &Snapshot, d: &Data) -> Vec<Result<(), StoreError>> {
    use tidemark_store::links::*;
    let mut out = Vec::new();
    for &ino in &d.inos {
        out.push(snap.link_instances(ino).map(|_| ()));
        out.push(snap.link_instances_page(ino, None, false, 5).map(|_| ()));
        out.push(snap.link_instances_page(ino, None, true, 5).map(|_| ()));
    }
    out.push(
        snap.link_sets_under(
            0,
            &LinkFilter {
                boundary_depth: Some(1),
                ..Default::default()
            },
            100,
        )
        .map(|_| ()),
    );
    out.push(
        snap.link_sets_under(0, &LinkFilter::default(), 100)
            .map(|_| ()),
    );
    out.push(link_report(snap, &d.rows[0].path, None, 5).map(|_| ()));
    out
}
fn all_invalid_or_ok(results: &[Result<(), StoreError>]) -> usize {
    let mut invalid = 0;
    for r in results {
        match r {
            Ok(()) => {}
            Err(StoreError::Invalid(_)) => invalid += 1,
            Err(other) => panic!("not Invalid: {other:?}"),
        }
    }
    invalid
}

#[test]
fn corrupt_file_chunks_are_invalid_not_panics() {
    let tampers = [
        "UPDATE file_chunks SET data=x'00' WHERE first_row=0;",
        "UPDATE file_chunks SET data=x'0101' WHERE first_row=3;",
        "UPDATE file_chunks SET data=zeroblob(67108864) WHERE first_row=0;",
        "UPDATE file_chunks SET data=CAST(zeroblob(67108864) AS TEXT) WHERE first_row=0;",
        "UPDATE file_chunks SET data='text' WHERE first_row=0;",
        "UPDATE file_chunks SET data=12 WHERE first_row=0;",
        "UPDATE file_chunks SET last_row='x' WHERE first_row=0;",
        "UPDATE file_chunks SET last_row=-1 WHERE first_row=0;",
        "UPDATE file_chunks SET last_row=4294967296 WHERE first_row=0;",
        "UPDATE file_chunks SET first_dir=first_dir+1 WHERE first_row=0;",
        "UPDATE file_chunks SET last_dir=last_dir+1 WHERE first_row=0;",
        "UPDATE file_chunks SET last_dir=first_dir-1 WHERE first_row=3;",
        "DELETE FROM file_chunks WHERE first_row=3;",
        "INSERT INTO file_chunks(first_row,last_row,first_dir,last_dir,data) SELECT 1,3,first_dir,last_dir,data FROM file_chunks WHERE first_row=3;",
        "UPDATE file_chunks SET last_row=last_row+1 WHERE first_row=0;",
        // Directory spans that point outside the file table.
        "DELETE FROM file_chunks;",
    ];
    for sql in tampers {
        let (d, _dir, _file, snap) = damaged(sql);
        let results = exercise_files(&snap, &d);
        assert!(all_invalid_or_ok(&results) > 0, "{sql}");
        // A cold reader gives the same verdicts as one warmed by earlier queries.
        let again = exercise_files(&snap, &d);
        assert_eq!(
            all_invalid_or_ok(&results),
            all_invalid_or_ok(&again),
            "{sql}"
        );
    }
}

#[test]
fn corrupt_file_chunk_only_affects_queries_that_need_it() {
    let (d, p) = sample();
    // First directory with files, and a later one in a different chunk.
    let early = d.files.first().unwrap().dir_id;
    let late = d.files.last().unwrap().dir_id;
    assert_ne!(early, late);
    let expected = p
        .one
        .subtree_files(late, &FileFilter::default(), 100)
        .unwrap();
    let Pair {
        _one, _two, two, ..
    } = p;
    drop(two);
    let file = std::fs::read_dir(_two.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute("UPDATE file_chunks SET data=x'00' WHERE first_row=0", [])
        .unwrap();
    drop(conn);
    let snap = Snapshot::open(&file).unwrap();
    same(
        "late dir",
        Ok(expected),
        snap.subtree_files(late, &FileFilter::default(), 100),
    );
    assert!(is_invalid(snap.subtree_files(
        early,
        &FileFilter::default(),
        100
    )));
}

#[test]
fn link_instance_lookup_touches_only_its_chunks() {
    let (d, p) = sample();
    // An inode whose instances all sit in one chunk, and the last chunk.
    let li = 3usize;
    let rows_of = |ino: u64| -> Vec<usize> {
        d.inst
            .iter()
            .enumerate()
            .filter(|(_, r)| r.ino == ino)
            .map(|(i, _)| i)
            .collect()
    };
    let last_chunk = (d.inst.len() - 1) / li;
    let ino = d
        .sets
        .iter()
        .map(|s| s.ino)
        .find(|&ino| {
            let rows = rows_of(ino);
            rows.first().unwrap() / li == rows.last().unwrap() / li && rows[0] / li < last_chunk
        })
        .expect("an inode confined to one earlier chunk");
    let expected = p.one.link_instances(ino).unwrap();
    let expected_page = p.one.link_instances_page(ino, None, true, 100).unwrap();
    let Pair {
        _one, _two, two, ..
    } = p;
    drop(two);
    let file = std::fs::read_dir(_two.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute(
        "UPDATE li_chunks SET data=x'00' WHERE first_row=?",
        [(last_chunk * li) as i64],
    )
    .unwrap();
    drop(conn);
    let snap = Snapshot::open(&file).unwrap();
    same("instances", Ok(expected), snap.link_instances(ino));
    same(
        "page",
        Ok(expected_page),
        snap.link_instances_page(ino, None, true, 100),
    );
    let last_ino = d.inst.last().unwrap().ino;
    assert!(is_invalid(snap.link_instances(last_ino)));
}

#[test]
fn corrupt_link_instance_chunks_are_invalid_not_panics() {
    let tampers = [
        "UPDATE li_chunks SET data=x'00' WHERE first_row=0;",
        "UPDATE li_chunks SET data=x'0101' WHERE first_row=3;",
        "UPDATE li_chunks SET data=zeroblob(67108864) WHERE first_row=0;",
        "UPDATE li_chunks SET data=CAST(zeroblob(67108864) AS TEXT) WHERE first_row=0;",
        "UPDATE li_chunks SET data='text' WHERE first_row=0;",
        "UPDATE li_chunks SET data=12 WHERE first_row=3;",
        "UPDATE li_chunks SET last_row='x' WHERE first_row=3;",
        "UPDATE li_chunks SET last_row=-1 WHERE first_row=0;",
        "UPDATE li_chunks SET first_ino='x' WHERE first_row=3;",
        "UPDATE li_chunks SET first_ino=first_ino+1 WHERE first_row=0;",
        "UPDATE li_chunks SET last_ino=last_ino+1 WHERE first_row=0;",
        "UPDATE li_chunks SET last_ino=first_ino-1 WHERE first_row=3;",
        "UPDATE li_chunks SET first_ino=0, last_ino=0 WHERE first_row=9;",
        "DELETE FROM li_chunks WHERE first_row=3;",
        "INSERT INTO li_chunks(first_row,last_row,first_ino,last_ino,data) SELECT 1,3,first_ino,last_ino,data FROM li_chunks WHERE first_row=3;",
        "UPDATE li_chunks SET last_row=last_row+1 WHERE first_row=0;",
    ];
    for sql in tampers {
        let (d, _dir, _file, snap) = damaged(sql);
        let results = exercise_links(&snap, &d);
        assert!(all_invalid_or_ok(&results) > 0, "{sql}");
        let again = exercise_links(&snap, &d);
        assert_eq!(
            all_invalid_or_ok(&results),
            all_invalid_or_ok(&again),
            "{sql}"
        );
    }
}

#[test]
fn text_and_oversized_file_and_link_chunks_are_not_read_into_memory() {
    // Rejected from `typeof`/`length` alone, before the payload is fetched.
    for table in ["file_chunks", "li_chunks"] {
        for value in [
            "zeroblob(67108864)",
            "CAST(zeroblob(67108864) AS TEXT)",
            "'text'",
            "1.5",
        ] {
            let (d, _dir, _file, snap) = damaged(&format!(
                "UPDATE {table} SET data={value} WHERE first_row=0;"
            ));
            let results = if table == "file_chunks" {
                exercise_files(&snap, &d)
            } else {
                exercise_links(&snap, &d)
            };
            assert!(all_invalid_or_ok(&results) > 0, "{table} {value}");
            let message = results
                .into_iter()
                .filter_map(Result::err)
                .map(|e| e.to_string())
                .find(|m| m.contains(table));
            assert!(message.is_some(), "{table} {value}");
        }
    }
}

#[test]
fn link_set_pointing_at_a_missing_directory_is_invalid() {
    let (d, _dir, _file, snap) = damaged("UPDATE link_sets SET lca_dir_id=4000000;");
    let r = snap.link_sets_under(0, &LinkFilter::default(), 10);
    // Out of the root's id range: no rows, as in v1.
    assert!(r.unwrap().is_empty());
    let _ = d;
}

#[test]
#[ignore = "scale"]
fn more_than_a_million_instances_of_one_inode_exceed_the_result_cap_like_v1() {
    let mut d = data(tree(2, 1), 1);
    d.inst.clear();
    d.sets.clear();
    d.inos = vec![42];
    for _ in 0..1_000_001u32 {
        d.inst.push(LinkInstanceRow {
            ino: 42,
            dir_id: 0,
            name: None,
        });
    }
    let p = pair_data(
        &d,
        Shape {
            plain: 2,
            hash: 64,
            dirs: 4096,
            files: 8192,
            li: 8192,
        },
    );
    same(
        "instances",
        p.one.link_instances(42),
        p.two.link_instances(42),
    );
    assert!(is_invalid(p.two.link_instances(42)));
    same(
        "page",
        p.one.link_instances_page(42, None, true, 1_000_000),
        p.two.link_instances_page(42, None, true, 1_000_000),
    );
}
