//! Owned by: W2-B
//! Pipeline arithmetic, names, real-aggregator and VM acceptance checks.
#![allow(clippy::unwrap_used)]
use agg::Aggregator;
use std::sync::atomic::AtomicBool;
#[cfg(feature = "privileged")]
use tidemark_core::scan::WalkOutput;
#[cfg(feature = "privileged")]
use tidemark_fs::walk::walk;
use tidemark_fs::walk::WalkConfig;
use tidemark_scan::{agg, bulk::*};
use tidemark_sys::xfs::XfsGeometry;
#[path = "../../src/bulk/join.rs"]
mod join;
#[path = "../../src/bulk/names.rs"]
mod names;
use agg::{AggConfig, VecSink};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::{DirId, FileKind, NoopGate, NsRecord},
};
fn aggregator(root: RawPath) -> Aggregator {
    Aggregator::new(AggConfig {
        root,
        scan_start: 0,
        top_files: 100,
        big_file_threshold: u64::MAX,
        max_notable: 200_000,
        link_instances: true,
        max_stored_errors: 10_000,
    })
}
fn config(root: RawPath) -> WalkConfig {
    WalkConfig {
        root,
        threads: 2,
        stat_entries: true,
        exclude: globset::GlobSet::empty(),
        skip_mounts: vec![],
        big_file_threshold: u64::MAX,
        top_files: 100,
        scan_start: 0,
        getdents_buf: 256 * 1024,
        io_idle: false,
    }
}
#[test]
fn estimate_rounds_up_and_saturates() {
    assert_eq!(estimate_memory(0, 0), 64 * 1024 * 1024);
    assert_eq!(estimate_memory(10, 2), 64 * 1024 * 1024 + 132 + 640);
    assert_eq!(estimate_memory(1, 0), 64 * 1024 * 1024 + 24);
    assert_eq!(estimate_memory(u64::MAX, 0), u64::MAX);
    assert_eq!(estimate_memory(0, u64::MAX), u64::MAX);
}
#[test]
fn names_smallest_bytes_missing_and_cancelled() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let name = std::ffi::OsString::from_vec(vec![0xff]);
    std::fs::write(dir.path().join(&name), []).unwrap();
    std::fs::hard_link(dir.path().join(&name), dir.path().join("a")).unwrap();
    let ino = std::fs::symlink_metadata(dir.path().join("a"))
        .unwrap()
        .ino();
    let path = RawPath::from_path(dir.path()).unwrap();
    let missing_path = path.join(&RawName::new("gone".as_bytes()).unwrap());
    let (names, missing) = names::resolve(
        vec![
            (path.clone(), ino),
            (path.clone(), ino),
            (path.clone(), u64::MAX),
            (missing_path, ino),
        ],
        &NoopGate,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(missing, 2);
    assert_eq!(
        names,
        vec![(path.clone(), ino, RawName::new(b"a").unwrap())]
    );
    assert!(matches!(
        names::resolve(vec![(path, ino)], &NoopGate, &AtomicBool::new(true)),
        Err(BulkError::Cancelled)
    ));
}
use std::os::unix::ffi::OsStringExt;

#[test]
fn synthetic_bulk_equals_walk() {
    let mut tree = test_support::golden::link_semantics_basic();
    tree.now = 0;
    let mut walk = aggregator(tree.root.clone());
    for record in tree.to_dir_records() {
        walk.add_dir_record(record);
    }
    let mut expected = VecSink::default();
    walk.prepare().unwrap().write(&mut expected).unwrap();
    let mut bulk = aggregator(tree.root.clone());
    let mut dents = Vec::new();
    let mut inodes = Vec::new();
    for (id, dir) in tree.dirs.iter().enumerate() {
        let ns = NsRecord {
            id: DirId(id as u32),
            parent: dir.parent.map(|p| DirId(p as u32)),
            name: dir.name.clone(),
            ino: dir.ino,
            dev: 1,
            entries: tree.entries.iter().filter(|e| e.dir == id).count() as u32
                + tree.dirs.iter().filter(|d| d.parent == Some(id)).count() as u32,
            children: tree
                .entries
                .iter()
                .filter(|e| e.dir == id)
                .map(|e| e.ino)
                .collect(),
            skipped: dir.skipped.clone(),
            errors: dir.errors.clone(),
            errors_total: dir.errors_total,
            races: 0,
        };
        bulk.add_skeleton(&ns);
        join::append_namespace(&mut dents, &ns).unwrap();
        inodes.push(tidemark_sys::xfs::BulkInode {
            ino: dir.ino,
            mode: 0o040000,
            nlink: 2,
            uid: 0,
            gid: 0,
            disk_bytes: dir.disk,
            size: dir.apparent,
            mtime: dir.mtime,
            projid: 0,
            xflags: 0,
        });
    }
    for (&ino, i) in &tree.inodes {
        let mode = match i.kind {
            FileKind::Regular => 0o100000,
            FileKind::Symlink => 0o120000,
            FileKind::Dir => 0o040000,
            FileKind::Other => 0o010000,
        };
        inodes.push(tidemark_sys::xfs::BulkInode {
            ino,
            mode,
            nlink: i.nlink,
            uid: 0,
            gid: 0,
            disk_bytes: i.disk,
            size: i.apparent,
            mtime: i.mtime,
            projid: 0,
            xflags: 0,
        });
    }
    inodes.sort_unstable_by_key(|i| i.ino);
    dents.sort_unstable_by_key(|d| d.ino());
    join::stream(
        &dents,
        std::iter::once(Ok(inodes)),
        &NoopGate,
        1,
        &AtomicBool::new(false),
        |b| {
            for item in b.items {
                item.dispatch(&mut bulk);
            }
            Ok(())
        },
    )
    .unwrap();
    let mut prepared = bulk.prepare().unwrap();
    let paths = tree.paths();
    let names = prepared
        .needs_names()
        .into_iter()
        .map(|(path, ino)| {
            let id = paths.iter().position(|p| p == &path).unwrap();
            let name = tree
                .entries
                .iter()
                .filter(|e| e.dir == id && e.ino == ino)
                .map(|e| e.name.clone())
                .min()
                .unwrap();
            (path, ino, name)
        })
        .collect();
    prepared.set_names(names);
    let mut actual = VecSink::default();
    prepared.write(&mut actual).unwrap();
    compare(&mut expected, &mut actual);
}
fn compare(expected: &mut VecSink, actual: &mut VecSink) {
    for d in &mut actual.dirs {
        d.projid = None;
    }
    for d in &mut expected.dirs {
        d.projid = None;
    }
    for i in &mut expected.link_instances {
        i.name = None;
    }
    for i in &mut actual.link_instances {
        i.name = None;
    }
    assert_eq!(expected.dirs, actual.dirs);
    assert_eq!(expected.files, actual.files);
    assert_eq!(expected.link_sets, actual.link_sets);
    assert_eq!(expected.link_instances, actual.link_instances);
    assert_eq!(expected.skipped, actual.skipped);
    assert_eq!(expected.errors, actual.errors);
}
#[cfg(feature = "privileged")]
mod vm;

