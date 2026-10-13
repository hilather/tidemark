//! Owned by: W6-A1
//! Unprivileged state, persistence and lifecycle evidence.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tidemark_core::{api::*, config::Config, live::*, path::RawPath};
use tidemark_live::alerts::AlertSource;
use tidemarkd::{
    component,
    services::{
        lvm, sampler,
        tswriter::{self, Message},
    },
    state::{LiveState, Publication},
    Shutdown,
};
fn wait(mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < end, "deadline exceeded");
        std::thread::yield_now();
    }
}
fn wait_ready(
    handle: tidemarkd::ServiceHandle,
    mut ready: impl FnMut() -> bool,
) -> tidemarkd::ServiceHandle {
    wait(|| ready() || handle.is_finished());
    if handle.is_finished() {
        panic!(
            "daemon lifecycle exited before readiness: {:?}",
            handle.join().unwrap()
        );
    }
    handle
}
fn sockets_available() -> bool {
    let dir = tempfile::tempdir().unwrap();
    match std::os::unix::net::UnixListener::bind(dir.path().join("probe.sock")) {
        Ok(_) => true,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
            ) =>
        {
            eprintln!("Unix sockets unavailable in sandbox; readiness test deferred to CI");
            false
        }
        Err(error) => panic!("socket availability probe: {error}"),
    }
}
fn health() -> ComponentState {
    component("test", "ok", "measured")
}
fn sample(ts: i64, used: u64) -> FsSample {
    FsSample {
        ts,
        mount: "/fixture".into(),
        size: 100000,
        used,
        avail: 100000 - used,
        inodes: 10,
        inodes_used: 2,
    }
}
fn publish(s: &LiveState, p: Publication) {
    s.publish(p, health()).unwrap();
}
fn containing_mount(path: &Path) -> PathBuf {
    let mounts = tidemark_fs::mount::read_mountinfo().unwrap();
    let raw = RawPath::from_path(&std::fs::canonicalize(path).unwrap()).unwrap();
    tidemark_fs::mount::mount_for_path(&mounts, &raw)
        .unwrap()
        .mount_point
        .as_path()
        .to_path_buf()
}
fn write_config(path: &Path, interval: &str) {
    let p = path.parent().unwrap();
    std::fs::write(path,format!("[general]\nstate_dir='{}'\nsocket='{}'\n[sampler]\nmounts=['{}']\ninterval='{}'\n[lvm]\nenabled=false\n[watch]\nenabled=false\nmounts=[]\n[procscan]\nenabled=false\n[metrics]\ntextfile='{}'\n[[roots]]\npath='{}'\nschedule='off'\n",p.join("state").display(),p.join("api.sock").display(),containing_mount(p).display(),interval,p.join("metrics.prom").display(),p.join("mount").display())).unwrap();
}
fn fixture() -> (tempfile::TempDir, PathBuf, Config) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    // The API socket lives in this directory. Under a sticky ancestor such as
    // /tmp the daemon requires an owned 0700 directory; tempfile yields 0755.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir(dir.path().join("mount")).unwrap();
    let path = dir.path().join("config.toml");
    write_config(&path, "1h");
    let c = Config::load(&path).unwrap();
    (dir, path, c)
}
fn flush(tx: &tswriter::Sender) {
    let (a, r) = crossbeam_channel::bounded(1);
    tx.send(Message::Flush(a)).unwrap();
    r.recv_timeout(Duration::from_secs(10)).unwrap();
}
#[test]
fn ring_eviction_and_injected_rates() {
    let s = LiveState::new(&Config::default(), 0);
    for ts in 0..(tidemarkd::state::MAX_FS_SAMPLES as i64 + 20) {
        publish(&s, Publication::Fs(sample(ts, ts as u64)));
    }
    assert!(s.view_at(0).fs_sample("/fixture").is_none());
    assert_eq!(
        s.view_at(7300)
            .fs_rate("/fixture", Duration::from_secs(300)),
        Some(1.0)
    );
    publish(&s, Publication::Fs(sample(2, 20)));
    for (ts, b) in [(3, 30), (4, 40)] {
        publish(&s, Publication::Fs(sample(ts, b)));
    }
    assert_eq!(
        s.view_at(4)
            .fs_rate("/fixture", Duration::from_millis(2500)),
        None
    );
    assert!(s
        .view_at(2)
        .fs_rate("/fixture", Duration::from_secs(10))
        .is_none());
}
#[test]
fn alert_source_unavailable_empty_and_measured() {
    let mut c = Config::default();
    c.roots[0].path = "/fixture".into();
    c.roots[0].window = Some("00:00-06:00".into());
    c.roots[0].schedule = "every 1h".into();
    let s = LiveState::new(&c, 100);
    let v = s.view_at(100);
    assert!(v.fs_sample("/fixture").is_none());
    assert!(v.fs_rate("/fixture", Duration::from_secs(1)).is_none());
    assert!(v.hot_dirs("/fixture", Duration::from_secs(90)).is_none());
    assert!(v
        .watch_overflows("/fixture", Duration::from_secs(90))
        .is_none());
    assert!(v.phantom_bytes("/fixture").is_none());
    assert!(v.lvm().is_none());
    assert!(v.dir_growth("/fixture", 2).is_none());
    assert!(v.scan_age("unknown").is_none());
    assert!(v.scan_age("/fixture").is_none());
    assert!(v.scans().is_none());
    publish(
        &s,
        Publication::Scan(ScanStatus {
            root: "/fixture".into(),
            running: None,
            last_complete: None,
            next_due: None,
        }),
    );
    assert_eq!(
        s.view().scan_age("/fixture").unwrap().effective_cadence,
        Some(Duration::from_secs(86400))
    );
    assert_eq!(
        s.view().scan_age("/fixture").unwrap().last_complete_at,
        None
    );
    publish(
        &s,
        Publication::Lvm(LvmResult {
            mounts: vec![],
            vgs: vec![],
            sampled_at: 100,
        }),
    );
    publish(&s, Publication::DirGrowth(("/fixture".into(), 2, vec![])));
    publish(&s, Publication::Alerts((vec![], vec![])));
    publish(
        &s,
        Publication::WatchWindow {
            mount: "/fixture".into(),
            window: Duration::from_secs(120),
            measured_at: 100,
            hot: Some(vec![]),
            overflows: Some(0),
        },
    );
    let v = s.view_at(100);
    assert_eq!(v.lvm(), Some(vec![]));
    assert_eq!(v.dir_growth("/fixture", 2), Some(vec![]));
    assert_eq!(v.active_alerts(), Some(vec![]));
    assert_eq!(
        v.hot_dirs("/fixture", Duration::from_secs(120)),
        Some(vec![])
    );
    assert_eq!(
        v.watch_overflows("/fixture", Duration::from_secs(120)),
        Some(0)
    );
    assert!(v.hot_dirs("/fixture", Duration::from_secs(60)).is_none());
    assert!(s
        .view_at(191)
        .watch_overflows("/fixture", Duration::from_secs(120))
        .is_none());
    let hot = HotEntry {
        key: b"/fixture/a".to_vec(),
        events: 1,
        files: 1,
        bytes_delta: 90,
        top_unit: None,
        lossy: false,
    };
    publish(
        &s,
        Publication::WatchWindow {
            mount: "/fixture".into(),
            window: Duration::from_secs(120),
            measured_at: 100,
            hot: Some(vec![hot.clone()]),
            overflows: Some(7),
        },
    );
    assert_eq!(
        s.view_at(100)
            .hot_dirs("/fixture", Duration::from_secs(120)),
        Some(vec![hot])
    );
    assert_eq!(
        s.view_at(100)
            .watch_overflows("/fixture", Duration::from_secs(120)),
        Some(7)
    );
    let lv = LvmNow {
        vg: "vg".into(),
        lv: "lv".into(),
        lv_size: 100,
        vg_free: 10,
        thin_pool: None,
        data_pct: None,
        meta_pct: None,
        sampled_at: 100,
    };
    publish(
        &s,
        Publication::Lvm(LvmResult {
            mounts: vec![("/fixture".into(), lv.clone())],
            vgs: vec![],
            sampled_at: 100,
        }),
    );
    assert_eq!(s.view().lvm(), Some(vec![lv]));
    publish(
        &s,
        Publication::DirGrowth((
            "/fixture".into(),
            2,
            vec![(RawPath::new(b"/fixture/a".to_vec()).unwrap(), 9)],
        )),
    );
    assert_eq!(s.view().dir_growth("/fixture", 2).unwrap()[0].1, 9);
    publish(
        &s,
        Publication::Scan(ScanStatus {
            root: "/fixture".into(),
            running: None,
            last_complete: Some(LastScan {
                scan_id: "a".into(),
                finished_at: 99,
                mode: "walk".into(),
                duration_ms: 1,
            }),
            next_due: None,
        }),
    );
    assert_eq!(
        s.view().scan_age("/fixture").unwrap().last_complete_at,
        Some(99)
    );
    assert!(s
        .view()
        .now(&NowParams { mount: None }, 100)
        .unwrap()
        .degraded
        .iter()
        .any(|s| s == "phantom: not measured"));
}
#[test]
fn scanner_measured_empty() {
    use tidemark_live::procscan::{ProcScanner, ProcScannerOptions};
    let s = LiveState::default();
    let mut scanner = ProcScanner::new(
        ProcScannerOptions {
            mounts: vec![(RawPath::new(b"/fixture".to_vec()).unwrap(), u64::MAX)],
            open_writers: false,
            exclude_prefixes: vec![],
        },
        Arc::new(tidemark_fs::attribution::AttributionCache::new(
            Duration::from_secs(1),
            0,
        )),
    );
    scanner.scan_once(1).unwrap();
    publish(
        &s,
        Publication::Procscan(Arc::new(scanner.report().unwrap())),
    );
    assert_eq!(s.view().phantom_bytes("/fixture"), Some(0));
    assert_eq!(
        s.view().writers(&WritersParams {
            mount: None,
            window: Duration::from_secs(1)
        }),
        Some(vec![])
    );
}
#[test]
fn reject_incomplete_over_cap_inventory() {
    let s = LiveState::default();
    publish(&s, Publication::Alerts((vec![], vec![])));
    let d = tidemark_live::alerts::AlertDiagnostic {
        rule: "r".into(),
        reason: "unknown".into(),
    };
    assert!(s
        .publish(
            Publication::Alerts((vec![], vec![d; tidemarkd::state::MAX_INVENTORY + 1])),
            health()
        )
        .is_err());
    assert_eq!(s.view().active_alerts(), Some(vec![]));
}
#[test]
fn writer_batch_flush_shutdown_and_ack() {
    let (_dir, _, c) = fixture();
    std::fs::create_dir(&c.general.state_dir).unwrap();
    let stop = Shutdown::default();
    let (tx, h) =
        tswriter::start(Arc::new(c.clone()), LiveState::new(&c, 0), stop.clone()).unwrap();
    assert_eq!(tx.capacity(), Some(tswriter::CHANNEL_CAPACITY));
    for ts in 0..20 {
        tx.send(Message::Fs(vec![sample(ts, ts as u64)])).unwrap();
    }
    flush(&tx);
    let db = tidemark_store::timeseries::TimeSeries::open(
        &c.general.state_dir.join("timeseries.db"),
        false,
    )
    .unwrap();
    assert_eq!(db.query_fs("/fixture", 0, 20).unwrap().len(), 20);
    drop(db);
    let mut entry = tidemark_store::catalog::ScanEntry {
        scan_id: "test".into(),
        root: RawPath::new(b"/fixture".to_vec()).unwrap(),
        mode: tidemark_core::scan::ScanMode::Walk,
        started_at: 1,
        finished_at: None,
        status: "running".into(),
        degraded: false,
        file: None,
        t_disk: None,
        t_inodes: None,
        t_dirs: None,
        errors: None,
        duration_ms: None,
    };
    let (a, r) = crossbeam_channel::bounded(1);
    tx.send(Message::ScanStarted(entry, a)).unwrap();
    r.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    entry = tidemark_store::catalog::Catalog::open(&c.general.state_dir, false)
        .unwrap()
        .get("test")
        .unwrap()
        .unwrap();
    entry.status = "failed".into();
    entry.finished_at = Some(2);
    let (a, r) = crossbeam_channel::bounded(1);
    tx.send(Message::ScanFinished(entry, a)).unwrap();
    r.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!(
        tidemark_store::catalog::Catalog::open(&c.general.state_dir, false)
            .unwrap()
            .get("test")
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
    tx.send(Message::Fs(vec![sample(21, 21)])).unwrap();
    stop.store(true, Ordering::Release);
    h.join().unwrap().unwrap();
    assert_eq!(
        tidemark_store::timeseries::TimeSeries::open(
            &c.general.state_dir.join("timeseries.db"),
            false
        )
        .unwrap()
        .query_fs("/fixture", 21, 21)
        .unwrap()
        .len(),
        1
    );
}
#[test]
fn full_channel_sampling_is_nonblocking() {
    let (_dir, _, c) = fixture();
    let s = LiveState::new(&c, 0);
    let (tx, rx) = crossbeam_channel::bounded(1);
    tx.send(Message::Fs(vec![])).unwrap();
    assert!(tx.try_send(Message::Fs(vec![])).is_err());
    let stop = Shutdown::default();
    let h = sampler::start(Arc::new(c), s.clone(), tx, stop.clone()).unwrap();
    wait(|| {
        s.view()
            .status(1)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "sampler" && c.state == "degraded")
    });
    let sample = s.view().fs_samples().unwrap().remove(0);
    assert!(s.view_at(sample.ts).fs_sample(&sample.mount).is_some());
    stop.store(true, Ordering::Release);
    h.join().unwrap().unwrap();
    drop(rx);
}
#[test]
fn compaction_uses_injected_clock() {
    let (_dir, _, mut c) = fixture();
    std::fs::create_dir(&c.general.state_dir).unwrap();
    c.retention.samples.raw = Duration::from_secs(1);
    let now = Arc::new(AtomicI64::new(0));
    let clock = {
        let n = now.clone();
        Arc::new(move || n.load(Ordering::Acquire)) as tidemarkd::Clock
    };
    let elapsed = Arc::new(AtomicI64::new(0));
    let monotonic = {
        let elapsed = elapsed.clone();
        Arc::new(move || elapsed.load(Ordering::Acquire))
    };
    let stop = Shutdown::default();
    let (tx, h) = tswriter::start_with_clocks(
        Arc::new(c.clone()),
        LiveState::new(&c, 0),
        stop.clone(),
        clock,
        monotonic,
    )
    .unwrap();
    tx.send(Message::Fs(vec![sample(1, 10)])).unwrap();
    flush(&tx);
    elapsed.store(3599, Ordering::Release);
    now.store(7200, Ordering::Release);
    flush(&tx);
    // The second barrier also waits for the prior iteration's compaction check.
    flush(&tx);
    assert_eq!(
        tidemark_store::timeseries::TimeSeries::open(
            &c.general.state_dir.join("timeseries.db"),
            false
        )
        .unwrap()
        .query_fs("/fixture", 1, 1)
        .unwrap()
        .len(),
        1
    );
    now.store(0, Ordering::Release);
    flush(&tx);
    flush(&tx);
    assert_eq!(
        tidemark_store::timeseries::TimeSeries::open(
            &c.general.state_dir.join("timeseries.db"),
            false
        )
        .unwrap()
        .query_fs("/fixture", 1, 1)
        .unwrap()
        .len(),
        1
    );
    now.store(3601, Ordering::Release);
    elapsed.store(3601, Ordering::Release);
    flush(&tx);
    let db = tidemark_store::timeseries::TimeSeries::open(
        &c.general.state_dir.join("timeseries.db"),
        false,
    )
    .unwrap();
    wait(|| db.query_fs("/fixture", 1, 1).unwrap().is_empty());
    stop.store(true, Ordering::Release);
    h.join().unwrap().unwrap();
}
#[test]
fn lvm_disabled_and_backoff() {
    let (_dir, _, c) = fixture();
    let s = LiveState::new(&c, 0);
    let (tx, _rx) = crossbeam_channel::bounded(1);
    let stop = Shutdown::default();
    let h = lvm::start(Arc::new(c), s.clone(), tx, stop.clone()).unwrap();
    wait(|| {
        s.view()
            .status(1)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "lvm" && c.state == "disabled")
    });
    stop.store(true, Ordering::Release);
    h.join().unwrap().unwrap();
    assert_eq!(
        lvm::retry_delay(1, Duration::from_secs(300)),
        Duration::from_secs(300)
    );
    assert_eq!(
        lvm::retry_delay(100, Duration::from_secs(4)),
        Duration::from_secs(3600)
    );
}
#[test]
fn lifecycle_reload_and_shutdown() {
    if !sockets_available() {
        return;
    }
    let (dir, path, c) = fixture();
    let s = LiveState::new(&c, 100);
    let stop = Shutdown::default();
    let reload = Shutdown::default();
    let h = {
        let s = s.clone();
        let stop = stop.clone();
        let reload = reload.clone();
        let path = path.clone();
        std::thread::spawn(move || {
            tidemarkd::run_controlled(
                c,
                tidemarkd::RunOptions {
                    config_path: path,
                    now: 100,
                },
                s,
                stop,
                reload,
            )
        })
    };
    let h = wait_ready(h, || {
        s.view()
            .status(100)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "daemon" && c.state == "ok")
    });
    assert_eq!(s.view().firstboot_inputs().unwrap().config_sha256.len(), 64);
    assert!(s.view().state_space_refused().is_some());
    let (c, g) = s.config();
    assert!(c.watch.exclude_prefixes.contains(&c.general.state_dir));
    assert!(c.procscan.exclude_prefixes.contains(&c.general.state_dir));
    assert!(!c.roots[0].exclude.is_empty());
    write_config(&path, "2h");
    reload.store(true, Ordering::Release);
    wait(|| s.config().1 > g);
    assert_eq!(s.config().0.sampler.interval, Duration::from_secs(7200));
    std::fs::write(&path, "[sampler]\ninterval='0s'\n").unwrap();
    assert!(
        !tidemarkd::reload_callback(path, s.clone())()
            .unwrap()
            .reloaded
    );
    assert_eq!(s.config().0.sampler.interval, Duration::from_secs(7200));
    stop.store(true, Ordering::Release);
    wait(|| h.is_finished());
    h.join().unwrap().unwrap();
    assert!(std::fs::read_dir(dir.path().join("state"))
        .unwrap()
        .all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
}
#[test]
fn unsafe_state_refused_without_creation() {
    let (dir, path, _) = fixture();
    let forbidden = dir.path().join("unsafe");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace(
            &dir.path().join("state").display().to_string(),
            &forbidden.display().to_string(),
        )
        .replace(
            &dir.path().join("mount").display().to_string(),
            &forbidden.join("tracked").display().to_string(),
        );
    std::fs::write(&path, text).unwrap();
    let c = Config::load(&path).unwrap();
    assert!(tidemarkd::run_controlled(
        c.clone(),
        tidemarkd::RunOptions {
            config_path: path,
            now: 1
        },
        LiveState::new(&c, 1),
        Shutdown::default(),
        Shutdown::default()
    )
    .is_err());
    assert!(!forbidden.exists());
}
#[test]
fn nonprivate_state_directories_refused_without_creation() {
    use std::os::unix::fs::PermissionsExt;
    for mode in [0o777, 0o750] {
        let (dir, path, cfg) = fixture();
        std::fs::create_dir(&cfg.general.state_dir).unwrap();
        std::fs::set_permissions(
            &cfg.general.state_dir,
            std::fs::Permissions::from_mode(mode),
        )
        .unwrap();
        let error = tidemarkd::run_controlled(
            cfg.clone(),
            tidemarkd::RunOptions {
                config_path: path,
                now: 1,
            },
            LiveState::new(&cfg, 1),
            Shutdown::default(),
            Shutdown::default(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("no group/world permissions"),
            "{error}"
        );
        assert_eq!(
            std::fs::read_dir(&cfg.general.state_dir).unwrap().count(),
            0
        );
        assert_eq!(
            std::fs::metadata(&cfg.general.state_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            mode
        );
        assert!(!dir.path().join("api.sock").exists());
        assert!(!dir.path().join("metrics.prom").exists());
    }
}

#[test]
fn private_and_missing_state_directories_start() {
    if !sockets_available() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    for existing in [true, false] {
        let (_dir, path, cfg) = fixture();
        if existing {
            std::fs::create_dir(&cfg.general.state_dir).unwrap();
            std::fs::set_permissions(
                &cfg.general.state_dir,
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        let state = LiveState::new(&cfg, 1);
        let stop = Shutdown::default();
        let handle = {
            let state = state.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                tidemarkd::run_controlled(
                    cfg,
                    tidemarkd::RunOptions {
                        config_path: path,
                        now: 1,
                    },
                    state,
                    stop,
                    Shutdown::default(),
                )
            })
        };
        let handle = wait_ready(handle, || {
            state
                .view()
                .status(1)
                .unwrap()
                .components
                .iter()
                .any(|c| c.name == "daemon" && c.state == "ok")
        });
        stop.store(true, Ordering::Release);
        wait(|| handle.is_finished());
        handle.join().unwrap().unwrap();
        assert_eq!(
            std::fs::metadata(_dir.path().join("state"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn binary_ready_and_stopping() {
    if !sockets_available() {
        return;
    }
    use std::os::unix::net::UnixDatagram;
    let (dir, path, _) = fixture();
    let notify = dir.path().join("notify");
    let sock = match UnixDatagram::bind(&notify) {
        Ok(socket) => {
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            Some(socket)
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => None,
        Err(error) => panic!("notification socket: {error}"),
    };
    let stderr = std::fs::File::create(dir.path().join("stderr.log")).unwrap();
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("tidemarkd"))
        .arg("--config")
        .arg(path)
        .env("NOTIFY_SOCKET", &notify)
        .env_remove("INVOCATION_ID")
        .env("RUST_LOG", "info")
        .stderr(stderr)
        .spawn()
        .unwrap();
    let mut bytes = [0; 4096];
    if let Some(sock) = &sock {
        let n = sock.recv(&mut bytes).unwrap();
        assert!(String::from_utf8_lossy(&bytes[..n]).contains("READY=1"));
    } else {
        wait(|| {
            std::fs::read_to_string(dir.path().join("stderr.log"))
                .unwrap()
                .contains("daemon ready")
        });
    }
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    if let Some(sock) = &sock {
        let n = sock.recv(&mut bytes).unwrap();
        assert!(String::from_utf8_lossy(&bytes[..n]).contains("STOPPING=1"));
    } else {
        wait(|| {
            std::fs::read_to_string(dir.path().join("stderr.log"))
                .unwrap()
                .contains("daemon stopping")
        });
    }
    let mut exit = None;
    wait(|| {
        exit = child.try_wait().unwrap();
        exit.is_some()
    });
    assert!(exit.unwrap().success());
}

#[test]
fn phantom_measurement_uses_allocated_byte_oracle() {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use tidemark_live::procscan::{ProcScanner, ProcScannerOptions};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("held");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(&[0u8; 32768]).unwrap();
    let metadata = file.metadata().unwrap();
    let allocated = metadata.blocks() * 512;
    std::fs::remove_file(&path).unwrap();
    let mount = RawPath::from_path(dir.path()).unwrap();
    let mut scanner = ProcScanner::new(
        ProcScannerOptions {
            mounts: vec![(mount.clone(), metadata.dev())],
            open_writers: false,
            exclude_prefixes: vec![],
        },
        Arc::new(tidemark_fs::attribution::AttributionCache::new(
            Duration::from_secs(1),
            0,
        )),
    );
    scanner.scan_once(1).unwrap();
    let state = LiveState::default();
    publish(
        &state,
        Publication::Procscan(Arc::new(scanner.report().unwrap())),
    );
    assert!(
        state
            .view()
            .phantom_bytes(mount.as_path().to_str().unwrap())
            .unwrap()
            >= allocated
    );
    assert!(state
        .view()
        .phantom(&PhantomParams { mount: None })
        .unwrap()
        .iter()
        .flat_map(|s| &s.holders)
        .any(|h| h.ino == metadata.ino() && h.bytes == allocated));
}
#[test]
fn corrupt_catalog_is_preserved_before_recovery() {
    if !sockets_available() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let (dir, path, cfg) = fixture();
    std::fs::create_dir(&cfg.general.state_dir).unwrap();
    std::fs::set_permissions(
        &cfg.general.state_dir,
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let bad = b"deliberately invalid sqlite header";
    std::fs::write(cfg.general.state_dir.join("timeseries.db"), bad).unwrap();
    let state = LiveState::new(&cfg, 1);
    let stop = Shutdown::default();
    let h = {
        let state = state.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            tidemarkd::run_controlled(
                cfg,
                tidemarkd::RunOptions {
                    config_path: path,
                    now: 1,
                },
                state,
                stop,
                Shutdown::default(),
            )
        })
    };
    let h = wait_ready(h, || {
        state
            .view()
            .status(1)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "daemon" && c.state == "ok")
    });
    stop.store(true, Ordering::Release);
    wait(|| h.is_finished());
    h.join().unwrap().unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("state/timeseries-recovery-0/timeseries.db")).unwrap(),
        bad
    );
}
#[test]
fn symlink_state_artifacts_are_refused() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (dir, path, cfg) = fixture();
    std::fs::create_dir(&cfg.general.state_dir).unwrap();
    std::fs::set_permissions(
        &cfg.general.state_dir,
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let target = dir.path().join("protected");
    std::fs::write(&target, b"unchanged").unwrap();
    symlink(&target, cfg.general.state_dir.join("timeseries.db")).unwrap();
    assert!(tidemarkd::run_controlled(
        cfg.clone(),
        tidemarkd::RunOptions {
            config_path: path,
            now: 1
        },
        LiveState::new(&cfg, 1),
        Shutdown::default(),
        Shutdown::default()
    )
    .is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"unchanged");
}

#[test]
fn startup_reads_last_complete_from_catalog() {
    if !sockets_available() {
        return;
    }
    let (_dir, path, cfg) = fixture();
    let catalog = tidemark_store::catalog::Catalog::open(&cfg.general.state_dir, true).unwrap();
    let entry = tidemark_store::catalog::ScanEntry {
        scan_id: "previous".into(),
        root: RawPath::from_path(&cfg.roots[0].path).unwrap(),
        mode: tidemark_core::scan::ScanMode::Walk,
        started_at: 10,
        finished_at: Some(20),
        status: "complete".into(),
        degraded: false,
        file: None,
        t_disk: Some(0),
        t_inodes: Some(0),
        t_dirs: Some(0),
        errors: Some(0),
        duration_ms: Some(10_000),
    };
    catalog.record_finish(&entry).unwrap();
    drop(catalog);
    let state = LiveState::new(&cfg, 100);
    let root = cfg.roots[0].path.to_str().unwrap().to_owned();
    let stop = Shutdown::default();
    let h = {
        let state = state.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            tidemarkd::run_controlled(
                cfg,
                tidemarkd::RunOptions {
                    config_path: path,
                    now: 100,
                },
                state,
                stop,
                Shutdown::default(),
            )
        })
    };
    let h = wait_ready(h, || {
        state
            .view()
            .status(100)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "daemon" && c.state == "ok")
    });
    assert_eq!(
        state.view().scan_age(&root).unwrap().last_complete_at,
        Some(20)
    );
    assert_eq!(
        state.view().scans().unwrap()[0]
            .last_complete
            .as_ref()
            .unwrap()
            .scan_id,
        "previous"
    );
    stop.store(true, Ordering::Release);
    wait(|| h.is_finished());
    h.join().unwrap().unwrap();
}

