//! Owned by: WSNAP-staging-nospace
//! A staging-row (input-row checkpoint) state-space refusal surfaces as `ScanError::NoSpace`, leaving no debris.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tidemark_core::{config::Config, path::RawPath, snapshot::DirRow, units::ByteSize};
use tidemark_scan::{
    mode::ModeRequest,
    orchestrator::{run_scan_with_clock, ScanError, ScanRecorder, ScanRequest},
};
use tidemark_store::catalog::{Catalog, ScanEntry};

struct Recorder {
    state: PathBuf,
    grow: AtomicBool,
}
impl ScanRecorder for Recorder {
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
        Catalog::open(&self.state, true)?.record_start(e)?;
        if self.grow.load(Ordering::SeqCst) {
            // Simulate concurrent state growth after the preflight passed.
            test_support::private_write(self.state.join("leaf"), vec![7u8; 32 << 20]).unwrap();
        }
        Ok(())
    }
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
        Ok(Catalog::open(&self.state, true)?.record_finish(e)?)
    }
    fn dir_series(&self, _: &RawPath, _: &str, _: i64, _: &[DirRow]) -> Result<(), ScanError> {
        Ok(())
    }
}

fn leftovers(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            leftovers(&path, out);
        } else if path.to_string_lossy().ends_with(".tmp") {
            out.push(path);
        }
    }
}

#[test]
fn staging_space_refusal_is_nospace() {
    let state = test_support::private_tempdir().unwrap();
    let root = test_support::private_tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("a/b/c/d")).unwrap();
    std::fs::create_dir_all(root.path().join("z")).unwrap();
    test_support::private_write(root.path().join("a/b/c/d/f"), [1u8; 5000]).unwrap();
    test_support::private_write(root.path().join("z/h"), [3u8; 9000]).unwrap();
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
    let rec = Recorder {
        state: state.path().to_owned(),
        grow: AtomicBool::new(false),
    };
    let first = run_scan_with_clock(&cfg, &req, &rec, &AtomicBool::new(false), &|_| {}, &|| {
        1_800_000_000
    })
    .unwrap();
    let prior = first.file.unwrap();

    cfg.retention.max_state_size = ByteSize(16 << 20);
    rec.grow.store(true, Ordering::SeqCst);
    let req = ScanRequest {
        annotations: (0..100_000)
            .map(|i| (req.root.clone(), format!("k{i}"), "v".to_string()))
            .collect(),
        ..req
    };
    let result = run_scan_with_clock(&cfg, &req, &rec, &AtomicBool::new(false), &|_| {}, &|| {
        1_800_000_100
    });
    assert!(
        matches!(result, Err(ScanError::NoSpace)),
        "unexpected result"
    );

    assert!(prior.exists(), "prior snapshot must survive");
    let entries = Catalog::open(state.path(), true)
        .unwrap()
        .list(Some(&req.root))
        .unwrap();
    let new: Vec<_> = entries.iter().filter(|e| e.status == "failed").collect();
    assert_eq!(new.len(), 1);
    assert!(new[0].file.is_none());
    let mut tmp = Vec::new();
    leftovers(state.path(), &mut tmp);
    assert!(tmp.is_empty(), "{tmp:?}");
}
