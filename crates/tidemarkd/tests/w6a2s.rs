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
    let dir = tempfile::tempdir().unwrap();
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
    state.replace_config(cfg).unwrap();
    wait(|| state.view_at(0).phantom(&PhantomParams { mount: None }) == Some(vec![]));
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
                ts: 0,
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
