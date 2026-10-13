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
        Self::with_nested_storage(schedule, throttle, false)
    }
    fn with_nested_storage(schedule: &str, throttle: &str, nested: bool) -> Self {
        Self::with_clocks(
            schedule,
            throttle,
            nested,
            Arc::new(tidemarkd::unix_now),
            tidemarkd::monotonic_clock(),
        )
    }
    fn with_clocks(
        schedule: &str,
        throttle: &str,
        nested: bool,
        clock: tidemarkd::Clock,
        monotonic: tidemarkd::Clock,
    ) -> Self {
        Self::with_root_count(schedule, throttle, nested, clock, monotonic, 1)
    }
    fn with_root_count(
        schedule: &str,
        throttle: &str,
        nested: bool,
        clock: tidemarkd::Clock,
        monotonic: tidemarkd::Clock,
        root_count: usize,
    ) -> Self {
        let root = ::test_support::private_tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        std::fs::write(root.path().join("child/file"), b"fixture").unwrap();
        let storage = if nested {
            tempfile::tempdir_in(root.path()).unwrap()
        } else {
            ::test_support::private_tempdir().unwrap()
        };
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
            schedule: schedule.into(),
            window: None,
            mode: RootMode::Walk,
            throttle: throttle.into(),
            ..RootConfig::default()
        }];
        for n in 1..root_count {
            let mut extra = cfg.roots[0].clone();
            extra.path = root.path().join(format!("root-{n}"));
            cfg.roots.push(extra);
        }
        cfg.scan.threads = 1;
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
            clock,
            monotonic,
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

#[test]
fn bound_storage_scheduled_then_api_scan_publishes_and_excludes_state() {
    let h = Harness::with_nested_storage("off", "max", true);
    let mut cfg = h.cfg.clone();
    cfg.roots[0].schedule = "every 1h".into();
    cfg.retention.snapshots.recent = 1;
    cfg.retention.snapshots.daily = 0;
    cfg.retention.snapshots.weekly = 0;
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
    let first = catalog.latest(&h.root()).unwrap().unwrap();
    let first_file = first.file.as_ref().unwrap();
    assert!(first_file.exists());
    assert!(std::fs::canonicalize(first_file)
        .unwrap()
        .starts_with(&h.cfg.general.state_dir));
    let snapshot = tidemark_store::reader::Snapshot::open(first_file).unwrap();
    assert!(snapshot
        .dir_by_path(&RawPath::from_path(&h.cfg.general.state_dir).unwrap())
        .unwrap()
        .is_none());
    assert_no_tmp(&h.cfg.general.state_dir);
    let context = tidemarkd::api::Context {
        state: h.state.clone(),
        scans: h.control.clone(),
        reload: Arc::new(|| {
            Ok(tidemark_core::api::ConfigReloadResult {
                reloaded: true,
                warnings: vec![],
            })
        }),
    };
    let request = tidemark_core::api::Request {
        v: 1,
        id: "bound-storage".into(),
        method: "scan.start".into(),
        params: serde_json::to_value(h.params()).unwrap(),
    };
    // Separate catalog/series timestamps for the two snapshots.
    wait(|| tidemarkd::unix_now() > first.started_at);
    let api_stop = Arc::new(AtomicBool::new(false));
    let mut api_cfg = h.cfg.clone();
    api_cfg.general.socket = h.cfg.general.state_dir.join("api.sock");
    let api_handle = match std::os::unix::net::UnixListener::bind(&api_cfg.general.socket) {
        Ok(probe) => {
            drop(probe);
            std::fs::remove_file(&api_cfg.general.socket).unwrap();
            Some(
                tidemarkd::api::start_with_peer(
                    Arc::new(api_cfg.clone()),
                    tidemarkd::api::Context {
                        state: context.state.clone(),
                        scans: context.scans.clone(),
                        reload: context.reload.clone(),
                    },
                    api_stop.clone(),
                    Arc::new(|_| Ok((0, 1))),
                )
                .unwrap(),
            )
        }
        Err(e) if e.raw_os_error() == Some(1) => {
            eprintln!("Unix socket denied: exercising scan.start dispatch with injected root uid");
            None
        }
        Err(e) => panic!("socket probe: {e}"),
    };
    wait(|| {
        let response = if api_handle.is_some() {
            use std::io::{BufRead, Write};
            let mut stream =
                std::os::unix::net::UnixStream::connect(&api_cfg.general.socket).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(11)))
                .unwrap();
            serde_json::to_writer(&mut stream, &request).unwrap();
            stream.write_all(b"\n").unwrap();
            let mut line = String::new();
            std::io::BufReader::new(stream)
                .read_line(&mut line)
                .unwrap();
            serde_json::from_str::<tidemark_core::api::Response>(&line).unwrap()
        } else {
            tidemarkd::api::dispatch(&request, 0, &context, tidemarkd::unix_now()).unwrap()
        };
        if response
            .error
            .as_ref()
            .is_some_and(|e| e.code == tidemark_core::api::ApiErrorCode::Busy)
        {
            return false;
        }
        assert!(response.ok, "{response:?}");
        true
    });
    api_stop.store(true, Ordering::Release);
    if let Some(handle) = api_handle {
        handle.join().unwrap().unwrap();
    }
    wait(|| {
        catalog
            .latest(&h.root())
            .unwrap()
            .is_some_and(|s| s.scan_id != first.scan_id)
    });
    h.idle();
    let second = catalog.latest(&h.root()).unwrap().unwrap();
    assert!(second.file.as_ref().unwrap().exists());
    assert!(std::fs::canonicalize(second.file.as_ref().unwrap())
        .unwrap()
        .starts_with(&h.cfg.general.state_dir));
    assert!(h
        .control
        .status(&ScanStatusParams {
            root: Some(h.root())
        })
        .unwrap()[0]
        .last_complete
        .as_ref()
        .is_some_and(|s| s.scan_id == second.scan_id));
    let snapshot = tidemark_store::reader::Snapshot::open(second.file.as_ref().unwrap()).unwrap();
    assert!(snapshot
        .dir_by_path(&RawPath::from_path(&h.cfg.general.state_dir).unwrap())
        .unwrap()
        .is_none());
    wait(|| !first_file.exists());
    assert!(tidemark_live::alerts::AlertSource::dir_growth(
        &h.state.view(),
        &h.root().display_lossy(),
        0
    )
    .is_some());
    let series = tidemark_store::timeseries::TimeSeries::open(
        &h.cfg.general.state_dir.join("timeseries.db"),
        false,
    )
    .unwrap();
    assert!(!series
        .query_dir_series(&h.root(), &h.root(), 0, i64::MAX)
        .unwrap()
        .is_empty());
    assert_no_tmp(&h.cfg.general.state_dir);
}

