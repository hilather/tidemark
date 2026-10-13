//! Owned by: W6-watch-windows
//! Periodic alert evaluation, bounded persistence and delivery.

use crate::{
    state::{LiveState, Publication},
    DaemonError, ServiceHandle, Shutdown,
};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tidemark_core::config::Config;
use tidemark_live::alerts::{AlertEngine, AlertInputs, Deliver, JournalDeliver};

/// Start the thirty-second alert service; reload replaces compiled rules at a boundary.
pub fn start(
    config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
) -> Result<ServiceHandle, DaemonError> {
    crate::state::validate_bounds(&config)?;
    std::thread::Builder::new()
        .name("alerts".into())
        .spawn(move || {
            let mut previous = None;
            let mut engine: Option<AlertEngine> = None;
            let mut previous_failures = (std::collections::BTreeMap::new(), 0, 0);
            let mut sinks: Vec<Box<dyn Deliver>> = vec![Box::new(JournalDeliver)];
            while !shutdown.load(Ordering::Acquire) {
                let (config, generation) = state.config();
                if previous.as_ref() != Some(&config.alerts) {
                    if let Some(engine) = engine.as_mut() {
                        engine
                            .reconfigure(&config.alerts.rule, config.alerts.renotify)
                            .map_err(|e| DaemonError::Config(e.to_string()))?;
                    } else {
                        engine = Some(
                            AlertEngine::new(&config.alerts.rule, config.alerts.renotify)
                                .map_err(|e| DaemonError::Config(e.to_string()))?,
                        );
                    }
                    previous = Some(config.alerts.clone());
                }
                let revision = state.alert_revision();
                if let Some(engine) = engine.as_mut() {
                    tick_current(
                        engine,
                        crate::unix_now(),
                        &state,
                        &writer,
                        &mut sinks,
                        &mut previous_failures,
                    )?;
                }
                wait_inputs(&state, generation, revision, &shutdown);
            }
            state.publish_component(crate::component("alerts", "stopped", "shutdown"))?;
            Ok(())
        })
        .map_err(|source| DaemonError::Io {
            context: "spawn alerts".into(),
            source,
        })
}

pub(crate) type CollectorFailures = (std::collections::BTreeMap<String, u64>, u64, u64);

pub(crate) fn tick_current(
    engine: &mut AlertEngine,
    now: i64,
    state: &LiveState,
    writer: &super::tswriter::Sender,
    sinks: &mut [Box<dyn Deliver>],
    previous_failures: &mut CollectorFailures,
) -> Result<(), DaemonError> {
    let view = state.view_at(now);
    let (config, _) = view.config();
    let procscan_ok = view.status(now).is_some_and(|status| {
        status
            .components
            .iter()
            .any(|c| c.name == "procscan" && matches!(c.state.as_str(), "ok" | "degraded"))
    });
    let (fs_failures, lvm_failures) = view.collector_failures();
    let failures = (fs_failures, lvm_failures, view.procscan_failure_epoch());
    let interrupted: Vec<_> = failures
        .0
        .iter()
        .filter(|(mount, epoch)| previous_failures.0.get(*mount).copied().unwrap_or(0) != **epoch)
        .map(|(mount, _)| mount.clone())
        .collect();
    let mut events = Vec::new();
    if !interrupted.is_empty()
        || previous_failures.1 != failures.1
        || previous_failures.2 != failures.2
    {
        // Replay a missed unavailability observation before recovery. A
        // failed and recovered pass between alert ticks still breaks continuity.
        let source = EnabledSource {
            source: &view,
            config: &config,
            procscan_ok: procscan_ok && previous_failures.2 == failures.2,
            interrupted_fs: &interrupted,
            interrupted_lvm: previous_failures.1 != failures.1,
        };
        events.extend(engine.evaluate(
            now,
            &AlertInputs {
                source: &source,
                state_space_refused: view.state_space_refused().unwrap_or(false),
            },
        ));
    }
    *previous_failures = failures;
    let source = EnabledSource {
        source: &view,
        config: &config,
        procscan_ok,
        interrupted_fs: &[],
        interrupted_lvm: false,
    };
    events.extend(engine.evaluate(
        now,
        &AlertInputs {
            source: &source,
            state_space_refused: view.state_space_refused().unwrap_or(false),
        },
    ));
    tick_evaluated(engine, now, state, writer, sinks, events)?;
    Ok(())
}