#[test]
fn preflight_refuses_ftype_without_io() {
    use std::os::fd::AsFd;
    let dir = tempfile::tempdir().unwrap();
    let fd = std::fs::File::open(dir.path()).unwrap();
    let root = RawPath::from_path(dir.path()).unwrap();
    let params = BulkParams {
        walk: config(root.clone()),
        batch: 4096,
        threads: 2,
    };
    let geo = XfsGeometry {
        blocksize: 4096,
        agblocks: 1024,
        agcount: 1,
        inodesize: 512,
        dirblocksize: 4096,
        uuid: [0; 16],
        v5: Some(false),
        ftype: Some(false),
        reflink: None,
    };
    assert!(matches!(
        run_bulk(
            &params,
            fd.as_fd(),
            &geo,
            &NoopGate,
            &AtomicBool::new(false),
            &|_| {},
            aggregator(root)
        ),
        Err(BulkError::Unsupported(_))
    ));
}

#[test]
fn namespace_failure_is_redoable_and_cancellation_is_not() {
    use std::os::fd::AsFd;
    let dir = tempfile::tempdir().unwrap();
    let fd = std::fs::File::open(dir.path()).unwrap();
    let root = RawPath::from_path(&dir.path().join("missing")).unwrap();
    let params = BulkParams {
        walk: config(root.clone()),
        batch: 4096,
        threads: 2,
    };
    let geo = XfsGeometry {
        blocksize: 4096,
        agblocks: 1024,
        agcount: 1,
        inodesize: 512,
        dirblocksize: 4096,
        uuid: [0; 16],
        v5: Some(true),
        ftype: Some(true),
        reflink: None,
    };
    let reports = std::sync::Mutex::new(Vec::new());
    let observer = |p: &tidemark_scan::bulk::BulkProgress| reports.lock().unwrap().push(p.phase);
    assert!(matches!(
        run_bulk(
            &params,
            fd.as_fd(),
            &geo,
            &NoopGate,
            &AtomicBool::new(false),
            &observer,
            aggregator(root.clone())
        ),
        Err(BulkError::BeforePhase2(_))
    ));
    let before_cancel = reports.lock().unwrap().len();
    assert!(matches!(
        run_bulk(
            &params,
            fd.as_fd(),
            &geo,
            &NoopGate,
            &AtomicBool::new(true),
            &observer,
            aggregator(root)
        ),
        Err(BulkError::Cancelled)
    ));
    assert_eq!(reports.lock().unwrap().len(), before_cancel);
}

#[test]
fn bulk_namespace_progress_reports_while_setup_is_blocked() {
    use std::{
        os::fd::AsFd,
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    struct SlowGate(AtomicBool);
    impl tidemark_core::scan::Gate for SlowGate {
        fn acquire(&self, _: u32) {
            if !self.0.swap(true, Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(2300));
            }
        }
        fn current_rate(&self) -> f64 {
            1.0
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let fd = std::fs::File::open(dir.path()).unwrap();
    let root = RawPath::from_path(&dir.path().join("missing")).unwrap();
    let params = BulkParams {
        walk: config(root.clone()),
        batch: 4096,
        threads: 1,
    };
    let geo = XfsGeometry {
        blocksize: 4096,
        agblocks: 1024,
        agcount: 1,
        inodesize: 512,
        dirblocksize: 4096,
        uuid: [0; 16],
        v5: Some(true),
        ftype: Some(true),
        reflink: None,
    };
    let start = Instant::now();
    let reports = std::sync::Mutex::new(Vec::new());
    let observer = |p: &tidemark_scan::bulk::BulkProgress| {
        reports
            .lock()
            .unwrap()
            .push((start.elapsed(), p.phase, p.dirs))
    };
    assert!(matches!(
        run_bulk(
            &params,
            fd.as_fd(),
            &geo,
            &SlowGate(AtomicBool::new(false)),
            &AtomicBool::new(false),
            &observer,
            aggregator(root)
        ),
        Err(BulkError::BeforePhase2(_))
    ));
    let reports = reports.lock().unwrap();
    assert!(!reports.is_empty());
    assert!(reports
        .windows(2)
        .all(|pair| pair[1].0 - pair[0].0 >= Duration::from_secs(2)));
    assert!(reports[0].0 >= Duration::from_secs(2));
    assert_eq!((reports[0].1, reports[0].2), ("namespace", 0));
}
