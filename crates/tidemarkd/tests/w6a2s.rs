//! Owned by: W6-A2b
//! Service reload, disabled health and prompt shutdown regressions.

#![allow(clippy::unwrap_used)]

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tidemark_core::{api::PhantomParams, config::Config};
use tidemarkd::{
    services::{alerts, procscan, tswriter, watcher},
    state::LiveState,
};

fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "state deadline elapsed");
        std::thread::yield_now();
    }
}
fn config() -> (tempfile::TempDir, Config) {
    let dir = ::test_support::private_tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.general.state_dir = dir.path().join("state");
    cfg.general.socket = dir.path().join("socket");
    cfg.watch.enabled = false;
    cfg.watch.mounts.clear();
    cfg.procscan.enabled = false;
    cfg.procscan.interval = Duration::from_secs(3600);
    (dir, cfg)
}
#[test]
fn disabled_services_reload_and_stop() {
    let (_dir, mut cfg) = config();
    let state = LiveState::new(&cfg, 0);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = crossbeam_channel::bounded::<tswriter::Message>(64);
    let watcher = watcher::start(
        Arc::new(cfg.clone()),
        state.clone(),
        tx.clone(),
        stop.clone(),
    )
    .unwrap();
    let procscan = procscan::start(
        Arc::new(cfg.clone()),
        state.clone(),
        tx.clone(),
        stop.clone(),
    )
    .unwrap();
    let alerts = alerts::start(Arc::new(cfg.clone()), state.clone(), tx, stop.clone()).unwrap();
    wait(|| state.view_at(0).active_alerts().is_some());
    cfg.procscan.enabled = true;
    state.replace_config(cfg.clone()).unwrap();
    wait(|| state.view_at(0).phantom(&PhantomParams { mount: None }) == Some(vec![]));
    cfg.watch.mounts = vec!["/tidemark-nonexistent-fixture".into()];
    state.replace_config(cfg.clone()).unwrap();
    wait(|| {
        state
            .view_at(0)
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "procscan" && c.state == "error")
    });
    assert!(state
        .view_at(0)
        .phantom(&PhantomParams { mount: None })
        .is_none());
    cfg.watch.mounts.clear();
    cfg.procscan.enabled = false;
    state.replace_config(cfg).unwrap();
    wait(|| {
        state
            .view_at(0)
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "procscan" && c.state == "disabled")
    });
    assert!(state
        .view_at(0)
        .phantom(&PhantomParams { mount: None })
        .is_none());
    stop.store(true, Ordering::Release);
    wait(|| watcher.is_finished() && procscan.is_finished() && alerts.is_finished());
    watcher.join().unwrap().unwrap();
    procscan.join().unwrap().unwrap();
    alerts.join().unwrap().unwrap();
    assert!(state.view_at(0).watch_stats().is_none_or(|s| s.is_empty()));
}

#[test]
fn procscan_bad_mount_reports_error_without_measurement() {
    let (dir, mut cfg) = config();
    cfg.procscan.enabled = true;
    cfg.watch.mounts = vec![dir.path().join("not-a-mount")];
    let state = LiveState::new(&cfg, 0);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = crossbeam_channel::bounded(1);
    let worker = procscan::start(Arc::new(cfg), state.clone(), tx, stop.clone()).unwrap();
    wait(|| {
        state
            .view_at(0)
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "procscan" && c.state == "error")
    });
    assert!(state
        .view_at(0)
        .phantom(&PhantomParams { mount: None })
        .is_none());
    stop.store(true, Ordering::Release);
    wait(|| worker.is_finished());
    worker.join().unwrap().unwrap();
}

