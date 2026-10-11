//! Owned by: W2W5-prep
//! Compile and inert-result coverage of scan contracts.
#![allow(clippy::unwrap_used)] // Test fixture construction only.
use std::{os::fd::AsFd, sync::atomic::AtomicBool};
use tidemark_core::{
    config::Config,
    path::{RawName, RawPath},
    scan::*,
};
use tidemark_scan::{agg::*, bulk::*, *};
fn cfg() -> AggConfig {
    AggConfig {
        root: RawPath::new(b"/".to_vec()).unwrap(),
        scan_start: 0,
        top_files: 0,
        big_file_threshold: 0,
        max_notable: 0,
        link_instances: false,
        max_stored_errors: 0,
    }
}
struct Recorder;
impl ScanRecorder for Recorder {
    fn scan_started(&self, _: &tidemark_store::catalog::ScanEntry) -> Result<(), ScanError> {
        Err(ScanError::NotYetImplemented)
    }
    fn scan_finished(&self, _: &tidemark_store::catalog::ScanEntry) -> Result<(), ScanError> {
        Err(ScanError::NotYetImplemented)
    }
    fn dir_series(
        &self,
        _: &RawPath,
        _: &str,
        _: i64,
        _: &[tidemark_core::snapshot::DirRow],
    ) -> Result<(), ScanError> {
        Err(ScanError::NotYetImplemented)
    }
}
#[test]
fn every_stub_has_an_inert_result() {
    let root = cfg().root;
    let mut agg = Aggregator::new(cfg());
    agg.add_dir_record(DirRecord {
        id: DirId(0),
        parent: None,
        name: None,
        ino: 0,
        dev: 0,
        mtime: 0,
        dir_disk: 0,
        dir_apparent: 0,
        projid: None,
        entries: 0,
        own: OwnStats::default(),
        links: vec![],
        notable: vec![],
        skipped: vec![],
        errors: vec![],
        races: 0,
    });
    agg.add_gone(&GoneRecord {
        id: DirId(1),
        parent: DirId(0),
        name: RawName::new(b"a".to_vec()).unwrap(),
        reason: GoneReason::Vanished,
    });
    agg.add_skeleton(&NsRecord {
        id: DirId(0),
        parent: None,
        name: None,
        ino: 0,
        dev: 0,
        entries: 0,
        children: vec![],
        skipped: vec![],
        errors: vec![],
        races: 0,
    });
    let attrs = InodeAttrs {
        ino: 0,
        kind: FileKind::Regular,
        nlink: 1,
        disk: 0,
        apparent: 0,
        mtime: 0,
        projid: None,
    };
    agg.set_dir_inode(DirId(0), &attrs);
    agg.add_single(DirId(0), &attrs);
    agg.add_link_set(&attrs, &[]);
    agg.add_races(None, 0);
    assert!(matches!(agg.prepare(), Err(AggError::NotYetImplemented)));
    let mut prepared = Prepared;
    let summary = prepared.summary();
    assert_eq!(
        [
            summary.dirs,
            summary.files,
            summary.inodes,
            summary.t_disk,
            summary.t_apparent,
            summary.t_reclaimable,
            summary.link_sets,
            summary.incomplete_link_sets,
            summary.errors,
            summary.races,
            summary.skipped
        ],
        [0; 11]
    );
    assert!(!summary.degraded);
    assert!(prepared.needs_names().is_empty());
    prepared.set_names(vec![]);
    prepared.annotate(&root, "", "");
    let mut sink = VecSink::default();
    for result in [
        sink.dirs(&[]),
        sink.files(&[]),
        sink.link_sets(&[]),
        sink.link_instances(&[]),
        sink.skipped(&[]),
        sink.errors(&[]),
        sink.annotations(&[]),
    ] {
        assert!(matches!(
            result,
            Err(SinkError::Store(
                tidemark_store::StoreError::NotYetImplemented
            ))
        ));
    }
    assert!(matches!(
        prepared.write(&mut sink),
        Err(AggError::NotYetImplemented)
    ));
    assert_eq!(estimate_memory(123, 456), 0);
    let p = BulkParams {
        walk: tidemark_fs::walk::WalkConfig {
            root: root.clone(),
            threads: 1,
            stat_entries: false,
            exclude: globset::GlobSet::empty(),
            skip_mounts: vec![],
            big_file_threshold: 0,
            top_files: 0,
            scan_start: 0,
            getdents_buf: 4096,
            io_idle: false,
        },
        batch: 1,
        threads: 1,
    };
    let geo = tidemark_sys::xfs::XfsGeometry {
        blocksize: 0,
        agblocks: 0,
        agcount: 0,
        inodesize: 0,
        dirblocksize: 0,
        uuid: [0; 16],
        v5: None,
        ftype: None,
        reflink: None,
    };
    let fd = std::fs::File::open("/").unwrap();
    let cancel = AtomicBool::new(false);
    assert!(matches!(
        run_bulk(
            &p,
            fd.as_fd(),
            &geo,
            &NoopGate,
            &cancel,
            Aggregator::new(cfg())
        ),
        Err(BulkError::NotYetImplemented)
    ));
    let facts = ModeFacts {
        is_xfs: false,
        bulkstat_v5: false,
        cap_sys_admin: false,
        ftype: None,
        root_is_mountpoint: false,
        prev_root_inodes: None,
        fs_used_inodes: 0,
        dirs_estimate: 0,
    };
    for req in [ModeRequest::Auto, ModeRequest::Walk, ModeRequest::Bulk] {
        let (mode, reason) = choose_mode(&facts, &req, 0);
        assert_eq!(mode, ScanMode::Walk);
        assert!(reason.is_empty());
    }
    let req = ScanRequest {
        root,
        mode: ModeRequest::Auto,
        throttle: None,
        threads: None,
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    };
    assert!(matches!(
        run_scan(
            &Config::default(),
            &req,
            &Recorder,
            &cancel,
            &|_: &Progress| panic!("stub called progress")
        ),
        Err(ScanError::NotYetImplemented)
    ));
    let entry = tidemark_store::catalog::ScanEntry {
        scan_id: String::new(),
        root: req.root.clone(),
        mode: ScanMode::Walk,
        started_at: 0,
        finished_at: None,
        status: String::new(),
        degraded: false,
        file: None,
        t_disk: None,
        t_inodes: None,
        t_dirs: None,
        errors: None,
        duration_ms: None,
    };
    assert!(matches!(
        Recorder.scan_started(&entry),
        Err(ScanError::NotYetImplemented)
    ));
    assert!(matches!(
        Recorder.scan_finished(&entry),
        Err(ScanError::NotYetImplemented)
    ));
    assert!(matches!(
        Recorder.dir_series(&req.root, "", 0, &[]),
        Err(ScanError::NotYetImplemented)
    ));
}