#[test]
fn repeated_gauge_keys_and_restart_do_not_poison_writer() {
    let (_dir, _, cfg) = fixture();
    std::fs::create_dir(&cfg.general.state_dir).unwrap();
    let state = LiveState::new(&cfg, 100);
    let stop = Shutdown::default();
    let (tx, handle) = tswriter::start(Arc::new(cfg.clone()), state, stop.clone()).unwrap();
    tx.send(Message::Fs(vec![sample(100, 1), sample(100, 2)]))
        .unwrap();
    flush(&tx);
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    drop(tx);
    let state = LiveState::new(&cfg, 100);
    let stop = Shutdown::default();
    let (tx, handle) = tswriter::start(Arc::new(cfg.clone()), state.clone(), stop.clone()).unwrap();
    tx.send(Message::Fs(vec![sample(100, 3), sample(101, 4)]))
        .unwrap();
    let lv = LvmSample {
        ts: 100,
        vg: "vg".into(),
        lv: "lv".into(),
        lv_size: 100,
        vg_free: 10,
        data_pct: None,
        meta_pct: None,
    };
    tx.send(Message::Lvm(vec![lv.clone(), lv.clone()])).unwrap();
    flush(&tx);
    tx.send(Message::Lvm(vec![lv])).unwrap();
    flush(&tx);
    assert!(state
        .view()
        .status(100)
        .unwrap()
        .components
        .iter()
        .any(|c| c.name == "tswriter" && c.state == "ok"));
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    let db = tidemark_store::timeseries::TimeSeries::open(
        &cfg.general.state_dir.join("timeseries.db"),
        false,
    )
    .unwrap();
    let rows = db.query_fs("/fixture", 100, 101).unwrap();
    assert_eq!(rows.iter().map(|r| r.used).collect::<Vec<_>>(), vec![1, 4]);
    assert_eq!(db.query_lvm("vg", "lv", 100, 100).unwrap().len(), 1);
}

