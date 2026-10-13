//! Owned by: W6-A1
//! Single bounded time-series writer and persistence barriers.

use tidemark_core::live::*;
/// Persistence work; producers must bound batches by MAX_BATCH_ROWS.
pub enum Message {
    /// Record scan start and acknowledge persistence to the scheduler recorder.
    ScanStarted(
        tidemark_store::catalog::ScanEntry,
        crossbeam_channel::Sender<Result<(), crate::DaemonError>>,
    ),
    /// Record scan completion and acknowledge persistence to the scheduler recorder.
    ScanFinished(
        tidemark_store::catalog::ScanEntry,
        crossbeam_channel::Sender<Result<(), crate::DaemonError>>,
    ),
    #[cfg(test)]
    /// Inject a worker panic for supervision coverage.
    Panic,
    /// Persist fs records.
    Fs(Vec<FsSample>),
    /// Persist lvm records.
    Lvm(Vec<LvmSample>),
    /// Persist hot records.
    Hot(Vec<HotMinute>),
    /// Persist phantom records.
    Phantom(Vec<PhantomSample>),
    /// Persist writers records.
    Writers(Vec<WriterSample>),
    /// Persist alert records.
    Alert(AlertEvent),
    /// Persist dirseries records.
    DirSeries(
        (
            tidemark_core::path::RawPath,
            String,
            i64,
            Vec<tidemark_core::snapshot::DirRow>,
        ),
    ),
    /// Flush queued inserts before acknowledging shutdown.
    Flush(crossbeam_channel::Sender<()>),
}
/// Sender from the writer's CHANNEL_CAPACITY bounded queue.
pub type Sender = crossbeam_channel::Sender<Message>;
/// Start the writer with CHANNEL_CAPACITY queued messages.
pub fn start(
    config: std::sync::Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
) -> Result<(Sender, crate::ServiceHandle), crate::DaemonError> {
    start_with_clock(
        config,
        state,
        shutdown,
        std::sync::Arc::new(crate::unix_now),
    )
}

/// Maximum queued persistence messages. Producers use try_send and report loss on full.
pub const CHANNEL_CAPACITY: usize = 64;
/// Maximum rows accepted in a single message (dir-series permits 20,000).
pub const MAX_BATCH_ROWS: usize = 20_000;
/// Inject wall time while operational intervals use the real monotonic clock.
pub fn start_with_clock(
    config: std::sync::Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
    clock: crate::Clock,
) -> Result<(Sender, crate::ServiceHandle), crate::DaemonError> {
    start_with_clocks(config, state, shutdown, clock, crate::monotonic_clock())
}

