//! Owned by: W2-A
//! Golden, reference, bulk-equivalence, overflow and iterative scale coverage.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use proptest::prelude::*;
use test_support::{
    golden,
    random::{model_tree, ModelTree, RandomTreeParams},
    reference,
};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::*,
    snapshot::*,
};
use tidemark_scan::agg::*;

fn config(root: RawPath, now: i64) -> AggConfig {
    AggConfig {
        root,
        scan_start: now,
        top_files: 100,
        big_file_threshold: 8192,
        max_notable: 200,
        link_instances: true,
        max_stored_errors: 5,
    }
}
fn collect(records: Vec<DirRecord>, cfg: AggConfig) -> VecSink {
    let mut agg = Aggregator::new(cfg);
    for r in records {
        agg.add_dir_record(r);
    }
    let p = agg.prepare().unwrap();
    let summary = p.summary();
    let mut sink = VecSink::default();
    assert_eq!(p.write(&mut sink).unwrap(), summary);
    sink
}
fn walk_model(tree: &ModelTree) -> VecSink {
    collect(tree.to_dir_records(), config(tree.root.clone(), tree.now))
}
fn assert_sink(a: &VecSink, b: &VecSink) {
    assert_eq!(a.dirs, b.dirs);
    assert_eq!(a.files, b.files);
    assert_eq!(a.link_sets, b.link_sets);
    assert_eq!(a.link_instances, b.link_instances);
    assert_eq!(a.errors, b.errors);
    assert_eq!(a.skipped, b.skipped);
    assert_eq!(a.annotations, b.annotations);
}
fn compare_reference(tree: &ModelTree, rows: &[DirRow]) {
    let paths = tree.paths();
    let expected = reference::compute(tree);
    for r in rows {
        let id = paths.iter().position(|p| p == &r.path).unwrap();
        let m = &expected[&DirId(id as u32)];
        assert_eq!(
            [
                r.t_disk,
                r.t_apparent,
                r.t_charged,
                r.t_reclaimable,
                r.t_files,
                r.t_inodes,
                r.t_dirs,
                r.t_errors
            ],
            [
                m.t_disk,
                m.t_apparent,
                m.t_charged,
                m.t_reclaimable,
                m.t_files,
                m.t_inodes,
                m.t_dirs,
                m.t_errors
            ],
            "{}",
            r.path.display_lossy()
        );
        assert_eq!([r.age0, r.age1, r.age2, r.age3, r.age4, r.age5], m.age);
        assert_eq!(r.newest_mtime, m.newest_mtime);
        assert_eq!(u64::from(r.max_entries), m.max_entries);
        assert!(r.t_reclaimable <= r.t_charged && r.t_reclaimable <= r.t_disk);
        assert_eq!(
            r.age0 + r.age1 + r.age2 + r.age3 + r.age4 + r.age5,
            r.t_charged
        );
        let children: Vec<_> = rows.iter().filter(|c| c.parent_id == Some(r.id)).collect();
        assert_eq!(
            r.t_charged,
            r.own_charged + children.iter().map(|c| c.t_charged).sum::<u64>()
        );
        assert_eq!(
            r.t_files,
            r.own_files + children.iter().map(|c| c.t_files).sum::<u64>()
        );
        assert_eq!(r.t_dirs, 1 + children.iter().map(|c| c.t_dirs).sum::<u64>());
        assert_eq!(
            r.t_errors,
            u64::from(r.errors) + children.iter().map(|c| c.t_errors).sum::<u64>()
        );
        for c in children {
            assert!(r.t_disk >= c.t_disk);
            assert!(c.id > r.id && c.subtree_end <= r.subtree_end);
        }
    }
    assert_eq!(rows[0].t_disk, rows[0].t_charged);
}
fn bulk_model(tree: &ModelTree) -> VecSink {
    let mut agg = Aggregator::new(config(tree.root.clone(), tree.now));
    for r in tree.to_dir_records() {
        agg.add_skeleton(&NsRecord {
            id: r.id,
            parent: r.parent,
            name: r.name,
            ino: r.ino,
            dev: r.dev,
            entries: r.entries,
            children: vec![],
            skipped: r.skipped,
            errors: r.errors,
            errors_total: r.errors_total,
            races: r.races,
        });
        agg.set_dir_inode(
            r.id,
            &InodeAttrs {
                ino: r.ino,
                kind: FileKind::Dir,
                nlink: 1,
                disk: r.dir_disk,
                apparent: r.dir_apparent,
                mtime: r.mtime,
                projid: None,
            },
        );
    }
    for (&ino, i) in &tree.inodes {
        let a = InodeAttrs {
            ino,
            kind: i.kind,
            nlink: i.nlink,
            disk: i.disk,
            apparent: i.apparent,
            mtime: i.mtime,
            projid: None,
        };
        let dirs: Vec<_> = tree
            .entries
            .iter()
            .filter(|e| e.ino == ino)
            .map(|e| DirId(e.dir as u32))
            .collect();
        if i.nlink == 1 {
            agg.add_single(dirs[0], &a);
        } else {
            agg.add_link_set(&a, &dirs);
        }
    }
    let mut p = agg.prepare().unwrap();
    let paths = tree.paths();
    let names = tree
        .entries
        .iter()
        .map(|e| (paths[e.dir].clone(), e.ino, e.name.clone()))
        .collect();
    p.set_names(names);
    let mut sink = VecSink::default();
    p.write(&mut sink).unwrap();
    sink
}
#[test]
fn golden_table_and_bulk_equivalence() {
    let t = golden::link_semantics_basic();
    let s = walk_model(&t);
    for g in golden::LINK_SEMANTICS_BASIC {
        let r = s
            .dirs
            .iter()
            .find(|r| r.path.as_bytes() == g.path.as_bytes())
            .unwrap();
        assert_eq!(
            [
                r.t_disk,
                r.t_charged,
                r.t_reclaimable,
                r.t_disk - r.t_reclaimable,
                r.t_files,
                r.t_inodes,
                r.t_dirs
            ],
            g.values
        );
    }
    compare_reference(&t, &s.dirs);
    let mut b = bulk_model(&t);
    for (x, y) in b.link_instances.iter_mut().zip(&s.link_instances) {
        assert!(x.name.is_none());
        x.name = y.name.clone();
    }
    assert_sink(&s, &b);
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]
    #[test]
    fn random_reference_and_order(seed in any::<u64>(), (dirs,files) in prop_oneof![
        9 => (1usize..100,1usize..600),
        1 => (100usize..=2000,600usize..=20000)
    ]) {
        let t=model_tree(seed,RandomTreeParams { dirs,files,link_sets:(files/5).min(2000),incomplete_ratio:0.3,..Default::default() });
        let s=walk_model(&t); compare_reference(&t,&s.dirs);
        let mut records=t.to_dir_records();
        // Deterministic Fisher-Yates shuffle, independent of directory id ordering.
        let mut rng=seed;
        for i in (1..records.len()).rev() { rng=rng.wrapping_mul(6364136223846793005).wrapping_add(1); records.swap(i,(rng as usize)%(i+1)); }
        let shuffled=collect(records,config(t.root.clone(),t.now)); assert_sink(&s,&shuffled);
        let mut bulk=bulk_model(&t);
        for (x,y) in bulk.link_instances.iter_mut().zip(&s.link_instances) { x.name=y.name.clone(); }
        assert_sink(&s,&bulk);
    }
}
#[test]
fn large_reference_bounds() {
    let t = model_tree(
        42,
        RandomTreeParams {
            dirs: 2000,
            files: 20000,
            link_sets: 2000,
            incomplete_ratio: 0.3,
            ..Default::default()
        },
    );
    compare_reference(&t, &walk_model(&t).dirs);
}
fn record(id: u32, parent: Option<u32>) -> DirRecord {
    DirRecord {
        id: DirId(id),
        parent: parent.map(DirId),
        name: parent.map(|_| RawName::new(format!("d{id:07}").into_bytes()).unwrap()),
        ino: u64::from(id) + 1,
        dev: 1,
        mtime: -100,
        dir_disk: 0,
        dir_apparent: 0,
        projid: None,
        entries: 0,
        own: OwnStats::default(),
        links: vec![],
        notable: vec![],
        skipped: vec![],
        errors: vec![],
        errors_total: 0,
        races: 0,
    }
}
#[test]
fn deep_tree_10000() {
    let mut agg = Aggregator::new(config(RawPath::new(b"/r".to_vec()).unwrap(), 0));
    for id in (0..=10000).rev() {
        agg.add_dir_record(record(id, (id > 0).then(|| id - 1)));
    }
    let p = agg.prepare().unwrap();
    assert_eq!(p.summary().dirs, 10001);
    // A counting sink avoids retaining quadratic full paths on deep trees.
    let mut s = Counting::default();
    p.write(&mut s).unwrap();
    assert_eq!(s.dirs, 10001);
    assert_eq!(s.depth, 10000);
}
#[derive(Default)]
struct Counting {
    dirs: u64,
    files: u64,
    depth: u32,
}
impl RowSink for Counting {
    fn dirs(&mut self, r: &[DirRow]) -> Result<(), SinkError> {
        self.dirs += r.len() as u64;
        self.depth = self.depth.max(r.iter().map(|r| r.depth).max().unwrap_or(0));
        Ok(())
    }
    fn files(&mut self, r: &[FileRow]) -> Result<(), SinkError> {
        self.files += r.len() as u64;
        Ok(())
    }
    fn link_sets(&mut self, _: &[LinkSetRow]) -> Result<(), SinkError> {
        Ok(())
    }
    fn link_instances(&mut self, _: &[LinkInstanceRow]) -> Result<(), SinkError> {
        Ok(())
    }
    fn skipped(&mut self, _: &[SkipRow]) -> Result<(), SinkError> {
        Ok(())
    }
    fn errors(&mut self, _: &[ErrorRow]) -> Result<(), SinkError> {
        Ok(())
    }
    fn annotations(&mut self, _: &[AnnotationRow]) -> Result<(), SinkError> {
        Ok(())
    }
}
#[test]
#[ignore = "scale"]
fn scale_two_million_directories_thirty_million_entries() {
    let start = std::time::Instant::now();
    let mut agg = Aggregator::new(config(RawPath::new(b"/r".to_vec()).unwrap(), 0));
    for id in 0..2_000_000 {
        let mut r = record(id, (id > 0).then_some(0));
        r.own.files = 15;
        r.own.disk = 15 * 512;
        r.own.apparent = r.own.disk;
        r.own.age.0[0] = r.own.disk;
        r.entries = if id == 0 { 2_000_014 } else { 15 };
        agg.add_dir_record(r);
    }
    let p = agg.prepare().unwrap();
    assert_eq!(p.summary().files, 30_000_000);
    let mut s = Counting::default();
    p.write(&mut s).unwrap();
    assert_eq!(s.dirs, 2_000_000);
    eprintln!("scale elapsed={:?}", start.elapsed());
}