#[test]
fn now_uses_watcher_directories_without_exact_alert_windows() {
    let mut config = Config::default();
    config.sampler.mounts = vec!["/fixture".into(), "/unwatched".into()];
    let at = tidemark_core::time::unix_now();
    let state = LiveState::new(&config, at);
    publish(&state, Publication::Fs(sample(at, 100)));
    let mut other = sample(at, 100);
    other.mount = "/unwatched".into();
    publish(&state, Publication::Fs(other));
    let events = [("a.service", 2), ("b.service", -9)].map(|(unit, delta)| {
        (
            at,
            [
                b"/fixture/hot".to_vec(),
                unit.as_bytes().to_vec(),
                b"writer".to_vec(),
            ],
            delta,
            Some(unit.to_owned()),
            (1, 1),
        )
    });
    let watcher = tidemark_live::watch::Watcher::from_injected_events(
        RawPath::new(b"/fixture".to_vec()).unwrap(),
        events,
    )
    .unwrap();
    publish(
        &state,
        Publication::Watcher(("/fixture".into(), Arc::new(watcher))),
    );
    let report = state
        .view_at(at)
        .now(&NowParams { mount: None }, at)
        .unwrap();
    let row = &report
        .mounts
        .iter()
        .find(|m| m.mount == "/fixture")
        .unwrap()
        .hot_dirs_5m[0];
    assert_eq!(row.key, b"/fixture/hot");
    assert_eq!(row.top_unit.as_deref(), Some("b.service"));
    assert_eq!(row.bytes_delta, -7);
    assert!(!report
        .degraded
        .iter()
        .any(|d| d == "hot_dirs_5m: not measured" || d == "hot_dirs_5m /fixture: not measured"));
    assert!(report
        .degraded
        .iter()
        .any(|d| d == "hot_dirs_5m /unwatched: not measured"));
    assert!(state
        .view_at(at)
        .hot_dirs("/fixture", Duration::from_secs(300))
        .is_none());
    state.remove_watcher("/fixture");
    let watcher = tidemark_live::watch::Watcher::from_injected_events(
        RawPath::new(b"/fixture".to_vec()).unwrap(),
        [],
    )
    .unwrap();
    publish(
        &state,
        Publication::Watcher(("/fixture".into(), Arc::new(watcher))),
    );
    let report = state
        .view_at(at)
        .now(
            &NowParams {
                mount: Some("/fixture".into()),
            },
            at,
        )
        .unwrap();
    assert!(report.mounts[0].hot_dirs_5m.is_empty());
    assert!(!report.degraded.iter().any(|d| d.starts_with("hot_dirs_5m")));
}