#[test]
fn alert_threshold_reload_applies_without_restarting_service() {
    use tidemark_core::{
        config::{AlertRule, AlertThreshold},
        live::{AlertState, FsSample},
    };
    use tidemarkd::state::Publication;
    let (_dir, mut cfg) = config();
    cfg.alerts.rule = vec![AlertRule {
        name: "usage".into(),
        mount: Some("/data".into()),
        above: Some(AlertThreshold::Number(90.)),
        for_: Duration::ZERO,
        ..Default::default()
    }];
    let state = LiveState::new(&cfg, 0);
    state
        .publish(
            Publication::Fs(FsSample {
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
                mount: "/data".into(),
                size: 100,
                used: 80,
                avail: 20,
                inodes: 1,
                inodes_used: 0,
            }),
            tidemarkd::component("sampler", "ok", "fixture"),
        )
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = crossbeam_channel::bounded(64);
    let worker = alerts::start(Arc::new(cfg.clone()), state.clone(), tx, stop.clone()).unwrap();
    wait(|| state.view_at(0).active_alerts().is_some());
    assert!(state.view_at(0).active_alerts().unwrap().is_empty());
    cfg.alerts.rule[0].above = Some(AlertThreshold::Number(50.));
    state.replace_config(cfg).unwrap();
    wait(|| {
        state
            .view_at(0)
            .active_alerts()
            .is_some_and(|a| a.iter().any(|a| a.rule == "usage"))
    });
    match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
        tswriter::Message::Alert(event) => {
            assert_eq!(event.rule, "usage");
            assert_eq!(event.state, AlertState::Firing);
        }
        _ => panic!("unexpected persistence message"),
    }
    stop.store(true, Ordering::Release);
    wait(|| worker.is_finished());
    worker.join().unwrap().unwrap();
}

#[cfg(feature = "privileged")]
#[test]
fn watcher_final_drop_joins_worker_on_vm() {
    if std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref() != Ok("1") {
        return;
    }
    fn threads() -> usize {
        std::fs::read_dir("/proc/self/task")
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                std::fs::read_to_string(e.path().join("comm"))
                    .is_ok_and(|s| s.trim() == "tidemark-watch")
            })
            .count()
    }
    let (_dir, mut cfg) = config();
    cfg.watch.enabled = true;
    cfg.watch.mounts = vec!["/".into()];
    let state = LiveState::new(&cfg, 0);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = crossbeam_channel::bounded(64);
    let before = threads();
    let worker = watcher::start(Arc::new(cfg.clone()), state.clone(), tx, stop.clone()).unwrap();
    wait(|| state.view_at(0).watch_stats().is_some_and(|s| s.len() == 1));
    assert_eq!(threads(), before + 1);
    cfg.watch.enabled = false;
    state.replace_config(cfg.clone()).unwrap();
    wait(|| state.view_at(0).watch_stats().is_none_or(|s| s.is_empty()));
    wait(|| threads() == before);
    cfg.watch.enabled = true;
    state.replace_config(cfg).unwrap();
    wait(|| state.view_at(0).watch_stats().is_some_and(|s| s.len() == 1));
    wait(|| threads() == before + 1);
    stop.store(true, Ordering::Release);
    wait(|| worker.is_finished());
    worker.join().unwrap().unwrap();
    wait(|| threads() == before);
    assert!(state.view_at(0).watch_stats().is_none_or(|s| s.is_empty()));
}

#[test]
fn missing_watch_mount_is_degraded_and_has_no_watcher() {
    let (dir, mut cfg) = config();
    cfg.watch.enabled = true;
    cfg.watch.mounts = vec![dir.path().to_path_buf()];
    let name = format!("watcher:{}", dir.path().display());
    let state = LiveState::new(&cfg, 100);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = crossbeam_channel::bounded(1);
    let worker = watcher::start(Arc::new(cfg), state.clone(), tx, stop.clone()).unwrap();
    wait(|| {
        state
            .view_at(100)
            .status(100)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == name && c.state == "degraded" && c.detail == "not mounted")
    });
    assert!(state
        .view_at(100)
        .watch_stats()
        .is_none_or(|s| s.is_empty()));
    stop.store(true, Ordering::Release);
    worker.join().unwrap().unwrap();
}

