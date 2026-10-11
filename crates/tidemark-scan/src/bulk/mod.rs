//! Owned by: W2-B
//! Three-phase, bounded XFS namespace and inode pipeline.

use crate::agg::{AggError, Aggregator, Prepared};
use rayon::prelude::*;
use std::{
    io,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use std::{os::fd::BorrowedFd, sync::atomic::AtomicBool};
use tidemark_core::scan::Gate;
use tidemark_core::scan::WalkOutput;
use tidemark_fs::walk::WalkConfig;
use tidemark_fs::walk::{walk, WalkError};
use tidemark_sys::xfs::XfsGeometry;
/// Bulk scan configuration.
pub struct BulkParams {
    /// Namespace traversal settings; stat_entries is forced false.
    pub walk: WalkConfig,
    /// Maximum v5 inodes per batch, 1..=65,536.
    pub batch: usize,
    /// AG workers and sorting threads, 1..=64; AG workers are capped at agcount.
    pub threads: usize,
}
/// Bounded-cadence observation of the current bulk phase.
#[derive(Default)]
pub struct BulkProgress {
    /// Current phase.
    pub phase: &'static str,
    /// Directories visited.
    pub dirs: u64,
    /// Namespace entries visited.
    pub entries: u64,
    /// Inodes streamed.
    pub inodes: u64,
    /// Direct errors, including omitted details.
    pub errors: u64,
    /// Observed races.
    pub races: u64,
}
/// Counters for the three bulk phases.
#[derive(Default)]
pub struct BulkStats {
    /// Elapsed monotonic milliseconds for namespace, join/prepare, and names.
    pub phase_ms: [u64; 3],
    /// Directory opens that fell back without O_NOATIME.
    pub noatime_fallbacks: u64,
    /// Namespace child references plus one self-reference per directory.
    pub dents: u64,
    /// All v5 inodes streamed, including those outside the root.
    pub inodes_streamed: u64,
    /// Namespace inode groups matched to a v5 inode.
    pub joined: u64,
    /// Namespace races plus vanished and type-changed join references.
    pub races: u64,
    /// Allocation groups successfully streamed.
    pub ags: u32,
    /// Notable representatives resolved to their smallest byte name.
    pub names_resolved: u64,
    /// Notable representatives absent at the name reread.
    pub names_missing: u64,
}
/// Conservative selection estimate: ceil(inodes * 1.1) * 12 + dirs * 320 + 64 MiB.
/// Arithmetic saturates so an overflow can never authorize bulk mode.
pub fn estimate_memory(used_inodes: u64, dirs_estimate: u64) -> u64 {
    let dents = used_inodes
        .saturating_add(used_inodes / 10)
        .saturating_add(u64::from(!used_inodes.is_multiple_of(10)));
    dents
        .saturating_mul(12)
        .saturating_add(dirs_estimate.saturating_mul(320))
        .saturating_add(64 * 1024 * 1024)
}

/// Walk namespace, join v5 inode attributes, and resolve selected file names.
pub fn run_bulk(
    p: &BulkParams,
    mount_fd: BorrowedFd<'_>,
    geo: &XfsGeometry,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&BulkProgress) + Sync),
    mut agg: Aggregator,
) -> Result<(Prepared, BulkStats), BulkError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(BulkError::Cancelled);
    }
    if geo.ftype != Some(true) {
        return Err(BulkError::Unsupported("XFS ftype=1 required"));
    }
    if p.batch == 0 || p.batch > 65_536 || p.threads == 0 || p.threads > 64 || geo.agcount == 0 {
        return Err(BulkError::BeforePhase2(
            "invalid batch, thread count or geometry".into(),
        ));
    }
    // A local configuration forces namespace mode without altering caller settings.
    let cfg = WalkConfig {
        root: p.walk.root.clone(),
        threads: p.walk.threads,
        stat_entries: false,
        exclude: p.walk.exclude.clone(),
        skip_mounts: p.walk.skip_mounts.clone(),
        big_file_threshold: p.walk.big_file_threshold,
        top_files: p.walk.top_files,
        scan_start: p.walk.scan_start,
        getdents_buf: p.walk.getdents_buf,
        io_idle: p.walk.io_idle,
    };
    let mut stats = BulkStats::default();
    let mut namespace_errors = 0;
    let mut observation = BulkProgress {
        phase: "namespace",
        ..BulkProgress::default()
    };
    let mut last_progress = Instant::now();
    let mut dents = Vec::new();
    let start = Instant::now();
    std::thread::scope(|scope| -> Result<(), BulkError> {
        let (tx, rx) = crossbeam_channel::bounded(2);
        let worker = scope.spawn(|| walk(&cfg, gate, cancel, tx));
        let mut failure = None;
        // Keep draining even after a collector error so the producer cannot deadlock.
        loop {
            let output = match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(output) => output,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    report_due(&observation, progress, &mut last_progress);
                    continue;
                }
            };
            if failure.is_some() {
                continue;
            }
            match output {
                WalkOutput::Ns(ns) => match join::append_namespace(&mut dents, &ns) {
                    Ok(()) => {
                        observation.dirs += 1;
                        observation.entries += u64::from(ns.entries);
                        observation.errors += u64::from(ns.errors_total);
                        stats.races += u64::from(ns.races);
                        agg.add_skeleton(&ns);
                    }
                    Err(e) => failure = Some(e),
                },
                WalkOutput::Gone(g) => {
                    stats.races += 1;
                    agg.add_gone(&g);
                }
                WalkOutput::Dir(_) => failure = Some(BulkError::Limit("unexpected stat record")),
            }
            observation.races = stats.races;
            report_due(&observation, progress, &mut last_progress);
        }
        let result = worker.join().map_err(|_| BulkError::Worker)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(BulkError::Cancelled);
        }
        if let Some(e) = failure {
            return Err(e);
        }
        let summary = result?;
        stats.noatime_fallbacks = summary.noatime_fallbacks;
        namespace_errors = summary.errors;
        Ok(())
    })
    .map_err(before_phase2)?;
    stats.dents = dents.len() as u64;
    stats.phase_ms[0] = millis(start);
    let start = Instant::now();
    let sort_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(p.threads)
        .build()
        .map_err(|_| BulkError::BeforePhase2("sort worker creation failed".into()))?;
    observe_operation(
        || {
            sort_pool.install(|| dents.par_sort_unstable_by_key(|d| d.ino()));
            Ok(())
        },
        &observation,
        progress,
        &mut last_progress,
    )
    .map_err(before_phase2)?;
    drop(sort_pool);
    let (last_start, last_end) = tidemark_sys::xfs::ag_ino_range(geo, geo.agcount - 1);
    if last_start >= last_end || dents.last().is_some_and(|d| d.ino() >= last_end) {
        return Err(BulkError::BeforePhase2("inode outside geometry".into()));
    }
    observation.phase = "bulkstat";
    let next_ag = std::sync::atomic::AtomicU32::new(0);
    std::thread::scope(|scope| -> Result<(), BulkError> {
        let (tx, rx) = crossbeam_channel::bounded(2);
        let workers = p.threads.min(geo.agcount as usize);
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let tx = tx.clone();
            let dents = &dents;
            let next_ag = &next_ag;
            handles.push(scope.spawn(move || -> Result<(), BulkError> {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(BulkError::Cancelled);
                    }
                    let ag = next_ag.fetch_add(1, Ordering::Relaxed);
                    if ag >= geo.agcount {
                        return Ok(());
                    }
                    let (lo, hi) = tidemark_sys::xfs::ag_ino_range(geo, ag);
                    let begin = dents.partition_point(|d| d.ino() < lo);
                    let end = dents.partition_point(|d| d.ino() < hi);
                    let source = join::checked_range(
                        tidemark_sys::xfs::bulkstat(mount_fd, lo, hi, p.batch),
                        lo,
                        hi,
                    );
                    let cluster = (16_384 / geo.inodesize.max(1)).max(1) as usize;
                    let cost = u32::try_from(p.batch.div_ceil(cluster))
                        .map_err(|_| BulkError::Limit("gate cost"))?;
                    join::stream(&dents[begin..end], source, gate, cost, cancel, |batch| {
                        tx.send(batch).map_err(|_| BulkError::Worker)
                    })?;
                }
            }));
        }
        drop(tx);
        loop {
            let batch = match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(batch) => batch,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    report_due(&observation, progress, &mut last_progress);
                    continue;
                }
            };
            stats.inodes_streamed += batch.streamed;
            observation.inodes = stats.inodes_streamed;
            for joined in batch.items {
                stats.joined += u64::from(joined.attrs.is_some());
                stats.races += joined.races.len() as u64;
                joined.dispatch(&mut agg);
            }
            observation.races = stats.races;
            report_due(&observation, progress, &mut last_progress);
        }
        let mut failure = None;
        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => (),
                Ok(Err(e)) => {
                    if failure.is_none() {
                        failure = Some(e);
                    }
                }
                Err(_) => failure = Some(BulkError::Worker),
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(BulkError::Cancelled);
        }
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(())
    })?;
    if cancel.load(Ordering::Relaxed) {
        return Err(BulkError::Cancelled);
    }
    stats.ags = geo.agcount;
    drop(dents);
    let mut prepared = agg.prepare()?;
    prepared.namespace_errors = Some(namespace_errors);
    stats.phase_ms[1] = millis(start);
    let start = Instant::now();
    observation.phase = "names";
    report_due(&observation, progress, &mut last_progress);
    let needed = prepared.needs_names();
    let (resolved, missing) = observe_operation(
        || names::resolve(needed, gate, cancel),
        &observation,
        progress,
        &mut last_progress,
    )?;
    stats.names_resolved = resolved.len() as u64;
    stats.names_missing = missing;
    prepared.set_names(resolved);
    stats.phase_ms[2] = millis(start);
    Ok((prepared, stats))
}