#[test]
fn missing_configured_mount_has_no_sample_or_persistence_and_unhealthy_firstboot() {
    let (dir, _, mut config) = fixture();
    config.sampler.mounts = vec![dir.path().join("mount")];
    let state = LiveState::new(&config, 100);
    state
        .publish(
            Publication::Deployment {
                config_sha256: "test".into(),
                started_at: 100,
            },
            health(),
        )
        .unwrap();
    for name in ["scheduler", "api", "tswriter"] {
        state
            .publish_component(component(name, "ok", "fixture"))
            .unwrap();
    }
    let (tx, rx) = crossbeam_channel::bounded(4);
    let stop = Shutdown::default();
    let worker = sampler::start(Arc::new(config), state.clone(), tx, stop.clone()).unwrap();
    wait(|| {
        state
            .view_at(100)
            .status(100)
            .unwrap()
            .components
            .iter()
            .any(|c| {
                c.name == "sampler" && c.state == "degraded" && c.detail.contains("not mounted:")
            })
    });
    assert!(state.view_at(100).fs_samples().is_none());
    assert!(rx.try_recv().is_err());
    assert!(
        !tidemarkd::firstboot::build(&state.view_at(100).firstboot_inputs().unwrap(), 100).healthy
    );
    stop.store(true, Ordering::Release);
    worker.join().unwrap().unwrap();
}

