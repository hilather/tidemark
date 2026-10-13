//! Owned by: W6-watch-windows
//! Mount watcher lifecycle and persistence of per-minute top-1,000 lists.
//!
//! Exact alert windows use completed facts; persistence uses their original rows.

use crate::{
    state::{LiveState, Publication},
    DaemonError, ServiceHandle, Shutdown,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tidemark_core::{
    config::{AlertKind, Config},
    live::HotEntry,
    path::RawPath,
};
use tidemark_live::watch::{CompletedMinute, WatchMode, Watcher, WatcherOptions};

fn options(config: &Config, path: &std::path::Path) -> Result<WatcherOptions, DaemonError> {
    let raw =
        |p: &std::path::Path| RawPath::from_path(p).map_err(|e| DaemonError::Config(e.to_string()));
    Ok(WatcherOptions {
        mount: raw(path)?,
        events: config.watch.events.clone(),
        hot_depth: config.watch.hot_depth as usize,
        exclude_prefixes: config
            .watch
            .exclude_prefixes
            .iter()
            .map(|p| raw(p))
            .collect::<Result<_, _>>()?,
        lru_entries: config.watch.lru_entries,
    })
}

/// Start watchers only after the library's host namespace check succeeds.
/// Restart only when watcher settings change, releasing state and service Arcs.
/// Daemon use never enables the foreground event buffer or clears dirty roots.
pub fn start(
    config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
) -> Result<ServiceHandle, DaemonError> {
    crate::state::validate_bounds(&config)?;
    std::thread::Builder::new()
        .name("watcher".into())
        .spawn(move || {
            let mut previous: Option<tidemark_core::config::WatchConfig> = None;
            let mut identities = BTreeMap::new();
            let mut minutes: BTreeMap<String, MinuteHistory> = BTreeMap::new();
            let mut previous_rules = Vec::new();
            let mut watchers: BTreeMap<String, Arc<Watcher>> = BTreeMap::new();
            let attribution = Arc::new(tidemark_fs::attribution::AttributionCache::new(
                Duration::from_secs(60),
                10_000,
            ));
            let result = (|| {
                while !shutdown.load(Ordering::Acquire) {
                    let (config, generation) = state.config();
                    if previous.as_ref() != Some(&config.watch) {
                        for mount in watchers.keys() {
                            state.remove_watcher(mount);
                        }
                        if let Some(old) = &previous {
                            for path in &old.mounts {
                                state.remove_component(&format!(
                                    "watcher:{}",
                                    path.to_string_lossy()
                                ));
                            }
                        }
                        minutes.clear();
                        watchers.clear();
                        crate::watch_fd_budget(if config.watch.enabled {
                            config.watch.mounts.len()
                        } else {
                            0
                        });
                        identities.retain(|path, _| config.watch.mounts.contains(path));
                        previous = Some(config.watch.clone());
                    }
                    if !config.watch.enabled {
                        state.publish_component(crate::component(
                            "watcher",
                            "disabled",
                            "disabled by config",
                        ))?;
                    } else {
                        let Some(mounts) = read_mount_table(
                            &state,
                            crate::mounts::read,
                            &mut watchers,
                            &mut minutes,
                        )?
                        else {
                            crate::wait_service(
                                &state,
                                generation,
                                &shutdown,
                                Duration::from_secs(1),
                            );
                            continue;
                        };
                        let changes = crate::mounts::reconcile_live(
                            &config.watch.mounts,
                            &mut identities,
                            &mounts,
                        )?;

                        // Release old marks before any replacement starts.
                        for change in &changes {
                            let mount = RawPath::from_path(&change.path)
                                .map_err(|e| DaemonError::Config(e.to_string()))?
                                .display_lossy();
                            state.remove_watcher(&mount);
                            watchers.remove(&mount);
                            minutes.remove(&mount);
                        }
                        let namespace = tidemark_live::watch::namespace_ok();
                        if matches!(&namespace, Err(error) if crate::resource_pressure(error)) {
                            state.publish_component(crate::component(
                                "watcher",
                                "degraded",
                                format!("namespace check: {namespace:?}"),
                            ))?;
                            crate::wait_service(
                                &state,
                                generation,
                                &shutdown,
                                Duration::from_secs(1),
                            );
                            continue;
                        }
                        if !matches!(namespace, Ok(true)) {
                            for mount in watchers.keys() {
                                state.remove_watcher(mount);
                            }
                            watchers.clear();
                            minutes.clear();
                            state.publish_component(crate::component(
                                "watcher",
                                "error",
                                format!("host mount namespace check failed: {namespace:?}"),
                            ))?;
                        }
                        for path in &config.watch.mounts {
                            if shutdown.load(Ordering::Acquire) {
                                break;
                            }
                            let mount = RawPath::from_path(path)
                                .map_err(|e| DaemonError::Config(e.to_string()))?
                                .display_lossy();
                            let name = format!("watcher:{mount}");
                            if identities.get(path) == Some(&None) {
                                state.publish_component(crate::component(
                                    &name,
                                    "degraded",
                                    "not mounted",
                                ))?;
                                continue;
                            }
                            if !matches!(namespace, Ok(true)) || watchers.contains_key(&mount) {
                                continue;
                            }
                            match Watcher::start(options(&config, path)?, attribution.clone()) {
                                Ok(w) => {
                                    if let Err(error) = verify_identity(
                                        identities.get(path).copied().flatten(),
                                        || w.mount_identity(),
                                    ) {
                                        drop(w);
                                        state.publish_component(crate::component(
                                            &name,
                                            "error",
                                            error.to_string(),
                                        ))?;
                                        continue;
                                    }
                                    let w = Arc::new(w);
                                    state.publish(
                                        Publication::Watcher((mount.clone(), w.clone())),
                                        crate::component(&name, "ok", "watcher started"),
                                    )?;
                                    minutes.insert(mount.clone(), MinuteHistory::default());
                                    watchers.insert(mount, w);
                                }
                                Err(e) => {
                                    state.publish_component(crate::component(
                                        &name,
                                        "error",
                                        e.to_string(),
                                    ))?;
                                }
                            }
                        }
                        if config.watch.mounts.is_empty() {
                            state.publish_component(crate::component(
                                "watcher",
                                "disabled",
                                "no watch mounts",
                            ))?;
                        }
                    }
                    for (mount, w) in &watchers {
                        let history = minutes.entry(mount.clone()).or_default();
                        let mut published = 0;
                        let mut rows = Vec::new();
                        for mut minute in w.drain_minutes(1000) {
                            rows.append(&mut minute.persisted);
                            history.push(minute);
                            published = history.publish(mount, &config, &state)?;
                        }
                        if previous_rules != config.alerts.rule {
                            published = history.publish(mount, &config, &state)?;
                        }
                        if published == 0 && !history.0.is_empty() {
                            published = requested_windows(&config, mount).len();
                        }
                        let mut dropped = false;
                        // Persist top 50 for each minute/scope, retaining the collector's rank.
                        let rows = persistence_rows(rows);
                        for rows in rows.chunks(super::tswriter::MAX_BATCH_ROWS) {
                            dropped |= writer
                                .try_send(super::tswriter::Message::Hot(rows.to_vec()))
                                .is_err();
                        }
                        state.publish_component(watcher_component(mount, w, dropped, published))?;
                    }
                    previous_rules = config.alerts.rule.clone();
                    crate::wait_service(&state, generation, &shutdown, Duration::from_secs(1));
                }
                Ok(())
            })();
            // Also release on publication failures: the last Arc drop joins the worker.
            for mount in watchers.keys() {
                state.remove_watcher(mount);
            }
            watchers.clear();
            result?;
            state.publish_component(crate::component(
                "watcher",
                "stopped",
                "all watcher handles released",
            ))?;
            Ok(())
        })
        .map_err(|source| DaemonError::Io {
            context: "spawn watcher".into(),
            source,
        })
}

fn watcher_component(
    mount: &str,
    watcher: &Watcher,
    dropped: bool,
    published: usize,
) -> tidemark_core::api::ComponentState {
    let stats = watcher.stats();
    let health = if stats.mode == WatchMode::Failed {
        "failed"
    } else if stats.recovering || dropped || stats.lossy_minutes_15m > 0 {
        "degraded"
    } else {
        "ok"
    };
    let mut detail = format!("mode={} recovering={} overflows_total={} read_errors={} persistence_dropped={dropped}; windows: {published} published", stats.mode, stats.recovering, stats.overflows_total, stats.read_errors);
    let reasons = watcher.health_detail();
    if !reasons.is_empty() {
        detail.push_str("; ");
        detail.push_str(&reasons);
    }
    crate::component(&format!("watcher:{mount}"), health, detail)
}

// Retain only facts: persistence rows are removed before insertion.
#[derive(Default)]
struct MinuteHistory(VecDeque<CompletedMinute>);
impl MinuteHistory {
    fn push(&mut self, minute: CompletedMinute) {
        if self
            .0
            .back()
            .is_some_and(|last| last.started_at >= minute.started_at)
        {
            self.0.clear();
        }
        if self.0.len() == 60 {
            self.0.pop_front();
        }
        self.0.push_back(minute);
    }
    fn facts(&self, count: usize) -> (Option<Vec<HotEntry>>, Option<u64>) {
        if !(1..=60).contains(&count) {
            return (None, None);
        }
        let Some(last) = self.0.back() else {
            return (None, None);
        };
        let start = last.started_at.saturating_sub((count as i64 - 1) * 60);
        let rows: Vec<_> = self.0.iter().filter(|m| m.started_at >= start).collect();
        // Boundaries may be supplied by either adjacent completed minute.
        // A gap removes hot data, but does not erase known cumulative totals.
        let boundary = |at| {
            let mut total = None;
            for minute in &self.0 {
                for candidate in [
                    (minute.started_at, minute.overflow_total_at_start),
                    (
                        minute.started_at.saturating_add(60),
                        minute.overflow_total_at_end,
                    ),
                ] {
                    if candidate.0 == at {
                        if total.is_some_and(|old| old != candidate.1) {
                            return None;
                        }
                        total = Some(candidate.1);
                    }
                }
            }
            total
        };
        let consistent = rows
            .iter()
            .all(|m| m.overflow_total_at_end >= m.overflow_total_at_start)
            && rows.windows(2).all(|pair| {
                if pair[0].started_at.saturating_add(60) == pair[1].started_at {
                    pair[0].overflow_total_at_end == pair[1].overflow_total_at_start
                } else {
                    pair[0].overflow_total_at_end <= pair[1].overflow_total_at_start
                }
            });
        let overflows = if consistent {
            boundary(start).and_then(|s| {
                boundary(last.started_at.saturating_add(60)).and_then(|e| e.checked_sub(s))
            })
        } else {
            None
        };
        if rows.len() != count
            || rows
                .iter()
                .enumerate()
                .any(|(i, m)| m.started_at != start + i as i64 * 60)
            || !consistent
            || rows.iter().any(|m| {
                m.lossy
                    || m.truncated
                    || m.overflow_total_at_end != m.overflow_total_at_start
                    || m.dirs.iter().any(|r| r.lossy)
            })
        {
            return (None, overflows);
        }
        let mut merged: BTreeMap<Vec<u8>, HotEntry> = BTreeMap::new();
        for minute in rows {
            for row in &minute.dirs {
                if !merged.contains_key(&row.key) && merged.len() == crate::state::MAX_INVENTORY {
                    return (None, overflows);
                }
                let entry = merged.entry(row.key.clone()).or_insert_with(|| HotEntry {
                    key: row.key.clone(),
                    events: 0,
                    files: 0,
                    bytes_delta: 0,
                    top_unit: None,
                    lossy: false,
                });
                entry.events = entry.events.saturating_add(row.events);
                entry.files = entry.files.saturating_add(row.files);
                entry.bytes_delta = entry.bytes_delta.saturating_add(row.bytes_delta);
            }
        }
        let mut hot: Vec<_> = merged.into_values().collect();
        hot.sort_by(|a, b| {
            b.bytes_delta
                .cmp(&a.bytes_delta)
                .then_with(|| a.key.cmp(&b.key))
        });
        (Some(hot), overflows)
    }
    fn publish(
        &self,
        mount: &str,
        config: &Config,
        state: &LiveState,
    ) -> Result<usize, DaemonError> {
        let Some(last) = self.0.back() else {
            return Ok(0);
        };
        let windows = requested_windows(config, mount);
        for count in &windows {
            let (hot, overflows) = self.facts(*count);
            let warming = self.0.len() < *count
                && self.0.iter().all(|m| {
                    !m.lossy
                        && !m.truncated
                        && m.overflow_total_at_start == m.overflow_total_at_end
                        && !m.dirs.iter().any(|row| row.lossy)
                })
                && self
                    .0
                    .iter()
                    .zip(self.0.iter().skip(1))
                    .all(|(a, b)| b.started_at == a.started_at + 60);
            state.publish_window_health(
                Publication::WatchWindow {
                    mount: mount.into(),
                    window: Duration::from_secs(*count as u64 * 60),
                    measured_at: last.started_at.saturating_add(60),
                    hot,
                    overflows,
                },
                crate::component(
                    &format!("watcher:{mount}"),
                    "ok",
                    "completed minute windows published",
                ),
                warming,
                Some(config),
            )?;
        }
        Ok(windows.len())
    }
}
fn requested_windows(config: &Config, mount: &str) -> BTreeSet<usize> {
    config
        .alerts
        .rule
        .iter()
        .filter(|r| {
            r.enabled
                && matches!(r.kind, AlertKind::HotDirGrowth | AlertKind::WatchLossy)
                && r.mount
                    .as_ref()
                    .is_some_and(|p| p.to_string_lossy() == mount)
        })
        .filter_map(|r| humantime::parse_duration(r.window.as_deref()?).ok())
        .filter(|w| super::alerts::minute_window(*w))
        .map(|w| (w.as_secs() / 60) as usize)
        .collect()
}

fn read_mount_table(
    state: &LiveState,
    read: impl FnOnce() -> Result<Vec<tidemark_fs::mount::MountInfo>, DaemonError>,
    watchers: &mut BTreeMap<String, Arc<Watcher>>,
    minutes: &mut BTreeMap<String, MinuteHistory>,
) -> Result<Option<Vec<tidemark_fs::mount::MountInfo>>, DaemonError> {
    match read() {
        Ok(mounts) => Ok(Some(mounts)),
        Err(error) => {
            if !matches!(&error, DaemonError::Io { source, .. } if crate::resource_pressure(source))
            {
                for mount in watchers.keys() {
                    state.remove_watcher(mount);
                }
                watchers.clear();
                minutes.clear();
            }
            state.publish_component(crate::component("watcher", "degraded", error.to_string()))?;
            Ok(None)
        }
    }
}

fn verify_identity(
    expected: Option<crate::mounts::Identity>,
    actual: impl FnOnce() -> std::io::Result<(u64, u32, u32)>,
) -> Result<(), DaemonError> {
    let actual = match actual() {
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => return Ok(()),
        result => result.map_err(|source| DaemonError::Io {
            context: "verify watcher mount identity".into(),
            source,
        })?,
    };
    if expected.map(|i| (u64::from(i.id), i.major, i.minor)) != Some(actual) {
        return Err(DaemonError::Invalid(format!(
            "watcher mount identity mismatch: configured {expected:?}, marked {actual:?}"
        )));
    }
    Ok(())
}

fn persistence_rows(
    rows: Vec<tidemark_core::live::HotMinute>,
) -> Vec<tidemark_core::live::HotMinute> {
    let mut counts = BTreeMap::new();
    rows.into_iter()
        .filter(|r| {
            let count = counts.entry((r.ts, r.scope.clone())).or_insert(0usize);
            *count += 1;
            *count <= 50
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn distinct_cap_detail_and_alert_window_remain_unavailable() {
        let at = crate::unix_now().div_euclid(60) * 60;
        let events = std::iter::once((at - 120, [vec![], vec![], vec![]], 0, None, (1, 0))).chain(
            (1..=100_000).map(|n| {
                (
                    at - 60,
                    [b"d".to_vec(), b"u".to_vec(), b"c".to_vec()],
                    1,
                    None,
                    (1, n),
                )
            }),
        );
        let watcher =
            Watcher::from_injected_events(RawPath::new(b"/fixture".to_vec()).unwrap(), events)
                .unwrap();
        let mut history = MinuteHistory::default();
        for minute in watcher.drain_minutes(1000) {
            history.push(minute);
        }
        let (hot, overflows) = history.facts(history.0.len());
        assert!(history
            .0
            .iter()
            .any(|minute| minute.truncated && minute.lossy));
        assert!(hot.is_none());
        assert_eq!(overflows, Some(0));
        let component = watcher_component("/fixture", &watcher, false, 1);
        assert_eq!(component.state, "degraded");
        assert!(component
            .detail
            .contains("truncated: distinct-file cap reached in the minute"));
        assert!(component
            .detail
            .contains("events complete, per-file counts capped"));
        assert!(!component.detail.contains("lost events:"));
    }

    use super::*;
    fn minute(at: i64, bytes: i64) -> CompletedMinute {
        CompletedMinute {
            started_at: at,
            overflow_total_at_start: 10,
            overflow_total_at_end: 10,
            lossy: false,
            truncated: false,
            dirs: if bytes == 0 {
                vec![]
            } else {
                vec![HotEntry {
                    key: b"/data/a".to_vec(),
                    events: 1,
                    files: 1,
                    bytes_delta: bytes,
                    top_unit: Some("writer.service".into()),
                    lossy: false,
                }]
            },
            persisted: vec![],
        }
    }
    fn rule(kind: AlertKind, window: &str) -> tidemark_core::config::AlertRule {
        tidemark_core::config::AlertRule {
            name: format!("{kind:?}"),
            kind,
            mount: Some("/data".into()),
            window: Some(window.into()),
            path: (kind == AlertKind::HotDirGrowth).then(|| "/data/*".into()),
            above: Some(tidemark_core::config::AlertThreshold::Number(1.)),
            for_: Duration::ZERO,
            resolve_for: Duration::ZERO,
            ..Default::default()
        }
    }
    #[test]
    fn mountinfo_emfile_retains_watcher_and_minute_history() {
        let state = LiveState::new(&Config::default(), 0);
        let watcher = Arc::new(
            Watcher::from_injected_events(RawPath::new(b"/fixture".to_vec()).unwrap(), []).unwrap(),
        );
        let mut watchers = BTreeMap::from([("/fixture".into(), watcher.clone())]);
        let mut minutes = BTreeMap::from([("/fixture".into(), MinuteHistory::default())]);
        let read = || {
            Err(DaemonError::Io {
                context: "mountinfo".into(),
                source: std::io::Error::from_raw_os_error(24),
            })
        };
        assert!(read_mount_table(&state, read, &mut watchers, &mut minutes)
            .unwrap()
            .is_none());
        assert!(Arc::ptr_eq(&watchers["/fixture"], &watcher));
        assert!(minutes.contains_key("/fixture"));
        assert!(state
            .view()
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "watcher" && c.state == "degraded"));
        assert!(
            read_mount_table(&state, || Ok(vec![]), &mut watchers, &mut minutes)
                .unwrap()
                .is_some()
        );
        assert!(watchers.contains_key("/fixture"));
    }
    #[test]
    fn exact_consecutive_minutes_loss_gaps_overflows_and_cap() {
        let mut h = MinuteHistory::default();
        h.push(minute(60, 120));
        assert_eq!(h.facts(2), (None, None));
        h.push(minute(120, -10));
        let (hot, overflow) = h.facts(2);
        assert_eq!(overflow, Some(0));
        let row = &hot.unwrap()[0];
        assert_eq!((row.bytes_delta, row.events, row.files), (110, 2, 2));
        assert_eq!(row.top_unit, None);
        for lossy in [true, false] {
            h.0[0].lossy = lossy;
            h.0[0].truncated = !lossy;
            assert_eq!(h.facts(2), (None, Some(0)));
        }
        h.0[0].truncated = false;
        h.0[0].overflow_total_at_end = 13;
        h.0[1].overflow_total_at_start = 13;
        h.0[1].overflow_total_at_end = 15;
        assert_eq!(h.facts(2), (None, Some(5)));
        h.0[1].started_at = 180;
        assert_eq!(h.facts(2), (None, Some(2)));
        assert_eq!(h.facts(3), (None, Some(5)));
        h.0[1].started_at = 120;
        h.0[1].overflow_total_at_start = 9;
        assert_eq!(h.facts(2), (None, None));
        let mut h = MinuteHistory::default();
        for i in 0..70 {
            h.push(minute(i * 60, 0));
        }
        assert_eq!(h.0.len(), 60);
        assert_eq!(h.facts(60), (Some(vec![]), Some(0)));
        // A union larger than the daemon inventory cap cannot be presented as complete.
        let mut h = MinuteHistory::default();
        for i in 0..11 {
            let mut m = minute(i * 60, 0);
            m.dirs = (0..1000)
                .map(|j| HotEntry {
                    key: (i * 1000 + j).to_string().into_bytes(),
                    events: 1,
                    files: 1,
                    bytes_delta: 1,
                    top_unit: None,
                    lossy: false,
                })
                .collect();
            h.push(m);
        }
        assert_eq!(h.facts(11), (None, Some(0)));
    }
    #[test]
    fn restart_and_duplicate_boundaries_do_not_supply_old_counts() {
        let mut history = MinuteHistory::default();
        history.push(minute(60, 0));
        history.push(minute(120, 0));
        assert_eq!(history.facts(2).1, Some(0));
        // Production creates a fresh history whenever a watcher is replaced.
        history = MinuteHistory::default();
        history.push(minute(180, 0));
        assert_eq!(history.facts(3), (None, None));
        history.push(minute(180, 0));
        assert_eq!(history.facts(2), (None, None));
        let mut reset = minute(240, 0);
        reset.overflow_total_at_start = 0;
        reset.overflow_total_at_end = 0;
        history.push(reset);
        assert_eq!(history.facts(2), (None, None));
    }
    #[test]
    fn injected_watcher_minutes_publish_through_real_reader_and_engine() {
        use tidemark_core::live::AlertState;
        use tidemark_live::alerts::{AlertEngine, AlertInputs, AlertSource};
        let end = crate::unix_now().div_euclid(60) * 60;
        let watcher = Watcher::from_injected_events(
            RawPath::from_path(std::path::Path::new("/data")).unwrap(),
            (0..4).map(|i| {
                (
                    end - 240 + i * 60,
                    [b"/data/a".to_vec(), b"unit".to_vec(), b"comm".to_vec()],
                    120,
                    None,
                    (1, i as u64),
                )
            }),
        )
        .unwrap();
        let mut cfg = Config::default();
        cfg.alerts.rule = vec![
            rule(AlertKind::HotDirGrowth, "2m"),
            rule(AlertKind::WatchLossy, "2m"),
        ];
        let state = LiveState::new(&cfg, end - 240);
        let mut engine = AlertEngine::new(&cfg.alerts.rule, Duration::from_secs(3600)).unwrap();
        let mut h = MinuteHistory::default();
        let facts = watcher.drain_minutes(1000);
        for mut m in facts.into_iter().filter(|m| m.started_at < end) {
            m.persisted.clear();
            h.push(m);
        }
        assert_eq!(h.0.len(), 3);
        h.publish("/data", &cfg, &state).unwrap();
        let evaluate = |engine: &mut AlertEngine, now| {
            engine.evaluate(
                now,
                &AlertInputs {
                    source: &state.view_at(now),
                    state_space_refused: false,
                },
            )
        };
        assert_eq!(evaluate(&mut engine, end)[0].state, AlertState::Firing);
        assert!(engine.diagnostics().is_empty());
        assert!(state
            .view_at(end + 90)
            .hot_dirs("/data", Duration::from_secs(120))
            .is_some());
        assert!(state
            .view_at(end + 91)
            .hot_dirs("/data", Duration::from_secs(120))
            .is_none());
        assert!(state
            .view_at(end - 1)
            .watch_overflows("/data", Duration::from_secs(120))
            .is_none());
        h.push(minute(end, 0));
        h.push(minute(end + 60, 0));
        h.publish("/data", &cfg, &state).unwrap();
        assert_eq!(
            evaluate(&mut engine, end + 120)[0].state,
            AlertState::Resolved
        );
        h.push(CompletedMinute {
            overflow_total_at_start: 10,
            overflow_total_at_end: 15,
            lossy: true,
            ..minute(end + 120, 0)
        });
        h.publish("/data", &cfg, &state).unwrap();
        let events = evaluate(&mut engine, end + 180);
        assert_eq!(events[0].rule, "WatchLossy");
        assert_eq!(events[0].state, AlertState::Firing);
        h.push(CompletedMinute {
            overflow_total_at_start: 15,
            overflow_total_at_end: 15,
            ..minute(end + 180, 0)
        });
        h.push(CompletedMinute {
            overflow_total_at_start: 15,
            overflow_total_at_end: 15,
            ..minute(end + 240, 0)
        });
        h.publish("/data", &cfg, &state).unwrap();
        assert_eq!(
            evaluate(&mut engine, end + 300)[0].state,
            AlertState::Resolved
        );
        assert!(engine.diagnostics().is_empty());
        state.remove_watcher("/data");
        assert!(state
            .view_at(end + 300)
            .hot_dirs("/data", Duration::from_secs(120))
            .is_none());
    }
    #[test]
    fn initial_incomplete_history_warms_but_loss_and_gap_do_not() {
        let mut cfg = Config::default();
        cfg.alerts.rule = vec![rule(AlertKind::HotDirGrowth, "3m")];
        for started in [i64::MIN, i64::MAX] {
            let state = LiveState::new(&cfg, started);
            assert!(!state
                .view_at(started)
                .watch_warming_up("/data", Duration::from_secs(180)));
        }
        let state = LiveState::new(&cfg, 0);
        let w = Arc::new(
            Watcher::from_injected_events(
                RawPath::from_path(std::path::Path::new("/data")).unwrap(),
                [],
            )
            .unwrap(),
        );
        state
            .publish(
                Publication::Watcher(("/data".into(), w)),
                crate::component("watcher:/data", "ok", "fixture"),
            )
            .unwrap();
        let mut h = MinuteHistory::default();
        h.push(minute(60, 0));
        h.publish("/data", &cfg, &state).unwrap();
        assert!(state
            .view_at(120)
            .watch_warming_up("/data", Duration::from_secs(180)));
        assert!(!state
            .view_at(241)
            .watch_warming_up("/data", Duration::from_secs(180)));
        h.0[0].lossy = true;
        h.publish("/data", &cfg, &state).unwrap();
        assert!(!state
            .view_at(120)
            .watch_warming_up("/data", Duration::from_secs(180)));
        h.0[0].lossy = false;
        h.push(minute(180, 0));
        h.publish("/data", &cfg, &state).unwrap();
        assert!(!state
            .view_at(200)
            .watch_warming_up("/data", Duration::from_secs(180)));
    }
    #[test]
    fn marked_identity_mismatch_fails_and_unsupported_is_accepted() {
        let intended = crate::mounts::Identity {
            id: 2,
            major: 8,
            minor: 1,
        };
        assert!(verify_identity(Some(intended), || Ok((2, 8, 1))).is_ok());
        let error = verify_identity(Some(intended), || Ok((3, 8, 1)))
            .unwrap_err()
            .to_string();
        assert!(error.contains("configured") && error.contains("marked (3, 8, 1)"));
        assert!(verify_identity(Some(intended), || Err(std::io::Error::from(
            std::io::ErrorKind::Unsupported
        )))
        .is_ok());
        assert!(verify_identity(Some(intended), || Err(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        )))
        .is_err());
    }
    #[test]
    fn persistence_keeps_fifty_per_minute_and_scope_in_rank_order() {
        let rows = [0, 60]
            .into_iter()
            .flat_map(|ts| {
                ["dir", "unit", "comm"].into_iter().flat_map(move |scope| {
                    (0..1000).map(move |rank| tidemark_core::live::HotMinute {
                        mount: "/data".into(),
                        scope: scope.into(),
                        key: vec![rank as u8],
                        ts,
                        events: 1,
                        files: 1,
                        bytes_delta: 1000 - rank,
                        lossy: ts == 60,
                    })
                })
            })
            .collect();
        let rows = persistence_rows(rows);
        assert_eq!(rows.len(), 300);
        for group in rows.chunks(50) {
            assert_eq!(group[0].bytes_delta, 1000);
            assert_eq!(group[49].bytes_delta, 951);
            assert!(group.iter().all(|r| r.lossy == (r.ts == 60)));
        }
    }
}
