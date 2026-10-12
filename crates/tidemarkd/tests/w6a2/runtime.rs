//! Owned by: W6-A2a
//! Walk snapshots, host exclusion, live reload and cancellation with state deadlines.
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tidemark_core::{
    api::{ScanCancelParams, ScanStartParams, ScanStatusParams},
    config::{Config, RootConfig, RootMode, ThrottlePreset},
    path::RawPath,
};
use tidemarkd::{scheduler, services::tswriter, state::LiveState};
fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "state deadline expired");
        std::thread::yield_now();
    }
}
struct Harness {
    _root: tempfile::TempDir,
    _state: tempfile::TempDir,
    cfg: Config,
    state: LiveState,
    control: scheduler::ScanControl,
    stop: Arc<AtomicBool>,
    writer_stop: Arc<AtomicBool>,
    handles: Option<(tidemarkd::ServiceHandle, tidemarkd::ServiceHandle)>,
}
impl Harness {
    fn new(schedule: &str, throttle: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        std::fs::write(root.path().join("child/file"), b"fixture").unwrap();
        let storage = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.general.state_dir = storage.path().to_owned();
        cfg.roots = vec![RootConfig {
            path: root.path().to_owned(),
            schedule: schedule.into(),
            window: None,
            mode: RootMode::Walk,
            throttle: throttle.into(),
            ..RootConfig::default()
        }];
        cfg.scan.threads = 1;
        let state = LiveState::new(&cfg, tidemarkd::unix_now());
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = Arc::new(AtomicBool::new(false));
        let (writer, wh) =
            tswriter::start(Arc::new(cfg.clone()), state.clone(), writer_stop.clone()).unwrap();
        let (control, sh) =
            scheduler::start(Arc::new(cfg.clone()), state.clone(), writer, stop.clone()).unwrap();
        Self {
            _root: root,
            _state: storage,
            cfg,
            state,
            control,
            stop,
            writer_stop,
            handles: Some((sh, wh)),
        }
    }
    fn root(&self) -> RawPath {
        RawPath::from_path(&self.cfg.roots[0].path).unwrap()
    }
    fn params(&self) -> ScanStartParams {
        ScanStartParams {
            root: self.root(),
            mode: Some(RootMode::Walk),
            throttle: None,
        }
    }
    fn idle(&self) {
        wait(|| {
            self.control
                .status(&ScanStatusParams { root: None })
                .unwrap()
                .iter()
                .all(|s| s.running.is_none())
        });
    }
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let (sh, wh) = self.handles.take().unwrap();
        sh.join().unwrap().unwrap();
        self.writer_stop.store(true, Ordering::Release);
        wh.join().unwrap().unwrap();
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        if self.handles.is_some() {
            self.stop();
        }
    }
}
#[test]
fn scheduled_walk_and_reload_from_off() {
    let h = Harness::new("off", "max");
    let mut cfg = h.cfg.clone();
    cfg.roots[0].schedule = "every 1h".into();
    h.state.replace_config(cfg).unwrap();
    wait(|| {
        h.control
            .status(&ScanStatusParams { root: None })
            .unwrap()
            .iter()
            .any(|s| s.last_complete.is_some())
    });
    h.idle();
    let catalog = tidemark_store::catalog::Catalog::open(&h.cfg.general.state_dir, false).unwrap();
    let scan = catalog.latest(&h.root()).unwrap().unwrap();
    let snapshot = tidemark_store::reader::Snapshot::open(scan.file.as_ref().unwrap()).unwrap();
    assert_eq!(scan.status, "complete");
    assert!(snapshot
        .dir_by_path(&RawPath::from_path(&h.cfg.roots[0].path.join("child")).unwrap())
        .unwrap()
        .is_some());
    // Allocation is measured, never asserted as a filesystem-specific literal.
    let oracle = test_support::oracle::du_disk(&h.cfg.roots[0].path);
    assert_eq!(snapshot.root().unwrap().t_disk, oracle);
}
#[test]
fn held_host_lock_refuses_start() {
    let h = Harness::new("off", "max");
    let lock = tidemark_scan::orchestrator::ScanLock::acquire(&h.cfg.general.state_dir).unwrap();
    let error = h.control.start(&h.params()).unwrap_err();
    assert_eq!(
        error.to_string(),
        tidemarkd::DaemonError::Scan("Busy".into()).to_string()
    );
    drop(lock);
    let mut params = h.params();
    params.throttle = Some(ThrottlePreset::Max);
    let mut accepted = None;
    wait(|| match h.control.start(&params) {
        Ok(r) => {
            accepted = Some(r);
            true
        }
        Err(e) if e.to_string().contains("Busy") => false,
        Err(e) => panic!("unexpected start failure: {e}"),
    });
    assert!(!accepted.unwrap().scan_id.is_empty());
    wait(|| {
        h.control
            .status(&ScanStatusParams { root: None })
            .unwrap()
            .iter()
            .any(|s| s.last_complete.is_some())
    });
}
#[test]
fn second_start_refused_cancel_and_shutdown_leave_no_snapshot() {
    let mut h = Harness::new("off", "normal");
    let mut cfg = h.cfg.clone();
    cfg.throttle.min_ops_per_sec = 10;
    cfg.throttle.max_ops_per_sec = 10;
    h.state.replace_config(cfg).unwrap();
    for n in 0..128 {
        std::fs::write(
            h.cfg.roots[0].path.join(format!("file-{n}")),
            b"metadata fixture",
        )
        .unwrap();
    }
    let accepted = h.control.start(&h.params()).unwrap();
    assert!(!accepted.scan_id.is_empty());
    assert!(h
        .control
        .start(&h.params())
        .unwrap_err()
        .to_string()
        .contains("Busy"));
    assert!(
        h.control
            .cancel(&ScanCancelParams { root: h.root() })
            .unwrap()
            .cancelled
    );
    h.stop();
    assert!(
        tidemark_store::catalog::Catalog::open(&h.cfg.general.state_dir, false)
            .unwrap()
            .latest(&h.root())
            .unwrap()
            .is_none()
    );
    assert_no_tmp(&h.cfg.general.state_dir);
    assert!(tidemark_scan::orchestrator::ScanLock::acquire(&h.cfg.general.state_dir).is_ok());
}
fn assert_no_tmp(root: &std::path::Path) {
    let snapshots = root.join("snapshots");
    if !snapshots.exists() {
        return;
    }
    for dir in std::fs::read_dir(snapshots).unwrap() {
        for file in std::fs::read_dir(dir.unwrap().path()).unwrap() {
            assert!(!file
                .unwrap()
                .file_name()
                .as_encoded_bytes()
                .ends_with(b".db.tmp"));
        }
    }
}