#[test]
fn scheduler_consumer_validation_is_shared_by_check_startup_and_reload() {
    let (_dir, path, cfg) = fixture();
    let state = LiveState::new(&cfg, 0);
    let before = state.config();
    let bytes = std::fs::read_to_string(&path)
        .unwrap()
        .replace("schedule='off'", "schedule='every 10000000000000000000s'");
    std::fs::write(&path, bytes).unwrap();
    let invalid = Config::load(&path).unwrap();
    assert!(invalid.validate().is_empty());
    let mut output = Vec::new();
    assert_eq!(
        tidemarkd::run_args(
            &tidemarkd::Args {
                config: path.clone(),
                check_config: true
            },
            &mut output
        ),
        2
    );
    assert!(String::from_utf8_lossy(&output).contains("roots[0].schedule"));
    let result = tidemarkd::run_controlled(
        invalid,
        tidemarkd::RunOptions {
            config_path: path.clone(),
            now: 0,
        },
        state.clone(),
        Shutdown::default(),
        Shutdown::default(),
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("roots[0].schedule"));
    let result = tidemarkd::reload_callback(path, state.clone())().unwrap();
    assert!(!result.reloaded);
    assert!(result.warnings[0].contains("roots[0].schedule"));
    assert_eq!(state.config(), before);
    assert!(state
        .view()
        .status(0)
        .unwrap()
        .components
        .iter()
        .any(|c| c.name == "config" && c.state == "error"));
}