#[cfg(feature = "privileged")]
#[test]
fn watcher_follows_mount_arrival_detach_and_remount_on_vm() {
    use std::{path::PathBuf, process::Command};
    assert_eq!(
        std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(),
        Ok("1"),
        "disposable VM only"
    );
    assert!(rustix::process::geteuid().is_root());
    struct Mounted(PathBuf);
    impl Drop for Mounted {
        fn drop(&mut self) {
            let _ = Command::new("umount").arg("-l").arg(&self.0).status();
        }
    }
    struct Worker {
        stop: Arc<AtomicBool>,
        handle: Option<tidemarkd::ServiceHandle>,
    }
    impl Drop for Worker {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let (dir, mut cfg) = config();
    let target = dir.path().join("late-mount");
    std::fs::create_dir(&target).unwrap();
    let mounted = Mounted(target.clone());
    cfg.watch.enabled = true;
    cfg.watch.mounts = vec![target.clone()];
    let state = LiveState::new(&cfg, 100);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = crossbeam_channel::bounded(64);
    let _worker = Worker {
        stop: stop.clone(),
        handle: Some(watcher::start(Arc::new(cfg), state.clone(), tx, stop).unwrap()),
    };
    let name = format!("watcher:{}", target.display());
    let absent = || {
        state
            .view_at(100)
            .status(100)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == name && c.state == "degraded" && c.detail == "not mounted")
    };
    wait(absent);
    for n in 0..2 {
        assert!(Command::new("mount")
            .arg("--bind")
            .arg(&fs.mountpoint)
            .arg(&target)
            .status()
            .unwrap()
            .success());
        wait(|| {
            state
                .view_at(100)
                .watch_stats()
                .is_some_and(|s| s.len() == 1)
        });
        assert!(Command::new("sh")
            .arg("-c")
            .arg("printf event > \"$1\"")
            .arg("writer")
            .arg(target.join(format!("event-{n}")))
            .status()
            .unwrap()
            .success());
        wait(|| {
            state
                .view_at(100)
                .watch_stats()
                .is_some_and(|s| s.iter().any(|(_, stats)| stats.events_total > 0))
        });
        assert!(Command::new("umount")
            .arg("-l")
            .arg(&mounted.0)
            .status()
            .unwrap()
            .success());
        wait(absent);
        assert!(state
            .view_at(100)
            .watch_stats()
            .is_none_or(|s| s.is_empty()));
    }
}

#[test]
fn lvm_publication_refreshes_alert_diagnostics_before_periodic_tick() {
    use tidemark_core::{
        api::LvmResult,
        config::{AlertKind, AlertRule},
        live::LvmNow,
    };
    use tidemarkd::state::Publication;
    let (_dir, mut cfg) = config();
    cfg.alerts.rule = vec![AlertRule {
        name: "thin".into(),
        kind: AlertKind::ThinPoolPct,
        data_above: Some(80.),
        meta_above: Some(80.),
        for_: Duration::ZERO,
        ..Default::default()
    }];
    let now = tidemark_core::time::unix_now();
    let state = LiveState::new(&cfg, now);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = crossbeam_channel::bounded(64);
    let worker = alerts::start(Arc::new(cfg), state.clone(), tx, stop.clone()).unwrap();
    wait(|| {
        state
            .view_at(now)
            .alert_diagnostics()
            .is_some_and(|d| d.iter().any(|d| d.reason == "LVM inventory unavailable"))
    });
    state
        .publish(
            Publication::Lvm(LvmResult {
                mounts: vec![(
                    "/data".into(),
                    LvmNow {
                        vg: "vg".into(),
                        lv: "thin".into(),
                        lv_size: 100,
                        vg_free: 10,
                        thin_pool: Some("pool".into()),
                        data_pct: Some(0.78),
                        meta_pct: Some(0.1),
                        sampled_at: now,
                    },
                )],
                vgs: vec![],
                sampled_at: now,
            }),
            tidemarkd::component("lvm", "ok", "fixture"),
        )
        .unwrap();
    // The common wait has a ten-second deadline, shorter than the 30-second tick.
    wait(|| {
        state
            .view_at(now)
            .alert_diagnostics()
            .is_some_and(|d| d.is_empty())
    });
    assert_eq!(
        state
            .view_at(now)
            .status(now)
            .unwrap()
            .components
            .iter()
            .find(|c| c.name == "alerts")
            .unwrap()
            .state,
        "ok"
    );
    stop.store(true, Ordering::Release);
    worker.join().unwrap().unwrap();
}

