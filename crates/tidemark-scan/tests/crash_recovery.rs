//! Owned by: WCRASH-recovery
//! Real killed scans are repaired by the following serialized scan.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::{
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use tidemark_core::{config::Config, path::RawPath, snapshot::DirRow};
use tidemark_scan::{
    mode::ModeRequest,
    orchestrator::{run_scan, ScanError, ScanRecorder, ScanRequest},
};
use tidemark_store::catalog::{Catalog, ScanEntry};
struct Recorder(std::path::PathBuf);
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
fn scan(root: &Path, state: &Path, slow: bool) {
    let mut cfg = Config::default();
    cfg.general.state_dir = state.to_owned();
    cfg.throttle.min_ops_per_sec = 10000;
    cfg.throttle.max_ops_per_sec = 10000;
    let request = ScanRequest {
        root: RawPath::from_path(root).unwrap(),
        mode: ModeRequest::Walk,
        throttle: Some(if slow { "normal" } else { "max" }.into()),
        threads: Some(1),
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    };
    let result = run_scan(
        &cfg,
        &request,
        &Recorder(state.to_owned()),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert!(result.file.is_some());
}
#[test]
fn scan_child() {
    let Some(state) = std::env::var_os("TIDEMARK_SCAN_CHILD") else {
        return;
    };
    scan(
        Path::new(&std::env::var_os("TIDEMARK_SCAN_ROOT").unwrap()),
        Path::new(&state),
        true,
    );
}
#[test]
fn killed_scan_is_cleaned_and_following_scan_completes() {
    use std::os::unix::process::ExitStatusExt;
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    for i in 0..15000 {
        std::fs::create_dir(root.path().join(format!("dir-{i}"))).unwrap();
    }
    let raw = RawPath::from_path(root.path()).unwrap();
    let snapshot_dir = state
        .path()
        .join("snapshots")
        .join(tidemark_store::catalog::root_slug(&raw));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "scan_child", "--nocapture"])
        .env("TIDEMARK_SCAN_CHILD", state.path())
        .env("TIDEMARK_SCAN_ROOT", root.path())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let temporary = loop {
        if let Ok(files) = std::fs::read_dir(&snapshot_dir) {
            if let Some(file) = files
                .filter_map(Result::ok)
                .find(|e| e.file_name().as_encoded_bytes().ends_with(b".db.tmp"))
            {
                break file.path();
            }
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "child completed before kill"
        );
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("no temporary before deadline");
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    child.kill().unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    let catalog = Catalog::open(state.path(), false).unwrap();
    let abandoned = catalog
        .list(None)
        .unwrap()
        .into_iter()
        .find(|e| e.status == "running")
        .unwrap();
    assert!(temporary.exists());
    scan(root.path(), state.path(), false);
    assert!(!temporary.exists());
    // Retention may rebuild the catalog; failed rows must survive that rebuild.
    let repaired = catalog.get(&abandoned.scan_id).unwrap().unwrap();
    assert_eq!(repaired.status, "failed");
    assert!(repaired.finished_at.is_some());
    assert!(repaired.file.is_none());
    assert!(catalog.latest(&raw).unwrap().is_some());
}