#[test]
fn reload_inventory_failure_is_contained_and_active_removed_root_is_retired() {
    let (_dir, path, cfg) = fixture();
    let state = LiveState::new(&cfg, 0);
    let before = state.config();
    // An unreadable catalog must reject before the configuration changes.
    let result = tidemarkd::reload_callback(path, state.clone())().unwrap();
    assert!(!result.reloaded);
    assert!(!result.warnings.is_empty());
    assert_eq!(state.config(), before);
    let root = cfg.roots[0].path.to_string_lossy().into_owned();
    state
        .publish(
            Publication::Scan(ScanStatus {
                root: root.clone(),
                running: Some(ScanProgress {
                    scan_id: "active".into(),
                    phase: "scan".into(),
                    dirs: 0,
                    entries: 0,
                    started_at: 0,
                    rate: 0.0,
                }),
                last_complete: None,
                next_due: None,
            }),
            health(),
        )
        .unwrap();
    let mut next = cfg.clone();
    next.roots = (0..128)
        .map(|n| {
            let mut r = cfg.roots[0].clone();
            r.path = cfg.roots[0].path.join(format!("new-{n}"));
            r
        })
        .collect();
    assert!(state
        .replace_config(next)
        .unwrap_err()
        .to_string()
        .contains("capacity"));
    assert_eq!(state.config(), before);
    let mut next = cfg;
    next.roots.clear();
    state.replace_config(next).unwrap();
    assert_eq!(state.view().scans().unwrap().len(), 1);
    state
        .publish(
            Publication::Scan(ScanStatus {
                root,
                running: None,
                last_complete: None,
                next_due: None,
            }),
            health(),
        )
        .unwrap();
    assert!(state.view().scans().unwrap().is_empty());
}

