//! Owned by: W3-B
//! Synthetic golden and region contract evidence.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::BTreeMap;
use tidemark_core::{
    config::*,
    path::{RawName, RawPath},
    scan::{ScanMode, SnapshotStatus},
    snapshot::*,
};
use tidemark_store::{links::*, reader::Snapshot, writer::SnapshotWriter};
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

fn fixture() -> (tempfile::TempDir, Snapshot) {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = meta();
    m.root = path(b"/r");
    let mut w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    let mut rows = Vec::new();
    for (id, g) in test_support::golden::LINK_SEMANTICS_BASIC
        .iter()
        .enumerate()
    {
        let mut d = dir(id as u32);
        d.path = path(g.path.as_bytes());
        d.path_hash = d.path.hash64();
        d.name = if id == 0 {
            b"/r".to_vec()
        } else {
            d.path.file_name().unwrap().as_bytes().to_vec()
        };
        d.parent_id = match id {
            0 => None,
            4 | 5 => Some(3),
            _ => Some(0),
        };
        d.depth = match id {
            0 => 0,
            4 | 5 => 2,
            _ => 1,
        };
        d.subtree_end = match id {
            0 | 3 => 5,
            _ => id as u32,
        };
        d.t_disk = g.values[0];
        d.t_charged = g.values[1];
        d.t_reclaimable = g.values[2];
        rows.push(d);
    }
    w.put_dirs(&rows).unwrap();
    let sets: Vec<_> = [
        (100, 2, 2, 4096, 0),
        (102, 3, 1, 8192, 2),
        (103, 2, 2, 12288, 3),
    ]
    .into_iter()
    .map(|(ino, nlink, found, disk, lca_dir_id)| LinkSetRow {
        ino,
        nlink,
        found,
        disk,
        lca_dir_id,
        complete: found >= nlink,
        apparent: disk,
        mtime: 0,
    })
    .collect();
    w.put_link_sets(&sets).unwrap();
    let instances: Vec<_> = [
        (100, 1, "x"),
        (100, 2, "y"),
        (102, 2, "z"),
        (103, 4, "w"),
        (103, 5, "v"),
    ]
    .into_iter()
    .map(|(ino, dir_id, n)| LinkInstanceRow {
        ino,
        dir_id,
        name: Some(RawName::new(n.as_bytes().to_vec()).unwrap()),
    })
    .collect();
    w.put_link_instances(&instances).unwrap();
    let p = w.finish(&m).unwrap();
    (tmp, Snapshot::open(&p).unwrap())
}
#[test]
fn link_semantics_basic_golden() {
    let (_tmp, s) = fixture();
    let root = path(b"/r");
    let r = Regions::build(&s, &root, &RegionSpec::Depth(1)).unwrap();
    let (sets, t) = crossing_sets(&s, &r, 10).unwrap();
    assert_eq!(t.sets, 1);
    assert_eq!(t.disk, 4096);
    assert_eq!(sets[0].ino, 100);
    assert_eq!(sets[0].sample_dirs, vec![1, 2]);
    assert_eq!(r.name(sets[0].regions[0].unwrap()), &path(b"/r/a"));
    assert_eq!(r.name(sets[0].regions[1].unwrap()), &path(b"/r/b"));
    assert_eq!(r.region_of(4), r.region_of(5));
    assert_eq!(r.name(r.region_of(4).unwrap()), &path(b"/r/c"));
    let report = link_report(&s, &root, Some(&RegionSpec::Depth(1)), 10).unwrap();
    assert_eq!(report.totals.sets, 3);
    assert_eq!(report.totals.instances, 5);
    assert_eq!(report.totals.disk, 24576);
    assert_eq!(report.totals.incomplete_sets, 1);
    assert_eq!(report.totals.incomplete_disk, 8192);
    assert_eq!(
        report.incomplete_top_dirs,
        vec![(path(b"/r"), 8192, 1), (path(b"/r/b"), 8192, 1)]
    );
    assert_eq!(report.largest_sets[0].row.ino, 103);
    assert_eq!(report.largest_sets[0].lca_path, path(b"/r/c"));
    assert_eq!(report.region_pairs[0].sets, 1);
    assert_eq!(report.region_pairs[0].disk, 4096);
    let (empty, t) = crossing_sets(&s, &r, 0).unwrap();
    assert!(empty.is_empty());
    assert_eq!(t.sets, 1);
}
#[test]
fn nested_globs_and_depth_are_relative() {
    let (_tmp, s) = fixture();
    let r = Regions::build(
        &s,
        &path(b"/r"),
        &RegionSpec::Globs(vec!["/r/c".into(), "/r/c/d".into()]),
    )
    .unwrap();
    assert_eq!(r.region_of(1), None);
    assert_eq!(r.region_of(6), None);
    assert_eq!(r.name(r.region_of(4).unwrap()), &path(b"/r/c/d"));
    assert_eq!(r.name(r.region_of(5).unwrap()), &path(b"/r/c"));
    let (sets, t) = crossing_sets(&s, &r, 10).unwrap();
    assert_eq!(t.sets, 1);
    assert_eq!(sets[0].ino, 103);
    let r = Regions::build(&s, &path(b"/r/c"), &RegionSpec::Depth(1)).unwrap();
    assert_ne!(r.region_of(4), r.region_of(5));
    assert_eq!(r.region_of(3), None);
    let r = Regions::build(&s, &path(b"/r"), &RegionSpec::Depth(0)).unwrap();
    assert_eq!(r.region_of(0), r.region_of(5));
    assert_eq!(crossing_sets(&s, &r, 10).unwrap().1.sets, 0);
    assert!(Regions::build(&s, &path(b"/r"), &RegionSpec::Globs(vec!["[".into()])).is_err());
    assert!(link_report(&s, &path(b"/missing"), None, 1).is_err());
    assert!(link_report(&s, &path(b"/r"), None, 1_000_001).is_err());
}
#[test]
fn top_does_not_limit_totals() {
    let (_tmp, s) = fixture();
    let report = link_report(&s, &path(b"/r"), Some(&RegionSpec::Depth(2)), 1).unwrap();
    assert_eq!(report.totals.sets, 3);
    assert_eq!(report.largest_sets.len(), 1);
    assert_eq!(report.totals.crossing.sets, 1);
    assert_eq!(report.crossing[0].ino, 103);
}
#[test]
fn globs_match_non_utf8_path_bytes_and_literal_separators() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = meta();
    m.root = path(b"/r");
    let mut w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    let mut rows = Vec::new();
    for (id, p) in [b"/r".as_slice(), b"/r/\xff", b"/r/\xff/child"]
        .into_iter()
        .enumerate()
    {
        let mut d = dir(id as u32);
        d.path = path(p);
        d.path_hash = d.path.hash64();
        d.name = d.path.file_name().unwrap().as_bytes().to_vec();
        d.depth = id as u32;
        d.parent_id = if id == 0 { None } else { Some(id as u32 - 1) };
        d.subtree_end = 2;
        rows.push(d);
    }
    w.put_dirs(&rows).unwrap();
    let p = w.finish(&m).unwrap();
    let s = Snapshot::open(&p).unwrap();
    let r = Regions::build(&s, &path(b"/r"), &RegionSpec::Globs(vec!["/r/*".into()])).unwrap();
    assert_eq!(r.name(r.region_of(2).unwrap()), &path(b"/r/\xff"));
}
#[test]
#[ignore = "scale"]
fn million_set_report_timing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = meta();
    m.root = path(b"/r");
    let mut w = SnapshotWriter::create(tmp.path(), &m).unwrap();
    let mut rows = Vec::new();
    for (id, p) in [b"/r".as_slice(), b"/r/a", b"/r/a/x", b"/r/b", b"/r/b/y"]
        .into_iter()
        .enumerate()
    {
        let mut d = dir(id as u32);
        d.path = path(p);
        d.path_hash = d.path.hash64();
        d.name = d.path.file_name().unwrap().as_bytes().to_vec();
        d.depth = match id {
            0 => 0,
            2 | 4 => 2,
            _ => 1,
        };
        d.parent_id = match id {
            0 => None,
            2 => Some(1),
            4 => Some(3),
            _ => Some(0),
        };
        d.subtree_end = match id {
            0 => 4,
            1 => 2,
            3 => 4,
            _ => id as u32,
        };
        rows.push(d);
    }
    w.put_dirs(&rows).unwrap();
    for base in (0..1_000_000u64).step_by(10_000) {
        let sets: Vec<_> = (base..base + 10_000)
            .map(|ino| LinkSetRow {
                ino,
                nlink: 2,
                found: 2,
                complete: true,
                disk: 1,
                apparent: 1,
                mtime: 0,
                lca_dir_id: 0,
            })
            .collect();
        let instances: Vec<_> = (base..base + 10_000)
            .flat_map(|ino| {
                [2, 4].map(|dir_id| LinkInstanceRow {
                    ino,
                    dir_id,
                    name: None,
                })
            })
            .collect();
        w.put_link_sets(&sets).unwrap();
        w.put_link_instances(&instances).unwrap();
    }
    let p = w.finish(&m).unwrap();
    let s = Snapshot::open(&p).unwrap();
    let start = std::time::Instant::now();
    let r = link_report(&s, &path(b"/r"), Some(&RegionSpec::Depth(2)), 25).unwrap();
    let elapsed = start.elapsed();
    eprintln!("one million link sets, boundary depth 2: {elapsed:?}");
    assert_eq!(r.totals.crossing.sets, 1_000_000);
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "elapsed {elapsed:?}"
    );
}

