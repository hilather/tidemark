//! Owned by: W6-A2b
//! Periodic metadata-only procfs collection without scan Gate charging.

use crate::{
    state::{LiveState, Publication},
    DaemonError, ServiceHandle, Shutdown,
};
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
    start_with(
        config,
        state,
        writer,
        shutdown,
        Arc::new(crate::unix_now),
        Box::new(|scanner, now| scanner.scan_once(now)),
    )
}
type ScanSource = Box<
    dyn FnMut(&mut ProcScanner, i64) -> std::io::Result<tidemark_live::procscan::ProcScanResult>
        + Send,
>;
fn start_with(
    config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
    clock: crate::Clock,
    mut scan: ScanSource,
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
                state.publish(Publication::ProcscanAttempt { generation, report: None }, crate::component("procscan", "disabled", "disabled by config"))?;
            } else {
                if previous.as_ref() != Some(&key) || collector.is_none() {
                    collector = None;
                    state.publish(Publication::ProcscanAttempt { generation, report: None }, crate::component("procscan", "degraded", "reconfigured; awaiting completed scan"))?;
                    match scanner(&config) {
                        Ok(s) => collector = Some(s),
                        Err(e) if matches!(&e, DaemonError::Io { source, .. } if crate::resource_pressure(source)) => {
                            state.publish(Publication::ProcscanAttempt { generation, report: None }, crate::component("procscan", "degraded", e.to_string()))?;
                        }
                        Err(e) => { state.publish(Publication::ProcscanAttempt { generation, report: None }, crate::component("procscan", "error", e.to_string()))?; }
                    }
                }
                if let Some(s) = &mut collector {
                    match scan(s, clock()) {
                        Ok(result) => {
                            let (phantom, writers) = s.take_samples();
                            let mut dropped = false;
                            for rows in phantom.chunks(super::tswriter::MAX_BATCH_ROWS) { dropped |= writer.try_send(super::tswriter::Message::Phantom(rows.to_vec())).is_err(); }
                            for rows in writers.chunks(super::tswriter::MAX_BATCH_ROWS) { dropped |= writer.try_send(super::tswriter::Message::Writers(rows.to_vec())).is_err(); }
                            let report = s.report().ok_or_else(|| DaemonError::Invalid("successful process scan has no report".into()))?;
                            state.publish(Publication::ProcscanCompleted { generation, report: Arc::new(report), completed_at: clock() }, crate::component("procscan", if dropped || result.writers_full { "degraded" } else { "ok" }, format!("fds_seen={} writers_full={} writer_evictions={} persistence_dropped={dropped}", result.fds_seen, result.writers_full, result.writer_evictions)))?;
                        }
                        Err(e) if crate::resource_pressure(&e) => {
                            state.publish(Publication::ProcscanAttempt { generation, report: None }, crate::component("procscan", "degraded", e.to_string()))?;
                        }
                        Err(e) => { state.publish(Publication::ProcscanAttempt { generation, report: None }, crate::component("procscan", "error", e.to_string()))?; }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tidemark_core::config::{AlertKind, AlertRule, AlertThreshold};
    use tidemark_live::alerts::{AlertEngine, AlertSource};
    #[test]
    fn emfile_invalidates_process_input_recovery_restarts_continuity_and_report_expires() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("deleted");
        std::fs::write(&file_path, vec![0u8; 8192]).unwrap();
        let _held = std::fs::File::open(&file_path).unwrap();
        std::fs::remove_file(&file_path).unwrap();
        let table = crate::mounts::read().unwrap();
        let raw = RawPath::from_path(dir.path()).unwrap();
        let mount_path = tidemark_fs::mount::mount_for_path(&table, &raw)
            .unwrap()
            .mount_point
            .as_path()
            .to_path_buf();
        let mount = mount_path.to_string_lossy().into_owned();
        let mut config = Config::default();
        config.watch.mounts = vec![mount_path.clone()];
        config.procscan.enabled = true;
        config.procscan.interval = Duration::from_secs(10);
        config.alerts.rule = vec![AlertRule {
            name: "phantom".into(),
            kind: AlertKind::PhantomBytes,
            mount: Some(mount_path),
            above: Some(AlertThreshold::Number(0.)),
            for_: Duration::from_secs(10),
            ..Default::default()
        }];
        let state = LiveState::new(&config, 100);
        let now = Arc::new(std::sync::atomic::AtomicI64::new(100));
        let clock = {
            let now = now.clone();
            Arc::new(move || now.load(Ordering::Acquire))
        };
        let (tx, _rx) = crossbeam_channel::bounded(1);
        tx.try_send(super::super::tswriter::Message::Fs(vec![]))
            .unwrap();
        let (commands, collect_commands) = crossbeam_channel::bounded(1);
        let scan = Box::new(move |scanner: &mut ProcScanner, at| {
            if collect_commands
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            {
                scanner.scan_once(at)
            } else {
                Err(std::io::Error::from_raw_os_error(24))
            }
        });
        let stop = Shutdown::default();
        let worker = start_with(
            Arc::new(config.clone()),
            state.clone(),
            tx.clone(),
            stop.clone(),
            clock,
            scan,
        )
        .unwrap();
        let wait = |predicate: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !predicate() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
        };
        commands.send(true).unwrap();
        wait(&|| state.view_at(100).phantom_bytes(&mount).is_some());
        assert!(state
            .view_at(100)
            .status(100)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "procscan"
                && c.state == "degraded"
                && c.detail.contains("persistence_dropped=true")));
        let mut engine = AlertEngine::new(&config.alerts.rule, config.alerts.renotify).unwrap();
        let mut failures = Default::default();
        super::super::alerts::tick_current(&mut engine, 100, &state, &tx, &mut [], &mut failures)
            .unwrap();
        super::super::alerts::tick_current(&mut engine, 110, &state, &tx, &mut [], &mut failures)
            .unwrap();
        assert_eq!(engine.active().len(), 1);
        let mut pending = AlertEngine::new(&config.alerts.rule, config.alerts.renotify).unwrap();
        let mut pending_failures = Default::default();
        super::super::alerts::tick_current(
            &mut pending,
            110,
            &state,
            &tx,
            &mut [],
            &mut pending_failures,
        )
        .unwrap();
        state.replace_config(config.clone()).unwrap();
        commands.send(false).unwrap();
        wait(&|| state.view_at(110).phantom_bytes(&mount).is_none());
        assert!(state
            .view_at(110)
            .phantom(&tidemark_core::api::PhantomParams { mount: None })
            .is_some());
        assert!(state.view_at(110).procscan_failure_epoch() > failures.2);
        // Reload wakes the long-interval service without changing process options.
        state.replace_config(config.clone()).unwrap();
        now.store(110, Ordering::Release);
        commands.send(true).unwrap();
        wait(&|| state.view_at(110).phantom_bytes(&mount).is_some());
        super::super::alerts::tick_current(&mut engine, 110, &state, &tx, &mut [], &mut failures)
            .unwrap();
        super::super::alerts::tick_current(
            &mut pending,
            120,
            &state,
            &tx,
            &mut [],
            &mut pending_failures,
        )
        .unwrap();
        assert!(
            pending.active().is_empty(),
            "missed failure must restart pending continuity"
        );
        super::super::alerts::tick_current(
            &mut pending,
            130,
            &state,
            &tx,
            &mut [],
            &mut pending_failures,
        )
        .unwrap();
        assert_eq!(pending.active().len(), 1);
        assert!(state.view_at(140).phantom_bytes(&mount).is_some());
        assert!(state.view_at(141).phantom_bytes(&mount).is_none());
        assert!(state
            .view_at(141)
            .phantom(&tidemark_core::api::PhantomParams { mount: None })
            .is_some());
        super::super::alerts::tick_current(&mut engine, 141, &state, &tx, &mut [], &mut failures)
            .unwrap();
        assert!(state
            .view_at(141)
            .alert_diagnostics()
            .unwrap()
            .iter()
            .any(|d| d.reason.contains("phantom measurement unavailable")));
        stop.store(true, Ordering::Release);
        commands.send(true).unwrap();
        worker.join().unwrap().unwrap();
    }
}