#[test]
fn failure_health_survives_idle_other_root_and_reload_until_recovery() {
    let h = Harness::new("off", "max");
    let missing = h.cfg.roots[0].path.join("missing");
    let mut cfg = h.cfg.clone();
    let mut root = cfg.roots[0].clone();
    root.path = missing.clone();
    root.schedule = "every 1h".into();
    cfg.roots.insert(0, root);
    h.state.replace_config(cfg.clone()).unwrap();
    let degraded = || {
        h.state
            .view()
            .status(tidemarkd::unix_now())
            .unwrap()
            .components
            .into_iter()
            .find(|c| c.name == "scheduler")
            .is_some_and(|c| c.state == "degraded" && c.detail.contains("missing"))
    };
    wait(degraded);
    let raw = RawPath::from_path(&missing).unwrap();
    wait(|| {
        h.control
            .status(&ScanStatusParams {
                root: Some(raw.clone()),
            })
            .unwrap()[0]
            .next_due
            .is_some_and(|due| due > tidemarkd::unix_now())
    });
    let due = h
        .control
        .status(&ScanStatusParams {
            root: Some(raw.clone()),
        })
        .unwrap()[0]
        .next_due
        .unwrap();
    assert!(due > tidemarkd::unix_now());
    // Another root's successful requested scan must leave this failure visible.
    wait(|| match h.control.start(&h.params()) {
        Ok(_) => true,
        Err(e) if e.to_string().contains("Busy") => false,
        Err(e) => panic!("{e}"),
    });
    wait(|| {
        h.control
            .status(&ScanStatusParams {
                root: Some(h.root()),
            })
            .unwrap()[0]
            .last_complete
            .is_some()
    });
    h.idle();
    assert!(degraded());
    h.state.replace_config(cfg.clone()).unwrap();
    assert!(degraded());
    let mut removed = cfg.clone();
    removed.roots.clear();
    h.state.replace_config(removed).unwrap();
    wait(|| !degraded());
    assert!(h.state.view().scans().unwrap().is_empty());
    h.state.replace_config(cfg).unwrap();
    wait(degraded);
    wait(|| {
        h.control
            .status(&ScanStatusParams {
                root: Some(raw.clone()),
            })
            .unwrap()[0]
            .next_due
            .is_some_and(|due| due > tidemarkd::unix_now())
    });
    std::fs::create_dir(&missing).unwrap();
    let params = ScanStartParams {
        root: raw.clone(),
        mode: Some(RootMode::Walk),
        throttle: Some(ThrottlePreset::Max),
    };
    wait(|| match h.control.start(&params) {
        Ok(_) => true,
        Err(e) if e.to_string().contains("Busy") => false,
        Err(e) => panic!("{e}"),
    });
    wait(|| {
        h.control
            .status(&ScanStatusParams {
                root: Some(raw.clone()),
            })
            .unwrap()[0]
            .last_complete
            .is_some()
    });
    wait(|| !degraded());
    let mut cfg = h.cfg.clone();
    cfg.roots.clear();
    h.state.replace_config(cfg).unwrap();
}