#[test]
fn retired_watcher_components_and_errors_release_capacity() {
    use tidemarkd::{component, state::MAX_SUBJECTS};
    let (_dir, mut cfg) = config();
    let state = LiveState::new(&cfg, 0);
    for i in 0..MAX_SUBJECTS * 2 {
        let name = format!("watcher:/fixture-{i}");
        state
            .publish_component(component(&name, "error", "fixture failure"))
            .unwrap();
        state.remove_component(&name);
    }
    assert!(state.view_at(0).status(0).unwrap().components.is_empty());
    assert!(state.view_at(0).status(0).unwrap().last_errors.is_empty());
    for i in 0..MAX_SUBJECTS * 2 {
        cfg.watch.mounts = vec![format!("/fixture-{i}").into()];
        state.replace_config(cfg.clone()).unwrap();
        state
            .publish_component(component(
                &format!("watcher:/fixture-{i}"),
                "error",
                "startup failed",
            ))
            .unwrap();
    }
    let status = state.view_at(0).status(0).unwrap();
    let current = format!("watcher:/fixture-{}", MAX_SUBJECTS * 2 - 1);
    let watchers: Vec<_> = status
        .components
        .iter()
        .filter(|c| c.name.starts_with("watcher:"))
        .collect();
    assert_eq!(watchers.len(), 1);
    assert_eq!(watchers[0].name, current);
    for component in status
        .components
        .iter()
        .filter(|c| !c.name.starts_with("watcher:"))
    {
        assert_eq!(component.name, "sampler");
        assert_eq!(component.state, "degraded");
        assert_eq!(
            component.detail,
            "waiting for a complete pass under the new configuration"
        );
    }
    assert!(status.last_errors.len() <= MAX_SUBJECTS);
    let watcher_errors: Vec<_> = status
        .last_errors
        .iter()
        .filter(|(name, _, _)| name.starts_with("watcher:"))
        .collect();
    assert_eq!(watcher_errors.len(), 1);
    assert_eq!(watcher_errors[0].0, current);
    for (name, _, detail) in status
        .last_errors
        .iter()
        .filter(|(name, _, _)| !name.starts_with("watcher:"))
    {
        assert_eq!(name, "sampler");
        assert_eq!(
            detail,
            "waiting for a complete pass under the new configuration"
        );
    }
}

#[cfg(feature = "privileged")]
#[test]
fn hot_growth_exact_minute_fires_on_loop_xfs() {
    use tidemark_core::config::{AlertKind, AlertRule, AlertThreshold};
    assert_eq!(
        std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(),
        Ok("1"),
        "disposable VM only"
    );
    assert!(rustix::process::geteuid().is_root());
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let (_dir, mut cfg) = config();
    cfg.watch.enabled = true;
    cfg.watch.mounts = vec![fs.mountpoint.clone()];
    let directory = fs.mountpoint.join("growth");
    std::fs::create_dir(&directory).unwrap();
    cfg.alerts.rule = vec![AlertRule {
        name: "hot".into(),
        kind: AlertKind::HotDirGrowth,
        mount: Some(fs.mountpoint.clone()),
        path: Some(format!("{}/*", fs.mountpoint.display())),
        above: Some(AlertThreshold::Number(1.)),
        window: Some("1m".into()),
        for_: Duration::ZERO,
        resolve_for: Duration::ZERO,
        ..Default::default()
    }];
    let state = LiveState::new(&cfg, tidemark_core::time::unix_now());
    struct Workers {
        stop: Arc<AtomicBool>,
        handles: Vec<tidemarkd::ServiceHandle>,
    }
    impl Drop for Workers {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            for handle in self.handles.drain(..) {
                let _ = handle.join();
            }
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = crossbeam_channel::bounded(1024);
    let mut workers = Workers {
        stop: stop.clone(),
        handles: vec![],
    };
    workers.handles.push(
        watcher::start(
            Arc::new(cfg.clone()),
            state.clone(),
            tx.clone(),
            stop.clone(),
        )
        .unwrap(),
    );
    workers
        .handles
        .push(alerts::start(Arc::new(cfg), state.clone(), tx, stop).unwrap());
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let now = tidemark_core::time::unix_now();
        let view = state.view_at(now);
        if view.alert_diagnostics().is_some_and(|d| d.is_empty())
            && view
                .status(now)
                .unwrap()
                .components
                .iter()
                .any(|c| c.name == "alerts" && c.state == "ok")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "idle alert window never became evaluable: {:?}",
            view.status(now)
        );
        std::thread::park_timeout(Duration::from_millis(100));
    }
    assert!(state.view().active_alerts().unwrap().is_empty());
    assert!(std::process::Command::new("sh")
        .arg("-c")
        .arg("dd if=/dev/zero of=\"$1\" bs=1M count=1 status=none; sleep 5")
        .arg("writer")
        .arg(directory.join("new-file"))
        .status()
        .unwrap()
        .success());
    let deadline = Instant::now() + Duration::from_secs(175);
    loop {
        let view = state.view();
        if view
            .active_alerts()
            .is_some_and(|a| a.iter().any(|a| a.rule == "hot"))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "hot rule did not fire within three minutes: {:?}",
            view.status(tidemark_core::time::unix_now())
        );
        std::thread::park_timeout(Duration::from_millis(100));
    }
}

