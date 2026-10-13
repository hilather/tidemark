//! Owned by: W6-A2b
//! Mount watcher lifecycle and persistence of per-minute top-1,000 lists.
//!
//! Ranked minute lists are not a complete inventory. Empty minutes and exact
//! boundary overflow counters are unavailable in the collector API, so this
//! service does not fabricate window observations from those lists.

use crate::{
    state::{LiveState, Publication},
    DaemonError, ServiceHandle, Shutdown,
};
use std::{
    collections::BTreeMap,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tidemark_core::{config::Config, path::RawPath};
use tidemark_live::watch::{WatchMode, Watcher, WatcherOptions};

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
    std::thread::Builder::new().name("watcher".into()).spawn(move || {
        let mut previous = None;
        let mut identities = BTreeMap::new();
        let mut watchers: BTreeMap<String, Arc<Watcher>> = BTreeMap::new();
        let attribution = Arc::new(tidemark_fs::attribution::AttributionCache::new(Duration::from_secs(60), 10_000));
        let result = (|| {
            while !shutdown.load(Ordering::Acquire) {
                let (config, generation) = state.config();
                if previous.as_ref() != Some(&config.watch) {
                    for mount in watchers.keys() { state.remove_watcher(mount); }
                    watchers.clear();
                    identities.retain(|path, _| config.watch.mounts.contains(path));
                    previous = Some(config.watch.clone());
                }
                if !config.watch.enabled {
                    state.publish_component(crate::component("watcher", "disabled", "disabled by config"))?;
                } else {
                    let mounts = match crate::mounts::read() {
                        Ok(mounts) => mounts,
                        Err(error) => {
                            // An unreadable table cannot authorize retaining a mark.
                            for mount in watchers.keys() { state.remove_watcher(mount); }
                            watchers.clear();
                            state.publish_component(crate::component("watcher", "degraded", error.to_string()))?;
                            crate::wait_service(&state, generation, &shutdown, Duration::from_secs(1));
                            continue;
                        }
                    };
                    let changes = crate::mounts::reconcile_live(&config.watch.mounts, &mut identities, &mounts)?;
                    crate::mounts::log(&changes, "watcher");
                    // Release old marks before any replacement starts.
                    for change in &changes {
                        let mount = RawPath::from_path(&change.path).map_err(|e| DaemonError::Config(e.to_string()))?.display_lossy();
                        state.remove_watcher(&mount);
                        watchers.remove(&mount);
                    }
                    let namespace = tidemark_live::watch::namespace_ok();
                    if !matches!(namespace, Ok(true)) {
                        for mount in watchers.keys() { state.remove_watcher(mount); }
                        watchers.clear();
                        state.publish_component(crate::component("watcher", "error", format!("host mount namespace check failed: {namespace:?}")))?;
                    }
                    for path in &config.watch.mounts {
                        if shutdown.load(Ordering::Acquire) { break; }
                        let mount = RawPath::from_path(path).map_err(|e| DaemonError::Config(e.to_string()))?.display_lossy();
                        let name = format!("watcher:{mount}");
                        if identities.get(path) == Some(&None) {
                            state.publish_component(crate::component(&name, "degraded", "not mounted"))?;
                            continue;
                        }
                        if !matches!(namespace, Ok(true)) || watchers.contains_key(&mount) { continue; }
                        match Watcher::start(options(&config, path)?, attribution.clone()) {
                            Ok(w) => {
                                let w = Arc::new(w);
                                state.publish(Publication::Watcher((mount.clone(), w.clone())), crate::component(&name, "ok", "watcher started"))?;
                                watchers.insert(mount, w);
                            }
                            Err(e) => { state.publish_component(crate::component(&name, "error", e.to_string()))?; }
                        }
                    }
                    if config.watch.mounts.is_empty() {
                        state.publish_component(crate::component("watcher", "disabled", "no watch mounts"))?;
                    }
                }
                for (mount, w) in &watchers {
                    let rows = w.drain_completed_minutes(1000);
                    let mut dropped = false;
                    // Persist top 50 for each minute/scope, retaining the collector's rank.
                    let rows = persistence_rows(rows);
                    for rows in rows.chunks(super::tswriter::MAX_BATCH_ROWS) { dropped |= writer.try_send(super::tswriter::Message::Hot(rows.to_vec())).is_err(); }
                    let stats = w.stats();
                    let health = if stats.mode == WatchMode::Failed { "failed" } else if stats.recovering || dropped || stats.lossy_minutes_15m > 0 { "degraded" } else { "ok" };
                    state.publish_component(crate::component(&format!("watcher:{mount}"), health, format!("mode={} recovering={} overflows_total={} read_errors={} persistence_dropped={dropped}; exact minute windows unavailable: collector omits empty-minute/loss records and boundary counters", stats.mode, stats.recovering, stats.overflows_total, stats.read_errors)))?;
                }
                crate::wait_service(&state, generation, &shutdown, Duration::from_secs(1));
            }
            Ok(())
        })();
        // Also release on publication failures: the last Arc drop joins the worker.
        for mount in watchers.keys() { state.remove_watcher(mount); }
        watchers.clear();
        result?;
        state.publish_component(crate::component("watcher", "stopped", "all watcher handles released"))?;
        Ok(())
    }).map_err(|source| DaemonError::Io { context: "spawn watcher".into(), source })
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
    use super::*;
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