#[test]
fn bound_storage_api_request_publishes_first_snapshot() {
    let h = Harness::with_nested_storage("off", "max", true);
    let context = tidemarkd::api::Context {
        state: h.state.clone(),
        scans: h.control.clone(),
        reload: Arc::new(|| {
            Ok(tidemark_core::api::ConfigReloadResult {
                reloaded: true,
                warnings: vec![],
            })
        }),
    };
    let request = tidemark_core::api::Request {
        v: 1,
        id: "first-request".into(),
        method: "scan.start".into(),
        params: serde_json::to_value(h.params()).unwrap(),
    };
    let response = tidemarkd::api::dispatch(&request, 0, &context, tidemarkd::unix_now()).unwrap();
    assert!(response.ok, "{response:?}");
    let id = response.result.unwrap()["scan_id"]
        .as_str()
        .unwrap()
        .to_owned();
    wait(|| {
        h.control
            .status(&ScanStatusParams {
                root: Some(h.root()),
            })
            .unwrap()[0]
            .last_complete
            .as_ref()
            .is_some_and(|s| s.scan_id == id)
    });
    h.idle();
    let catalog = tidemark_store::catalog::Catalog::open(&h.cfg.general.state_dir, false).unwrap();
    let entry = catalog.get(&id).unwrap().unwrap();
    assert!(entry.file.as_ref().unwrap().exists());
    assert!(std::fs::canonicalize(entry.file.as_ref().unwrap())
        .unwrap()
        .starts_with(&h.cfg.general.state_dir));
    let snapshot = tidemark_store::reader::Snapshot::open(entry.file.as_ref().unwrap()).unwrap();
    assert!(snapshot
        .dir_by_path(&RawPath::from_path(&h.cfg.general.state_dir).unwrap())
        .unwrap()
        .is_none());
    assert_no_tmp(&h.cfg.general.state_dir);
}

#[test]
fn scheduled_failures_back_off_with_injected_clock_and_reload_resets() {
    use std::sync::atomic::AtomicI64;
    let now = Arc::new(AtomicI64::new(tidemarkd::unix_now()));
    let clock = {
        let now = now.clone();
        Arc::new(move || now.load(Ordering::Acquire))
    };
    let mono = Arc::new(AtomicI64::new(0));
    let monotonic = {
        let mono = mono.clone();
        Arc::new(move || mono.load(Ordering::Acquire))
    };
    let h = Harness::with_clocks("off", "max", false, clock, monotonic);
    let mut cfg = h.cfg.clone();
    cfg.roots[0].path = h.cfg.roots[0].path.join("absent");
    cfg.roots[0].schedule = "every 1h".into();
    let raw = RawPath::from_path(&cfg.roots[0].path).unwrap();
    h.state.replace_config(cfg.clone()).unwrap();
    let next_due = || {
        h.control
            .status(&ScanStatusParams {
                root: Some(raw.clone()),
            })
            .unwrap()
            .first()
            .and_then(|s| s.next_due)
    };
    for delay in [30, 60, 120, 240, 480, 960, 1920, 3600, 3600] {
        let expected = now.load(Ordering::Acquire) + delay;
        wait(|| next_due() == Some(expected));
        mono.fetch_add(delay, Ordering::AcqRel);
        now.store(expected, Ordering::Release);
    }
    h.state.replace_config(cfg).unwrap();
    let expected = now.load(Ordering::Acquire) + 30;
    wait(|| next_due() == Some(expected));
}