#[test]
fn alert_reload_keeps_unchanged_firing_rule_without_duplicate_event() {
    use tidemark_core::{
        config::{AlertRule, AlertThreshold},
        live::{AlertState, FsSample},
    };
    use tidemarkd::state::Publication;
    let (_dir, mut cfg) = config();
    cfg.alerts.rule = ["keep", "change"]
        .into_iter()
        .map(|name| AlertRule {
            name: name.into(),
            mount: Some("/data".into()),
            above: Some(AlertThreshold::Number(50.)),
            for_: Duration::ZERO,
            ..Default::default()
        })
        .collect();
    let now = tidemark_core::time::unix_now();
    let state = LiveState::new(&cfg, now);
    state
        .publish(
            Publication::Fs(FsSample {
                ts: now,
                mount: "/data".into(),
                size: 100,
                used: 80,
                avail: 20,
                inodes: 1,
                inodes_used: 0,
            }),
            tidemarkd::component("sampler", "ok", "fixture"),
        )
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = crossbeam_channel::bounded(64);
    let worker = alerts::start(Arc::new(cfg.clone()), state.clone(), tx, stop.clone()).unwrap();
    wait(|| {
        state
            .view_at(now)
            .active_alerts()
            .is_some_and(|a| a.len() == 2)
    });
    let initial = state
        .view_at(now)
        .active_alerts()
        .unwrap()
        .into_iter()
        .find(|a| a.rule == "keep")
        .unwrap();
    for _ in 0..2 {
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            tswriter::Message::Alert(_)
        ));
    }
    cfg.alerts.rule[1].above = Some(AlertThreshold::Number(95.));
    cfg.alerts.rule.reverse();
    state.replace_config(cfg).unwrap();
    wait(|| {
        state
            .view_at(now)
            .active_alerts()
            .is_some_and(|a| a.len() == 1)
    });
    assert_eq!(state.view_at(now).active_alerts().unwrap()[0], initial);
    assert!(!rx.try_iter().any(|m| matches!(m, tswriter::Message::Alert(e) if e.rule == "keep" && e.state == AlertState::Firing)));
    stop.store(true, Ordering::Release);
    worker.join().unwrap().unwrap();
}

