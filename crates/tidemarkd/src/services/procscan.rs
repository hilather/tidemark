//! Owned by: W6-A2b
//! Periodic metadata-only procfs collection without scan Gate charging.

use crate::{
    state::{LiveState, Publication},
    DaemonError, ServiceHandle, Shutdown,
};
use parking_lot::Mutex;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tidemark_core::{config::Config, path::RawPath};
use tidemark_live::procscan::{ProcScanner, ProcScannerOptions};

fn scanner(config: &Config) -> Result<ProcScanner, DaemonError> {
    let table = tidemark_fs::mount::read_mountinfo().map_err(|source| DaemonError::Io {
        context: "procscan mount table".into(),
        source,
    })?;
    let mut mounts = Vec::new();
    for path in &config.watch.mounts {
        let raw = RawPath::from_path(path).map_err(|e| DaemonError::Config(e.to_string()))?;
        let mount = table
            .iter()
            .rev()
            .find(|m| m.mount_point == raw)
            .ok_or_else(|| {
                DaemonError::Unavailable(format!("{} is not a mount", raw.display_lossy()))
            })?;
        if !mounts.iter().any(|(_, dev)| *dev == mount.dev) {
            mounts.push((raw, mount.dev));
        }
    }
    let exclude_prefixes = config
        .procscan
        .exclude_prefixes
        .iter()
        .map(|p| RawPath::from_path(p))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DaemonError::Config(e.to_string()))?;
    Ok(ProcScanner::new(
        ProcScannerOptions {
            mounts,
            open_writers: config.procscan.open_writers,
            exclude_prefixes,
        },
        Arc::new(tidemark_fs::attribution::AttributionCache::new(
            Duration::from_secs(60),
            10_000,
        )),
    ))
}

/// Start immediate procfs sampling and observe interval, exclusions and mount reloads.
pub fn start(
    config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
) -> Result<ServiceHandle, DaemonError> {
    crate::state::validate_bounds(&config)?;
    std::thread::Builder::new().name("procscan".into()).spawn(move || {
        let mut previous = None;
        let mut collector = None;
        while !shutdown.load(Ordering::Acquire) {
            let (config, generation) = state.config();
            let key = (
                config.procscan.enabled,
                config.procscan.open_writers,
                config.procscan.exclude_prefixes.clone(),
                config.watch.mounts.clone(),
            );
            if !config.procscan.enabled {
                collector = None;
                state.publish_component(crate::component("procscan", "disabled", "disabled by config"))?;
            } else {
                if previous.as_ref() != Some(&key) || collector.is_none() {
                    collector = None;
                    match scanner(&config) {
                        Ok(s) => collector = Some(Arc::new(Mutex::new(s))),
                        Err(e) => { state.publish_component(crate::component("procscan", "error", e.to_string()))?; }
                    }
                }
                if let Some(s) = &collector {
                    let mut guard = s.lock();
                    match guard.scan_once(crate::unix_now()) {
                        Ok(result) => {
                            let (phantom, writers) = guard.take_samples();
                            drop(guard);
                            let mut dropped = false;
                            for rows in phantom.chunks(super::tswriter::MAX_BATCH_ROWS) { dropped |= writer.try_send(super::tswriter::Message::Phantom(rows.to_vec())).is_err(); }
                            for rows in writers.chunks(super::tswriter::MAX_BATCH_ROWS) { dropped |= writer.try_send(super::tswriter::Message::Writers(rows.to_vec())).is_err(); }
                            state.publish(Publication::Procscan(s.clone()), crate::component("procscan", if dropped || result.writers_full { "degraded" } else { "ok" }, format!("fds_seen={} writers_full={} writer_evictions={} persistence_dropped={dropped}", result.fds_seen, result.writers_full, result.writer_evictions)))?;
                        }
                        Err(e) => { drop(guard); state.publish_component(crate::component("procscan", "error", e.to_string()))?; }
                    }
                }
            }
            previous = Some(key);
            crate::wait_service(&state, generation, &shutdown, config.procscan.interval);
        }
        state.publish_component(crate::component("procscan", "stopped", "shutdown"))?;
        Ok(())
    }).map_err(|source| DaemonError::Io { context: "spawn procscan".into(), source })
}