#[cfg(test)]
fn tick(
    engine: &mut AlertEngine,
    now: i64,
    inputs: &AlertInputs<'_>,
    state: &LiveState,
    writer: &super::tswriter::Sender,
    sinks: &mut [Box<dyn Deliver>],
) -> Result<(), DaemonError> {
    let events = engine.evaluate(now, inputs);
    tick_evaluated(engine, now, state, writer, sinks, events)
}

fn tick_evaluated(
    engine: &AlertEngine,
    now: i64,
    state: &LiveState,
    writer: &super::tswriter::Sender,
    sinks: &mut [Box<dyn Deliver>],
    events: Vec<tidemark_core::live::AlertEvent>,
) -> Result<(), DaemonError> {
    let mut errors = Vec::new();
    let (config, _) = state.config();
    for event in events {
        for sink in sinks.iter_mut() {
            if let Err(e) = sink.deliver(&event) {
                errors.push(e.to_string());
            }
        }
        if writer
            .try_send(super::tswriter::Message::Alert(event))
            .is_err()
        {
            errors.push("writer queue full or disconnected; alert persistence dropped".into());
        }
    }
    let mut diagnostics = engine.diagnostics().to_vec();
    let view = state.view_at(now);
    decorate_diagnostics(&mut diagnostics, &config, &view);
    diagnostics.sort_by(|a, b| a.rule.cmp(&b.rule));
    let faults: Vec<_> = diagnostics
        .iter()
        .filter(|d| !d.reason.starts_with("warming up"))
        .collect();
    let health = if !errors.is_empty() {
        crate::component("alerts", "degraded", errors.join("; "))
    } else if !faults.is_empty() {
        crate::component(
            "alerts",
            "degraded",
            format!(
                "{} rules not evaluable: {}",
                faults.len(),
                faults
                    .iter()
                    .take(3)
                    .map(|d| d.rule.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    } else {
        crate::component("alerts", "ok", "rules evaluated")
    };
    state.publish(Publication::Alerts((engine.active(), diagnostics)), health)
}

fn wait_inputs(state: &LiveState, generation: u64, revision: u64, shutdown: &Shutdown) {
    let start = std::time::Instant::now();
    while !shutdown.load(Ordering::Acquire)
        && state.config().1 == generation
        && state.alert_revision() == revision
        && start.elapsed() < Duration::from_secs(30)
    {
        std::thread::park_timeout(Duration::from_millis(50));
    }
}
fn decorate_diagnostics(
    diagnostics: &mut [tidemark_live::alerts::AlertDiagnostic],
    config: &Config,
    view: &crate::state::LiveStateView,
) {
    for diagnostic in diagnostics {
        let Some(rule) = config
            .alerts
            .rule
            .iter()
            .find(|r| r.name == diagnostic.rule)
        else {
            continue;
        };
        if rule.kind == tidemark_core::config::AlertKind::TimeToFull
            && diagnostic.reason == "window rate unavailable or nonfinite"
            && rule.mount.as_ref().is_some_and(|mount| {
                rule.window
                    .as_deref()
                    .and_then(|w| humantime::parse_duration(w).ok())
                    .is_some_and(|window| view.rate_warming_up(&mount.to_string_lossy(), window))
            })
        {
            diagnostic.reason = "warming up: collecting filesystem rate samples".into();
        }
        if !matches!(
            diagnostic.reason.as_str(),
            "hot directory window unavailable" | "overflow window unavailable"
        ) {
            continue;
        }
        if !matches!(
            rule.kind,
            tidemark_core::config::AlertKind::HotDirGrowth
                | tidemark_core::config::AlertKind::WatchLossy
        ) {
            continue;
        }
        let window = rule
            .window
            .as_deref()
            .and_then(|w| humantime::parse_duration(w).ok());
        diagnostic.reason = if !window.is_some_and(minute_window) {
            "watcher windows require whole minutes from 1 through 60".into()
        } else if rule.mount.as_ref().is_some_and(|mount| {
            view.watch_warming_up(&mount.to_string_lossy(), window.unwrap_or_default())
        }) {
            "warming up: waiting for completed whole minutes".into()
        } else {
            "exact completed-minute observation unavailable: missing, stale, lossy or truncated window".into()
        };
    }
}

pub(crate) fn minute_window(window: Duration) -> bool {
    window.subsec_nanos() == 0
        && (60..=3600).contains(&window.as_secs())
        && window.as_secs().is_multiple_of(60)
}

// Collector disablement must not turn retained measurements into healthy observations.
struct EnabledSource<'a> {
    source: &'a dyn tidemark_live::alerts::AlertSource,
    config: &'a Config,
    procscan_ok: bool,
    interrupted_fs: &'a [String],
    interrupted_lvm: bool,
}
impl tidemark_live::alerts::AlertSource for EnabledSource<'_> {
    fn fs_sample(&self, mount: &str) -> Option<tidemark_core::live::FsSample> {
        if self.interrupted_fs.iter().any(|m| m == mount) {
            return None;
        }
        self.source.fs_sample(mount)
    }
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64> {
        if self.interrupted_fs.iter().any(|m| m == mount) {
            return None;
        }
        self.source.fs_rate(mount, window)
    }
    fn hot_dirs(
        &self,
        mount: &str,
        window: Duration,
    ) -> Option<Vec<tidemark_core::live::HotEntry>> {
        if !self.config.watch.enabled || !minute_window(window) {
            return None;
        }
        self.source.hot_dirs(mount, window)
    }
    fn watch_overflows(&self, mount: &str, window: Duration) -> Option<u64> {
        if !self.config.watch.enabled || !minute_window(window) {
            return None;
        }
        self.source.watch_overflows(mount, window)
    }
    fn phantom_bytes(&self, mount: &str) -> Option<u64> {
        if !self.config.procscan.enabled || !self.procscan_ok {
            return None;
        }
        self.source.phantom_bytes(mount)
    }
    fn lvm(&self) -> Option<Vec<tidemark_core::live::LvmNow>> {
        if !self.config.lvm.enabled || self.interrupted_lvm {
            return None;
        }
        self.source.lvm()
    }
    fn dir_growth(
        &self,
        root: &str,
        depth: u32,
    ) -> Option<Vec<(tidemark_core::path::RawPath, i64)>> {
        self.source.dir_growth(root, depth)
    }
    fn scan_age(&self, root: &str) -> Option<tidemark_live::alerts::ScanAge> {
        self.source.scan_age(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidemark_core::{
        config::{AlertRule, AlertThreshold},
        live::{AlertEvent, AlertState, FsSample, HotEntry, LvmNow},
        path::RawPath,
    };
    use tidemark_live::alerts::{AlertSource, ScanAge};
    #[test]
    fn warming_health_fault_names_and_now_diagnostics() {
        use tidemark_core::config::AlertKind;
        let mut cfg = Config::default();
        cfg.alerts.rule = vec![AlertRule {
            name: "hot".into(),
            kind: AlertKind::HotDirGrowth,
            mount: Some("/data".into()),
            path: Some("/data/*".into()),
            window: Some("1m".into()),
            above: Some(AlertThreshold::Number(1.)),
            ..Default::default()
        }];
        let state = LiveState::new(&cfg, 0);
        state
            .publish(
                Publication::Watcher((
                    "/data".into(),
                    Arc::new(
                        tidemark_live::watch::Watcher::from_injected_events(
                            RawPath::from_path(std::path::Path::new("/data")).unwrap(),
                            [],
                        )
                        .unwrap(),
                    ),
                )),
                crate::component("watcher:/data", "ok", "fixture"),
            )
            .unwrap();
        let mut engine = AlertEngine::new(&cfg.alerts.rule, cfg.alerts.renotify).unwrap();
        let (tx, _rx) = crossbeam_channel::bounded(64);
        let mut failures = Default::default();
        let mut sinks = [];
        tick_current(&mut engine, 30, &state, &tx, &mut sinks, &mut failures).unwrap();
        let view = state.view_at(30);
        assert_eq!(
            view.status(30)
                .unwrap()
                .components
                .iter()
                .find(|c| c.name == "alerts")
                .unwrap()
                .state,
            "ok"
        );
        assert!(view.alert_diagnostics().unwrap()[0]
            .reason
            .starts_with("warming up"));
        assert!(view
            .now(&tidemark_core::api::NowParams { mount: None }, 30)
            .unwrap()
            .degraded
            .iter()
            .any(|d| d.starts_with("alert hot: warming up")));
        tick_current(&mut engine, 121, &state, &tx, &mut sinks, &mut failures).unwrap();
        let view = state.view_at(121);
        let health = view
            .status(121)
            .unwrap()
            .components
            .into_iter()
            .find(|c| c.name == "alerts")
            .unwrap();
        assert_eq!(health.state, "degraded");
        assert_eq!(health.detail, "1 rules not evaluable: hot");
        assert!(view
            .now(&tidemark_core::api::NowParams { mount: None }, 121)
            .unwrap()
            .degraded
            .iter()
            .any(|d| d.starts_with("alert hot: not evaluable:")));
        state
            .publish(
                Publication::WatchWindow {
                    mount: "/data".into(),
                    window: Duration::from_secs(60),
                    measured_at: 120,
                    hot: Some(vec![]),
                    overflows: Some(0),
                },
                crate::component("watcher:/data", "ok", "fixture"),
            )
            .unwrap();
        tick_current(&mut engine, 150, &state, &tx, &mut sinks, &mut failures).unwrap();
        assert!(state.view_at(150).alert_diagnostics().unwrap().is_empty());
        assert_eq!(
            state
                .view_at(150)
                .status(150)
                .unwrap()
                .components
                .iter()
                .find(|c| c.name == "alerts")
                .unwrap()
                .state,
            "ok"
        );
        cfg.alerts.rule = (0..4)
            .map(|i| AlertRule {
                name: format!("missing{i}"),
                mount: Some("/missing".into()),
                above: Some(AlertThreshold::Number(50.)),
                ..Default::default()
            })
            .collect();
        state.replace_config(cfg.clone()).unwrap();
        engine
            .reconfigure(&cfg.alerts.rule, cfg.alerts.renotify)
            .unwrap();
        tick_current(&mut engine, 150, &state, &tx, &mut sinks, &mut failures).unwrap();
        assert_eq!(
            state
                .view_at(150)
                .status(150)
                .unwrap()
                .components
                .iter()
                .find(|c| c.name == "alerts")
                .unwrap()
                .detail,
            "4 rules not evaluable: missing0, missing1, missing2"
        );
        assert_eq!(
            state
                .view_at(150)
                .now(&tidemark_core::api::NowParams { mount: None }, 150)
                .unwrap()
                .degraded
                .iter()
                .filter(|d| d.starts_with("alert missing"))
                .count(),
            4
        );
    }
    #[test]
    fn rate_window_warmup_requires_a_healthy_sample_and_expires() {
        let mut cfg = Config::default();
        cfg.alerts.rule = vec![AlertRule {
            name: "full".into(),
            kind: tidemark_core::config::AlertKind::TimeToFull,
            mount: Some("/data".into()),
            below: Some("1h".into()),
            window: Some("1h".into()),
            ..Default::default()
        }];
        let state = LiveState::new(&cfg, 0);
        let mut engine = AlertEngine::new(&cfg.alerts.rule, cfg.alerts.renotify).unwrap();
        let (tx, _rx) = crossbeam_channel::bounded(64);
        let mut failures = Default::default();
        let sample = |ts| {
            state
                .publish(
                    Publication::Fs(FsSample {
                        ts,
                        mount: "/data".into(),
                        size: 100,
                        used: 50,
                        avail: 50,
                        inodes: 1,
                        inodes_used: 0,
                    }),
                    crate::component("sampler", "ok", "fixture"),
                )
                .unwrap()
        };
        sample(0);
        tick_current(&mut engine, 30, &state, &tx, &mut [], &mut failures).unwrap();
        assert!(state.view_at(30).alert_diagnostics().unwrap()[0]
            .reason
            .starts_with("warming up"));
        tick_current(&mut engine, 91, &state, &tx, &mut [], &mut failures).unwrap();
        assert_eq!(
            state.view_at(91).alert_diagnostics().unwrap()[0].reason,
            "filesystem sample unavailable"
        );
        sample(3601);
        tick_current(&mut engine, 3601, &state, &tx, &mut [], &mut failures).unwrap();
        assert!(!state.view_at(3601).alert_diagnostics().unwrap()[0]
            .reason
            .starts_with("warming up"));
        sample(3751);
        sample(3901);
        tick_current(&mut engine, 3901, &state, &tx, &mut [], &mut failures).unwrap();
        assert!(state.view_at(3901).alert_diagnostics().unwrap().is_empty());
    }
    struct Source(Option<u64>);
    impl AlertSource for Source {
        fn fs_sample(&self, mount: &str) -> Option<FsSample> {
            self.0.map(|used| FsSample {
                ts: 0,
                mount: mount.into(),
                size: 100,
                used,
                avail: 100 - used,
                inodes: 1,
                inodes_used: 0,
            })
        }
        fn fs_rate(&self, _: &str, _: Duration) -> Option<f64> {
            None
        }
        fn hot_dirs(&self, _: &str, _: Duration) -> Option<Vec<HotEntry>> {
            Some(vec![])
        }
        fn watch_overflows(&self, _: &str, _: Duration) -> Option<u64> {
            Some(0)
        }
        fn phantom_bytes(&self, _: &str) -> Option<u64> {
            Some(0)
        }
        fn lvm(&self) -> Option<Vec<LvmNow>> {
            Some(vec![])
        }
        fn dir_growth(&self, _: &str, _: u32) -> Option<Vec<(RawPath, i64)>> {
            None
        }
        fn scan_age(&self, _: &str) -> Option<ScanAge> {
            None
        }
    }
    struct Recording(Arc<parking_lot::Mutex<Vec<AlertEvent>>>);
    impl Deliver for Recording {
        fn deliver(
            &mut self,
            event: &AlertEvent,
        ) -> Result<(), tidemark_live::alerts::DeliverError> {
            self.0.lock().push(event.clone());
            Ok(())
        }
    }
    #[test]
    fn collector_failure_expiry_and_recovery_through_enabled_source() {
        let mut cfg = Config::default();
        cfg.sampler.mounts = vec!["/data".into(), "/other".into()];
        cfg.sampler.interval = Duration::from_secs(10);
        cfg.lvm.interval = Duration::from_secs(10);
        cfg.alerts.rule = vec![AlertRule {
            name: "usage".into(),
            mount: Some("/data".into()),
            above: Some(AlertThreshold::Number(50.)),
            for_: Duration::from_secs(10),
            ..Default::default()
        }];
        let state = LiveState::new(&cfg, 0);
        let publish_sample = |mount: &str, ts| {
            state
                .publish(
                    Publication::FsAttempt {
                        generation: 0,
                        mount: mount.into(),
                        sample: Some(FsSample {
                            ts,
                            mount: mount.into(),
                            size: 100,
                            used: 95,
                            avail: 5,
                            inodes: 1,
                            inodes_used: 0,
                        }),
                    },
                    crate::component("sampler", "ok", "sample"),
                )
                .unwrap()
        };
        publish_sample("/data", 0);
        publish_sample("/other", 0);
        let mut engine = AlertEngine::new(&cfg.alerts.rule, Duration::from_secs(20)).unwrap();
        let evaluate = |engine: &mut AlertEngine, now| {
            let view = state.view_at(now);
            engine.evaluate(
                now,
                &AlertInputs {
                    source: &EnabledSource {
                        source: &view,
                        config: &cfg,
                        procscan_ok: false,
                        interrupted_fs: &[],
                        interrupted_lvm: false,
                    },
                    state_space_refused: false,
                },
            )
        };
        assert!(evaluate(&mut engine, 0).is_empty());
        state
            .publish(
                Publication::FsAttempt {
                    generation: 0,
                    mount: "/data".into(),
                    sample: None,
                },
                crate::component("sampler", "degraded", "collection failed"),
            )
            .unwrap();
        assert!(evaluate(&mut engine, 10).is_empty());
        assert!(evaluate(&mut engine, 30).is_empty());
        assert_eq!(state.view_at(30).fs_samples().unwrap().len(), 2);
        assert!(state.view_at(30).fs_sample("/other").is_some());
        assert!(state.view_at(31).fs_sample("/other").is_none());
        assert!(state.view_at(86400).fs_sample("/data").is_none());
        publish_sample("/data", 40);
        // Persistence and another mount's error do not invalidate this sample.
        state
            .publish(
                Publication::FsAttempt {
                    generation: 0,
                    mount: "/other".into(),
                    sample: None,
                },
                crate::component("sampler", "degraded", "other mount failed"),
            )
            .unwrap();
        state
            .publish_component(crate::component("sampler", "degraded", "writer congested"))
            .unwrap();
        assert!(evaluate(&mut engine, 40).is_empty());
        assert_eq!(evaluate(&mut engine, 50)[0].state, AlertState::Firing);
        assert!(evaluate(&mut engine, 71).is_empty());
        state
            .publish(
                Publication::LvmAttempt {
                    generation: 0,
                    report: Some(tidemark_core::api::LvmResult {
                        mounts: vec![],
                        vgs: vec![],
                        sampled_at: 100,
                    }),
                },
                crate::component("lvm", "ok", "report"),
            )
            .unwrap();
        assert_eq!(state.view_at(130).lvm(), Some(vec![]));
        assert!(state.view_at(131).lvm().is_none());
        state
            .publish(
                Publication::LvmAttempt {
                    generation: 0,
                    report: None,
                },
                crate::component("lvm", "error", "failed"),
            )
            .unwrap();
        assert!(state.view_at(100).lvm().is_none());
        assert!(state
            .view_at(100)
            .lvm_report(&tidemark_core::api::LvmParams { mount: None })
            .is_some());
        state
            .publish(
                Publication::LvmAttempt {
                    generation: 0,
                    report: Some(tidemark_core::api::LvmResult {
                        mounts: vec![],
                        vgs: vec![],
                        sampled_at: 140,
                    }),
                },
                crate::component("lvm", "ok", "recovered"),
            )
            .unwrap();
        assert_eq!(state.view_at(140).lvm(), Some(vec![]));
    }
    #[test]
    fn failure_between_alert_ticks_restarts_continuity() {
        let mut cfg = Config::default();
        cfg.sampler.mounts = vec!["/data".into()];
        cfg.sampler.interval = Duration::from_secs(10);
        cfg.alerts.rule = vec![AlertRule {
            name: "usage".into(),
            mount: Some("/data".into()),
            above: Some(AlertThreshold::Number(50.)),
            for_: Duration::from_secs(10),
            ..Default::default()
        }];
        let state = LiveState::new(&cfg, 0);
        let publish = |ts| {
            state
                .publish(
                    Publication::FsAttempt {
                        generation: 0,
                        mount: "/data".into(),
                        sample: Some(FsSample {
                            ts,
                            mount: "/data".into(),
                            size: 100,
                            used: 95,
                            avail: 5,
                            inodes: 1,
                            inodes_used: 0,
                        }),
                    },
                    crate::component("sampler", "ok", "sample"),
                )
                .unwrap()
        };
        let mut engine = AlertEngine::new(&cfg.alerts.rule, Duration::from_secs(20)).unwrap();
        let (tx, rx) = crossbeam_channel::bounded(8);
        let mut failures = CollectorFailures::default();
        publish(0);
        tick_current(&mut engine, 0, &state, &tx, &mut [], &mut failures).unwrap();
        state
            .publish(
                Publication::FsAttempt {
                    generation: 0,
                    mount: "/data".into(),
                    sample: None,
                },
                crate::component("sampler", "error", "failed"),
            )
            .unwrap();
        publish(9);
        tick_current(&mut engine, 10, &state, &tx, &mut [], &mut failures).unwrap();
        assert!(
            rx.try_recv().is_err(),
            "recovery must restart pending evidence"
        );
        tick_current(&mut engine, 20, &state, &tx, &mut [], &mut failures).unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            super::super::tswriter::Message::Alert(_)
        ));
        // A freshness gap between ticks also breaks continuity, even when
        // neither tick directly observes the expired sample.
        let mut engine = AlertEngine::new(&cfg.alerts.rule, Duration::from_secs(20)).unwrap();
        tick_current(&mut engine, 20, &state, &tx, &mut [], &mut failures).unwrap();
        publish(100);
        tick_current(&mut engine, 100, &state, &tx, &mut [], &mut failures).unwrap();
        assert!(rx.try_recv().is_err());
        tick_current(&mut engine, 110, &state, &tx, &mut [], &mut failures).unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            super::super::tswriter::Message::Alert(_)
        ));
    }
    #[test]
    fn transitions_delivery_persistence_and_unavailable_pause() {
        let mut cfg = Config::default();
        cfg.alerts.rule = vec![AlertRule {
            name: "usage".into(),
            mount: Some("/data".into()),
            above: Some(AlertThreshold::Number(50.)),
            for_: Duration::from_secs(10),
            resolve_for: Duration::from_secs(10),
            ..Default::default()
        }];
        let state = LiveState::new(&cfg, 0);
        let mut engine = AlertEngine::new(&cfg.alerts.rule, Duration::from_secs(20)).unwrap();
        let (tx, rx) = crossbeam_channel::bounded(8);
        let recorded = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let mut sinks: Vec<Box<dyn Deliver>> = vec![Box::new(Recording(recorded.clone()))];
        for (now, used) in [
            (0, Some(80)),
            (10, Some(80)),
            (30, Some(80)),
            (60, None),
            (70, Some(0)),
            (80, Some(0)),
        ] {
            tick(
                &mut engine,
                now,
                &AlertInputs {
                    source: &Source(used),
                    state_space_refused: false,
                },
                &state,
                &tx,
                &mut sinks,
            )
            .unwrap();
            if used.is_none() {
                assert_eq!(recorded.lock().len(), 2);
                assert_eq!(state.view_at(now).active_alerts().unwrap().len(), 1);
                assert_eq!(state.view_at(now).alert_diagnostics().unwrap().len(), 1);
            }
        }
        let events = recorded.lock();
        assert_eq!(
            events.iter().map(|e| e.state.clone()).collect::<Vec<_>>(),
            [
                AlertState::Firing,
                AlertState::Renotify,
                AlertState::Renotify,
                AlertState::Resolved
            ]
        );
        assert!(state.view_at(80).active_alerts().unwrap().is_empty());
        for event in events.iter() {
            match rx.try_recv().unwrap() {
                super::super::tswriter::Message::Alert(persisted) => {
                    assert_eq!(persisted.ts, event.ts)
                }
                _ => panic!("wrong message"),
            }
        }
    }
    #[test]
    fn state_space_and_full_writer_do_not_block_delivery() {
        let cfg = Config::default();
        let state = LiveState::new(&cfg, 0);
        let mut engine = AlertEngine::new(&[], Duration::from_secs(20)).unwrap();
        let (tx, _rx) = crossbeam_channel::bounded(0);
        let recorded = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let mut sinks: Vec<Box<dyn Deliver>> = vec![Box::new(Recording(recorded.clone()))];
        tick(
            &mut engine,
            0,
            &AlertInputs {
                source: &Source(None),
                state_space_refused: true,
            },
            &state,
            &tx,
            &mut sinks,
        )
        .unwrap();
        assert_eq!(recorded.lock()[0].rule, "state_space");
        assert_eq!(state.view_at(0).active_alerts().unwrap().len(), 1);
        tick(
            &mut engine,
            1,
            &AlertInputs {
                source: &Source(None),
                state_space_refused: false,
            },
            &state,
            &tx,
            &mut sinks,
        )
        .unwrap();
        tick(
            &mut engine,
            601,
            &AlertInputs {
                source: &Source(None),
                state_space_refused: false,
            },
            &state,
            &tx,
            &mut sinks,
        )
        .unwrap();
        assert_eq!(recorded.lock()[1].state, AlertState::Resolved);
    }
    #[test]
    fn exact_window_gate_and_disabled_collectors() {
        let mut config = Config::default();
        config.watch.enabled = false;
        config.procscan.enabled = false;
        config.lvm.enabled = false;
        let source = EnabledSource {
            source: &Source(None),
            config: &config,
            procscan_ok: true,
            interrupted_fs: &[],
            interrupted_lvm: false,
        };
        assert!(source.hot_dirs("/data", Duration::from_secs(60)).is_none());
        assert!(source.phantom_bytes("/data").is_none());
        assert!(source.lvm().is_none());
        config.watch.enabled = true;
        let source = EnabledSource {
            source: &Source(None),
            config: &config,
            procscan_ok: true,
            interrupted_fs: &[],
            interrupted_lvm: false,
        };
        for window in [
            Duration::ZERO,
            Duration::from_secs(59),
            Duration::from_secs(61),
            Duration::from_secs(3601),
            Duration::from_millis(60001),
        ] {
            assert!(source.hot_dirs("/data", window).is_none());
            assert!(source.watch_overflows("/data", window).is_none());
        }
        for minute in 1..=60 {
            assert_eq!(
                source.watch_overflows("/data", Duration::from_secs(minute * 60)),
                Some(0)
            );
        }
    }
}
