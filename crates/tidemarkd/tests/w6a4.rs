//! Owned by: CORE-fix-cancel-record
//! Socket-free catalog outcome regressions using bound daemon storage.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
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
        std::thread::sleep(Duration::from_millis(10));
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
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        std::fs::write(root.path().join("child/file"), b"fixture").unwrap();
        let storage = tempfile::tempdir().unwrap();
        // An API socket may be bound below these directories. Under a sticky
        // ancestor such as /tmp the daemon requires an owned 0700 descendant;
        // tempfile yields 0755.
        for dir in [root.path(), storage.path()] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut cfg = Config::default();
        cfg.general.state_dir = storage.path().to_owned();
        cfg.roots = vec![RootConfig {
            path: root.path().to_owned(),
            schedule: "off".into(),
            window: None,
            mode: RootMode::Walk,
            throttle: "normal".into(),
            ..RootConfig::default()
        }];
        cfg.scan.threads = 1;
        cfg.throttle.min_ops_per_sec = 10;
        cfg.throttle.max_ops_per_sec = 10;
        let state = LiveState::new(&cfg, tidemarkd::unix_now());
        state.bind_storage(std::fs::File::open(storage.path()).unwrap().into());
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = Arc::new(AtomicBool::new(false));
        let (writer, wh) =
            tswriter::start(Arc::new(cfg.clone()), state.clone(), writer_stop.clone()).unwrap();
        let (control, sh) = scheduler::start_with_clocks(
            Arc::new(cfg.clone()),
            state.clone(),
            writer,
            stop.clone(),
            Arc::new(tidemarkd::unix_now),
            tidemarkd::monotonic_clock(),
        )
        .unwrap();
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
            self.api::<Vec<tidemark_core::live::ScanStatus>>(
                "scan.status",
                ScanStatusParams { root: None },
            )
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