#[test]
fn holes_errors_races_and_annotations() {
    let root = RawPath::new(b"/r".to_vec()).unwrap();
    let mut cfg = config(root.clone(), 0);
    cfg.max_stored_errors = 1;
    let mut agg = Aggregator::new(cfg);
    let mut r = record(0, None);
    r.errors = vec![
        ScanErrorRec {
            path: root.join(&RawName::new(b"z".to_vec()).unwrap()),
            op: ErrOp::Open,
            errno: 13,
        },
        ScanErrorRec {
            path: root.join(&RawName::new(b"a".to_vec()).unwrap()),
            op: ErrOp::Statx,
            errno: 5,
        },
    ];
    r.errors_total = 2;
    agg.add_dir_record(r);
    agg.add_dir_record(record(3, Some(0)));
    agg.add_gone(&GoneRecord {
        id: DirId(1),
        parent: DirId(0),
        name: RawName::new(b"gone".to_vec()).unwrap(),
        reason: GoneReason::Vanished,
    });
    agg.add_gone(&GoneRecord {
        id: DirId(2),
        parent: DirId(0),
        name: RawName::new(b"mount".to_vec()).unwrap(),
        reason: GoneReason::OtherFs,
    });
    agg.add_races(None, 2);
    let mut p = agg.prepare().unwrap();
    let summary = p.summary();
    assert_eq!(summary.races, 4);
    assert_eq!(summary.errors, 2);
    assert!(summary.degraded);
    p.annotate(&root, "pg.db", "example");
    p.annotate(
        &RawPath::new(b"/missing".to_vec()).unwrap(),
        "ignored",
        "ignored",
    );
    let mut s = VecSink::default();
    p.write(&mut s).unwrap();
    assert_eq!(s.dirs.len(), 2);
    assert_eq!(s.dirs[1].id, 1);
    assert_eq!(s.dirs[0].subtree_end, 1);
    assert_eq!(s.errors.len(), 1);
    assert_eq!(s.errors[0].path.as_bytes(), b"/r/a");
    assert_eq!(s.skipped.len(), 1);
    assert_eq!(s.annotations.len(), 1);
}
#[test]
fn duplicate_instances_complete_race_and_last_stat() {
    let root = RawPath::new(b"/r".to_vec()).unwrap();
    let mut agg = Aggregator::new(config(root, 0));
    let mut r = record(0, None);
    r.links = vec![
        LinkRef {
            ino: 99,
            nlink: 3,
            kind: FileKind::Regular,
            disk: 4,
            apparent: 5,
            mtime: -100,
            name: Some(RawName::new(b"z".to_vec()).unwrap()),
        },
        LinkRef {
            ino: 99,
            nlink: 2,
            kind: FileKind::Regular,
            disk: 8,
            apparent: 9,
            mtime: 1,
            name: Some(RawName::new(b"a".to_vec()).unwrap()),
        },
        LinkRef {
            ino: 99,
            nlink: 2,
            kind: FileKind::Regular,
            disk: 8,
            apparent: 9,
            mtime: 1,
            name: Some(RawName::new(b"b".to_vec()).unwrap()),
        },
    ];
    agg.add_dir_record(r);
    let mut s = VecSink::default();
    agg.prepare().unwrap().write(&mut s).unwrap();
    assert_eq!(s.link_sets[0].nlink, 3);
    assert!(s.link_sets[0].complete);
    assert_eq!(s.link_sets[0].found, 3);
    assert_eq!(s.dirs[0].t_disk, 8);
    assert_eq!(s.dirs[0].t_files, 3);
    assert_eq!(s.dirs[0].t_inodes, 1);
    assert_eq!(s.dirs[0].t_reclaimable, 8);
    assert_eq!(s.files[0].name.as_ref().unwrap().as_bytes(), b"a");
    assert_eq!(s.dirs[0].newest_mtime, 1);
}
#[test]
fn selection_uses_final_ids_and_names_use_minimum() {
    let root = RawPath::new(b"/r".to_vec()).unwrap();
    let mut cfg = config(root.clone(), 0);
    cfg.top_files = 1;
    cfg.big_file_threshold = 10;
    cfg.max_notable = 2;
    let mut agg = Aggregator::new(cfg);
    agg.add_dir_record(record(0, None));
    let mut z = record(1, Some(0));
    z.name = Some(RawName::new(b"z".to_vec()).unwrap());
    agg.add_dir_record(z);
    let mut a = record(2, Some(0));
    a.name = Some(RawName::new(b"a".to_vec()).unwrap());
    agg.add_dir_record(a);
    for (dir, ino, disk) in [(1, 1, 10), (2, 9, 10), (2, 3, 10), (1, 2, 9)] {
        agg.add_single(
            DirId(dir),
            &InodeAttrs {
                ino,
                kind: FileKind::Regular,
                nlink: 1,
                disk,
                apparent: disk,
                mtime: 0,
                projid: None,
            },
        );
    }
    let mut p = agg.prepare().unwrap();
    assert_eq!(p.needs_names().len(), 2);
    let ap = root.join(&RawName::new(b"a".to_vec()).unwrap());
    p.set_names(vec![
        (ap.clone(), 3, RawName::new(b"z".to_vec()).unwrap()),
        (ap, 3, RawName::new(b"a".to_vec()).unwrap()),
    ]);
    let mut s = VecSink::default();
    p.write(&mut s).unwrap();
    assert_eq!(
        s.files.iter().map(|f| f.ino).collect::<Vec<_>>(),
        vec![3, 9]
    );
    assert_eq!(s.files[0].id, 0);
    assert_eq!(s.files[1].id, 1);
    assert_eq!(s.files[0].name.as_ref().unwrap().as_bytes(), b"a");
    assert!(s.files[1].name.is_none());
}
#[test]
fn bad_tree_and_overflow_are_typed_errors() {
    let root = RawPath::new(b"/r".to_vec()).unwrap();
    let mut agg = Aggregator::new(config(root.clone(), 0));
    agg.add_dir_record(record(0, None));
    agg.add_dir_record(record(1, Some(2)));
    agg.add_dir_record(record(2, Some(1)));
    assert!(matches!(agg.prepare(), Err(AggError::Tree)));
    let mut agg = Aggregator::new(config(root.clone(), 0));
    let mut r = record(0, None);
    r.own.disk = u64::MAX;
    r.dir_disk = 1;
    agg.add_dir_record(r);
    assert!(agg.prepare().is_err());
    let mut agg = Aggregator::new(config(root.clone(), 0));
    let mut r = record(0, None);
    r.own.files = u64::MAX;
    agg.add_dir_record(r);
    let mut c = record(1, Some(0));
    c.own.files = 1;
    agg.add_dir_record(c);
    assert!(matches!(agg.prepare(), Err(AggError::Overflow)));
    let mut agg = Aggregator::new(config(root, 0));
    agg.add_dir_record(record(0, None));
    agg.add_link_set(
        &InodeAttrs {
            ino: 1,
            kind: FileKind::Regular,
            nlink: 2,
            disk: 1,
            apparent: 1,
            mtime: 0,
            projid: None,
        },
        &[DirId(999)],
    );
    assert!(matches!(agg.prepare(), Err(AggError::Tree)));
}
#[test]
fn sink_failure_stops_stream() {
    struct Fail;
    impl RowSink for Fail {
        fn dirs(&mut self, _: &[DirRow]) -> Result<(), SinkError> {
            Err(SinkError::Other("injected".into()))
        }
        fn files(&mut self, _: &[FileRow]) -> Result<(), SinkError> {
            panic!("write continued")
        }
        fn link_sets(&mut self, _: &[LinkSetRow]) -> Result<(), SinkError> {
            panic!("write continued")
        }
        fn link_instances(&mut self, _: &[LinkInstanceRow]) -> Result<(), SinkError> {
            panic!("write continued")
        }
        fn skipped(&mut self, _: &[SkipRow]) -> Result<(), SinkError> {
            panic!("write continued")
        }
        fn errors(&mut self, _: &[ErrorRow]) -> Result<(), SinkError> {
            panic!("write continued")
        }
        fn annotations(&mut self, _: &[AnnotationRow]) -> Result<(), SinkError> {
            panic!("write continued")
        }
    }
    let t = golden::link_semantics_basic();
    let mut agg = Aggregator::new(config(t.root.clone(), t.now));
    for r in t.to_dir_records() {
        agg.add_dir_record(r);
    }
    assert!(matches!(
        agg.prepare().unwrap().write(&mut Fail),
        Err(AggError::Sink(SinkError::Other(_)))
    ));
}
fn scan_disk(root: &std::path::Path) -> VecSink {
    use std::sync::atomic::AtomicBool;
    let root = RawPath::from_path(root).unwrap();
    let cfg = tidemark_fs::walk::WalkConfig {
        root: root.clone(),
        threads: 2,
        stat_entries: true,
        exclude: globset::GlobSetBuilder::new().build().unwrap(),
        skip_mounts: vec![],
        big_file_threshold: 8192,
        top_files: 100,
        scan_start: 0,
        getdents_buf: 32768,
        io_idle: false,
    };
    let (tx, rx) = crossbeam_channel::bounded(4);
    let mut agg = Aggregator::new(config(root, 0));
    std::thread::scope(|scope| {
        let cancel = AtomicBool::new(false);
        let worker = scope.spawn(move || tidemark_fs::walk::walk(&cfg, &NoopGate, &cancel, tx));
        for r in rx {
            match r {
                WalkOutput::Dir(r) => agg.add_dir_record(r),
                WalkOutput::Gone(g) => agg.add_gone(&g),
                _ => panic!("namespace in walk mode"),
            }
        }
        worker.join().unwrap().unwrap();
    });
    let mut s = VecSink::default();
    agg.prepare().unwrap().write(&mut s).unwrap();
    s
}
#[test]
fn walker_golden_measured_du() {
    if !test_support::oracle::gnu_available("du") {
        eprintln!("GNU du unavailable");
        return;
    }
    let fixture = test_support::materialize(include_str!(
        "../../../test-support/fixtures/link_semantics_basic.toml"
    ));
    let s = scan_disk(&fixture.root);
    for row in &s.dirs {
        assert_eq!(
            row.t_disk,
            test_support::oracle::du_disk(row.path.as_path())
        );
    }
    let counts = test_support::oracle::find_counts(&fixture.root);
    assert_eq!(s.dirs[0].t_files, counts.files);
    assert_eq!(s.dirs[0].t_dirs, counts.dirs);
}
#[cfg(feature = "privileged")]
#[test]
fn loop_xfs_walker_golden() {
    use std::{fs, io::Write};
    let mut xfs = test_support::xfs::LoopXfs::new(
        512 * 1024 * 1024,
        &["-b", "size=4096", "-m", "reflink=0"],
        "noatime",
    );
    let fixture = tempfile::tempdir_in(&xfs.mountpoint).unwrap();
    let root = fixture.path().join("r");
    for d in ["a", "b", "c/d", "c/e"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    let model = golden::link_semantics_basic();
    for (ino, path) in [(100, "a/x"), (101, "a/big"), (102, "b/z"), (103, "c/d/w")] {
        let mut f = fs::File::create(root.join(path)).unwrap();
        f.write_all(&vec![0; model.inodes[&ino].apparent as usize])
            .unwrap();
        f.sync_all().unwrap();
    }
    fs::hard_link(root.join("a/x"), root.join("b/y")).unwrap();
    fs::hard_link(root.join("c/d/w"), root.join("c/e/v")).unwrap();
    fs::hard_link(root.join("b/z"), fixture.path().join("z2")).unwrap();
    fs::hard_link(root.join("b/z"), fixture.path().join("z3")).unwrap();
    std::os::unix::fs::symlink("../a", root.join("c/s")).unwrap();
    let s = scan_disk(&root);
    for g in golden::LINK_SEMANTICS_BASIC {
        let suffix = g.path.strip_prefix("/r").unwrap().trim_start_matches('/');
        let path = root.join(suffix);
        let r = s.dirs.iter().find(|r| r.path.as_path() == path).unwrap();
        // Subtract measured directory contributions before asserting synthetic table.
        let dir_bytes = s
            .dirs
            .iter()
            .filter(|d| d.id >= r.id && d.id <= r.subtree_end)
            .map(|d| d.dir_disk)
            .sum::<u64>();
        assert_eq!(
            [
                r.t_disk - dir_bytes,
                r.t_charged - dir_bytes,
                r.t_reclaimable - dir_bytes,
                r.t_disk - r.t_reclaimable,
                r.t_files,
                r.t_inodes,
                r.t_dirs
            ],
            g.values
        );
        assert_eq!(r.t_disk, test_support::oracle::du_disk(path));
    }
    drop(fixture);
    xfs.cleanup().unwrap();
}

#[test]
#[ignore = "scale"]
fn scale_half_million_bulk_link_sets() {
    let start = std::time::Instant::now();
    let root = RawPath::new(b"/r".to_vec()).unwrap();
    let mut cfg = config(root, 0);
    cfg.max_notable = 1;
    cfg.top_files = 1;
    cfg.big_file_threshold = u64::MAX;
    let mut agg = Aggregator::new(cfg);
    agg.add_dir_record(record(0, None));
    agg.add_dir_record(record(1, Some(0)));
    for ino in 0..500_000 {
        agg.add_link_set(
            &InodeAttrs {
                ino,
                kind: FileKind::Regular,
                nlink: 2,
                disk: 512,
                apparent: 512,
                mtime: 0,
                projid: None,
            },
            &[DirId(0), DirId(1)],
        );
    }
    let p = agg.prepare().unwrap();
    assert_eq!(p.summary().link_sets, 500_000);
    assert_eq!(p.summary().files, 1_000_000);
    p.write(&mut Counting::default()).unwrap();
    eprintln!("link scale elapsed={:?}", start.elapsed());
}

#[test]
fn ages_nonutf8_errors_and_own_metrics() {
    let mut t = golden::link_semantics_basic();
    t.now = 60_000_000;
    let ages = [-1, 86_400, 604_800, 2_592_000, 7_776_000, 31_536_000];
    t.dirs[1].name = Some(RawName::new(vec![0xff, b'a']).unwrap());
    for (id, d) in t.dirs.iter_mut().enumerate() {
        d.disk = 512;
        d.apparent = 123;
        d.mtime = t.now - ages[id];
    }
    for (id, i) in t.inodes.values_mut().enumerate() {
        i.mtime = t.now - ages[id];
    }
    t.inodes.get_mut(&101).unwrap().kind = FileKind::Other;
    let paths = t.paths();
    for (id, d) in t.dirs.iter_mut().enumerate() {
        d.errors_total += 1;
        d.errors.push(ScanErrorRec {
            path: paths[id].clone(),
            op: ErrOp::Open,
            errno: 13,
        });
    }
    let s = walk_model(&t);
    compare_reference(&t, &s.dirs);
    let mut records = t.to_dir_records();
    records.reverse();
    let shuffled = collect(records, config(t.root.clone(), t.now));
    assert_sink(&s, &shuffled);
    for r in &s.dirs {
        let id = paths.iter().position(|p| p == &r.path).unwrap();
        let direct: Vec<_> = t.entries.iter().filter(|e| e.dir == id).collect();
        assert_eq!(r.own_files, direct.len() as u64);
        assert_eq!(
            r.own_symlinks,
            direct
                .iter()
                .filter(|e| t.inodes[&e.ino].kind == FileKind::Symlink)
                .count() as u64
        );
    }
    assert_eq!(s.dirs[0].t_errors, 6);
    assert_eq!(s.errors.len(), 5);
    let mut bulk = bulk_model(&t);
    for (x, y) in bulk.link_instances.iter_mut().zip(&s.link_instances) {
        x.name = y.name.clone();
    }
    assert_sink(&s, &bulk);
}
#[test]
fn bulk_projid_and_disabled_instances() {
    let root = RawPath::new(b"/".to_vec()).unwrap();
    let mut cfg = config(root.clone(), 0);
    cfg.link_instances = false;
    let mut agg = Aggregator::new(cfg);
    let r = record(0, None);
    agg.add_skeleton(&NsRecord {
        id: r.id,
        parent: r.parent,
        name: r.name,
        ino: r.ino,
        dev: 1,
        entries: 1,
        children: vec![9],
        skipped: vec![],
        errors: vec![],
        errors_total: 0,
        races: 0,
    });
    agg.set_dir_inode(
        DirId(0),
        &InodeAttrs {
            ino: 1,
            kind: FileKind::Dir,
            nlink: 2,
            disk: 512,
            apparent: 100,
            mtime: -100,
            projid: Some(42),
        },
    );
    agg.add_link_set(
        &InodeAttrs {
            ino: 9,
            kind: FileKind::Symlink,
            nlink: 2,
            disk: 0,
            apparent: 2,
            mtime: -1,
            projid: None,
        },
        &[DirId(0)],
    );
    let mut p = agg.prepare().unwrap();
    p.annotate(&root, "test", "root");
    let mut s = VecSink::default();
    p.write(&mut s).unwrap();
    assert_eq!(s.dirs[0].projid, Some(42));
    assert_eq!(s.dirs[0].own_symlinks, 1);
    assert_eq!(s.dirs[0].t_apparent, 102);
    assert_eq!(s.annotations.len(), 1);
    assert!(s.link_instances.is_empty());
    assert_eq!(s.link_sets.len(), 1);
}

#[test]
fn uncapped_directory_errors_survive_walk_and_namespace_sampling() {
    let root = RawPath::new(b"/r".to_vec()).unwrap();
    let mut records = vec![record(0, None), record(1, Some(0)), record(2, Some(0))];
    records[1].errors_total = 10_017;
    records[2].errors_total = 10_123;
    records[1].errors.push(ScanErrorRec {
        path: root.clone(),
        op: ErrOp::Open,
        errno: 13,
    });
    let walk = collect(records.clone(), config(root.clone(), 0));
    assert_eq!(walk.dirs[0].t_errors, 20_140);
    assert_eq!(walk.errors.len(), 1);
    let mut agg = Aggregator::new(config(root, 0));
    for r in records {
        agg.add_skeleton(&NsRecord {
            id: r.id,
            parent: r.parent,
            name: r.name,
            ino: r.ino,
            dev: r.dev,
            entries: r.entries,
            children: vec![],
            skipped: r.skipped,
            errors: r.errors,
            errors_total: r.errors_total,
            races: r.races,
        });
    }
    let prepared = agg.prepare().unwrap();
    assert_eq!(prepared.summary().errors, 20_140);
    let mut bulk = VecSink::default();
    prepared.write(&mut bulk).unwrap();
    for (a, b) in walk.dirs.iter().zip(&bulk.dirs) {
        assert_eq!((a.errors, a.t_errors), (b.errors, b.t_errors));
    }
}

#[test]
fn error_order_below_at_and_above_cap_preserves_record_order() {
    for cap in [3, 6, 10] {
        let mut records = vec![record(0, None), record(2, Some(0)), record(1, Some(0))];
        for r in &mut records {
            for suffix in ["z", "a"] {
                r.errors.push(ScanErrorRec {
                    path: RawPath::new(format!("/r/{}/{suffix}", r.id.0).into_bytes()).unwrap(),
                    op: ErrOp::Statx,
                    errno: 5,
                });
            }
            r.errors_total = 2;
        }
        let mut cfg = config(RawPath::new(b"/r".to_vec()).unwrap(), 0);
        cfg.max_stored_errors = cap;
        let forward = collect(records.clone(), cfg);
        records.reverse();
        let mut cfg = config(RawPath::new(b"/r".to_vec()).unwrap(), 0);
        cfg.max_stored_errors = cap;
        let reverse = collect(records, cfg);
        assert_sink(&forward, &reverse);
        assert!(forward
            .errors
            .windows(2)
            .all(|p| p[0].dir_id <= p[1].dir_id));
        if cap == 10 {
            assert!(forward
                .errors
                .chunks(2)
                .all(|r| r[0].path.as_bytes().ends_with(b"z")
                    && r[1].path.as_bytes().ends_with(b"a")));
        }
    }
}

#[test]
fn directory_limit_and_five_million_discovery_id() {
    let mut agg = Aggregator::new(config(RawPath::new(b"/r".to_vec()).unwrap(), 0));
    agg.add_dir_record(record(0, None));
    agg.add_dir_record(record(5_000_000, Some(0)));
    assert_eq!(agg.prepare().unwrap().summary().dirs, 2);
    let mut agg = Aggregator::new(config(RawPath::new(b"/r".to_vec()).unwrap(), 0));
    agg.add_dir_record(record(0, None));
    agg.add_dir_record(record(15_999_999, Some(0)));
    assert_eq!(agg.prepare().unwrap().summary().dirs, 2);
    let mut agg = Aggregator::new(config(RawPath::new(b"/r".to_vec()).unwrap(), 0));
    agg.add_dir_record(record(0, None));
    agg.add_dir_record(record(16_000_000, Some(0)));
    assert!(matches!(agg.prepare(), Err(AggError::Capacity)));
}
