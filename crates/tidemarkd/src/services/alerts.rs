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
use tidemark_live::alerts::{AlertEngine, AlertInputs, Deliver, JournalDeliver, WebhookDeliver};

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
            let mut sinks: Vec<Box<dyn Deliver>> = vec![Box::new(JournalDeliver)];
            while !shutdown.load(Ordering::Acquire) {
                let (config, generation) = state.config();
                if previous.as_ref() != Some(&config.alerts) {
                    engine = Some(
                        AlertEngine::new(&config.alerts.rule, config.alerts.renotify)
                            .map_err(|e| DaemonError::Config(e.to_string()))?,
                    );
                    sinks.truncate(1);
                    if !config.alerts.webhook.is_empty() {
                        let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into());
                        match WebhookDeliver::new(&config.alerts.webhook, &host) {
                            Ok(sink) => sinks.push(Box::new(sink)),
                            Err(e) => {
                                state.publish_component(crate::component(
                                    "alerts",
                                    "degraded",
                                    e.to_string(),
                                ))?;
                            }
                        }
                    }
                    previous = Some(config.alerts.clone());
                }
                if let Some(engine) = engine.as_mut() {
                    let now = crate::unix_now();
                    let view = state.view_at(now);
                    let procscan_ok = view.status(now).is_some_and(|status| {
                        status.components.iter().any(|c| {
                            c.name == "procscan" && matches!(c.state.as_str(), "ok" | "degraded")
                        })
                    });
                    let source = EnabledSource {
                        source: &view,
                        config: &config,
                        procscan_ok,
                    };
                    tick(
                        engine,
                        now,
                        &AlertInputs {
                            source: &source,
                            state_space_refused: view.state_space_refused().unwrap_or(false),
                        },
                        &state,
                        &writer,
                        &mut sinks,
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

fn tick(
    engine: &mut AlertEngine,
    now: i64,
    inputs: &AlertInputs<'_>,
    state: &LiveState,
    writer: &super::tswriter::Sender,
    sinks: &mut [Box<dyn Deliver>],
) -> Result<(), DaemonError> {
    let mut errors = Vec::new();
    let (config, _) = state.config();
    if !config.alerts.webhook.is_empty() && sinks.len() < 2 {
        errors.push("configured webhook deliverer unavailable".into());
    }
    for event in engine.evaluate(now, inputs) {
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
}
impl tidemark_live::alerts::AlertSource for EnabledSource<'_> {
    fn fs_sample(&self, mount: &str) -> Option<tidemark_core::live::FsSample> {
        self.source.fs_sample(mount)
    }
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64> {
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
        if !self.config.lvm.enabled {
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
        };
        assert!(source.hot_dirs("/data", Duration::from_secs(60)).is_none());
        assert!(source.phantom_bytes("/data").is_none());
        assert!(source.lvm().is_none());
        config.watch.enabled = true;
        let source = EnabledSource {
            source: &Source(None),
            config: &config,
            procscan_ok: true,
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