impl Harness {
    fn api<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: impl serde::Serialize,
    ) -> T {
        let context = tidemarkd::api::Context {
            state: self.state.clone(),
            scans: self.control.clone(),
            reload: Arc::new(|| {
                Ok(tidemark_core::api::ConfigReloadResult {
                    reloaded: true,
                    warnings: Vec::new(),
                })
            }),
        };
        let request = tidemark_core::api::Request {
            v: 1,
            id: "regression".into(),
            method: method.into(),
            params: serde_json::to_value(params).unwrap(),
        };
        let response =
            tidemarkd::api::dispatch(&request, 0, &context, tidemarkd::unix_now()).unwrap();
        assert!(response.ok, "{:?}", response.error);
        serde_json::from_value(response.result.unwrap()).unwrap()
    }
    fn slow_tree(&self) {
        for n in 0..1000 {
            std::fs::create_dir(self._root.path().join(format!("dir-{n}"))).unwrap();
        }
    }
    fn entry(&self, id: &str) -> tidemark_store::catalog::ScanEntry {
        tidemark_store::catalog::Catalog::open(self._state.path(), false)
            .unwrap()
            .get(id)
            .unwrap()
            .unwrap()
    }
    fn failed(&self, id: &str) {
        wait(|| self.entry(id).status == "failed");
        assert!(self.entry(id).file.is_none());
        let series = tidemark_store::timeseries::TimeSeries::open(
            &self._state.path().join("timeseries.db"),
            false,
        )
        .unwrap();
        assert!(series
            .query_dir_series(&self.root(), &self.root(), 0, i64::MAX)
            .unwrap()
            .is_empty());
    }
}
#[test]
fn cancelled_scan_records_failure_and_following_scan_completes() {
    let h = Harness::new();
    h.slow_tree();
    let scan: tidemark_core::api::ScanStartResult = h.api("scan.start", h.params());
    assert_eq!(h.entry(&scan.scan_id).status, "running");
    let cancelled: tidemark_core::api::ScanCancelResult =
        h.api("scan.cancel", ScanCancelParams { root: h.root() });
    assert!(cancelled.cancelled);
    h.idle();
    h.failed(&scan.scan_id);
    let mut params = h.params();
    params.throttle = Some(ThrottlePreset::Max);
    let mut next = None;
    wait(|| match h.control.start(&params) {
        Ok(scan) => {
            next = Some(scan);
            true
        }
        Err(e) if e.to_string().contains("Busy") => false,
        Err(e) => panic!("following scan failed: {e}"),
    });
    let next = next.unwrap();
    h.idle();
    wait(|| h.entry(&next.scan_id).status == "complete");
    assert!(h.entry(&next.scan_id).file.is_some());
}
#[test]
fn non_cancel_failure_records_failed_without_snapshot() {
    let h = Harness::new();
    // The orchestrator rejects scanning its own state directory after admission.
    let mut params = h.params();
    params.root = RawPath::from_path(h._state.path()).unwrap();
    params.throttle = Some(ThrottlePreset::Max);
    let scan = h.control.start(&params).unwrap();
    h.idle();
    h.failed(&scan.scan_id);
}
#[test]
fn shutdown_interrupted_outcome_is_repaired_at_startup() {
    let mut h = Harness::new();
    h.slow_tree();
    let scan: tidemark_core::api::ScanStartResult = h.api("scan.start", h.params());
    h.stop();
    // Shared daemon shutdown can interrupt final persistence.
    assert_eq!(h.entry(&scan.scan_id).status, "running");
    assert!(h.entry(&scan.scan_id).file.is_none());
    let tmp = h._state.path().join("snapshots/fixture/dead.db.tmp");
    std::fs::create_dir_all(tmp.parent().unwrap()).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        h._state.path().join("snapshots"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    std::fs::write(&tmp, b"abandoned").unwrap();
    startup(&h);
    assert_eq!(h.entry(&scan.scan_id).status, "failed");
    assert_eq!(h.entry(&scan.scan_id).finished_at, Some(123));
    assert!(!tmp.exists());
}

fn startup(h: &Harness) {
    let path = h._root.path().join("config.toml");
    std::fs::write(&path, format!("[general]\nstate_dir='{}'\nsocket='{}'\n[watch]\nenabled=false\n[lvm]\nenabled=false\n[procscan]\nenabled=false\n[[roots]]\npath='{}'\nschedule='off'\n", h._state.path().display(), h._state.path().join("api.sock").display(), h._root.path().display())).unwrap();
    let cfg = Config::load(&path).unwrap();
    // Cleanup precedes socket binding. A denied socket is expected in the worker
    // sandbox; a pre-set shutdown also bounds startup on unrestricted CI.
    let _result = tidemarkd::run_controlled(
        cfg.clone(),
        tidemarkd::RunOptions {
            config_path: path,
            now: 123,
        },
        LiveState::new(&cfg, 123),
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicBool::new(false)),
    );
}

#[test]
fn scan_lock_child() {
    let Some(state) = std::env::var_os("TIDEMARK_LOCK_CHILD") else {
        return;
    };
    let state = std::path::PathBuf::from(state);
    let _lock = tidemark_scan::orchestrator::ScanLock::acquire(&state).unwrap();
    std::fs::write(state.join("lock-ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(30));
}
#[test]
fn startup_skips_cleanup_while_another_process_holds_lock() {
    let mut h = Harness::new();
    h.slow_tree();
    let scan: tidemark_core::api::ScanStartResult = h.api("scan.start", h.params());
    h.stop();
    let tmp = h._state.path().join("snapshots/fixture/dead.db.tmp");
    std::fs::create_dir_all(tmp.parent().unwrap()).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        h._state.path().join("snapshots"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    std::fs::write(&tmp, b"abandoned").unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "scan_lock_child"])
        .env("TIDEMARK_LOCK_CHILD", h._state.path())
        .spawn()
        .unwrap();
    wait(|| h._state.path().join("lock-ready").exists());
    startup(&h);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(h.entry(&scan.scan_id).status, "running");
    assert!(tmp.exists());
    startup(&h);
    assert_eq!(h.entry(&scan.scan_id).status, "failed");
    assert!(!tmp.exists());
}