#[test]
fn manual_byte_root_uses_frozen_request_and_status_contract() {
    use std::os::unix::ffi::OsStringExt;
    let h = Harness::new("off", "max");
    let path = h.cfg.roots[0]
        .path
        .join(std::ffi::OsString::from_vec(b"manual-\xff".to_vec()));
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("file"), b"metadata fixture").unwrap();
    let raw = RawPath::from_path(&path).unwrap();
    let params = ScanStartParams {
        root: raw.clone(),
        mode: Some(RootMode::Walk),
        throttle: Some(ThrottlePreset::Max),
    };
    let result = h.control.start(&params).unwrap();
    wait(|| {
        h.control
            .status(&ScanStatusParams {
                root: Some(raw.clone()),
            })
            .unwrap()
            .iter()
            .any(|s| {
                s.last_complete
                    .as_ref()
                    .is_some_and(|last| last.scan_id == result.scan_id)
            })
    });
    let entry = tidemark_store::catalog::Catalog::open(&h.cfg.general.state_dir, false)
        .unwrap()
        .get(&result.scan_id)
        .unwrap()
        .unwrap();
    assert_eq!(entry.root.as_bytes(), raw.as_bytes());
    assert_eq!(entry.status, "complete");
}

#[test]
fn successful_short_cadence_has_no_failure_backoff() {
    let h = Harness::new("every 1s", "max");
    let mut first = None;
    wait(|| {
        first = h
            .control
            .status(&ScanStatusParams {
                root: Some(h.root()),
            })
            .unwrap()
            .first()
            .and_then(|s| s.last_complete.as_ref())
            .map(|s| s.scan_id.clone());
        first.is_some()
    });
    wait(|| {
        h.control
            .status(&ScanStatusParams {
                root: Some(h.root()),
            })
            .unwrap()
            .iter()
            .any(|s| {
                s.last_complete
                    .as_ref()
                    .is_some_and(|last| Some(&last.scan_id) != first.as_ref())
            })
    });
}

#[test]
fn failed_root_does_not_starve_another_due_root() {
    let h = Harness::new("off", "max");
    let mut cfg = h.cfg.clone();
    cfg.roots[0].schedule = "every 1h".into();
    let mut missing = cfg.roots[0].clone();
    missing.path = h.cfg.roots[0].path.join("missing-root");
    cfg.roots.insert(0, missing);
    h.state.replace_config(cfg).unwrap();
    wait(|| {
        h.control
            .status(&ScanStatusParams {
                root: Some(h.root()),
            })
            .unwrap()
            .iter()
            .any(|s| s.last_complete.is_some())
    });
}
