//! Owned by: W6-A2b
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
            let mut engine = None;
            let mut previous_failures = (std::collections::BTreeMap::new(), 0);
            let mut sinks: Vec<Box<dyn Deliver>> = vec![Box::new(JournalDeliver)];
            while !shutdown.load(Ordering::Acquire) {
                let (config, generation) = state.config();
                if previous.as_ref() != Some(&config.alerts) {
                    engine = Some(
                        AlertEngine::new(&config.alerts.rule, config.alerts.renotify)
                            .map_err(|e| DaemonError::Config(e.to_string()))?,
                    );
                    previous = Some(config.alerts.clone());
                }
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
                crate::wait_service(&state, generation, &shutdown, Duration::from_secs(30));
            }
            state.publish_component(crate::component("alerts", "stopped", "shutdown"))?;
            Ok(())
        })
        .map_err(|source| DaemonError::Io {
            context: "spawn alerts".into(),
            source,
        })
}

type CollectorFailures = (std::collections::BTreeMap<String, u64>, u64);

fn tick_current(
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
    let failures = view.collector_failures();
    let interrupted: Vec<_> = failures
        .0
        .iter()
        .filter(|(mount, epoch)| previous_failures.0.get(*mount).copied().unwrap_or(0) != **epoch)
        .map(|(mount, _)| mount.clone())
        .collect();
    let mut events = Vec::new();
    if !interrupted.is_empty() || previous_failures.1 != failures.1 {
        // Replay a missed unavailability observation before recovery. A
        // failed and recovered pass between alert ticks still breaks continuity.
        let source = EnabledSource {
            source: &view,
            config: &config,
            procscan_ok,
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
    tick_evaluated(engine, state, writer, sinks, events)?;
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
    tick_evaluated(engine, state, writer, sinks, events)
}

fn tick_evaluated(
    engine: &AlertEngine,
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
    for diagnostic in &mut diagnostics {
        if let Some(rule) = config
            .alerts
            .rule
            .iter()
            .find(|r| r.name == diagnostic.rule)
        {
            if matches!(
                rule.kind,
                tidemark_core::config::AlertKind::HotDirGrowth
                    | tidemark_core::config::AlertKind::WatchLossy
            ) {
                let window = rule
                    .window
                    .as_deref()
                    .and_then(|w| humantime::parse_duration(w).ok());
                diagnostic.reason = if !window.is_some_and(minute_window) {
                    "watcher windows require whole minutes from 1 through 60".into()
                } else {
                    "exact completed-minute observation unavailable: collector lacks empty-minute/loss records and boundary overflow counters".into()
                };
            }
        }
    }
    let health = if !errors.is_empty() {
        crate::component("alerts", "degraded", errors.join("; "))
    } else if !diagnostics.is_empty() {
        crate::component(
            "alerts",
            "degraded",
            "rules not evaluable; see alert diagnostics",
        )
    } else {
        crate::component("alerts", "ok", "rules evaluated")
    };
    state.publish(Publication::Alerts((engine.active(), diagnostics)), health)
}

fn minute_window(window: Duration) -> bool {
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
