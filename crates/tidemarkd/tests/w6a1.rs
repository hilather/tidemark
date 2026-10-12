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
fn write_config(path: &Path, interval: &str) {
    let p = path.parent().unwrap();
    std::fs::write(path,format!("[general]\nstate_dir='{}'\nsocket='{}'\n[sampler]\nmounts=['{}']\ninterval='{}'\n[lvm]\nenabled=false\n[watch]\nenabled=false\nmounts=[]\n[procscan]\nenabled=false\n[metrics]\ntextfile='{}'\n[[roots]]\npath='{}'\nschedule='off'\n",p.join("state").display(),p.join("api.sock").display(),p.join("mount").display(),interval,p.join("metrics.prom").display(),p.join("mount").display())).unwrap();
}
fn fixture() -> (tempfile::TempDir, PathBuf, Config) {
    let dir = tempfile::tempdir().unwrap();
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
        Some(10.0)
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
            window: Duration::from_secs(90),
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
        v.hot_dirs("/fixture", Duration::from_secs(90)),
        Some(vec![])
    );
    assert_eq!(
        v.watch_overflows("/fixture", Duration::from_secs(90)),
        Some(0)
    );
    assert!(v.hot_dirs("/fixture", Duration::from_secs(60)).is_none());
    assert!(s
        .view_at(101)
        .watch_overflows("/fixture", Duration::from_secs(90))
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
            window: Duration::from_secs(90),
            measured_at: 100,
            hot: Some(vec![hot.clone()]),
            overflows: Some(7),
        },
    );
    assert_eq!(
        s.view_at(100).hot_dirs("/fixture", Duration::from_secs(90)),
        Some(vec![hot])
    );
    assert_eq!(
        s.view_at(100)
            .watch_overflows("/fixture", Duration::from_secs(90)),
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
        Publication::Procscan(Arc::new(parking_lot::Mutex::new(scanner))),
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
    assert!(s.view().fs_samples().is_some());
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
    let stop = Shutdown::default();
    let (tx, h) = tswriter::start_with_clock(
        Arc::new(c.clone()),
        LiveState::new(&c, 0),
        stop.clone(),
        clock,
    )
    .unwrap();
    tx.send(Message::Fs(vec![sample(1, 10)])).unwrap();
    flush(&tx);
    now.store(3599, Ordering::Release);
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
        Duration::from_secs(1)
    );
    assert_eq!(
        lvm::retry_delay(100, Duration::from_secs(4)),
        Duration::from_secs(4)
    );
}
#[test]
fn lifecycle_reload_and_shutdown() {
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
    wait(|| {
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
fn binary_ready_and_stopping() {
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
        Publication::Procscan(Arc::new(parking_lot::Mutex::new(scanner))),
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
    wait(|| {
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
    wait(|| {
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
