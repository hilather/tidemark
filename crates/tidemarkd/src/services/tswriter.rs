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
/// Injectable-time writer entry point; all other signatures remain compatible.
pub fn start_with_clock(
    config: std::sync::Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
    clock: crate::Clock,
) -> Result<(Sender, crate::ServiceHandle), crate::DaemonError> {
    let path = state.storage_path();
    let catalog = tidemark_store::catalog::Catalog::open(&path, true).map_err(store_error)?;
    let ts = tidemark_store::timeseries::TimeSeries::open(&path.join("timeseries.db"), true)
        .map_err(store_error)?;
    let (sender, receiver) = crossbeam_channel::bounded(CHANNEL_CAPACITY);
    state.publish_component(crate::component("tswriter", "ok", "bounded single writer"))?;
    let handle = std::thread::Builder::new()
        .name("ts-writer".into())
        .spawn(move || writer_loop(config, state, shutdown, clock, receiver, catalog, ts))
        .map_err(|source| crate::DaemonError::Io {
            context: "spawn ts-writer".into(),
            source,
        })?;
    Ok((sender, handle))
}
fn store_error(error: tidemark_store::StoreError) -> crate::DaemonError {
    crate::DaemonError::Store(error.to_string())
}
fn writer_loop(
    config: std::sync::Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
    clock: crate::Clock,
    receiver: crossbeam_channel::Receiver<Message>,
    catalog: tidemark_store::catalog::Catalog,
    ts: tidemark_store::timeseries::TimeSeries,
) -> Result<(), crate::DaemonError> {
    use std::{sync::atomic::Ordering, time::Duration};
    let mut compact_at = clock().saturating_add(3600);
    let mut fs_batch = Vec::new();
    let mut failure = None;
    loop {
        let message = match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(m) => Some(m),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        if let Some(message) = message {
            let result = match message {
                Message::Fs(rows) => {
                    if rows.len() > MAX_BATCH_ROWS {
                        Err(crate::DaemonError::Invalid("fs batch too large".into()))
                    } else {
                        if fs_batch.len() + rows.len() > MAX_BATCH_ROWS {
                            flush_fs(&ts, &mut fs_batch)?;
                        }
                        fs_batch.extend(rows);
                        Ok(())
                    }
                }
                message => {
                    let flushed = flush_fs(&ts, &mut fs_batch);
                    match message {
                        Message::Flush(ack) => {
                            if let Err(e) = flushed {
                                failure = Some(e.to_string());
                            }
                            // The legacy unit ack is a completion barrier; health carries failures.
                            let _ = ack.try_send(());
                            Ok(())
                        }
                        Message::ScanStarted(entry, ack) => {
                            let r = flushed
                                .and_then(|()| catalog.record_start(&entry).map_err(store_error));
                            let failed = r.as_ref().err().map(ToString::to_string);
                            let _ = ack.try_send(r);
                            failed.map_or(Ok(()), |s| Err(crate::DaemonError::Store(s)))
                        }
                        Message::ScanFinished(entry, ack) => {
                            let r = flushed
                                .and_then(|()| catalog.record_finish(&entry).map_err(store_error));
                            let failed = r.as_ref().err().map(ToString::to_string);
                            let _ = ack.try_send(r);
                            failed.map_or(Ok(()), |s| Err(crate::DaemonError::Store(s)))
                        }
                        other => flushed.and_then(|()| persist(&ts, &state, other)),
                    }
                }
            };
            if let Err(error) = result {
                failure = Some(error.to_string());
            }
        }
        if receiver.is_empty() {
            if let Err(error) = flush_fs(&ts, &mut fs_batch) {
                failure = Some(error.to_string());
            }
        }
        let now = clock();
        if now >= compact_at {
            let policy = state.config().0.retention.samples.clone();
            if let Err(error) = ts.compact(now, &policy) {
                failure = Some(error.to_string());
            }
            compact_at = now.saturating_add(3600);
        }
        if let Some(error) = failure.take() {
            tracing::error!(%error,"time-series writer failure");
            state.publish_component(crate::component("tswriter", "error", error))?;
        }
        if shutdown.load(Ordering::Acquire) && receiver.is_empty() {
            break;
        }
    }
    flush_fs(&ts, &mut fs_batch)?;
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
    let result = ts.insert_fs(rows).map_err(store_error);
    rows.clear();
    result
}
fn persist(
    ts: &tidemark_store::timeseries::TimeSeries,
    state: &crate::state::LiveState,
    message: Message,
) -> Result<(), crate::DaemonError> {
    let too_big = || crate::DaemonError::Invalid("persistence batch exceeds 20,000 rows".into());
    match message {
        Message::Lvm(rows) => {
            if rows.len() > MAX_BATCH_ROWS {
                return Err(too_big());
            }
            ts.insert_lvm(&rows).map_err(store_error)
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