#[test]
fn busy_keeps_fixed_deferral_and_does_not_increment_failure_attempts() {
    use std::sync::atomic::AtomicI64;
    let base = tidemarkd::unix_now();
    let now = Arc::new(AtomicI64::new(base));
    let clock = {
        let now = now.clone();
        Arc::new(move || now.load(Ordering::Acquire))
    };
    let mono = Arc::new(AtomicI64::new(0));
    let monotonic = {
        let mono = mono.clone();
        Arc::new(move || mono.load(Ordering::Acquire))
    };
    let h = Harness::with_clocks("off", "max", false, clock, monotonic);
    let lock = tidemark_scan::orchestrator::ScanLock::acquire(&h.cfg.general.state_dir).unwrap();
    let mut cfg = h.cfg.clone();
    cfg.roots[0].schedule = "every 1h".into();
    h.state.replace_config(cfg).unwrap();
    let next_due = || {
        h.control
            .status(&ScanStatusParams {
                root: Some(h.root()),
            })
            .unwrap()
            .first()
            .and_then(|s| s.next_due)
    };
    wait(|| next_due() == Some(base + 30));
    let component = h
        .state
        .view()
        .status(base)
        .unwrap()
        .components
        .into_iter()
        .find(|c| c.name == "scheduler")
        .unwrap();
    assert_eq!(component.state, "ok");
    drop(lock);
    std::fs::remove_dir_all(&h.cfg.roots[0].path).unwrap();
    mono.store(30, Ordering::Release);
    now.store(base + 30, Ordering::Release);
    // First actual failure gets 30 seconds, rather than the second-attempt minute.
    wait(|| next_due() == Some(base + 60));
}

#[test]
fn catalog_reader_in_separate_process() {
    let Some(state) = std::env::var_os("TIDEMARK_CATALOG_READER_STATE") else {
        return;
    };
    let root = std::env::var_os("TIDEMARK_CATALOG_READER_ROOT").unwrap();
    let catalog =
        tidemark_store::catalog::Catalog::open(std::path::Path::new(&state), false).unwrap();
    let root = RawPath::from_path(std::path::Path::new(&root)).unwrap();
    let entry = catalog.latest(&root).unwrap().unwrap();
    let file = entry.file.unwrap();
    assert!(file.exists());
    assert!(tidemark_store::reader::Snapshot::open(&file).is_ok());
}