#[test]
fn oversized_metrics_interval_is_rejected_before_startup_and_reload() {
    let (_dir, path, cfg) = fixture();
    let state = LiveState::new(&cfg, 0);
    let before = state.config();
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("[metrics]", "[metrics]\ninterval='18446744073709551615s'");
    std::fs::write(&path, text).unwrap();
    let invalid = Config::load(&path).unwrap();
    assert!(invalid.validate().is_empty());
    let mut output = Vec::new();
    assert_eq!(
        tidemarkd::run_args(
            &tidemarkd::Args {
                config: path.clone(),
                check_config: true
            },
            &mut output
        ),
        2
    );
    assert!(String::from_utf8_lossy(&output)
        .contains("metrics.interval: interval must not exceed 366 days"));
    assert!(tidemarkd::run_controlled(
        invalid,
        tidemarkd::RunOptions {
            config_path: path.clone(),
            now: 0
        },
        state.clone(),
        Shutdown::default(),
        Shutdown::default()
    )
    .unwrap_err()
    .to_string()
    .contains("366 days"));
    let result = tidemarkd::reload_callback(path, state.clone())().unwrap();
    assert!(!result.reloaded);
    assert!(result.warnings[0].contains("366 days"));
    assert_eq!(state.config(), before);
}

#[test]
fn configured_roots_reserve_capacity_against_inactive_manual_history() {
    let cfg = Config::default();
    let state = LiveState::new(&cfg, 0);
    for n in 0..128 {
        state
            .publish(
                Publication::Scan(ScanStatus {
                    root: format!("/manual/{n}"),
                    running: None,
                    last_complete: None,
                    next_due: None,
                }),
                health(),
            )
            .unwrap();
    }
    let mut next = cfg;
    next.roots = (0..128)
        .map(|n| tidemark_core::config::RootConfig {
            path: format!("/configured/{n}").into(),
            schedule: "off".into(),
            ..tidemark_core::config::RootConfig::default()
        })
        .collect();
    state.replace_config(next).unwrap();
    let scans = state.view().scans().unwrap();
    assert_eq!(scans.len(), 128);
    assert!(scans.iter().all(|s| s.root.starts_with("/configured/")));
}

#[test]
fn late_sampler_pass_cannot_resurrect_removed_mount() {
    let mut config = Config::default();
    config.sampler.mounts = vec!["/fixture".into()];
    let state = LiveState::new(&config, 0);
    let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
    let (resume_tx, resume_rx) = crossbeam_channel::bounded(1);
    let worker_state = state.clone();
    let worker = std::thread::spawn(move || {
        let (_, generation) = worker_state.config();
        ready_tx.send(()).unwrap();
        resume_rx.recv().unwrap();
        worker_state
            .publish(
                Publication::FsAttempt {
                    generation,
                    mount: "/fixture".into(),
                    sample: Some(sample(10, 95)),
                },
                health(),
            )
            .unwrap();
        worker_state
            .publish(
                Publication::FsAttempt {
                    generation,
                    mount: "/fixture".into(),
                    sample: None,
                },
                health(),
            )
            .unwrap();
    });
    ready_rx.recv().unwrap();
    config.sampler.mounts = vec!["/new".into()];
    state.replace_config(config).unwrap();
    resume_tx.send(()).unwrap();
    worker.join().unwrap();
    assert!(state.view_at(10).fs_samples().is_none());
    // Even a current-generation pass may publish only current mounts.
    let (_, generation) = state.config();
    state
        .publish(
            Publication::FsAttempt {
                generation,
                mount: "/fixture".into(),
                sample: Some(sample(11, 95)),
            },
            health(),
        )
        .unwrap();
    assert!(state.view_at(11).fs_samples().is_none());
}

#[test]
fn internal_alert_has_reserved_publication_slot() {
    use tidemark_core::config::{AlertKind, AlertRule, AlertThreshold};
    use tidemark_live::alerts::{AlertDiagnostic, AlertEngine, AlertInputs, MAX_ALERT_SUBJECTS};
    let mut config = Config::default();
    config.alerts.rule = vec![AlertRule {
        name: "growth".into(),
        kind: AlertKind::DirGrowth,
        root: Some("/fixture".into()),
        depth: Some(2),
        above: Some(AlertThreshold::Number(1.)),
        for_: Duration::ZERO,
        ..Default::default()
    }];
    let state = LiveState::new(&config, 0);
    let rows = (0..MAX_ALERT_SUBJECTS)
        .map(|i| {
            (
                RawPath::new(format!("/fixture/{i}").into_bytes()).unwrap(),
                10,
            )
        })
        .collect();
    publish(&state, Publication::DirGrowth(("/fixture".into(), 2, rows)));
    let mut engine = AlertEngine::new(&config.alerts.rule, config.alerts.renotify).unwrap();
    let events = engine.evaluate(
        0,
        &AlertInputs {
            source: &state.view_at(0),
            state_space_refused: true,
        },
    );
    assert_eq!(events.len(), MAX_ALERT_SUBJECTS + 1);
    publish(
        &state,
        Publication::Alerts((engine.active(), engine.diagnostics().to_vec())),
    );
    assert_eq!(
        state.view_at(0).active_alerts().unwrap().len(),
        MAX_ALERT_SUBJECTS + 1
    );
    let diagnostics = vec![
        AlertDiagnostic {
            rule: "missing".into(),
            reason: "unavailable".into()
        };
        MAX_ALERT_SUBJECTS + 1
    ];
    assert!(state
        .publish(Publication::Alerts((vec![], diagnostics)), health())
        .is_err());
    let mut extra = engine.active();
    extra.push(extra.iter().find(|a| a.rule == "growth").unwrap().clone());
    assert!(state
        .publish(Publication::Alerts((extra, vec![])), health())
        .is_err());
}