#[test]
fn streamed_selection_matches_reader_order_and_subtree_scope() {
    let (_tmp, snap) = fixture();
    for under in [b"/r".as_slice(), b"/r/c", b"/r/b"] {
        let under = path(under);
        let root = snap.dir_by_path(&under).unwrap().unwrap();
        for top in [0, 1, 2, 10] {
            let report = link_report(&snap, &under, Some(&RegionSpec::Depth(1)), top).unwrap();
            let expected = snap
                .link_sets_under(root.id, &LinkFilter::default(), top)
                .unwrap();
            assert_eq!(
                report
                    .largest_sets
                    .iter()
                    .map(|h| (h.row.ino, &h.lca_path, h.instance_count))
                    .collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|h| (h.row.ino, &h.lca_path, h.instance_count))
                    .collect::<Vec<_>>()
            );
            let regions = Regions::build(&snap, &under, &RegionSpec::Depth(1)).unwrap();
            let (crossing, totals) = crossing_sets(&snap, &regions, top).unwrap();
            let expected = snap
                .link_sets_under(
                    root.id,
                    &LinkFilter {
                        boundary_depth: Some(1),
                        ..Default::default()
                    },
                    top,
                )
                .unwrap();
            assert_eq!(
                crossing.iter().map(|h| h.ino).collect::<Vec<_>>(),
                expected.iter().map(|h| h.row.ino).collect::<Vec<_>>()
            );
            assert_eq!(totals.sets, report.totals.crossing.sets);
        }
    }
}
