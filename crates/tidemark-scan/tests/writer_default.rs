//! Owned by: WSNAP-writer-default
//! A real scan publishes snapshot format v2 that reads the same as a v1 snapshot of the tree.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::{path::PathBuf, sync::atomic::AtomicBool};
use tidemark_core::{
    config::Config,
    path::RawPath,
    snapshot::{DirRow, SortKey},
};
use tidemark_scan::{
    mode::ModeRequest,
    orchestrator::{run_scan_with_clock, ScanError, ScanRecorder, ScanRequest},
};
use tidemark_store::{
    catalog::{Catalog, ScanEntry},
    reader::Snapshot,
    writer::SnapshotWriter,
};

struct Recorder(PathBuf);
impl ScanRecorder for Recorder {
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
        Ok(Catalog::open(&self.0, true)?.record_start(e)?)
    }
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
        Ok(Catalog::open(&self.0, true)?.record_finish(e)?)
    }
    fn dir_series(&self, _: &RawPath, _: &str, _: i64, _: &[DirRow]) -> Result<(), ScanError> {
        Ok(())
    }
}

fn all_dirs(s: &Snapshot) -> Vec<DirRow> {
    let n = u32::try_from(s.meta().dirs).unwrap();
    (0..n).map(|id| s.dir(id).unwrap()).collect()
}

#[test]
fn scan_publishes_v2_equal_to_v1_reading() {
    let state = test_support::private_tempdir().unwrap();
    let root = test_support::private_tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("a/b/c/d")).unwrap();
    std::fs::create_dir_all(root.path().join("z")).unwrap();
    test_support::private_write(root.path().join("a/b/c/d/f"), &[1u8; 5000]).unwrap();
    test_support::private_write(root.path().join("a/g"), &[2u8; 700]).unwrap();
    test_support::private_write(root.path().join("z/h"), &[3u8; 9000]).unwrap();
    let mut cfg = Config::default();
    cfg.general.state_dir = state.path().to_owned();
    let req = ScanRequest {
        root: RawPath::from_path(root.path()).unwrap(),
        mode: ModeRequest::Walk,
        throttle: Some("max".into()),
        threads: Some(1),
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    };
    let out = run_scan_with_clock(
        &cfg,
        &req,
        &Recorder(state.path().to_owned()),
        &AtomicBool::new(false),
        &|_| {},
        &|| 1_800_000_000,
    )
    .unwrap();
    let file = out.file.unwrap();
    let header = std::fs::read(&file).unwrap();
    assert_eq!(&header[60..64], &2u32.to_be_bytes(), "user_version");
    let v2 = Snapshot::open(&file).unwrap();
    assert_eq!(v2.meta().schema_version, 2);
    let dirs = all_dirs(&v2);
    assert_eq!(dirs.len(), 6);
    assert!(dirs.iter().any(|d| d.depth == 4));

    // Same rows written with the v1 writer must read identically.
    let v1_dir = test_support::private_tempdir().unwrap();
    let mut meta = v2.meta().clone();
    meta.schema_version = 1;
    let mut w = SnapshotWriter::create(v1_dir.path(), &meta).unwrap();
    w.put_dirs(&dirs).unwrap();
    let v1_file = w.finish(&meta).unwrap();
    let v1 = Snapshot::open(&v1_file).unwrap();
    assert_eq!(
        &std::fs::read(&v1_file).unwrap()[60..64],
        &1u32.to_be_bytes()
    );
    for d in &dirs {
        assert_eq!(
            v2.dir_by_path(&d.path).unwrap(),
            v1.dir_by_path(&d.path).unwrap()
        );
        assert_eq!(v2.dir(d.id).unwrap(), v1.dir(d.id).unwrap());
        for sort in [SortKey::Disk, SortKey::Apparent] {
            assert_eq!(
                v2.children(d.id, sort, 100).unwrap(),
                v1.children(d.id, sort, 100).unwrap()
            );
        }
    }
    let root_row = v2.root().unwrap();
    let kids = v2.children(root_row.id, SortKey::Disk, 10).unwrap();
    assert_eq!(kids.len(), 2);
    assert!(kids[0].t_apparent >= kids[1].t_apparent || kids[0].t_disk >= kids[1].t_disk);
}