/// Start with independent wall and elapsed-seconds clocks.
pub fn start_with_clocks(
    config: std::sync::Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
    clock: crate::Clock,
    monotonic: crate::Clock,
) -> Result<(Sender, crate::ServiceHandle), crate::DaemonError> {
    let path = state.storage_path();
    let catalog = tidemark_store::catalog::Catalog::open(&path, true).map_err(store_error)?;
    let ts = tidemark_store::timeseries::TimeSeries::open(&path.join("timeseries.db"), true)
        .map_err(store_error)?;
    let (sender, receiver) = crossbeam_channel::bounded(CHANNEL_CAPACITY);
    state.publish_component(crate::component("tswriter", "ok", "bounded single writer"))?;
    let handle = std::thread::Builder::new()
        .name("ts-writer".into())
        .spawn(move || {
            writer_loop(
                config,
                state,
                shutdown,
                (clock, monotonic),
                receiver,
                catalog,
                ts,
            )
        })
        .map_err(|source| crate::DaemonError::Io {
            context: "spawn ts-writer".into(),
            source,
        })?;
    Ok((sender, handle))
}
/// Start the lifecycle writer supervisor, retaining the bounded queue across restarts.
pub(crate) fn start_supervised(
    config: std::sync::Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
) -> Result<(Sender, crate::ServiceHandle), crate::DaemonError> {
    start_supervised_with_clocks(
        config,
        state,
        shutdown,
        std::sync::Arc::new(crate::unix_now),
        crate::monotonic_clock(),
    )
}
fn start_supervised_with_clocks(
    config: std::sync::Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
    wall: crate::Clock,
    monotonic: crate::Clock,
) -> Result<(Sender, crate::ServiceHandle), crate::DaemonError> {
    let path = state.storage_path();
    let catalog = tidemark_store::catalog::Catalog::open(&path, true).map_err(store_error)?;
    let ts = tidemark_store::timeseries::TimeSeries::open(&path.join("timeseries.db"), true)
        .map_err(store_error)?;
    let (sender, receiver) = crossbeam_channel::bounded(CHANNEL_CAPACITY);
    state.publish_component(crate::component(
        "tswriter",
        "ok",
        "bounded supervised writer",
    ))?;
    let handle = std::thread::Builder::new()
        .name("ts-supervisor".into())
        .spawn(move || {
            let mut stores = Some((catalog, ts));
            let mut failures = 0u32;
            loop {
                let result = match stores.take() {
                    Some((catalog, ts)) => {
                        let config = config.clone();
                        let state = state.clone();
                        let shutdown = shutdown.clone();
                        let clocks = (wall.clone(), monotonic.clone());
                        let receiver = receiver.clone();
                        match std::thread::Builder::new().name("ts-writer".into()).spawn(
                            move || {
                                writer_loop(config, state, shutdown, clocks, receiver, catalog, ts)
                            },
                        ) {
                            Ok(worker) => worker.join().unwrap_or_else(|panic| {
                                let cause = panic
                                    .downcast_ref::<String>()
                                    .map(String::as_str)
                                    .or_else(|| panic.downcast_ref::<&str>().copied())
                                    .unwrap_or("unknown panic");
                                Err(crate::DaemonError::Internal(format!(
                                    "tswriter panicked: {cause}"
                                )))
                            }),
                            Err(source) => Err(crate::DaemonError::Io {
                                context: "spawn ts-writer".into(),
                                source,
                            }),
                        }
                    }
                    None => Err(crate::DaemonError::Internal(
                        "writer stores unavailable".into(),
                    )),
                };
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    if let Err(error) = &result {
                        state.publish_component(crate::component(
                            "tswriter",
                            "error",
                            error.to_string(),
                        ))?;
                    }
                    return result;
                }
                if state.view().status(wall()).is_some_and(|s| {
                    s.components
                        .iter()
                        .any(|c| c.name == "tswriter" && c.state == "ok")
                }) {
                    failures = 0;
                }
                let cause = result
                    .err()
                    .map_or_else(|| "writer exited unexpectedly".into(), |e| e.to_string());
                state.publish_component(crate::component(
                    "tswriter",
                    "error",
                    format!("persistence lost: {cause}"),
                ))?;
                failures = failures.saturating_add(1);
                loop {
                    let delay = std::time::Duration::from_secs(
                        (5u64 << failures.saturating_sub(1).min(6)).min(300),
                    );
                    let end = monotonic()
                        .saturating_add(i64::try_from(delay.as_secs()).unwrap_or(i64::MAX));
                    while monotonic() < end {
                        if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                            return Ok(());
                        }
                        std::thread::park_timeout(std::time::Duration::from_millis(50));
                    }
                    let reopened =
                        tidemark_store::catalog::Catalog::open(&path, true).and_then(|catalog| {
                            tidemark_store::timeseries::TimeSeries::open(
                                &path.join("timeseries.db"),
                                true,
                            )
                            .map(|ts| (catalog, ts))
                        });
                    match reopened {
                        Ok(pair) => {
                            stores = Some(pair);
                            break;
                        }
                        Err(error) => {
                            state.publish_component(crate::component(
                                "tswriter",
                                "error",
                                format!("persistence lost: {error}"),
                            ))?;
                            failures = failures.saturating_add(1);
                        }
                    }
                }
            }
        })
        .map_err(|source| crate::DaemonError::Io {
            context: "spawn ts-supervisor".into(),
            source,
        })?;
    Ok((sender, handle))
}
fn store_error(error: tidemark_store::StoreError) -> crate::DaemonError {
    if crate::store_pressure(&error) {
        crate::DaemonError::Unavailable(format!("resource pressure: {error}"))
    } else {
        crate::DaemonError::Store(error.to_string())
    }
}
fn writer_pressure(error: &crate::DaemonError) -> bool {
    matches!(error, crate::DaemonError::Unavailable(detail) if detail.starts_with("resource pressure:"))
        || matches!(error, crate::DaemonError::Io { source, .. } if crate::resource_pressure(source))
}
fn writer_loop(
    config: std::sync::Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
    clocks: (crate::Clock, crate::Clock),
    receiver: crossbeam_channel::Receiver<Message>,
    catalog: tidemark_store::catalog::Catalog,
    ts: tidemark_store::timeseries::TimeSeries,
) -> Result<(), crate::DaemonError> {
    use std::{sync::atomic::Ordering, time::Duration};
    let (clock, monotonic) = clocks;
    let mut compact_at = monotonic().saturating_add(3600);
    let mut fs_batch = Vec::new();
    let mut failure = None;
    let mut pressure = false;
    loop {
        let mut recovered = false;
        let message = match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(m) => Some(m),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        if let Some(message) = message {
            let persisted = !matches!(&message, Message::Fs(_) | Message::Flush(_))
                || (!matches!(&message, Message::Fs(_)) && !fs_batch.is_empty());
            let result = match message {
                Message::Fs(rows) => {
                    if rows.len() > MAX_BATCH_ROWS {
                        Err(crate::DaemonError::Invalid("fs batch too large".into()))
                    } else {
                        if fs_batch.len() + rows.len() > MAX_BATCH_ROWS {
                            let result = flush_fs(&ts, &mut fs_batch);
                            recovered = result.is_ok();
                            if result
                                .as_ref()
                                .err()
                                .is_none_or(|error| !writer_pressure(error))
                            {
                                fs_batch.extend(rows);
                            }
                            result
                        } else {
                            fs_batch.extend(rows);
                            Ok(())
                        }
                    }
                }
                message => {
                    let flushed = flush_fs(&ts, &mut fs_batch);
                    match message {
                        Message::Flush(ack) => {
                            if let Err(e) = flushed {
                                pressure = writer_pressure(&e);
                                failure = Some(e.to_string());
                            }
                            // The legacy unit ack is a completion barrier; health carries failures.
                            let _ = ack.try_send(());
                            Ok(())
                        }
                        Message::ScanStarted(entry, ack) => {
                            let r = flushed
                                .and_then(|()| catalog.record_start(&entry).map_err(store_error));
                            let failed = r
                                .as_ref()
                                .err()
                                .map(|error| (error.to_string(), writer_pressure(error)));
                            let _ = ack.try_send(r);
                            failed.map_or(Ok(()), |(s, pressure)| {
                                Err(if pressure {
                                    crate::DaemonError::Unavailable(format!(
                                        "resource pressure: {s}"
                                    ))
                                } else {
                                    crate::DaemonError::Store(s)
                                })
                            })
                        }
                        Message::ScanFinished(entry, ack) => {
                            let r = flushed
                                .and_then(|()| catalog.record_finish(&entry).map_err(store_error));
                            let failed = r
                                .as_ref()
                                .err()
                                .map(|error| (error.to_string(), writer_pressure(error)));
                            let _ = ack.try_send(r);
                            failed.map_or(Ok(()), |(s, pressure)| {
                                Err(if pressure {
                                    crate::DaemonError::Unavailable(format!(
                                        "resource pressure: {s}"
                                    ))
                                } else {
                                    crate::DaemonError::Store(s)
                                })
                            })
                        }
                        other => flushed.and_then(|()| persist(&ts, &state, other)),
                    }
                }
            };
            match result {
                Err(error) => {
                    pressure = writer_pressure(&error);
                    failure = Some(error.to_string());
                }
                Ok(()) => recovered |= persisted,
            }
        }
        if receiver.is_empty() {
            let had_rows = !fs_batch.is_empty();
            match flush_fs(&ts, &mut fs_batch) {
                Err(error) => {
                    pressure = writer_pressure(&error);
                    failure = Some(error.to_string());
                }
                Ok(()) => recovered |= had_rows,
            }
        }
        let now = clock();
        if monotonic() >= compact_at {
            let policy = state.config().0.retention.samples.clone();
            if let Err(error) = ts.compact(now, &policy) {
                pressure = crate::store_pressure(&error);
                failure = Some(error.to_string());
            }
            compact_at = monotonic().saturating_add(3600);
        }
        if let Some(error) = failure.take() {
            tracing::error!(%error,"time-series writer failure");
            state.publish_component(crate::component(
                "tswriter",
                if pressure { "degraded" } else { "error" },
                error,
            ))?;
        } else if recovered {
            state.publish_component(crate::component("tswriter", "ok", "persistence recovered"))?;
        }
        if shutdown.load(Ordering::Acquire) && receiver.is_empty() {
            break;
        }
    }
    if let Err(error) = flush_fs(&ts, &mut fs_batch) {
        if writer_pressure(&error) {
            state.publish_component(crate::component(
                "tswriter",
                "degraded",
                format!("shutdown flush unavailable: {error}"),
            ))?;
        } else {
            return Err(error);
        }
    }
    let _ = config;
    Ok(())
}
fn flush_fs(
    ts: &tidemark_store::timeseries::TimeSeries,
    rows: &mut Vec<FsSample>,
) -> Result<(), crate::DaemonError> {
    if rows.is_empty() {
        return Ok(());
    }
    // The frozen primary key is mount/res/whole-second timestamp. Reloads and
    // quick restarts can repeat a key; keep its first persisted observation.
    let result = (|| {
        let mut keys = std::collections::BTreeSet::new();
        let mut unique = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            if keys.insert((row.mount.clone(), row.ts))
                && ts.query_fs(&row.mount, row.ts, row.ts)?.is_empty()
            {
                unique.push(row.clone());
            }
        }
        ts.insert_fs(&unique)
    })();
    // Retain bounded pending rows only for transient failures. Invalid rows must
    // be dropped so that a later valid batch can recover (existing behavior).
    if result
        .as_ref()
        .err()
        .is_none_or(|error| !crate::store_pressure(error))
    {
        rows.clear();
    }
    result.map_err(store_error)
}
fn persist(
    ts: &tidemark_store::timeseries::TimeSeries,
    state: &crate::state::LiveState,
    message: Message,
) -> Result<(), crate::DaemonError> {
    let too_big = || crate::DaemonError::Invalid("persistence batch exceeds 20,000 rows".into());
    match message {
        #[cfg(test)]
        Message::Panic => panic!("injected writer death"),
        Message::Lvm(rows) => {
            if rows.len() > MAX_BATCH_ROWS {
                return Err(too_big());
            }
            let mut keys = std::collections::BTreeSet::new();
            let mut unique = Vec::with_capacity(rows.len());
            for row in rows {
                if keys.insert((row.vg.clone(), row.lv.clone(), row.ts))
                    && ts
                        .query_lvm(&row.vg, &row.lv, row.ts, row.ts)
                        .map_err(store_error)?
                        .is_empty()
                {
                    unique.push(row);
                }
            }
            ts.insert_lvm(&unique).map_err(store_error)
        }
        Message::Hot(rows) => {
            if rows.len() > MAX_BATCH_ROWS {
                return Err(too_big());
            }
            ts.insert_hot_minutes(&rows).map_err(store_error)
        }
        Message::Phantom(rows) => {
            if rows.len() > MAX_BATCH_ROWS {
                return Err(too_big());
            }
            ts.insert_phantom(&rows).map_err(store_error)
        }
        Message::Writers(rows) => {
            if rows.len() > MAX_BATCH_ROWS {
                return Err(too_big());
            }
            ts.insert_writers(&rows).map_err(store_error)
        }
        Message::Alert(event) => {
            ts.insert_alert(&event).map_err(store_error)?;
            state.publish(
                crate::state::Publication::AlertEvent(event),
                crate::component("tswriter", "ok", "alert persisted"),
            )
        }
        Message::DirSeries((root, id, ts_at, rows)) => {
            if rows.len() > MAX_BATCH_ROWS {
                return Err(too_big());
            }
            ts.insert_dir_series(&root, &id, ts_at, &rows)
                .map_err(store_error)
        }
        _ => Err(crate::DaemonError::Internal(
            "writer barrier dispatched as data".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{atomic::Ordering, Arc},
        time::{Duration, Instant},
    };
    fn sample(ts: i64) -> FsSample {
        FsSample {
            ts,
            mount: "/fixture".into(),
            size: 100,
            used: 10,
            avail: 90,
            inodes: 10,
            inodes_used: 1,
        }
    }
    fn wait(mut predicate: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(15);
        while !predicate() {
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
    }
    #[test]
    fn rollover_error_is_published_and_next_batch_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = tidemark_core::config::Config::default();
        config.general.state_dir = dir.path().into();
        let state = crate::state::LiveState::new(&config, 0);
        let catalog = tidemark_store::catalog::Catalog::open(dir.path(), true).unwrap();
        let ts =
            tidemark_store::timeseries::TimeSeries::open(&dir.path().join("timeseries.db"), true)
                .unwrap();
        let (tx, rx) = crossbeam_channel::bounded(CHANNEL_CAPACITY);
        let mut rows = vec![sample(0); MAX_BATCH_ROWS];
        rows[0].size = u64::MAX;
        tx.send(Message::Fs(rows)).unwrap();
        tx.send(Message::Fs(vec![sample(30_000)])).unwrap();
        let stop = crate::Shutdown::default();
        let handle = {
            let state = state.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                writer_loop(
                    Arc::new(config),
                    state,
                    stop,
                    (Arc::new(|| 0), Arc::new(|| 0)),
                    rx,
                    catalog,
                    ts,
                )
            })
        };
        wait(|| {
            state
                .view()
                .status(0)
                .unwrap()
                .components
                .iter()
                .any(|c| c.name == "tswriter" && c.state == "error")
        });
        let health = state
            .view()
            .status(0)
            .unwrap()
            .components
            .into_iter()
            .find(|c| c.name == "tswriter")
            .unwrap();
        assert_eq!(health.state, "error");
        assert!(health.detail.contains("integer outside storage range"));
        tx.send(Message::Fs(vec![sample(30_001)])).unwrap();
        wait(|| {
            state
                .view()
                .status(0)
                .unwrap()
                .components
                .iter()
                .any(|c| c.name == "tswriter" && c.detail == "persistence recovered")
        });
        stop.store(true, Ordering::Release);
        handle.join().unwrap().unwrap();
        let db =
            tidemark_store::timeseries::TimeSeries::open(&dir.path().join("timeseries.db"), false)
                .unwrap();
        assert_eq!(db.query_fs("/fixture", 30_000, 30_000).unwrap().len(), 1);
    }
    #[test]
    fn supervisor_reports_panic_and_preserves_producer_queue() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = tidemark_core::config::Config::default();
        config.general.state_dir = dir.path().into();
        let state = crate::state::LiveState::new(&config, 0);
        let stop = crate::Shutdown::default();
        let (tx, handle) = start_supervised(Arc::new(config), state.clone(), stop.clone()).unwrap();
        tx.send(Message::Panic).unwrap();
        wait(|| {
            state.view().status(0).unwrap().components.iter().any(|c| {
                c.name == "tswriter"
                    && c.state == "error"
                    && c.detail.contains("injected writer death")
            })
        });
        assert!(!handle.is_finished());
        assert!(!stop.load(Ordering::Acquire));
        state
            .publish(
                crate::state::Publication::Fs(sample(1)),
                crate::component("sampler", "ok", "in-memory sampling continues"),
            )
            .unwrap();
        assert_eq!(state.view().fs_samples().unwrap()[0].ts, 1);
        tx.send(Message::Fs(vec![sample(1)])).unwrap();
        let db =
            tidemark_store::timeseries::TimeSeries::open(&dir.path().join("timeseries.db"), false)
                .unwrap();
        wait(|| !db.query_fs("/fixture", 1, 1).unwrap().is_empty());
        wait(|| {
            state
                .view()
                .status(0)
                .unwrap()
                .components
                .iter()
                .any(|c| c.name == "tswriter" && c.state == "ok")
        });
        stop.store(true, Ordering::Release);
        handle.join().unwrap().unwrap();
    }
}