fn report_due(
    observation: &BulkProgress,
    progress: &(dyn Fn(&BulkProgress) + Sync),
    last: &mut Instant,
) {
    if last.elapsed() >= Duration::from_secs(2) {
        progress(observation);
        *last = Instant::now();
    }
}

// Sort and name resolution do not stream counter updates. Keep observing their phase
// from the owner thread while a bounded, joined worker completes the operation.
fn observe_operation<T: Send>(
    operation: impl FnOnce() -> Result<T, BulkError> + Send,
    observation: &BulkProgress,
    progress: &(dyn Fn(&BulkProgress) + Sync),
    last: &mut Instant,
) -> Result<T, BulkError> {
    std::thread::scope(|scope| {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let worker = scope.spawn(move || {
            let _ = tx.send(operation());
        });
        let result = loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => break result,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    break Err(BulkError::Worker)
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    report_due(observation, progress, last)
                }
            }
        };
        worker.join().map_err(|_| BulkError::Worker)?;
        result
    })
}

fn millis(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

mod join;
mod names;

/// Fatal bulk failures; per-entry races remain in the aggregator.
#[derive(Debug, thiserror::Error)]
pub enum BulkError {
    /// Failure before any phase-two bulkstat call, safe to redo in walk mode.
    #[error("before bulk phase 2: {0}")]
    BeforePhase2(String),
    /// Scan was cancelled.
    #[error("Cancelled")]
    Cancelled,
    /// Bulk mode cannot run on this filesystem or kernel.
    #[error("bulk mode unavailable: {0}")]
    Unsupported(&'static str),
    /// A documented resource or representation bound was exceeded.
    #[error("bulk limit: {0}")]
    Limit(&'static str),
    /// Metadata I/O failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Namespace traversal failed.
    #[error(transparent)]
    Walk(#[from] WalkError),
    /// Aggregation failed.
    #[error(transparent)]
    Agg(#[from] AggError),
    /// A worker panicked or its consumer disconnected.
    #[error("bulk worker failed")]
    Worker,
}

fn before_phase2(error: BulkError) -> BulkError {
    match error {
        BulkError::Cancelled | BulkError::Walk(WalkError::Cancelled) => BulkError::Cancelled,
        BulkError::Unsupported(_) => error,
        _ => BulkError::BeforePhase2(error.to_string()),
    }
}
