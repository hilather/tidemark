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
            .filter(|p| p.starts_with(path) || path.starts_with(p))
            .map(|p| raw(p))
            .collect::<Result<_, _>>()?,
        lru_entries: config.watch.lru_entries,
    })
}

fn namespace_detail(result: &std::io::Result<bool>) -> String {
    match result {
        Ok(false) => {
            "host mount namespace check failed: process is outside the host mount namespace".into()
        }
        Err(error) => format!("host mount namespace check failed: {error}"),
        Ok(true) => "host mount namespace check passed".into(),
    }
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
                        reconcile_settings(
                            previous.as_ref(),
                            &config.watch,
                            &state,
                            &writer,
                            &mut watchers,
                            &mut minutes,
                            crate::unix_now(),
                        );
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
                            flush_completed(&watchers, &mount, &writer, crate::unix_now());
                            retire_watcher(&state, &mut watchers, &mut minutes, &mount);
                        }
                        let namespace = tidemark_live::watch::namespace_ok();
                        if matches!(&namespace, Err(error) if crate::resource_pressure(error)) {
                            state.publish_component(crate::component(
                                "watcher",
                                "degraded",
                                namespace_detail(&namespace),
                            ))?;
                            crate::wait_service(
                                &state,
                                generation,
                                &shutdown,
                                Duration::from_secs(1),
                            );
                            continue;
                        }
                        recover_coordinator(&state, &namespace)?;
                        if !matches!(namespace, Ok(true)) {
                            for (mount, watcher) in &watchers {
                                watcher.shutdown();
                                state.remove_watcher(mount);
                            }
                            watchers.clear();
                            minutes.clear();
                            state.publish_component(crate::component(
                                "watcher",
                                "error",
                                namespace_detail(&namespace),
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
                                    minutes.insert(
                                        mount.clone(),
                                        MinuteHistory::for_watcher(w.clone()),
                                    );
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
                    }
                    for (mount, w) in &watchers {
                        let history = minutes
                            .entry(mount.clone())
                            .or_insert_with(|| MinuteHistory::for_watcher(w.clone()));
                        publish_tick(
                            mount,
                            w,
                            history,
                            &config,
                            &state,
                            &writer,
                            (previous_rules != config.alerts.rule, crate::unix_now()),
                        )?;
                    }
                    previous_rules = config.alerts.rule.clone();
                    crate::wait_service(&state, generation, &shutdown, Duration::from_secs(1));
                }
                Ok(())
            })();
            // Also stop on publication failures, independently of retained reader views.
            for (mount, watcher) in &watchers {
                watcher.shutdown();
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

fn publish_tick(
    mount: &str,
    w: &Watcher,
    history: &mut MinuteHistory,
    config: &Config,
    state: &LiveState,
    writer: &super::tswriter::Sender,
    tick: (bool, i64),
) -> Result<(), DaemonError> {
    publish_tick_with_status_hook(
        mount,
        w,
        history,
        config,
        state,
        writer,
        (tick.0, tick.1, || {}),
    )
}

fn publish_tick_with_status_hook(
    mount: &str,
    w: &Watcher,
    history: &mut MinuteHistory,
    config: &Config,
    state: &LiveState,
    writer: &super::tswriter::Sender,
    tick: (bool, i64, impl FnOnce()),
) -> Result<(), DaemonError> {
    let (rules_changed, at, before_status) = tick;
    let mut published = 0;
    let mut rows = Vec::new();
    for mut minute in w.drain_minutes_at(at, 1000) {
        rows.append(&mut minute.persisted);
        history.push(minute);
        published = history.publish(mount, config, state)?;
    }
    if rules_changed {
        published = history.publish(mount, config, state)?;
    }
    if published == 0 && !history.0.is_empty() {
        published = requested_windows(config, mount).len();
    }
    let rows = persistence_rows(rows);
    let dropped = enqueue_persistence(w, writer, rows);
    before_status();
    state.publish_component(watcher_component_at(mount, w, dropped, published, at))
}

#[cfg(test)]
fn watcher_component(
    mount: &str,
    watcher: &Watcher,
    dropped: bool,
    published: usize,
) -> tidemark_core::api::ComponentState {
    watcher_component_at(mount, watcher, dropped, published, crate::unix_now())
}

fn watcher_component_at(
    mount: &str,
    watcher: &Watcher,
    dropped: bool,
    published: usize,
    at: i64,
) -> tidemark_core::api::ComponentState {
    let stats = watcher.stats_monotonic_at(at);
    let health = if stats.mode == WatchMode::Failed {
        "failed"
    } else if stats.recovering || dropped || stats.lossy_minutes_15m > 0 {
        "degraded"
    } else {
        "ok"
    };
    let mut detail = format!("mode={} recovering={} overflows_total={} read_errors={} persistence_dropped={dropped}; windows: {published} published", stats.mode, stats.recovering, stats.overflows_total, stats.read_errors);
    let reasons = watcher.health_detail_at(at);
    if !reasons.is_empty() {
        detail.push_str("; ");
        detail.push_str(&reasons);
    }
    if watcher.unmapped_events() != 0 {
        detail.push_str(&format!("; unmapped_events={}", watcher.unmapped_events()));
    }
    if stats.mode == WatchMode::FdMount {
        detail.push_str("; writes from other mount namespaces and bind mounts are not seen");
    }
    crate::component(&format!("watcher:{mount}"), health, detail)
}

// Retain only facts: persistence rows are removed before insertion.
#[derive(Default)]
struct MinuteHistory(
    VecDeque<CompletedMinute>,
    Option<Arc<Watcher>>,
    VecDeque<(
        Option<tidemark_live::watch::WatcherKeyReservation>,
        Option<tidemark_live::watch::WatcherKeyReservation>,
    )>,
    usize,
);
const HISTORY_BYTES: usize = 8 * 1024 * 1024;
impl MinuteHistory {
    fn for_watcher(watcher: Arc<Watcher>) -> Self {
        Self(VecDeque::new(), Some(watcher), VecDeque::new(), 0)
    }
    fn row_bytes(minute: &CompletedMinute) -> usize {
        minute.dirs.capacity() * std::mem::size_of::<HotEntry>()
            + minute
                .dirs
                .iter()
                .map(|r| r.key.capacity() + r.top_unit.as_ref().map_or(0, String::capacity))
                .sum::<usize>()
    }
    fn push(&mut self, mut minute: CompletedMinute) {
        // Persistence ownership leaves this consumer before minute history admission.
        minute.persisted = Vec::new();
        if self
            .0
            .back()
            .is_some_and(|last| last.started_at >= minute.started_at)
        {
            self.0.clear();
            self.2.clear();
            self.3 = 0;
        }
        if self.0.len() == 60 {
            if let Some(old) = self.0.pop_front() {
                self.3 -= Self::row_bytes(&old);
            }
            self.2.pop_front();
        }
        let bytes = Self::row_bytes(&minute);
        let keys = minute
            .dirs
            .iter()
            .map(|r| r.key.capacity() + r.top_unit.as_ref().map_or(0, String::capacity))
            .sum();
        let (payload, nodes) = self.1.as_ref().map_or((None, None), |w| {
            (
                w.reserve_key_bytes(keys),
                w.reserve_node_bytes(bytes - keys),
            )
        });
        let admitted = bytes <= HISTORY_BYTES.saturating_sub(self.3)
            && (self.1.is_none() || (payload.is_some() && nodes.is_some()));
        if admitted {
            self.3 += bytes;
            self.2.push_back((payload, nodes));
        } else {
            drop((payload, nodes));
            if let Some(w) = &self.1 {
                w.key_memory_budget_exceeded();
            }
            minute.dirs = Vec::new();
            minute.truncated = true;
            minute.lossy = true;
            self.2.push_back((None, None));
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
            let keys: usize = self
                .0
                .iter()
                .rev()
                .take(*count)
                .flat_map(|m| &m.dirs)
                .map(|r| r.key.capacity())
                .sum();
            let rows: usize = self.0.iter().rev().take(*count).map(|m| m.dirs.len()).sum();
            let scratch = self.1.as_ref().map(|w| {
                (
                    w.reserve_key_bytes(keys.saturating_mul(2)),
                    w.reserve_node_bytes(rows.saturating_mul(384)),
                )
            });
            let scratch_ok = scratch
                .as_ref()
                .is_none_or(|(p, n)| p.is_some() && n.is_some());
            let (mut hot, overflows) = if scratch_ok {
                self.facts(*count)
            } else {
                let mut facts = MinuteHistory::default();
                for minute in &self.0 {
                    facts.0.push_back(CompletedMinute {
                        started_at: minute.started_at,
                        overflow_total_at_start: minute.overflow_total_at_start,
                        overflow_total_at_end: minute.overflow_total_at_end,
                        lossy: true,
                        dirs: Vec::new(),
                        persisted: Vec::new(),
                        truncated: true,
                    });
                }
                facts.facts(*count)
            };
            if !scratch_ok {
                hot = None;
                if let Some(w) = &self.1 {
                    w.key_memory_budget_exceeded();
                }
            }
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
    let raw = config
        .watch
        .mounts
        .iter()
        .find(|path| RawPath::from_path(path).is_ok_and(|p| p.display_lossy() == mount));
    config
        .alerts
        .rule
        .iter()
        .filter(|r| {
            r.enabled
                && matches!(r.kind, AlertKind::HotDirGrowth | AlertKind::WatchLossy)
                && r.mount.as_ref().is_some_and(|p| Some(p) == raw)
        })
        .filter_map(|r| humantime::parse_duration(r.window.as_deref()?).ok())
        .filter(|w| super::alerts::minute_window(*w))
        .map(|w| (w.as_secs() / 60) as usize)
        .collect()
}

fn recover_coordinator(
    state: &LiveState,
    namespace: &std::io::Result<bool>,
) -> Result<(), DaemonError> {
    if matches!(namespace, Ok(true)) {
        state.publish_component(crate::component(
            "watcher",
            "ok",
            "mount table and host mount namespace checked",
        ))?;
    }
    Ok(())
}

fn flush_completed(
    watchers: &BTreeMap<String, Arc<Watcher>>,
    mount: &str,
    writer: &super::tswriter::Sender,
    at: i64,
) {
    if let Some(watcher) = watchers.get(mount) {
        let rows = persistence_rows(
            watcher
                .drain_minutes_at(at, 1000)
                .into_iter()
                .flat_map(|m| m.persisted)
                .collect(),
        );
        if enqueue_persistence(watcher, writer, rows) {
            tracing::warn!(mount, "persistence queue rejected retired watcher minutes");
        }
    }
}

fn relevant_exclusions<'a>(
    config: &'a tidemark_core::config::WatchConfig,
    mount: &std::path::Path,
) -> BTreeSet<&'a std::path::PathBuf> {
    config
        .exclude_prefixes
        .iter()
        .filter(|path| path.starts_with(mount) || mount.starts_with(path))
        .collect()
}

pub(crate) fn same_mount_settings(
    old: &tidemark_core::config::WatchConfig,
    configured: &tidemark_core::config::WatchConfig,
    mount: &std::path::Path,
) -> bool {
    old.enabled == configured.enabled
        && old.events == configured.events
        && old.hot_depth == configured.hot_depth
        && old.lru_entries == configured.lru_entries
        && relevant_exclusions(old, mount) == relevant_exclusions(configured, mount)
}

#[allow(clippy::too_many_arguments)]
fn reconcile_settings(
    previous: Option<&tidemark_core::config::WatchConfig>,
    configured: &tidemark_core::config::WatchConfig,
    state: &LiveState,
    writer: &super::tswriter::Sender,
    watchers: &mut BTreeMap<String, Arc<Watcher>>,
    minutes: &mut BTreeMap<String, MinuteHistory>,
    at: i64,
) {
    let retired: Vec<_> = watchers
        .iter()
        .filter(|(_, watcher)| {
            let mount = watcher.mount_path().as_path();
            !configured.enabled
                || previous.is_some_and(|old| !same_mount_settings(old, configured, mount))
                || !configured.mounts.iter().any(|path| path == mount)
        })
        .map(|(label, _)| label.clone())
        .collect();
    for mount in retired {
        flush_completed(watchers, &mount, writer, at);
        retire_watcher_at(state, watchers, minutes, &mount, at);
        state.remove_component(&format!("watcher:{mount}"));
    }
    if let Some(old) = previous {
        for path in &old.mounts {
            if !configured.enabled || !configured.mounts.contains(path) {
                if let Ok(raw) = RawPath::from_path(path) {
                    state.remove_component(&format!("watcher:{}", raw.display_lossy()));
                }
            }
        }
    }
}

fn retire_watcher(
    state: &LiveState,
    watchers: &mut BTreeMap<String, Arc<Watcher>>,
    minutes: &mut BTreeMap<String, MinuteHistory>,
    mount: &str,
) {
    retire_watcher_at(state, watchers, minutes, mount, crate::unix_now());
}

fn retire_watcher_at(
    state: &LiveState,
    watchers: &mut BTreeMap<String, Arc<Watcher>>,
    minutes: &mut BTreeMap<String, MinuteHistory>,
    mount: &str,
    at: i64,
) {
    if let Some(watcher) = watchers.remove(mount) {
        watcher.retire_at(at);
    }
    state.remove_watcher(mount);
    minutes.remove(mount);
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
                for (mount, watcher) in watchers.iter() {
                    watcher.shutdown();
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

// Includes cloned row structures, all three strings/keys and allocation slack.
// With the existing 64-message writer channel this producer queues <= 4 MiB.
const PERSISTENCE_MESSAGE_BYTES: usize = 64 * 1024;
fn enqueue_persistence(
    _watcher: &Watcher,
    writer: &super::tswriter::Sender,
    rows: Vec<tidemark_core::live::HotMinute>,
) -> bool {
    // Move the admitted export, including emergency aggregates, instead of
    // cloning keys against normal admission. Queue ownership has its own fixed
    // message byte and channel count bounds. Only bounded vector slots are new.
    let mut rows = rows.into_iter().peekable();
    let mut dropped = false;
    while rows.peek().is_some() {
        let mut chunk = Vec::new();
        let mut bytes = 0;
        while let Some(row) = rows.peek() {
            let cost = std::mem::size_of::<tidemark_core::live::HotMinute>()
                + 128
                + row.key.len()
                + row.mount.len()
                + row.scope.len();
            if cost > PERSISTENCE_MESSAGE_BYTES {
                rows.next();
                dropped = true;
                continue;
            }
            if cost > PERSISTENCE_MESSAGE_BYTES.saturating_sub(bytes)
                || chunk.len() == super::tswriter::MAX_BATCH_ROWS
            {
                break;
            }
            if chunk.try_reserve(1).is_err() {
                return true;
            }
            bytes += cost;
            if let Some(row) = rows.next() {
                chunk.push(row);
            }
        }
        if !chunk.is_empty() {
            dropped |= writer
                .try_send(super::tswriter::Message::Hot(chunk))
                .is_err();
        }
    }
    dropped
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
    fn combined_mount_window_returns_known_rows_and_names_the_incomplete_mount() {
        use tidemark_core::api::{HotBy, HotParams, HotWindow};
        let cfg = Config::default();
        let state = LiveState::new(&cfg, 0);
        let a = Arc::new(
            Watcher::from_injected_events_at(
                RawPath::new(b"/A".to_vec()).unwrap(),
                -3600,
                [(
                    60,
                    [b"a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                    14,
                    None,
                    (1, 1),
                )],
            )
            .unwrap(),
        );
        let b = Arc::new(
            Watcher::from_injected_events_at(RawPath::new(b"/B".to_vec()).unwrap(), 120, [])
                .unwrap(),
        );
        state
            .publish(
                Publication::Watcher(("/A".into(), a)),
                crate::component("watcher:/A", "ok", "test"),
            )
            .unwrap();
        state
            .publish(
                Publication::Watcher(("/B".into(), b)),
                crate::component("watcher:/B", "ok", "test"),
            )
            .unwrap();
        let p = HotParams {
            mount: None,
            window: HotWindow::FiveMinutes,
            by: HotBy::Dir,
            limit: 1000,
        };
        let rows = state.view_at(120).hot(&p).unwrap();
        assert_eq!(rows[0].bytes_delta, 14);
        assert!(rows[0].lossy);
        let empty_a = Arc::new(
            Watcher::from_injected_events_at(RawPath::new(b"/A".to_vec()).unwrap(), -3600, [])
                .unwrap(),
        );
        state
            .publish(
                Publication::Watcher(("/A".into(), empty_a)),
                crate::component("watcher:/A", "ok", "test"),
            )
            .unwrap();
        assert!(state.view_at(120).hot(&p).is_none());
        assert_eq!(
            state.view_at(120).hot_unavailable_detail(&p),
            "watch history for /B starts at 00:03 UTC; no complete window yet"
        );
        assert_eq!(state.view_at(3720).hot(&p), Some(Vec::new()));
    }

    #[test]
    fn rejected_writer_queue_keeps_persistence_drop_signal() {
        let cfg = Config::default();
        let state = LiveState::new(&cfg, 0);
        let watcher = activity_watcher(b"/data", 0);
        let mut history = MinuteHistory::for_watcher(watcher.clone());
        let (tx, _rx) = crossbeam_channel::bounded(0);
        publish_tick(
            "/data",
            &watcher,
            &mut history,
            &cfg,
            &state,
            &tx,
            (false, 120),
        )
        .unwrap();
        assert!(state
            .view_at(120)
            .status(120)
            .unwrap()
            .components
            .iter()
            .find(|c| c.name == "watcher:/data")
            .unwrap()
            .detail
            .contains("persistence_dropped=true"));
        assert!(watcher.drain_minutes_at(120, 1000).is_empty());
    }

    fn activity_watcher(mount: &[u8], start: i64) -> Arc<Watcher> {
        Arc::new(
            Watcher::from_injected_events(
                RawPath::new(mount.to_vec()).unwrap(),
                [
                    (
                        start,
                        [b"a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                        0,
                        None,
                        (1, 0),
                    ),
                    (
                        start + 60,
                        [b"a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                        82,
                        None,
                        (1, 1),
                    ),
                ],
            )
            .unwrap(),
        )
    }

    #[test]
    fn production_tick_moves_emergency_aggregate_through_full_normal_admission() {
        let cfg = Config::default();
        let state = LiveState::new(&cfg, 0);
        let watcher = Arc::new(
            Watcher::from_injected_events_at(
                RawPath::new(b"/data".to_vec()).unwrap(),
                0,
                [
                    (
                        60,
                        [b"/data/a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                        71,
                        None,
                        (1, 1),
                    ),
                    (
                        61,
                        [b"/data/a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                        -9,
                        None,
                        (1, 2),
                    ),
                ],
            )
            .unwrap(),
        );
        let mut history = MinuteHistory::for_watcher(watcher.clone());
        // Reserve all remaining normal capacity while leaving emergency export
        // headroom available. Binary search handles conservative container charges.
        let mut low: usize = 0;
        let mut high = 64 * 1024 * 1024;
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if watcher.reserve_key_bytes(middle).is_some() {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        let _full = watcher.reserve_key_bytes(low).unwrap();
        assert!(watcher.reserve_key_bytes(1).is_none());
        let (tx, rx) = crossbeam_channel::bounded(64);
        publish_tick(
            "/data",
            &watcher,
            &mut history,
            &cfg,
            &state,
            &tx,
            (false, 120),
        )
        .unwrap();
        let rows: Vec<_> = rx
            .try_iter()
            .flat_map(|message| match message {
                super::super::tswriter::Message::Hot(rows) => rows,
                _ => panic!("wrong message"),
            })
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].bytes_delta, 62);
        assert_eq!(rows[0].events, 2);
        assert_eq!(rows[0].scope, "dir");
        assert_eq!(rows[0].key, b"/data");
        assert!(rows[0].lossy);
        assert!(!state
            .view_at(120)
            .status(120)
            .unwrap()
            .components
            .iter()
            .find(|c| c.name == "watcher:/data")
            .unwrap()
            .detail
            .contains("persistence_dropped=true"));
        publish_tick(
            "/data",
            &watcher,
            &mut history,
            &cfg,
            &state,
            &tx,
            (false, 121),
        )
        .unwrap();
        assert!(rx.is_empty());
    }
    #[test]
    fn reload_preserves_raw_mount_history_and_windows() {
        use tidemark_core::api::{HotBy, HotParams, HotWindow};
        for bytes in [b"/data\\archive".as_slice(), b"/data\narchive".as_slice()] {
            let raw = RawPath::new(bytes.to_vec()).unwrap();
            let path = raw.as_path().to_path_buf();
            let label = raw.display_lossy();
            let mut cfg = Config::default();
            cfg.watch.mounts = vec![path.clone(), "/other".into()];
            let mut alert = rule(AlertKind::HotDirGrowth, "1m");
            alert.mount = Some(path.clone());
            cfg.alerts.rule = vec![alert];
            let state = LiveState::new(&cfg, 0);
            let watcher = activity_watcher(bytes, 0);
            state
                .publish(
                    Publication::Watcher((label.clone(), watcher.clone())),
                    crate::component(&format!("watcher:{label}"), "ok", "test"),
                )
                .unwrap();
            let mut history = MinuteHistory::for_watcher(watcher.clone());
            for minute in watcher.drain_minutes_at(120, 1000) {
                history.push(minute);
            }
            assert!(!history.0.is_empty());
            assert_eq!(history.publish(&label, &cfg, &state).unwrap(), 1);
            let mut histories = BTreeMap::from([(label.clone(), history)]);
            let mut watchers = BTreeMap::from([(label.clone(), watcher.clone())]);
            let (tx, _) = crossbeam_channel::bounded(64);
            let p = HotParams {
                mount: Some(label.clone()),
                by: HotBy::Dir,
                window: HotWindow::FiveMinutes,
                limit: 1000,
            };
            let before = state.view_at(120).hot(&p).unwrap();
            let history_before = histories[&label].0.back().unwrap().dirs.clone();
            for extend in [false, true] {
                let old = cfg.watch.clone();
                if extend {
                    cfg.watch.mounts.push("/new".into());
                    cfg.watch.exclude_prefixes.push("/new/cache".into());
                } else {
                    cfg.watch.mounts.reverse();
                }
                state.replace_config(cfg.clone()).unwrap();
                reconcile_settings(
                    Some(&old),
                    &cfg.watch,
                    &state,
                    &tx,
                    &mut watchers,
                    &mut histories,
                    120,
                );
                assert!(Arc::ptr_eq(&watchers[&label], &watcher));
                assert_eq!(state.view_at(120).hot(&p).unwrap(), before);
                assert_eq!(histories[&label].0.back().unwrap().dirs, history_before);
                assert_eq!(
                    state
                        .view_at(120)
                        .watch_window_at(&label, Duration::from_secs(60)),
                    Some(120)
                );
            }
            let old = cfg.watch.clone();
            cfg.watch.exclude_prefixes.push(path.join("cache"));
            let held = state.view_at(120);
            state.replace_config(cfg.clone()).unwrap();
            reconcile_settings(
                Some(&old),
                &cfg.watch,
                &state,
                &tx,
                &mut watchers,
                &mut histories,
                120,
            );
            assert!(watchers.is_empty());
            assert!(histories.is_empty());
            assert_eq!(watcher.retired_at(), Some(120));
            assert!(held.hot(&p).is_none());
            assert_eq!(
                state
                    .view_at(120)
                    .watch_window_at(&label, Duration::from_secs(60)),
                None
            );
        }
    }
    #[test]
    fn held_mature_view_is_unavailable_after_retirement() {
        use tidemark_core::api::{HotBy, HotParams, HotWindow, NowParams};
        let state = LiveState::new(&Config::default(), 0);
        let watcher = Arc::new(
            Watcher::from_injected_events_at(
                RawPath::new(b"/A".to_vec()).unwrap(),
                0,
                [(
                    3600,
                    [b"a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                    82,
                    None,
                    (1, 1),
                )],
            )
            .unwrap(),
        );
        state
            .publish(
                Publication::Watcher(("/A".into(), watcher.clone())),
                crate::component("watcher:/A", "ok", "test"),
            )
            .unwrap();
        let queries: Vec<_> = [HotBy::Dir, HotBy::Unit, HotBy::Comm]
            .into_iter()
            .flat_map(|by| {
                [
                    HotWindow::OneMinute,
                    HotWindow::FiveMinutes,
                    HotWindow::FifteenMinutes,
                    HotWindow::SixtyMinutes,
                ]
                .map(|window| HotParams {
                    mount: Some("/A".into()),
                    by,
                    window,
                    limit: 1000,
                })
            })
            .collect();
        let held = state.view_at(3600);
        for p in &queries {
            assert!(held.hot(p).is_some());
        }
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let ready = barrier.clone();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            ready.wait();
            watcher.retire_at(3600);
            worker_state.remove_watcher("/A");
            let replacement = activity_watcher(b"/A", 3600);
            worker_state
                .publish(
                    Publication::Watcher(("/A".into(), replacement)),
                    crate::component("watcher:/A", "ok", "test"),
                )
                .unwrap();
            ready.wait();
        });
        barrier.wait();
        barrier.wait();
        worker.join().unwrap();
        for p in &queries {
            assert!(held.hot(p).is_none());
            assert_eq!(
                held.hot_unavailable_detail(p),
                "watcher for /A was replaced at 01:00 UTC"
            );
            assert_eq!(state.view_at(3660).hot(p).unwrap()[0].bytes_delta, 82);
        }
        let report = held
            .now(
                &NowParams {
                    mount: Some("/A".into()),
                },
                3600,
            )
            .unwrap();
        assert!(report
            .degraded
            .iter()
            .any(|s| s == "watcher for /A was replaced at 01:00 UTC"));
    }
    #[test]
    fn reload_membership_preserves_unchanged_watchers_and_flushes_replacements() {
        use tidemark_core::api::{HotBy, HotParams, HotWindow, NowParams};
        let mut cfg = Config::default();
        cfg.watch.mounts = vec!["/A".into(), "/C".into()];
        let state = LiveState::new(&cfg, 0);
        let a = activity_watcher(b"/A", 0);
        state
            .publish(
                Publication::Watcher(("/A".into(), a.clone())),
                crate::component("watcher:/A", "ok", "test"),
            )
            .unwrap();
        let mut watchers = BTreeMap::from([("/A".into(), a.clone())]);
        let mut histories = BTreeMap::from([("/A".into(), MinuteHistory::for_watcher(a.clone()))]);
        let (tx, rx) = crossbeam_channel::bounded(64);
        let queries: Vec<_> = [HotBy::Dir, HotBy::Unit, HotBy::Comm]
            .into_iter()
            .flat_map(|by| {
                [
                    HotWindow::FiveMinutes,
                    HotWindow::FifteenMinutes,
                    HotWindow::SixtyMinutes,
                ]
                .map(|window| HotParams {
                    mount: Some("/A".into()),
                    by,
                    window,
                    limit: 1000,
                })
            })
            .collect();
        let before: Vec<_> = queries.iter().map(|p| state.view_at(120).hot(p)).collect();
        let old = cfg.watch.clone();
        cfg.watch.mounts.reverse();
        state.replace_config(cfg.clone()).unwrap();
        reconcile_settings(
            Some(&old),
            &cfg.watch,
            &state,
            &tx,
            &mut watchers,
            &mut histories,
            120,
        );
        assert!(Arc::ptr_eq(&watchers["/A"], &a));
        assert!(rx.is_empty());
        let old = cfg.watch.clone();
        cfg.watch.mounts.push("/B".into());
        cfg.watch.exclude_prefixes.push("/B/cache".into());
        state.replace_config(cfg.clone()).unwrap();
        reconcile_settings(
            Some(&old),
            &cfg.watch,
            &state,
            &tx,
            &mut watchers,
            &mut histories,
            120,
        );
        assert!(Arc::ptr_eq(&watchers["/A"], &a));
        assert_eq!(
            before,
            queries
                .iter()
                .map(|p| state.view_at(120).hot(p))
                .collect::<Vec<_>>()
        );
        assert!(rx.is_empty());
        // The mount-identity path flushes before retiring, then starts independent history.
        flush_completed(&watchers, "/A", &tx, 120);
        retire_watcher(&state, &mut watchers, &mut histories, "/A");
        assert_eq!(
            rx.try_iter()
                .map(|message| match message {
                    super::super::tswriter::Message::Hot(rows) => rows.len(),
                    _ => 0,
                })
                .sum::<usize>(),
            3
        );
        let replacement = activity_watcher(b"/A", 120);
        state
            .publish(
                Publication::Watcher(("/A".into(), replacement.clone())),
                crate::component("watcher:/A", "ok", "test"),
            )
            .unwrap();
        let view = state.view_at(240);
        for p in &queries {
            let rows = view.hot(p).unwrap();
            assert_eq!(rows[0].bytes_delta, 82);
            assert!(rows[0].lossy);
        }
        let report = view
            .now(
                &NowParams {
                    mount: Some("/A".into()),
                },
                240,
            )
            .unwrap();
        assert!(report
            .degraded
            .iter()
            .any(|s| s == "watcher /A: history starts at 00:03 UTC"));
        assert!(!state
            .view_at(3720)
            .now(
                &NowParams {
                    mount: Some("/A".into())
                },
                3720
            )
            .unwrap()
            .degraded
            .iter()
            .any(|s| s.contains("history starts")));
        // A new daemon and a replacement with no rows have the same unavailable semantics.
        let empty = Arc::new(
            Watcher::from_injected_events_at(RawPath::new(b"/A".to_vec()).unwrap(), 120, [])
                .unwrap(),
        );
        state
            .publish(
                Publication::Watcher(("/A".into(), empty.clone())),
                crate::component("watcher:/A", "ok", "test"),
            )
            .unwrap();
        let p = queries[1].clone();
        let fresh = LiveState::new(&cfg, 120);
        fresh
            .publish(
                Publication::Watcher(("/A".into(), empty)),
                crate::component("watcher:/A", "ok", "test"),
            )
            .unwrap();
        assert!(fresh.view_at(120).hot(&p).is_none());
        assert!(fresh
            .view_at(120)
            .now(
                &NowParams {
                    mount: Some("/A".into())
                },
                120
            )
            .unwrap()
            .degraded
            .iter()
            .any(|s| s == "watcher /A: history starts at 00:03 UTC"));
        for query in &queries {
            assert!(fresh.view_at(120).hot(query).is_none());
        }
        let unavailable_detail = state.view_at(120).hot_unavailable_detail(&p);
        let retiring = activity_watcher(b"/A", 120);
        watchers.insert("/A".into(), retiring.clone());
        histories.insert("/A".into(), MinuteHistory::for_watcher(retiring));
        let old = cfg.watch.clone();
        cfg.watch.hot_depth += 1;
        reconcile_settings(
            Some(&old),
            &cfg.watch,
            &state,
            &tx,
            &mut watchers,
            &mut histories,
            240,
        );
        assert!(watchers.is_empty());
        assert!(histories.is_empty());
        assert_eq!(
            rx.try_iter()
                .map(|message| match message {
                    super::super::tswriter::Message::Hot(rows) => rows.len(),
                    _ => 0,
                })
                .sum::<usize>(),
            3
        );
        reconcile_settings(
            Some(&old),
            &cfg.watch,
            &state,
            &tx,
            &mut watchers,
            &mut histories,
            240,
        );
        assert!(rx.is_empty());
        assert!(state.view_at(120).hot(&p).is_none());
        assert_eq!(
            unavailable_detail,
            "watch history for /A starts at 00:03 UTC; no complete window yet"
        );
    }
    #[test]
    fn delayed_status_after_drain_does_not_rollback_history() {
        use tidemark_live::watch::HotScope;
        let w = Arc::new(
            Watcher::from_injected_events_at(
                RawPath::new(b"/data".to_vec()).unwrap(),
                -3600,
                [(
                    60,
                    [b"/data/a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                    82,
                    None,
                    (1, 1),
                )],
            )
            .unwrap(),
        );
        let cfg = Config::default();
        let state = LiveState::new(&cfg, 0);
        let (tx, rx) = crossbeam_channel::bounded(64);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let tick = {
            let w = w.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut history = MinuteHistory::for_watcher(w.clone());
                publish_tick_with_status_hook(
                    "/data",
                    &w,
                    &mut history,
                    &cfg,
                    &state,
                    &tx,
                    (false, 179, || {
                        barrier.wait();
                        barrier.wait();
                    }),
                )
                .unwrap();
                state
                    .view_at(180)
                    .status(180)
                    .unwrap()
                    .components
                    .into_iter()
                    .find(|c| c.name == "watcher:/data")
                    .unwrap()
            })
        };
        barrier.wait();
        w.stats_at(180);
        let before: Vec<_> = [HotScope::Dir, HotScope::Unit, HotScope::Comm]
            .into_iter()
            .flat_map(|scope| [5, 15, 60].map(|window| w.hot_at(180, window, scope, 1000)))
            .collect();
        assert!(before.iter().flatten().all(|row| !row.lossy));
        barrier.wait();
        assert_eq!(tick.join().unwrap().state, "ok");
        assert_eq!(
            rx.try_iter()
                .map(|message| match message {
                    super::super::tswriter::Message::Hot(rows) => rows.len(),
                    _ => 0,
                })
                .sum::<usize>(),
            3
        );
        let after: Vec<_> = [HotScope::Dir, HotScope::Unit, HotScope::Comm]
            .into_iter()
            .flat_map(|scope| [5, 15, 60].map(|window| w.hot_at(180, window, scope, 1000)))
            .collect();
        assert_eq!(before, after);
        assert!(w
            .drain_minutes_at(180, 1000)
            .iter()
            .all(|m| m.persisted.is_empty()));
        w.stats_at(119);
        for scope in [HotScope::Dir, HotScope::Unit, HotScope::Comm] {
            for window in [5, 15, 60] {
                let rows = w.hot_at(180, window, scope, 1000);
                assert_eq!(rows[0].bytes_delta, 82);
                assert!(rows[0].lossy);
            }
        }
        assert!(w.drain_minutes_at(180, 1000).is_empty());
    }
    #[test]
    fn second_ticks_keep_live_history_and_mapping_loss_expires() {
        use tidemark_live::{alerts::AlertSource, watch::HotScope};
        for errno in [None, Some(9), Some(116)] {
            let mut cfg = Config::default();
            cfg.alerts.rule = vec![rule(AlertKind::HotDirGrowth, "1m")];
            let state = LiveState::new(&cfg, 0);
            let w = Arc::new(
                Watcher::from_injected_events(
                    RawPath::new(b"/data".to_vec()).unwrap(),
                    [
                        (
                            0,
                            [b"/data/a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                            11,
                            None,
                            (1, 1),
                        ),
                        (
                            60,
                            [b"/data/a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                            71,
                            None,
                            (1, 2),
                        ),
                    ],
                )
                .unwrap(),
            );
            let mut history = MinuteHistory::for_watcher(w.clone());
            let (tx, rx) = crossbeam_channel::bounded(64);
            for at in 60..120 {
                publish_tick("/data", &w, &mut history, &cfg, &state, &tx, (false, at)).unwrap();
            }
            let before = w.hot_at(120, 5, HotScope::Dir, 1000);
            publish_tick("/data", &w, &mut history, &cfg, &state, &tx, (false, 120)).unwrap();
            for scope in [HotScope::Dir, HotScope::Unit, HotScope::Comm] {
                for window in [5, 15, 60] {
                    assert_eq!(w.hot_at(120, window, scope, 1000)[0].bytes_delta, 82);
                }
            }
            assert_eq!(before, w.hot_at(120, 5, HotScope::Dir, 1000));
            assert!(state
                .view_at(120)
                .hot_dirs("/data", Duration::from_secs(60))
                .is_some());
            let delivered: usize = rx
                .try_iter()
                .map(|m| {
                    if let super::super::tswriter::Message::Hot(rows) = m {
                        rows.len()
                    } else {
                        0
                    }
                })
                .sum();
            assert_eq!(delivered, 3);
            publish_tick("/data", &w, &mut history, &cfg, &state, &tx, (false, 121)).unwrap();
            assert!(rx.is_empty());
            // A subsequent full minute contains an unmapped event.
            w.inject_mapping_failure_at(121, errno);
            publish_tick("/data", &w, &mut history, &cfg, &state, &tx, (false, 180)).unwrap();
            assert_eq!(
                watcher_component_at("/data", &w, false, 1, 180).state,
                "degraded"
            );
            assert!(state
                .view_at(180)
                .hot_dirs("/data", Duration::from_secs(60))
                .is_none());
            assert!(w.health_detail_at(180).contains("unmapped events: 1"));
            for at in 181..=17 * 60 {
                publish_tick("/data", &w, &mut history, &cfg, &state, &tx, (false, at)).unwrap();
            }
            assert_eq!(
                watcher_component_at("/data", &w, false, 1, 17 * 60).state,
                "ok"
            );
            assert!(w.health_detail_at(17 * 60).is_empty());
            assert_eq!(w.unmapped_events(), 1);
            assert!(state
                .view_at(17 * 60)
                .hot_dirs("/data", Duration::from_secs(60))
                .is_some());
        }
    }
    #[test]
    fn namespace_failures_name_the_cause_without_debug_results() {
        assert_eq!(
            super::namespace_detail(&Ok(false)),
            "host mount namespace check failed: process is outside the host mount namespace"
        );
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::Other,
        ] {
            let result = Err(std::io::Error::new(kind, "namespace metadata unavailable"));
            assert_eq!(
                super::namespace_detail(&result),
                "host mount namespace check failed: namespace metadata unavailable"
            );
        }
        assert_eq!(
            super::namespace_detail(&Ok(true)),
            "host mount namespace check passed"
        );
    }
    #[test]
    fn history_and_window_views_share_watcher_byte_admission_and_release() {
        let mut cfg = Config::default();
        cfg.alerts.rule = vec![rule(AlertKind::HotDirGrowth, "1m")];
        let watcher = Arc::new(
            Watcher::from_injected_events(RawPath::new(b"/data".to_vec()).unwrap(), []).unwrap(),
        );
        let baseline = watcher.retained_key_bytes();
        let state = LiveState::new(&cfg, 0);
        state
            .publish(
                Publication::Watcher(("/data".into(), watcher.clone())),
                crate::component("watcher:/data", "ok", "fixture"),
            )
            .unwrap();
        let mut history = MinuteHistory::for_watcher(watcher.clone());
        let mut m = minute(60, 7);
        m.dirs[0].key = vec![b'x'; 4096];
        history.push(m);
        let retained = watcher.retained_key_bytes();
        assert!(retained > baseline);
        history.publish("/data", &cfg, &state).unwrap();
        let published = watcher.retained_key_bytes();
        assert!(published > retained);
        let old_view = state.view_at(120);
        assert_eq!(watcher.retained_key_bytes(), published); // immutable window keys are shared
        drop(history);
        state.remove_watcher("/data");
        assert!(watcher.retained_key_bytes() > baseline); // old view owns its window reservation
        watcher.shutdown();
        assert_eq!(watcher.retained_key_bytes(), baseline);
        assert!(tidemark_live::alerts::AlertSource::hot_dirs(
            &old_view,
            "/data",
            Duration::from_secs(60)
        )
        .is_none());
        drop(old_view);
        assert_eq!(watcher.retained_key_bytes(), baseline);
    }
    #[test]
    fn minute_history_byte_cap_preserves_boundary_facts_and_marks_truncation() {
        let watcher = Arc::new(
            Watcher::from_injected_events(RawPath::new(b"/data".to_vec()).unwrap(), []).unwrap(),
        );
        let mut history = MinuteHistory::for_watcher(watcher.clone());
        for n in 0..16 {
            let mut m = minute(n * 60, 7);
            m.dirs[0].key = vec![b'x'; 1024 * 1024];
            history.push(m);
            assert!(history.3 <= HISTORY_BYTES);
            assert!(watcher.retained_memory_charge() <= 64 * 1024 * 1024);
        }
        assert!(history.0.back().unwrap().truncated);
        assert_eq!(history.facts(16), (None, Some(0)));
        let baseline = watcher.retained_key_bytes()
            - history
                .0
                .iter()
                .flat_map(|m| &m.dirs)
                .map(|r| r.key.capacity() + r.top_unit.as_ref().map_or(0, String::capacity))
                .sum::<usize>();
        drop(history);
        assert_eq!(watcher.retained_key_bytes(), baseline);
    }
    #[test]
    fn queued_persistence_messages_have_a_byte_bound_with_long_keys() {
        let watcher =
            Watcher::from_injected_events(RawPath::new(b"/data".to_vec()).unwrap(), []).unwrap();
        let rows: Vec<_> = (0..200)
            .map(|n| tidemark_core::live::HotMinute {
                mount: "/data".into(),
                scope: "dir".into(),
                key: vec![b'x'; 4000],
                ts: 0,
                events: 1,
                files: 1,
                bytes_delta: n,
                lossy: false,
            })
            .collect();
        let (tx, rx) = crossbeam_channel::bounded(super::super::tswriter::CHANNEL_CAPACITY);
        assert!(!enqueue_persistence(&watcher, &tx, rows));
        let mut count = 0;
        for message in rx.try_iter() {
            let super::super::tswriter::Message::Hot(rows) = message else {
                panic!("wrong persistence kind");
            };
            let bytes = rows.capacity() * std::mem::size_of::<tidemark_core::live::HotMinute>()
                + rows
                    .iter()
                    .map(|r| r.key.capacity() + r.mount.capacity() + r.scope.capacity() + 128)
                    .sum::<usize>();
            assert!(bytes <= PERSISTENCE_MESSAGE_BYTES);
            count += rows.len();
        }
        assert_eq!(count, 200);
    }
    #[test]
    fn retained_view_does_not_delay_retirement_before_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let descriptor = rustix::fs::open(
            temp.path(),
            rustix::fs::OFlags::PATH | rustix::fs::OFlags::DIRECTORY,
            rustix::fs::Mode::empty(),
        )
        .unwrap();
        let order = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let recorded = order.clone();
        let watcher = Arc::new(
            Watcher::from_injected_events_with_shutdown_probe(
                RawPath::new(b"/fixture".to_vec()).unwrap(),
                descriptor,
                move |step| recorded.lock().push(step),
            )
            .unwrap(),
        );
        let state = LiveState::new(&Config::default(), 0);
        state
            .publish(
                Publication::Watcher(("/fixture".into(), watcher.clone())),
                crate::component("watcher:/fixture", "ok", "fixture"),
            )
            .unwrap();
        let old_view = state.view_at(0);
        let mut watchers = BTreeMap::from([("/fixture".into(), watcher.clone())]);
        let mut minutes = BTreeMap::from([("/fixture".into(), MinuteHistory::default())]);
        retire_watcher(&state, &mut watchers, &mut minutes, "/fixture");
        assert_eq!(
            watcher.mount_identity().unwrap_err().kind(),
            std::io::ErrorKind::Unsupported
        );
        // This seam occupies the same ordering position as Watcher::start in a pass.
        order.lock().push("replacement starts");
        let replacement =
            Watcher::from_injected_events(RawPath::new(b"/fixture".to_vec()).unwrap(), []).unwrap();
        assert_eq!(
            *order.lock(),
            vec!["worker stopped", "group closed", "replacement starts"]
        );
        assert!(watchers.is_empty() && minutes.is_empty());
        watcher.shutdown();
        assert_eq!(order.lock().len(), 3);
        assert!(old_view.status(0).is_some());
        drop(replacement);
    }
    #[test]
    fn filesystem_mode_detail_is_healthy() {
        let watcher = super::Watcher::from_injected_events_in_mode(
            tidemark_core::path::RawPath::new(b"/fixture".to_vec()).unwrap(),
            [],
            tidemark_live::watch::WatchMode::FdFilesystem,
        )
        .unwrap();
        let component = super::watcher_component("/fixture", &watcher, false, 0);
        assert_eq!(component.state, "ok");
        assert!(component.detail.starts_with("mode=fd/filesystem "));
        assert!(!component.detail.contains("not seen"));
    }
    #[test]
    fn mount_mode_detail_reports_blind_spots() {
        let watcher = super::Watcher::from_injected_events(
            tidemark_core::path::RawPath::new(b"/fixture".to_vec()).unwrap(),
            [],
        )
        .unwrap();
        let component = super::watcher_component("/fixture", &watcher, false, 0);
        assert_eq!(component.state, "ok");
        assert!(component
            .detail
            .ends_with("writes from other mount namespaces and bind mounts are not seen"));
        assert!(!component.detail.contains("unmapped_events"));
    }

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
        state
            .publish_component(crate::component(
                "watcher:/fixture",
                "degraded",
                "retained mount loss",
            ))
            .unwrap();
        recover_coordinator(&state, &Ok(false)).unwrap();
        assert!(state
            .view_at(0)
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "watcher" && c.state == "degraded"));
        recover_coordinator(&state, &Ok(true)).unwrap();
        let components = state.view_at(0).status(0).unwrap().components;
        assert!(components
            .iter()
            .any(|c| c.name == "watcher" && c.state == "ok"));
        assert!(components
            .iter()
            .any(|c| c.name == "watcher:/fixture" && c.state == "degraded"));
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