#[test]
fn retired_watcher_transport_stops_while_an_old_state_view_is_held() {
    use tidemark_core::path::RawPath;
    use tidemark_live::watch::Watcher;
    use tidemarkd::state::Publication;
    let (_dir, cfg) = config();
    let state = LiveState::new(&cfg, 0);
    let descriptor = rustix::fs::open(
        "/",
        rustix::fs::OFlags::PATH | rustix::fs::OFlags::DIRECTORY,
        rustix::fs::Mode::empty(),
    )
    .unwrap();
    let steps = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured = steps.clone();
    let watched = Arc::new(
        Watcher::from_injected_events_with_shutdown_probe(
            RawPath::new(b"/fixture".to_vec()).unwrap(),
            descriptor,
            move |step| captured.lock().push(step),
        )
        .unwrap(),
    );
    state
        .publish(
            Publication::Watcher(("/fixture".into(), watched.clone())),
            tidemarkd::component("watcher:/fixture", "ok", "fixture"),
        )
        .unwrap();
    let old_view = state.view_at(0);
    let weak = Arc::downgrade(&watched);
    watched.shutdown();
    state.remove_watcher("/fixture");
    assert_eq!(*steps.lock(), vec!["worker stopped", "group closed"]);
    assert_eq!(
        watched.mount_identity().unwrap_err().kind(),
        std::io::ErrorKind::Unsupported
    );
    drop(watched);
    assert!(weak.upgrade().is_some());
    assert!(old_view.status(0).is_some());
    drop(old_view);
    assert!(weak.upgrade().is_none());
}

#[cfg(feature = "privileged")]
#[test]
fn daemon_sigterm_releases_loop_xfs_for_ordinary_unmount() {
    use std::process::{Child, Command, Stdio};
    if !rustix::process::geteuid().is_root()
        || std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref() != Ok("1")
    {
        eprintln!("SKIP daemon_sigterm_releases_loop_xfs: requires root and TIDEMARK_DEPLOY_TEST_VM=1 in a disposable guest");
        return;
    }
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let (dir, _) = config();
    let path = dir.path().join("daemon.toml");
    let log = dir.path().join("daemon.log");
    std::fs::write(&path, format!(
        "[general]\nstate_dir='{}'\nsocket='{}'\n[sampler]\nmounts=['{}']\ninterval='1h'\n[lvm]\nenabled=false\n[watch]\nenabled=true\nmounts=['{}']\n[procscan]\nenabled=false\n[metrics]\ntextfile='{}'\n[[roots]]\npath='{}'\nschedule='off'\n",
        dir.path().join("state").display(), dir.path().join("api.sock").display(),
        fs.mountpoint.display(), fs.mountpoint.display(), dir.path().join("metrics.prom").display(),
        fs.mountpoint.display())).unwrap();
    let mut child = ChildGuard(
        Command::new(assert_cmd::cargo::cargo_bin!("tidemarkd"))
            .arg("--config")
            .arg(&path)
            .env("RUST_LOG", "info")
            .env_remove("NOTIFY_SOCKET")
            .env_remove("INVOCATION_ID")
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let mode = loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "daemon exited before watch started"
        );
        let text = std::fs::read_to_string(&log).unwrap();
        if text.contains("daemon ready") {
            if let Some(line) = text
                .lines()
                .find(|line| line.contains("watcher started: mount"))
            {
                break line.to_owned();
            }
        }
        assert!(Instant::now() < deadline, "watch startup deadline: {text}");
        std::thread::park_timeout(Duration::from_millis(50));
    };
    eprintln!("guest watcher mode: {mode}");
    // This is a separate process: its state view cannot be held by this harness.
    // The retained-view lifecycle is covered by the service-level regression above.
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.0.id() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(exit) = child.0.try_wait().unwrap() {
            assert!(exit.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "SIGTERM exit exceeded ten seconds"
        );
        std::thread::park_timeout(Duration::from_millis(20));
    }
    // Fixture cleanup uses ordinary umount, then detaches its owned loop device.
    // Bound completion from the parent so a busy mount cannot silently pass.
    let (tx, rx) = crossbeam_channel::bounded(1);
    let cleanup = std::thread::spawn(move || {
        let _ = tx.send(fs.cleanup());
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("ordinary unmount exceeded ten seconds")
        .expect("ordinary unmount failed after daemon SIGTERM");
    cleanup.join().unwrap();
}