#[test]
fn process_publication_is_independent_of_collector_and_clears_on_invalidation() {
    use tidemark_live::procscan::{ProcScanner, ProcScannerOptions};
    let mut config = Config::default();
    config.watch.mounts = vec!["/fixture".into()];
    let state = LiveState::new(&config, 0);
    let mut scanner = ProcScanner::new(
        ProcScannerOptions {
            mounts: vec![(RawPath::new(b"/fixture".to_vec()).unwrap(), u64::MAX)],
            open_writers: true,
            exclude_prefixes: vec![],
        },
        Arc::new(tidemark_fs::attribution::AttributionCache::new(
            Duration::ZERO,
            0,
        )),
    );
    scanner.scan_once(1).unwrap();
    let report = Arc::new(scanner.report().unwrap());
    publish(&state, Publication::Procscan(report.clone()));
    let scanner = Arc::new(parking_lot::Mutex::new(scanner));
    let guard = scanner.lock();
    let (tx, rx) = crossbeam_channel::bounded(1);
    let reader_state = state.clone();
    let reader = std::thread::spawn(move || {
        tx.send((
            reader_state
                .view_at(1)
                .phantom(&PhantomParams { mount: None }),
            reader_state.view_at(1).writers(&WritersParams {
                mount: None,
                window: Duration::from_millis(1500),
            }),
        ))
        .unwrap();
    });
    let read = rx.recv_timeout(Duration::from_millis(50));
    drop(guard);
    reader.join().unwrap();
    let (phantom, writers) = read.expect("reader must not lock the mutable collector");
    assert_eq!(phantom.unwrap()[0].total_bytes, 0);
    assert!(writers.unwrap().is_empty());
    state
        .publish(
            Publication::ProcscanAttempt {
                generation: 0,
                report: None,
            },
            component("procscan", "error", "failed pass"),
        )
        .unwrap();
    assert!(state
        .view_at(2)
        .phantom(&PhantomParams { mount: None })
        .is_none());
    assert!(state
        .view_at(2)
        .writers(&WritersParams {
            mount: None,
            window: Duration::from_secs(300)
        })
        .is_none());
    publish(&state, Publication::Procscan(report.clone()));
    config.procscan.open_writers = !config.procscan.open_writers;
    state.replace_config(config.clone()).unwrap();
    assert!(state
        .view_at(2)
        .phantom(&PhantomParams { mount: None })
        .is_none());
    state
        .publish(
            Publication::ProcscanAttempt {
                generation: 0,
                report: Some(report.clone()),
            },
            health(),
        )
        .unwrap();
    assert!(state
        .view_at(2)
        .phantom(&PhantomParams { mount: None })
        .is_none());
    state
        .publish(
            Publication::ProcscanAttempt {
                generation: 1,
                report: Some(report),
            },
            health(),
        )
        .unwrap();
    config.procscan.enabled = false;
    state.replace_config(config).unwrap();
    let now = state.view_at(2).now(&NowParams { mount: None }, 2).unwrap();
    assert!(now.degraded.iter().any(|s| s == "phantom: not measured"));
}

#[test]
fn check_config_rejects_removed_http_keys() {
    let (_dir, path, _cfg) = fixture();
    for (table, key) in [("metrics", "listen"), ("alerts", "webhook")] {
        std::fs::write(&path, format!("[{table}]\n{key} = \"\"\n")).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_tidemarkd"))
            .arg("--config")
            .arg(&path)
            .arg("--check-config")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(message.contains(&format!("{table}.{key}")), "{message}");
        assert!(message.contains("removed in v1 (ADR-020)"), "{message}");
    }
}

#[test]
fn mount_presence_recovery_remount_and_rate_history() {
    let mut config = Config::default();
    config.sampler.mounts = vec!["/fixture".into()];
    config.watch.mounts.clear();
    config.sampler.interval = Duration::from_secs(600);
    let state = LiveState::new(&config, 0);
    let table = |id| {
        tidemarkd::mounts::parse(
            format!("{id} 0 8:1 / /fixture rw - xfs /dev/test rw\n").as_bytes(),
        )
        .unwrap()
    };
    state.publish_mount_presence(0, &[]).unwrap();
    let report = state.view_at(0).now(&NowParams { mount: None }, 0).unwrap();
    assert!(report.mounts.is_empty());
    assert!(report
        .degraded
        .contains(&"mount not present: /fixture".into()));
    state.publish_mount_presence(0, &table(2)).unwrap();
    for ts in [0, 100, 299] {
        publish(&state, Publication::Fs(sample(ts, ts as u64)));
    }
    let report = state
        .view_at(299)
        .now(&NowParams { mount: None }, 299)
        .unwrap();
    assert_eq!(report.mounts[0].rate_15m, Some(0.0));
    assert_eq!(report.mounts[0].rate_1h, Some(0.0));
    assert!(report.mounts[0].ttf_secs.is_none());
    assert!(report
        .degraded
        .contains(&"rate: collecting /fixture".into()));
    assert!(!report
        .degraded
        .contains(&"mount not present: /fixture".into()));
    publish(&state, Publication::Fs(sample(300, 300)));
    assert_eq!(
        state
            .view_at(300)
            .fs_rate("/fixture", Duration::from_secs(900)),
        Some(1.0)
    );
    state.publish_mount_presence(0, &table(3)).unwrap();
    assert!(state.view_at(300).fs_samples().is_none());
    publish(&state, Publication::Fs(sample(301, 900)));
    assert!(state
        .view_at(301)
        .fs_rate("/fixture", Duration::from_secs(900))
        .is_none());
    state.publish_mount_presence(0, &[]).unwrap();
    assert!(state.view_at(301).fs_samples().is_none());
    config.sampler.mounts.clear();
    state.replace_config(config).unwrap();
    assert!(!state
        .view_at(301)
        .now(&NowParams { mount: None }, 301)
        .unwrap()
        .degraded
        .iter()
        .any(|d| d.starts_with("mount not present:")));
    // Obsolete collector passes cannot reintroduce removed subjects.
    state.publish_mount_presence(0, &table(2)).unwrap();
    assert!(!state
        .view_at(301)
        .now(&NowParams { mount: None }, 301)
        .unwrap()
        .degraded
        .iter()
        .any(|d| d.starts_with("mount not present:")));
}