#[test]
fn bound_storage_snapshot_readable_in_separate_process_without_sockets() {
    let h = Harness::new("every 1h", "max");
    wait(|| {
        h.control
            .status(&ScanStatusParams { root: None })
            .unwrap()
            .iter()
            .any(|s| s.last_complete.is_some())
    });
    h.idle();
    let catalog = tidemark_store::catalog::Catalog::open(&h.cfg.general.state_dir, false).unwrap();
    let entry = catalog.latest(&h.root()).unwrap().unwrap();
    let file = entry.file.unwrap();
    assert!(file.exists());
    assert!(tidemark_store::reader::Snapshot::open(&file).is_ok());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "runtime::catalog_reader_in_separate_process",
            "--nocapture",
        ])
        .env("TIDEMARK_CATALOG_READER_STATE", &h.cfg.general.state_dir)
        .env("TIDEMARK_CATALOG_READER_ROOT", &h.cfg.roots[0].path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    // Cargo does not expose another package's binary to this test crate.
    // An explicit path allows the workspace validation run to exercise the CLI.
    if let Some(cli) = std::env::var_os("TIDEMARK_CATALOG_CLI") {
        let output = std::process::Command::new(cli)
            .arg("--config")
            .arg(h.cfg.general.state_dir.join("missing.toml"))
            .arg("--state-dir")
            .arg(&h.cfg.general.state_dir)
            .args(["--json", "snapshots", "list"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output
            .stdout
            .windows(entry.scan_id.len())
            .any(|w| w == entry.scan_id.as_bytes()));
        assert_eq!(json["schema"], "tidemark.snapshots.v1");
    }
}

#[test]
fn exact_capacity_root_replacement_and_cumulative_churn_keep_scheduler_alive() {
    let h = Harness::with_root_count(
        "off",
        "max",
        false,
        Arc::new(tidemarkd::unix_now),
        tidemarkd::monotonic_clock(),
        128,
    );
    wait(|| {
        h.state
            .view()
            .scans()
            .is_some_and(|scans| scans.len() == 128)
    });
    for n in 0..140 {
        let mut cfg = h.cfg.clone();
        cfg.roots.truncate(1);
        cfg.roots[0].path = h.cfg.roots[0].path.join(format!("replacement-{n}"));
        let label = cfg.roots[0].path.to_string_lossy().into_owned();
        h.state.replace_config(cfg).unwrap();
        wait(|| {
            h.state
                .view()
                .scans()
                .is_some_and(|scans| scans.len() == 1 && scans[0].root == label)
        });
        assert!(!h.stop.load(Ordering::Acquire));
    }
    // Allow a real scheduler boundary after the last replacement.
    std::thread::sleep(Duration::from_millis(150));
    assert!(!h.stop.load(Ordering::Acquire));
}

#[test]
fn wall_steps_do_not_consume_retry_delay_or_suspend_polling() {
    use std::sync::atomic::AtomicI64;
    let base = tidemarkd::unix_now();
    let wall = Arc::new(AtomicI64::new(base));
    let elapsed = Arc::new(AtomicI64::new(0));
    let clock = {
        let wall = wall.clone();
        Arc::new(move || wall.load(Ordering::Acquire))
    };
    let monotonic = {
        let elapsed = elapsed.clone();
        Arc::new(move || elapsed.load(Ordering::Acquire))
    };
    let h = Harness::with_clocks("off", "max", false, clock, monotonic);
    let mut cfg = h.cfg.clone();
    cfg.roots[0].path = h.cfg.roots[0].path.join("absent");
    cfg.roots[0].schedule = "every 1h".into();
    let raw = RawPath::from_path(&cfg.roots[0].path).unwrap();
    h.state.replace_config(cfg).unwrap();
    let next = || {
        h.control
            .status(&ScanStatusParams {
                root: Some(raw.clone()),
            })
            .unwrap()
            .first()
            .and_then(|s| s.next_due)
    };
    wait(|| next() == Some(base + 30));
    wall.store(base + 7200, Ordering::Release);
    elapsed.store(1, Ordering::Release);
    wait(|| next() == Some(base + 7200 + 29));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(next(), Some(base + 7200 + 29));
    wall.store(base - 7200, Ordering::Release);
    elapsed.store(2, Ordering::Release);
    wait(|| next() == Some(base - 7200 + 28));
    elapsed.store(30, Ordering::Release);
    wait(|| next() == Some(base - 7200 + 60));
}

#[test]
fn successful_cadence_survives_wall_steps() {
    use std::sync::atomic::AtomicI64;
    // The orchestrator owns its wall timestamps; a different injected startup
    // wall time must not turn this new manual completion into restart history.
    let base = tidemarkd::unix_now() + 7200;
    let wall = Arc::new(AtomicI64::new(base));
    let elapsed = Arc::new(AtomicI64::new(0));
    let clock = {
        let wall = wall.clone();
        Arc::new(move || wall.load(Ordering::Acquire))
    };
    let monotonic = {
        let elapsed = elapsed.clone();
        Arc::new(move || elapsed.load(Ordering::Acquire))
    };
    let h = Harness::with_clocks("off", "max", false, clock, monotonic);
    h.control.start(&h.params()).unwrap();
    wait(|| {
        !h.control
            .status(&ScanStatusParams {
                root: Some(h.root()),
            })
            .unwrap()
            .is_empty()
    });
    let status = || {
        h.control
            .status(&ScanStatusParams {
                root: Some(h.root()),
            })
            .unwrap()
            .remove(0)
    };
    wait(|| status().last_complete.is_some() && status().running.is_none());
    let id = status().last_complete.unwrap().scan_id;
    elapsed.store(1, Ordering::Release);
    let mut cfg = h.cfg.clone();
    cfg.roots[0].schedule = "every 1h".into();
    h.state.replace_config(cfg.clone()).unwrap();
    wait(|| status().next_due.is_some());
    let remaining = status().next_due.unwrap() - base;
    assert!(
        (3599..=3600).contains(&remaining),
        "remaining cadence: {remaining}"
    );
    wall.store(base + 7200, Ordering::Release);
    elapsed.store(2, Ordering::Release);

    wait(|| status().next_due == Some(base + 7200 + remaining - 1));
    assert_eq!(status().last_complete.unwrap().scan_id, id);
    h.state.replace_config(cfg.clone()).unwrap();
    cfg.roots[0].schedule = "every 2h".into();
    h.state.replace_config(cfg).unwrap();
    // A changed interval makes publication observable, without a sleep or
    // confusing the recorder's preceding completion status with a fresh poll.
    wait(|| status().next_due == Some(base + 7200 + remaining - 1 + 3600));
    assert_eq!(status().last_complete.unwrap().scan_id, id);
    wall.store(base - 7200, Ordering::Release);
    elapsed.store(3, Ordering::Release);
    wait(|| status().next_due == Some(base - 7200 + remaining - 2 + 3600));
    assert_eq!(status().last_complete.unwrap().scan_id, id);
}
