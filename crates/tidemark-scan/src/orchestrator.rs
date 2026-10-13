//! Owned by: W2-C
//! Scan lifecycle, host lock, bounded collection and snapshot publication.

use crate::{agg::AggSummary, mode::ModeRequest, progress::Progress};
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Duration};
use tidemark_core::{
    config::Config,
    path::RawPath,
    scan::{ScanMode, SnapshotStatus},
    snapshot::DirRow,
};
use tidemark_store::catalog::ScanEntry;
/// Root, mode and output policy for one serialized scan.
pub struct ScanRequest {
    /// Contract field.
    pub root: RawPath,
    /// Contract field.
    pub mode: ModeRequest,
    /// Contract field.
    pub throttle: Option<String>,
    /// Contract field.
    pub threads: Option<usize>,
    /// Contract field.
    pub exclude_extra: Vec<String>,
    /// Contract field.
    pub keep_partial: bool,
    /// Contract field.
    pub annotations: Vec<(RawPath, String, String)>,
}
/// Published snapshot identity, mode, metrics and timing.
pub struct ScanOutcome {
    /// Contract field.
    pub scan_id: String,
    /// Contract field.
    pub file: Option<PathBuf>,
    /// Contract field.
    pub status: SnapshotStatus,
    /// Contract field.
    pub degraded: bool,
    /// Contract field.
    pub mode: ScanMode,
    /// Contract field.
    pub mode_reason: String,
    /// Contract field.
    pub summary: AggSummary,
    /// Contract field.
    pub duration: Duration,
}

/// Lifecycle and series sink supplied by the CLI or daemon.
/// Successful completion calls scan_finished, then dir_series, then retention.
/// The previous snapshot still exists during scan_finished for growth queries.
pub trait ScanRecorder: Send + Sync {
    /// Reserved for W2-C.
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError>;
    /// Called after publication, before series and retention; the predecessor
    /// snapshot remains available for synchronous growth calculation.
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError>;
    /// Reserved for W2-C.
    fn dir_series(
        &self,
        root: &RawPath,
        scan_id: &str,
        ts: i64,
        rows: &[DirRow],
    ) -> Result<(), ScanError>;
}
/// Run one scan under the host lock and publish its snapshot without replacement.
/// Calls scan_finished before dir_series and retention, preserving the previous
/// snapshot for the recorder's growth calculation.
pub fn run_scan(
    cfg: &Config,
    req: &ScanRequest,
    recorder: &dyn ScanRecorder,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&Progress) + Sync),
) -> Result<ScanOutcome, ScanError> {
    run_scan_with_environment(
        cfg,
        req,
        recorder,
        cancel,
        progress,
        &tidemark_core::time::unix_now,
        &|| read_os_release(std::fs::File::open("/etc/os-release")),
    )
}

// Host metadata is separate from scanned regular files; tests replace both seams.
fn run_scan_with_environment(
    cfg: &Config,
    req: &ScanRequest,
    recorder: &dyn ScanRecorder,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&Progress) + Sync),
    clock: &dyn Fn() -> i64,
    os_release: &dyn Fn() -> String,
) -> Result<ScanOutcome, ScanError> {
    use std::{os::fd::AsFd, sync::atomic::Ordering, time::Instant};
    use tidemark_core::config::RootConfig;
    use tidemark_fs::{blockdev, mount, throttle};
    let start = Instant::now();
    let root_cfg = cfg
        .roots
        .iter()
        .find(|r| r.path == req.root.as_path())
        .cloned()
        .unwrap_or_else(RootConfig::default);
    let threads = req.threads.unwrap_or(cfg.scan.threads as usize);
    if !(1..=64).contains(&threads) {
        return Err(ScanError::Configuration("threads must be 1..=64".into()));
    }
    let mut exclude = root_cfg.exclude.clone();
    exclude.extend(req.exclude_extra.clone());
    let mut globs = globset::GlobSetBuilder::new();
    for pattern in &exclude {
        globs
            .add(globset::Glob::new(pattern).map_err(|e| ScanError::Configuration(e.to_string()))?);
    }
    let excludes = globs
        .build()
        .map_err(|e| ScanError::Configuration(e.to_string()))?;
    let _lock = ScanLock::acquire(&cfg.general.state_dir)?;
    let preflight_now = clock();
    tidemark_store::catalog::cleanup_abandoned_scans(&cfg.general.state_dir, preflight_now);
    // The daemon can have a live sample writer: recovery belongs to startup,
    // or to the daemon-absent CLI preflight with all connections closed.
    let catalog = tidemark_store::catalog::Catalog::open(&cfg.general.state_dir, true)?;
    let previous = catalog.latest(&req.root)?;
    let mounts = mount::read_mountinfo()?;
    let mount = mount::mount_for_path(&mounts, &req.root)
        .ok_or_else(|| ScanError::Precondition("scan root mount is unknown".into()))?;
    let profile = req.throttle.as_deref().unwrap_or(&root_cfg.throttle);
    let leaves = blockdev::resolve_leaves(mount.dev).unwrap_or_default();
    let sample_leaves = leaves.clone();
    let scan_gate = throttle::gate_from_config(
        profile,
        &cfg.throttle,
        &leaves,
        Box::new(move || blockdev::read_disk_stats(&sample_leaves)),
    )
    .map_err(|e| ScanError::Precondition(e.to_string()))?;
    with_cancellation(&scan_gate, cancel, || {
        let gate = scan_gate.gate();
        let (root_fd, setup_noatime) = open_directory(&req.root, gate.as_ref(), cancel)?;
        gate.acquire(1);
        check_cancel(cancel)?;
        let root_stat = rustix::fs::fstat(&root_fd)?;
        if root_stat.st_dev != mount.dev {
            return Err(ScanError::Precondition(
                "root mount changed during setup".into(),
            ));
        }
        gate.acquire(1);
        check_cancel(cancel)?;
        let fs = rustix::fs::fstatvfs(&root_fd)?;
        let used = fs.f_files.saturating_sub(fs.f_ffree);
        let dirs_estimate = previous
            .as_ref()
            .and_then(|p| p.t_dirs)
            .unwrap_or(used / 20);
        let estimate = match previous.as_ref().and_then(|p| p.file.as_ref()) {
            Some(file) => std::fs::metadata(file)?.len().saturating_mul(5).div_ceil(4),
            None => dirs_estimate.saturating_mul(300),
        };
        guard_and_prune(
            &cfg.general.state_dir,
            &catalog,
            cfg.retention.snapshots.recent,
            preflight_now,
            || {
                Ok(tidemark_store::space_guard::space_check(
                    &cfg.general.state_dir,
                    estimate,
                    &cfg.retention,
                )?)
            },
        )?;
        gate.acquire(1);
        check_cancel(cancel)?;
        let is_xfs = tidemark_sys::xfs::is_xfs(root_fd.as_fd())?;
        let geometry = if is_xfs {
            gate.acquire(1);
            check_cancel(cancel)?;
            tidemark_sys::xfs::geometry(root_fd.as_fd()).ok()
        } else {
            None
        };
        let v5 = if is_xfs {
            gate.acquire(2);
            check_cancel(cancel)?;
            tidemark_sys::xfs::bulkstat_probe(root_fd.as_fd()).v5
        } else {
            false
        };
        let facts = crate::mode::ModeFacts {
            is_xfs,
            bulkstat_v5: v5,
            cap_sys_admin: effective_sys_admin().unwrap_or(false),
            ftype: geometry.as_ref().and_then(|g| g.ftype),
            root_is_mountpoint: mount.mount_point == req.root,
            prev_root_inodes: previous.as_ref().and_then(|p| p.t_inodes),
            fs_used_inodes: used,
            dirs_estimate,
        };
        let (mut mode, mut reason) =
            crate::mode::choose_mode(&facts, &req.mode, cfg.scan.memory_budget.0);
        if matches!(req.mode, ModeRequest::Bulk) && mode != ScanMode::Bulk {
            return Err(ScanError::Precondition(format!(
                "bulk mode unavailable: {reason}"
            )));
        }
        let mut memory_estimate = if mode == ScanMode::Bulk {
            crate::bulk::estimate_memory(used, dirs_estimate)
        } else {
            dirs_estimate
                .saturating_mul(320)
                .saturating_add(used.saturating_mul(72))
                .saturating_add(64 << 20)
        };
        tracing::info!(
            memory_estimate,
            memory_budget = cfg.scan.memory_budget.0,
            "scan memory admission estimate"
        );
        let capacity = channel_capacity(
            cfg.scan.memory_budget.0,
            threads,
            root_cfg.top_files,
            cfg.scan.getdents_buf.0,
        )?;
        if mode == ScanMode::Bulk && capacity < 2 {
            return Err(ScanError::Resource(
                "memory budget cannot fit the bulk namespace queue; raise scan.memory_budget"
                    .into(),
            ));
        }
        let (_, max_record_bytes) = record_limits(cfg.scan.memory_budget.0, threads)?;
        admit_memory(
            cfg.scan.memory_budget.0,
            memory_estimate,
            threads,
            cfg.scan.getdents_buf.0,
            root_cfg.top_files,
        )?;
        let now = clock();
        let dir = cfg
            .general
            .state_dir
            .join("snapshots")
            .join(tidemark_store::catalog::root_slug(&req.root));
        let id = fresh_id(now, &dir, |s| Ok(catalog.get(s)?.is_some()))?;
        drop(catalog);
        let mut entry = ScanEntry {
            scan_id: id.to_string(),
            root: req.root.clone(),
            mode,
            started_at: now,
            finished_at: None,
            status: "running".into(),
            degraded: false,
            file: None,
            t_disk: None,
            t_inodes: None,
            t_dirs: None,
            errors: None,
            duration_ms: None,
        };
        // Every post-start failure is recorded, including cancellation and writer failure.
        recorded(
            &mut entry,
            recorder,
            || (clock(), milliseconds(start.elapsed())),
            |entry, recorder| {
                // Escape the state path as literal glob bytes; Glob only accepts UTF-8.
                // Non-UTF-8 state directories are handled by an explicit walker mount-style boundary.
                let mut skips = mount::submounts_under(&mounts, &req.root);
                if let Ok(state) = std::fs::canonicalize(&cfg.general.state_dir)
                    .and_then(|p| RawPath::from_path(&p).map_err(std::io::Error::other))
                {
                    if state == req.root {
                        return Err(ScanError::Precondition(
                            "cannot scan the state directory".into(),
                        ));
                    }
                    if state.starts_with(&req.root) {
                        skips.push(state);
                    }
                }
                let mut walk_cfg = tidemark_fs::walk::WalkConfig {
                    root: req.root.clone(),
                    threads,
                    stat_entries: mode == ScanMode::Walk,
                    exclude: excludes,
                    skip_mounts: skips,
                    big_file_threshold: root_cfg.big_file_threshold.0,
                    top_files: root_cfg.top_files,
                    scan_start: now,
                    getdents_buf: usize::try_from(cfg.scan.getdents_buf.0)
                        .map_err(|_| ScanError::Resource("getdents buffer overflow".into()))?,
                    io_idle: true,
                    max_record_bytes,
                };
                let new_agg = || {
                    crate::agg::Aggregator::new(crate::agg::AggConfig {
                        root: req.root.clone(),
                        scan_start: now,
                        top_files: root_cfg.top_files,
                        big_file_threshold: root_cfg.big_file_threshold.0,
                        max_notable: cfg.scan.max_notable,
                        link_instances: cfg.scan.link_instances
                            == tidemark_core::config::LinkInstances::All,
                        max_stored_errors: cfg.scan.max_stored_errors,
                    })
                };
                let mut phases = std::collections::BTreeMap::new();
                let mut walk_totals = None;
                let (mut prepared, noatime, partial) = loop {
                    let agg = new_agg();
                    if mode == ScanMode::Walk {
                        let walk_started = Instant::now();
                        let (mut agg, walk_result) = collect_walk(
                            &walk_cfg,
                            gate.as_ref(),
                            cancel,
                            agg,
                            capacity,
                            progress,
                            previous.as_ref().and_then(|p| p.t_inodes),
                            req.keep_partial,
                        )?;
                        let (noatime, partial) = match walk_result {
                            Ok(s) => {
                                phases.insert("walk".into(), milliseconds(s.elapsed));
                                walk_totals = Some((s.errors, s.races));
                                (s.noatime_fallbacks, false)
                            }
                            Err(e) if req.keep_partial && recoverable_walk_error(&e) => {
                                tracing::warn!(error = %e, "retaining collected walk records as partial");
                                (0, true)
                            }
                            Err(e) => return Err(e.into()),
                        };
                        record_walk_phase(&mut phases, walk_started.elapsed());
                        if partial && agg.is_empty() {
                            // Cancellation can arrive during walker setup, before any record is emitted.
                            // Preserve only the already observed root metadata in that partial snapshot.
                            agg.add_dir_record(tidemark_core::scan::DirRecord {
                                id: tidemark_core::scan::DirId(0),
                                parent: None,
                                name: None,
                                ino: root_stat.st_ino,
                                dev: root_stat.st_dev,
                                mtime: root_stat.st_mtime,
                                dir_disk: u64::try_from(root_stat.st_blocks)
                                    .map_err(|_| {
                                        ScanError::Resource("negative root block count".into())
                                    })?
                                    .saturating_mul(512),
                                dir_apparent: u64::try_from(root_stat.st_size).map_err(|_| {
                                    ScanError::Resource("negative root size".into())
                                })?,
                                projid: None,
                                entries: 0,
                                own: Default::default(),
                                links: vec![],
                                notable: vec![],
                                skipped: vec![],
                                errors: vec![],
                                errors_total: 0,
                                races: 0,
                            });
                        }
                        break (agg.prepare()?, noatime, partial);
                    } else {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(ScanError::Cancelled);
                        }
                        let geo = geometry.as_ref().ok_or_else(|| {
                            ScanError::Precondition("XFS geometry unavailable".into())
                        })?;
                        let params = crate::bulk::BulkParams {
                            walk: walk_cfg.clone(),
                            batch: cfg.scan.bulk_batch as usize,
                            threads,
                        };
                        let result = crate::bulk::run_bulk(
                            &params,
                            root_fd.as_fd(),
                            geo,
                            gate.as_ref(),
                            cancel,
                            &|p| {
                                progress(&Progress {
                                    phase: p.phase,
                                    dirs: p.dirs,
                                    entries: p.entries,
                                    errors: p.errors,
                                    races: p.races,
                                    rate: gate.current_rate(),
                                    elapsed: start.elapsed(),
                                    eta: None,
                                })
                            },
                            agg,
                        );
                        let (prepared, stats) = match result {
                            Ok(value) => value,
                            Err(_) if cancel.load(Ordering::Relaxed) => {
                                return Err(ScanError::Cancelled)
                            }
                            Err(
                                e @ (crate::bulk::BulkError::Unsupported(_)
                                | crate::bulk::BulkError::BeforePhase2(_)),
                            ) if matches!(req.mode, ModeRequest::Auto) => {
                                reason = format!("walk fallback: {e}");
                                tracing::warn!(%reason, "bulk scan fell back to walk");
                                memory_estimate = dirs_estimate
                                    .saturating_mul(320)
                                    .saturating_add(used.saturating_mul(72))
                                    .saturating_add(64 << 20);
                                admit_memory(
                                    cfg.scan.memory_budget.0,
                                    memory_estimate,
                                    threads,
                                    cfg.scan.getdents_buf.0,
                                    root_cfg.top_files,
                                )?;
                                mode = ScanMode::Walk;
                                entry.mode = mode;
                                walk_cfg.stat_entries = true;
                                continue;
                            }
                            Err(crate::bulk::BulkError::Cancelled) => {
                                return Err(ScanError::Cancelled)
                            }
                            Err(e) => return Err(e.into()),
                        };
                        for (name, ms) in ["namespace", "bulkstat", "names"]
                            .into_iter()
                            .zip(stats.phase_ms)
                        {
                            phases.insert(name.into(), ms);
                        }
                        break (prepared, stats.noatime_fallbacks, false);
                    }
                };
                if cancel.load(Ordering::Relaxed) && (mode == ScanMode::Bulk || !req.keep_partial) {
                    return Err(ScanError::Cancelled);
                }
                for (path, key, value) in &req.annotations {
                    prepared.annotate(path, key, value);
                }
                let expected_errors = walk_totals
                    .map(|(errors, _)| errors)
                    .or(prepared.namespace_errors);
                let mut summary = prepared.summary();
                if let Some(errors) = expected_errors {
                    if summary.errors != errors {
                        tracing::warn!(
                            records = summary.errors,
                            walker = errors,
                            "directory error totals disagree with walker summary"
                        );
                        summary.degraded = true;
                    }
                }
                if let Some((_, races)) = walk_totals {
                    summary.races = races;
                }
                let mut status = if partial || cancel.load(Ordering::Relaxed) {
                    SnapshotStatus::Partial
                } else {
                    SnapshotStatus::Complete
                };
                if cancel.load(Ordering::Acquire) {
                    scan_gate.cancel();
                }
                scan_gate.stop_sampling();
                let stats = scan_gate.stats();
                let mut meta = tidemark_core::snapshot::SnapshotMeta {
                    schema_version: 1,
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                    scan_id: id,
                    root: req.root.clone(),
                    root_display: req.root.display_lossy(),
                    mode,
                    mode_reason: reason.clone(),
                    status,
                    degraded: summary.degraded,
                    exclude,
                    link_instances: cfg.scan.link_instances,
                    started_at: now,
                    finished_at: now,
                    host: rustix::system::uname()
                        .nodename()
                        .to_string_lossy()
                        .into_owned(),
                    kernel: rustix::system::uname()
                        .release()
                        .to_string_lossy()
                        .into_owned(),
                    os_release: os_release(),
                    fs_uuid: geometry
                        .as_ref()
                        .map(|g| g.uuid.iter().map(|b| format!("{b:02x}")).collect())
                        .unwrap_or_default(),
                    fs_dev: root_stat.st_dev,
                    fs_blocksize: fs.f_bsize,
                    threads: threads as u32,
                    throttle_profile: profile.into(),
                    dirs: summary.dirs,
                    files: summary.files,
                    inodes: summary.inodes,
                    errors: summary.errors,
                    races: summary.races,
                    skipped: summary.skipped,
                    noatime_fallbacks: noatime + setup_noatime,
                    phase_ms: phases,
                    gate_stats: stats,
                };
                let catalog =
                    tidemark_store::catalog::Catalog::open(&cfg.general.state_dir, false)?;
                let mut writer = create_writer(&dir, &catalog, &mut meta, entry, recorder)?;
                drop(catalog);
                let state_dir = cfg.general.state_dir.clone();
                let policy = cfg.retention.clone();
                writer.space_check_every(
                    100_000,
                    Box::new(move || {
                        tidemark_store::space_guard::space_check(&state_dir, 0, &policy)
                            .unwrap_or_else(|e| tidemark_store::space_guard::SpaceVerdict::Refuse {
                                reason: e.to_string(),
                            })
                    }),
                );
                let mut sink = WriterSink {
                    writer: &mut writer,
                    series: std::collections::BinaryHeap::new(),
                    depth: root_cfg.series_depth,
                    cancel,
                    keep_partial: req.keep_partial && mode == ScanMode::Walk,
                };
                let writing = Instant::now();
                let written = prepared.write(&mut sink);
                if cancel.load(Ordering::Acquire) && !sink.keep_partial {
                    return Err(ScanError::Cancelled);
                }
                let mut summary = written?;
                if let Some(errors) = expected_errors {
                    if summary.errors != errors {
                        tracing::warn!(
                            records = summary.errors,
                            walker = errors,
                            "directory error totals disagree with walker summary"
                        );
                        summary.degraded = true;
                    }
                }
                if let Some((_, races)) = walk_totals {
                    summary.races = races;
                }
                meta.phase_ms
                    .insert("write".into(), milliseconds(writing.elapsed()));
                let series: Vec<_> = sink
                    .series
                    .into_sorted_vec()
                    .into_iter()
                    .map(|r| r.0 .0)
                    .collect();
                status = publication_status(status, mode, req.keep_partial, cancel)?;
                meta.status = status;
                // Rows are complete. The consuming writer still owns index/sync timing;
                // its frozen API cannot refresh metadata after those operations.
                meta.finished_at = clock();
                let file = finish_snapshot(writer, &meta, &|| cancel.load(Ordering::Acquire))?;
                entry.status = if status == SnapshotStatus::Partial {
                    "partial"
                } else {
                    "complete"
                }
                .into();
                entry.finished_at = Some(meta.finished_at);
                entry.degraded = summary.degraded;
                entry.file = Some(file.clone());
                entry.t_disk = Some(summary.t_disk);
                entry.t_inodes = Some(summary.inodes);
                entry.t_dirs = Some(summary.dirs);
                entry.errors = Some(summary.errors);
                entry.duration_ms = Some(milliseconds(start.elapsed()));
                recorder.scan_finished(entry)?;
                recorder.dir_series(&req.root, &entry.scan_id, now, &series)?;
                let catalog = tidemark_store::catalog::Catalog::open(&cfg.general.state_dir, true)?;
                let entries = catalog.list(Some(&req.root))?;
                for old in tidemark_store::retention::retention_plan(
                    &entries,
                    &cfg.retention.snapshots,
                    meta.finished_at,
                ) {
                    if let Some(path) = entries
                        .iter()
                        .find(|e| e.scan_id == old)
                        .and_then(|e| e.file.as_ref())
                    {
                        delete_snapshot(&cfg.general.state_dir, path)?;
                    }
                }
                catalog.rebuild_after_scan()?;
                tracing::info!(
                    memory_estimate,
                    peak_rss_bytes = observed_peak_rss(),
                    "scan memory observation"
                );
                Ok(ScanOutcome {
                    scan_id: entry.scan_id.clone(),
                    file: Some(file),
                    status,
                    degraded: summary.degraded,
                    mode,
                    mode_reason: reason,
                    summary,
                    duration: start.elapsed(),
                })
            },
        )
    })
}

/// Typed scan lifecycle failures.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// Another snapshot already owns the publication path; no retry is performed.
    #[error("snapshot publication collision; existing snapshot preserved")]
    PublicationCollision,
    /// Filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Safe syscall failed.
    #[error(transparent)]
    Sys(#[from] rustix::io::Errno),
    /// Core validation failed.
    #[error(transparent)]
    Core(#[from] tidemark_core::error::CoreError),
    /// Storage operation failed.
    #[error(transparent)]
    Store(#[from] tidemark_store::StoreError),
    /// Aggregation failed.
    #[error(transparent)]
    Agg(#[from] crate::agg::AggError),
    /// Walk failed.
    #[error(transparent)]
    Walk(#[from] tidemark_fs::walk::WalkError),
    /// Bulk pipeline failed.
    #[error(transparent)]
    Bulk(#[from] crate::bulk::BulkError),
    /// Invalid usage or configuration.
    #[error("{0}")]
    Configuration(String),
    /// Unsupported scan precondition.
    #[error("{0}")]
    Precondition(String),
    /// Memory settings cannot fit bounded queues.
    #[error("{0}")]
    Resource(String),
    /// Cancellation requested.
    #[error("scan cancelled")]
    Cancelled,
    /// Host scan lock is held.
    #[error("Busy")]
    Busy,
    /// State space refused.
    #[error("NoSpace")]
    NoSpace,
}

/// Exclusive nonblocking host scan lock, released by closing its descriptor.
pub struct ScanLock {
    _file: std::fs::File,
}
impl ScanLock {
    /// Open or recover shared storage while this scan ownership guard is retained.
    pub fn open_catalog(
        &self,
        state: &std::path::Path,
    ) -> Result<tidemark_store::catalog::Catalog, ScanError> {
        use std::os::unix::fs::MetadataExt;
        let held = self._file.metadata()?;
        let named = std::fs::symlink_metadata(state.join("scan.lock"))?;
        if held.dev() != named.dev() || held.ino() != named.ino() {
            return Err(ScanError::Busy);
        }
        match tidemark_store::catalog::Catalog::open(state, true) {
            Ok(catalog) => Ok(catalog),
            Err(tidemark_store::StoreError::Sql(ref error))
                if error
                    .sqlite_error()
                    .is_some_and(|e| matches!(e.extended_code & 255, 11 | 26)) =>
            {
                Ok(tidemark_store::catalog::recover_catalog_with_lock(
                    state,
                    &self._file,
                )?)
            }
            Err(error) => Err(error.into()),
        }
    }
    /// Create only inside state_dir and refuse a concurrent scan.
    pub fn acquire(state: &std::path::Path) -> Result<Self, ScanError> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(state)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(state.join("scan.lock"))?;
        match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(Self { _file: file }),
            Err(rustix::io::Errno::WOULDBLOCK) => Err(ScanError::Busy),
            Err(e) => Err(e.into()),
        }
    }
}
/// Open shared storage, acquiring scan ownership for writable opens and recovery.
/// Callers already holding a scan lock use `ScanLock::open_catalog` instead.
pub fn open_catalog(
    state: &std::path::Path,
    rw: bool,
) -> Result<tidemark_store::catalog::Catalog, ScanError> {
    if rw {
        ScanLock::acquire(state)?.open_catalog(state)
    } else {
        Ok(tidemark_store::catalog::Catalog::open(state, false)?)
    }
}
fn open_directory(
    path: &RawPath,
    gate: &dyn tidemark_core::scan::Gate,
    cancel: &AtomicBool,
) -> Result<(rustix::fd::OwnedFd, u64), ScanError> {
    let mut fallbacks = 0;
    let result = tidemark_fs::walk::open_directory(path, gate, &mut fallbacks, cancel);
    check_cancel(cancel)?;
    Ok((result?, fallbacks))
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), ScanError> {
    if cancel.load(std::sync::atomic::Ordering::Acquire) {
        Err(ScanError::Cancelled)
    } else {
        Ok(())
    }
}

// A scoped monitor covers preflight and every pipeline phase, including joins.
// Disconnecting the bounded channel wakes it immediately on success, error or panic.
fn with_cancellation<T>(
    gate: &tidemark_fs::throttle::ScanGate,
    cancel: &AtomicBool,
    work: impl FnOnce() -> T,
) -> T {
    std::thread::scope(|scope| {
        let (done, rx) = crossbeam_channel::bounded::<()>(1);
        scope.spawn(move || loop {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                gate.cancel();
                break;
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                _ => break,
            }
        });
        let result = work();
        drop(done);
        result
    })
}
fn effective_sys_admin() -> std::io::Result<bool> {
    use std::io::Read;
    // Capability metadata from procfs, never contents from the scanned tree.
    let file = std::fs::File::open("/proc/thread-self/status")?;
    let mut text = String::new();
    file.take(65_537).read_to_string(&mut text)?;
    if text.len() > 65_536 {
        return Err(std::io::Error::other("oversized capability metadata"));
    }
    let bits = text
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:"))
        .and_then(|s| u64::from_str_radix(s.trim(), 16).ok())
        .ok_or_else(|| std::io::Error::other("effective capabilities unavailable"))?;
    Ok(bits & (1 << 21) != 0)
}

fn milliseconds(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}
fn fresh_id(
    now: i64,
    dir: &std::path::Path,
    mut occupied: impl FnMut(&str) -> Result<bool, ScanError>,
) -> Result<tidemark_core::time::ScanId, ScanError> {
    for _ in 0..1024 {
        let id = tidemark_core::time::ScanId::new(now)?;
        if !dir.join(format!("{id}.db")).try_exists()?
            && !dir.join(format!("{id}.db.tmp")).try_exists()?
            && !occupied(&id.to_string())?
        {
            return Ok(id);
        }
    }
    Err(ScanError::Resource(
        "scan id collision retry limit reached".into(),
    ))
}
/// Derive bounded queued and in-flight record storage from one quarter of the budget.
/// Capacity is reduced from 64 to 1 before the record bound falls below 16 MiB.
/// Refuse when the quarter cannot fit 16 MiB per worker even without the queue.
/// `(threads + capacity) * max_record_bytes <= budget / 4`; the remaining
/// three quarters must also fit the aggregator estimate and worker buffers.
pub fn record_limits(budget: u64, threads: usize) -> Result<(usize, u64), ScanError> {
    const MIN_RECORD: u64 = 16 << 20;
    if !(1..=64).contains(&threads) {
        return Err(ScanError::Resource("threads must be 1..=64".into()));
    }
    let quarter = budget / 4;
    let slots = quarter / MIN_RECORD;
    if slots < threads as u64 {
        return Err(ScanError::Resource(
            "memory budget cannot fit 16 MiB per worker; raise scan.memory_budget".into(),
        ));
    }
    let capacity = (slots - threads as u64).clamp(1, 64) as usize;
    Ok((capacity, quarter / (threads + capacity) as u64))
}
/// Compute queue capacity, reserving buffers and worker candidate heaps separately.
pub fn channel_capacity(
    budget: u64,
    threads: usize,
    top: usize,
    buffer: u64,
) -> Result<usize, ScanError> {
    let (capacity, _) = record_limits(budget, threads)?;
    let workers = threads as u128 * (u128::from(buffer) + top as u128 * 16);
    if workers + (64u128 << 20) > u128::from(budget - budget / 4) {
        return Err(ScanError::Resource(
            "memory budget cannot fit worker buffers and scan state".into(),
        ));
    }
    Ok(capacity)
}
#[allow(clippy::too_many_arguments)]
fn collect_walk(
    cfg: &tidemark_fs::walk::WalkConfig,
    gate: &dyn tidemark_core::scan::Gate,
    cancel: &AtomicBool,
    mut agg: crate::agg::Aggregator,
    capacity: usize,
    progress: &(dyn Fn(&Progress) + Sync),
    previous: Option<u64>,
    keep: bool,
) -> Result<
    (
        crate::agg::Aggregator,
        Result<tidemark_fs::walk::WalkSummary, tidemark_fs::walk::WalkError>,
    ),
    ScanError,
> {
    use std::{sync::atomic::Ordering, time::Instant};
    use tidemark_core::scan::WalkOutput;
    std::thread::scope(|scope| {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        let walker = scope.spawn(move || tidemark_fs::walk::walk(cfg, gate, cancel, tx));
        let collector = scope.spawn(move || {
            let start = Instant::now();
            let mut cadence = crate::progress::Cadence::new(Duration::from_secs(2));
            let mut counts = Progress {
                phase: "walk",
                dirs: 0,
                entries: 0,
                errors: 0,
                races: 0,
                rate: 0.0,
                elapsed: Duration::ZERO,
                eta: None,
            };
            loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(WalkOutput::Dir(rec)) => {
                        counts.dirs += 1;
                        counts.entries += u64::from(rec.entries);
                        counts.errors += u64::from(rec.errors_total);
                        counts.races += u64::from(rec.races);
                        agg.add_dir_record(rec);
                    }
                    Ok(WalkOutput::Gone(g)) => {
                        counts.races += 1;
                        agg.add_gone(&g);
                    }
                    Ok(WalkOutput::Ns(_)) => {
                        return Err(ScanError::Precondition(
                            "namespace record in walk mode".into(),
                        ))
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                }
                // Always drain while cancelled to release bounded producers.
                if cadence.due(start.elapsed()) {
                    counts.elapsed = start.elapsed();
                    counts.rate = gate.current_rate();
                    counts.eta =
                        crate::progress::estimate_eta(counts.entries, previous, counts.elapsed);
                    progress(&counts);
                }
            }
            if cancel.load(Ordering::Relaxed) && !keep {
                return Err(ScanError::Cancelled);
            }
            Ok(agg)
        });
        let walk_result = walker
            .join()
            .map_err(|_| ScanError::Resource("walker thread panicked".into()))?;
        let agg = collector
            .join()
            .map_err(|_| ScanError::Resource("collector thread panicked".into()))??;
        Ok((agg, walk_result))
    })
}
struct SeriesRow(DirRow);
impl PartialEq for SeriesRow {
    fn eq(&self, other: &Self) -> bool {
        self.0.t_disk == other.0.t_disk && self.0.path == other.0.path
    }
}
impl Eq for SeriesRow {}
impl Ord for SeriesRow {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .t_disk
            .cmp(&other.0.t_disk)
            .then_with(|| other.0.path.cmp(&self.0.path))
    }
}
impl PartialOrd for SeriesRow {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
struct WriterSink<'a> {
    writer: &'a mut tidemark_store::writer::SnapshotWriter,
    series: std::collections::BinaryHeap<std::cmp::Reverse<SeriesRow>>,
    depth: u32,
    cancel: &'a AtomicBool,
    keep_partial: bool,
}
impl WriterSink<'_> {
    fn check_cancel(&self) -> Result<(), crate::agg::SinkError> {
        if self.cancel.load(std::sync::atomic::Ordering::Acquire) && !self.keep_partial {
            return Err(crate::agg::SinkError::Other("scan cancelled".into()));
        }
        Ok(())
    }
}
impl crate::agg::RowSink for WriterSink<'_> {
    fn dirs(&mut self, rows: &[DirRow]) -> Result<(), crate::agg::SinkError> {
        self.check_cancel()?;
        self.writer.put_dirs(rows)?;
        for row in rows.iter().filter(|r| r.depth <= self.depth) {
            if self.series.len() < 20_000 {
                self.series.push(std::cmp::Reverse(SeriesRow(row.clone())));
            } else if self.series.peek().is_some_and(|s| {
                row.t_disk > s.0 .0.t_disk
                    || (row.t_disk == s.0 .0.t_disk && row.path < s.0 .0.path)
            }) {
                self.series.pop();
                self.series.push(std::cmp::Reverse(SeriesRow(row.clone())));
            }
        }
        Ok(())
    }
    fn files(
        &mut self,
        r: &[tidemark_core::snapshot::FileRow],
    ) -> Result<(), crate::agg::SinkError> {
        self.check_cancel()?;
        Ok(self.writer.put_files(r)?)
    }
    fn link_sets(
        &mut self,
        r: &[tidemark_core::snapshot::LinkSetRow],
    ) -> Result<(), crate::agg::SinkError> {
        self.check_cancel()?;
        Ok(self.writer.put_link_sets(r)?)
    }
    fn link_instances(
        &mut self,
        r: &[tidemark_core::snapshot::LinkInstanceRow],
    ) -> Result<(), crate::agg::SinkError> {
        self.check_cancel()?;
        Ok(self.writer.put_link_instances(r)?)
    }
    fn skipped(
        &mut self,
        r: &[tidemark_core::snapshot::SkipRow],
    ) -> Result<(), crate::agg::SinkError> {
        self.check_cancel()?;
        Ok(self.writer.put_skipped(r)?)
    }
    fn errors(
        &mut self,
        r: &[tidemark_core::snapshot::ErrorRow],
    ) -> Result<(), crate::agg::SinkError> {
        self.check_cancel()?;
        Ok(self.writer.put_errors(r)?)
    }
    fn annotations(
        &mut self,
        r: &[tidemark_core::snapshot::AnnotationRow],
    ) -> Result<(), crate::agg::SinkError> {
        self.check_cancel()?;
        Ok(self.writer.put_annotations(r)?)
    }
}

fn delete_snapshot(state: &std::path::Path, file: &std::path::Path) -> Result<(), ScanError> {
    let allowed = std::fs::canonicalize(state.join("snapshots"))?;
    let parent = std::fs::canonicalize(
        file.parent()
            .ok_or_else(|| ScanError::Precondition("snapshot has no parent".into()))?,
    )?;
    if !parent.starts_with(allowed) || file.extension() != Some(std::ffi::OsStr::new("db")) {
        return Err(ScanError::Precondition(
            "snapshot deletion outside state directory refused".into(),
        ));
    }
    match std::fs::remove_file(file) {
        Ok(()) => {
            tracing::info!(path=%file.display(), "pruned snapshot");
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn guard_and_prune(
    state: &std::path::Path,
    catalog: &tidemark_store::catalog::Catalog,
    recent: usize,
    now: i64,
    mut check: impl FnMut() -> Result<tidemark_store::space_guard::SpaceVerdict, ScanError>,
) -> Result<(), ScanError> {
    use tidemark_store::space_guard::SpaceVerdict;
    if matches!(check()?, SpaceVerdict::Ok) {
        return Ok(());
    }
    let entries = catalog.list(None)?;
    for e in &entries {
        if matches!(e.status.as_str(), "partial" | "failed")
            && now.saturating_sub(e.finished_at.unwrap_or(e.started_at)) > 86400
        {
            if let Some(file) = &e.file {
                delete_snapshot(state, file)?;
                catalog.rebuild_after_scan()?;
                if matches!(check()?, SpaceVerdict::Ok) {
                    return Ok(());
                }
            }
        }
    }
    let mut roots = std::collections::BTreeMap::<RawPath, Vec<&ScanEntry>>::new();
    for e in &entries {
        if e.status == "complete" {
            roots.entry(e.root.clone()).or_default().push(e);
        }
    }
    let mut groups: Vec<_> = roots.into_values().collect();
    for group in &mut groups {
        group.sort_by(|a, b| {
            b.started_at
                .cmp(&a.started_at)
                .then_with(|| b.scan_id.cmp(&a.scan_id))
        });
    }
    groups.sort_by(|a, b| {
        b[0].t_disk
            .cmp(&a[0].t_disk)
            .then_with(|| a[0].root.cmp(&b[0].root))
    });
    for group in groups {
        for e in group.into_iter().skip(recent.max(1)).rev() {
            if let Some(file) = &e.file {
                delete_snapshot(state, file)?;
                catalog.rebuild_after_scan()?;
                if matches!(check()?, SpaceVerdict::Ok) {
                    return Ok(());
                }
            }
        }
    }
    Err(ScanError::NoSpace)
}

fn recorded<T>(
    entry: &mut ScanEntry,
    recorder: &dyn ScanRecorder,
    finish: impl Fn() -> (i64, u64),
    work: impl FnOnce(&mut ScanEntry, &dyn ScanRecorder) -> Result<T, ScanError>,
) -> Result<T, ScanError> {
    let recorder = CompletionRecorder {
        inner: recorder,
        acknowledged: AtomicBool::new(false),
    };
    recorder.scan_started(entry)?;
    let result = work(entry, &recorder);
    if result.is_err()
        && !recorder
            .acknowledged
            .load(std::sync::atomic::Ordering::Acquire)
    {
        let (now, duration) = finish();
        // Cancellation is a failed catalog lifecycle unless a partial was published.
        entry.status = "failed".into();
        entry.finished_at = Some(now);
        entry.duration_ms = Some(duration);
        if let Err(cleanup) = recorder.scan_finished(entry) {
            tracing::error!(error = %cleanup, "failed to finalize failed scan; original error preserved");
        }
    }
    result
}

// Only acknowledged writes finish a lifecycle, including writer-creation ID retries.
struct CompletionRecorder<'a> {
    inner: &'a dyn ScanRecorder,
    acknowledged: AtomicBool,
}
impl ScanRecorder for CompletionRecorder<'_> {
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
        self.acknowledged
            .store(false, std::sync::atomic::Ordering::Release);
        self.inner.scan_started(e)
    }
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
        self.inner.scan_finished(e)?;
        self.acknowledged
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    fn dir_series(
        &self,
        root: &RawPath,
        id: &str,
        ts: i64,
        rows: &[DirRow],
    ) -> Result<(), ScanError> {
        self.inner.dir_series(root, id, ts, rows)
    }
}

fn recoverable_walk_error(error: &tidemark_fs::walk::WalkError) -> bool {
    // Collector/worker corruption and lost output cannot establish a valid partial tree.
    matches!(
        error,
        tidemark_fs::walk::WalkError::Cancelled | tidemark_fs::walk::WalkError::Io(_)
    ) || matches!(error, tidemark_fs::walk::WalkError::Limit(reason) if reason != "worker panicked")
}

fn create_writer(
    dir: &std::path::Path,
    catalog: &tidemark_store::catalog::Catalog,
    meta: &mut tidemark_core::snapshot::SnapshotMeta,
    entry: &mut ScanEntry,
    recorder: &dyn ScanRecorder,
) -> Result<tidemark_store::writer::SnapshotWriter, ScanError> {
    for _ in 0..1024 {
        match tidemark_store::writer::SnapshotWriter::create(dir, meta) {
            Ok(writer) => return Ok(writer),
            Err(tidemark_store::StoreError::Io(e))
                if e.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                entry.status = "failed".into();
                entry.finished_at = Some(meta.finished_at);
                recorder.scan_finished(entry)?;
                meta.scan_id = fresh_id(meta.started_at, dir, |id| Ok(catalog.get(id)?.is_some()))?;
                entry.scan_id = meta.scan_id.to_string();
                entry.status = "running".into();
                entry.finished_at = None;
                recorder.scan_started(entry)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(ScanError::Resource(
        "snapshot creation collision retry limit reached".into(),
    ))
}

fn publication_status(
    status: SnapshotStatus,
    mode: ScanMode,
    keep: bool,
    cancel: &AtomicBool,
) -> Result<SnapshotStatus, ScanError> {
    // Atomically consume the interruption that selected a kept walk partial.
    // Publication starts at this boundary: a later signal sets the flag again
    // and finish_with must discard the candidate, including partial candidates.
    let interrupted = if mode == ScanMode::Walk && keep {
        cancel.swap(false, std::sync::atomic::Ordering::AcqRel)
    } else {
        cancel.load(std::sync::atomic::Ordering::Acquire)
    };
    if interrupted {
        if mode == ScanMode::Bulk || !keep {
            return Err(ScanError::Cancelled);
        }
        return Ok(SnapshotStatus::Partial);
    }
    Ok(status)
}

fn admit_memory(
    budget: u64,
    estimate: u64,
    threads: usize,
    buffer: u64,
    top: usize,
) -> Result<(), ScanError> {
    let total = u128::from(estimate)
        + u128::from(budget / 4)
        + threads as u128 * (u128::from(buffer) + top as u128 * 16);
    if total > u128::from(budget) {
        return Err(ScanError::Resource(
            "memory budget cannot fit aggregator estimate and records; raise scan.memory_budget"
                .into(),
        ));
    }
    Ok(())
}

const OS_RELEASE_LIMIT: usize = 16 * 1024;
fn read_os_release<R: std::io::Read>(reader: std::io::Result<R>) -> String {
    use std::io::Read;
    let Ok(reader) = reader else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if reader
        .take(OS_RELEASE_LIMIT as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn record_walk_phase(phases: &mut std::collections::BTreeMap<String, u64>, elapsed: Duration) {
    phases
        .entry("walk".into())
        .or_insert_with(|| milliseconds(elapsed));
}

fn finish_snapshot(
    writer: tidemark_store::writer::SnapshotWriter,
    meta: &tidemark_core::snapshot::SnapshotMeta,
    cancelled: &dyn Fn() -> bool,
) -> Result<PathBuf, ScanError> {
    writer.finish_with(meta, cancelled).map_err(|e| match e {
        tidemark_store::StoreError::Cancelled => ScanError::Cancelled,
        tidemark_store::StoreError::Io(ref io)
            if io.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            ScanError::PublicationCollision
        }
        _ => ScanError::Store(e),
    })
}

// Linux reports process high-water RSS in KiB. This includes other work in the process.
fn observed_peak_rss() -> Option<u64> {
    use std::io::Read;
    let file = std::fs::File::open("/proc/self/status").ok()?;
    let mut status = String::new();
    file.take(65_536).read_to_string(&mut status).ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    })
}

/// Record direct-mode directory samples, preserving the first sample within each second.
/// The host scan lock must be held by the caller so the collision check is serialized.
pub fn record_dir_series(
    state: &std::path::Path,
    root: &RawPath,
    id: &str,
    ts: i64,
    rows: &[DirRow],
) -> Result<(), ScanError> {
    if rows.len() > 20_000 {
        return Err(ScanError::Resource(
            "directory series row limit exceeded".into(),
        ));
    }
    let series = tidemark_store::timeseries::TimeSeries::open(&state.join("timeseries.db"), true)?;
    // The frozen series key has second precision. Preserve the first observation
    // at a given second when multiple serialized scans finish within that second.
    let mut fresh = Vec::with_capacity(rows.len());
    for row in rows {
        if series.query_dir_series(root, &row.path, ts, ts)?.is_empty() {
            fresh.push(row.clone());
        }
    }
    if fresh.len() != rows.len() {
        tracing::warn!(
            scan_id = id,
            timestamp = ts,
            omitted = rows.len() - fresh.len(),
            "directory series already sampled this second; preserving earlier samples"
        );
    }
    Ok(series.insert_dir_series(root, id, ts, &fresh)?)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::{atomic::Ordering, Mutex};
    use tidemark_store::{catalog::Catalog, space_guard::SpaceVerdict};
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);
    impl ScanRecorder for Recorder {
        fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
            self.0.lock().unwrap().push(format!("start:{}", e.status));
            Ok(())
        }
        fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
            self.0.lock().unwrap().push(format!("finish:{}", e.status));
            Ok(())
        }
        fn dir_series(&self, _: &RawPath, _: &str, _: i64, _: &[DirRow]) -> Result<(), ScanError> {
            self.0.lock().unwrap().push("series".into());
            Ok(())
        }
    }
    fn entry() -> ScanEntry {
        ScanEntry {
            scan_id: "test".into(),
            root: RawPath::new(b"/fixture".to_vec()).unwrap(),
            mode: ScanMode::Walk,
            started_at: 100,
            finished_at: None,
            status: "running".into(),
            degraded: false,
            file: None,
            t_disk: None,
            t_inodes: None,
            t_dirs: None,
            errors: None,
            duration_ms: None,
        }
    }
    #[test]
    fn scan_provenance_uses_injected_environment() {
        let fixture = test_support::materialize("[[entry]]\npath='file'\nkind='file'\nsize=8\n");
        let state = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.general.state_dir = state.path().to_owned();
        let req = ScanRequest {
            root: RawPath::from_path(&fixture.root).unwrap(),
            mode: crate::mode::ModeRequest::Walk,
            throttle: Some("max".into()),
            threads: Some(1),
            exclude_extra: vec![],
            keep_partial: false,
            annotations: vec![],
        };
        let calls = std::cell::Cell::new(0);
        let recorder = Recorder::default();
        let result = run_scan_with_environment(
            &cfg,
            &req,
            &recorder,
            &AtomicBool::new(false),
            &|_| {},
            &|| {
                let call = calls.get();
                calls.set(call + 1);
                100 + call * 10
            },
            &|| "NAME=Fixture OS\n".into(),
        )
        .unwrap();
        let snapshot =
            tidemark_store::reader::Snapshot::open(result.file.as_ref().unwrap()).unwrap();
        assert_eq!(snapshot.meta().os_release, "NAME=Fixture OS\n");
        // The first clock read is preflight retention, the second starts the scan,
        // and the third is after row writing, immediately before writer finalization.
        assert_eq!(snapshot.meta().started_at, 110);
        assert_eq!(snapshot.meta().finished_at, 120);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn walk_phase_preserves_measured_duration() {
        let mut phases = std::collections::BTreeMap::new();
        phases.insert("walk".to_owned(), milliseconds(Duration::from_millis(17)));
        record_walk_phase(&mut phases, Duration::from_millis(999));
        assert_eq!(phases["walk"], 17);
    }

    #[test]
    fn os_release_read_is_bounded_and_replaceable() {
        assert_eq!(
            read_os_release(Ok(std::io::Cursor::new(b"NAME=Fixture\n"))),
            "NAME=Fixture\n"
        );
        assert!(
            read_os_release::<std::io::Cursor<Vec<u8>>>(Err(std::io::Error::other("unavailable")))
                .is_empty()
        );
        let data = vec![b'x'; OS_RELEASE_LIMIT + 1];
        assert_eq!(
            read_os_release(Ok(std::io::Cursor::new(data))).len(),
            OS_RELEASE_LIMIT
        );
    }

    #[test]
    fn cancellation_monitor_releases_bulk_cost_during_preflight_or_any_phase() {
        use std::sync::atomic::Ordering;
        use tidemark_fs::throttle::gate_from_config;
        let mut cfg = Config::default();
        cfg.throttle.min_ops_per_sec = 1;
        cfg.throttle.max_ops_per_sec = 1;
        let gate = gate_from_config("gentle", &cfg.throttle, &[], Box::new(|| Ok(vec![]))).unwrap();
        let cancel = AtomicBool::new(false);
        let barrier = std::sync::Barrier::new(2);
        let start = std::sync::Mutex::new(None);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                *start.lock().unwrap() = Some(std::time::Instant::now());
                cancel.store(true, Ordering::Release);
            });
            with_cancellation(&gate, &cancel, || {
                barrier.wait();
                gate.gate().acquire(129);
            });
        });
        assert!(start.lock().unwrap().unwrap().elapsed() < Duration::from_secs(2));
        gate.cancel();
        assert_eq!(gate.stats().min_rate, 1.0);
        gate.gate().acquire(129);
        assert_eq!(gate.stop().mean_rate, 1.0);
    }

    #[test]
    fn root_open_refuses_symlink_ancestor() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("real/child")).unwrap();
        let root = RawPath::from_path(&temp.path().join("real/child")).unwrap();
        std::fs::rename(temp.path().join("real"), temp.path().join("old")).unwrap();
        std::os::unix::fs::symlink("old", temp.path().join("real")).unwrap();
        assert!(open_directory(
            &root,
            &tidemark_core::scan::NoopGate,
            &AtomicBool::new(false),
        )
        .is_err());
    }

    #[test]
    fn host_lock_is_exclusive_and_released() {
        let state = tempfile::tempdir().unwrap();
        let lock = ScanLock::acquire(state.path()).unwrap();
        assert!(matches!(
            ScanLock::acquire(state.path()),
            Err(ScanError::Busy)
        ));
        drop(lock);
        assert!(ScanLock::acquire(state.path()).is_ok());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(state.path().join("scan.lock"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[test]
    fn lock_rejects_symlink() {
        let state = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), state.path().join("scan.lock")).unwrap();
        assert!(ScanLock::acquire(state.path()).is_err());
    }
    #[test]
    fn cancellation_after_last_batch_changes_publication_policy() {
        let cancel = AtomicBool::new(false);
        assert_eq!(
            publication_status(SnapshotStatus::Complete, ScanMode::Walk, false, &cancel).unwrap(),
            SnapshotStatus::Complete
        );
        cancel.store(true, std::sync::atomic::Ordering::Release);
        assert!(matches!(
            publication_status(SnapshotStatus::Complete, ScanMode::Walk, false, &cancel),
            Err(ScanError::Cancelled)
        ));
        assert_eq!(
            publication_status(SnapshotStatus::Complete, ScanMode::Walk, true, &cancel).unwrap(),
            SnapshotStatus::Partial
        );
        assert!(!cancel.load(Ordering::Acquire));
        cancel.store(true, Ordering::Release);
        assert!(matches!(
            publication_status(SnapshotStatus::Complete, ScanMode::Bulk, true, &cancel),
            Err(ScanError::Cancelled)
        ));
    }

    #[test]
    fn record_limit_derivation_table() {
        for (mib, threads, expected) in [
            (128, 1, Some((1, 16))),
            (256, 1, Some((3, 16))),
            (1280, 8, Some((12, 16))),
            (8192, 8, Some((64, 28))),
            (64, 1, Some((1, 8))),
            (512, 8, Some((1, 14))),
            (511, 8, None),
            (1280, 0, None),
        ] {
            match expected {
                Some((capacity, min_mib)) => {
                    let (actual, bound) = record_limits(mib << 20, threads).unwrap();
                    assert_eq!(actual, capacity);
                    assert_eq!(bound, (mib << 20) / 4 / (threads + actual) as u64);
                    assert!(bound >= min_mib << 20);
                    assert!((threads + actual) as u64 * bound <= (mib << 20) / 4);
                }
                None => assert!(record_limits(mib << 20, threads).is_err()),
            }
        }
    }

    #[test]
    fn rejected_completion_is_finalized_as_failed_preserving_original_error() {
        struct Reject {
            events: Mutex<Vec<String>>,
            fail_cleanup: bool,
        }
        impl ScanRecorder for Reject {
            fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("start:{}", e.status));
                Ok(())
            }
            fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("finish:{}", e.status));
                if e.status == "complete" {
                    return Err(ScanError::Resource("original".into()));
                }
                if self.fail_cleanup {
                    return Err(ScanError::Resource("cleanup".into()));
                }
                Ok(())
            }
            fn dir_series(
                &self,
                _: &RawPath,
                _: &str,
                _: i64,
                _: &[DirRow],
            ) -> Result<(), ScanError> {
                Ok(())
            }
        }
        for fail_cleanup in [false, true] {
            let rec = Reject {
                events: Mutex::new(vec![]),
                fail_cleanup,
            };
            let state = tempfile::tempdir().unwrap();
            let published = state.path().join("published.db");
            std::fs::write(&published, b"published data").unwrap();
            let mut e = entry();
            let result: Result<(), ScanError> = recorded(
                &mut e,
                &rec,
                || (102, 2000),
                |e, recorder| {
                    e.status = "complete".into();
                    e.file = Some(published.clone());
                    recorder.scan_finished(e)
                },
            );
            assert_eq!(result.unwrap_err().to_string(), "original");
            assert_eq!(
                *rec.events.lock().unwrap(),
                ["start:running", "finish:complete", "finish:failed"]
            );
            assert_eq!(e.status, "failed");
            assert_eq!(e.file.as_ref(), Some(&published));
            assert!(published.exists());
        }
    }

    #[test]
    fn emergency_prune_includes_expired_partial_and_failed_artifacts() {
        for status in ["partial", "failed"] {
            let state = tempfile::tempdir().unwrap();
            let catalog = Catalog::open(state.path(), true).unwrap();
            let dir = state.path().join("snapshots/root");
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("expired.db");
            std::fs::write(&file, b"expired artifact").unwrap();
            let mut e = entry();
            e.status = status.into();
            e.file = Some(file.clone());
            e.finished_at = Some(100);
            catalog.record_start(&e).unwrap();
            guard_and_prune(state.path(), &catalog, 4, 86501, || {
                Ok(if file.exists() {
                    SpaceVerdict::Refuse {
                        reason: "injected".into(),
                    }
                } else {
                    SpaceVerdict::Ok
                })
            })
            .unwrap();
            assert!(!file.exists());
            assert!(catalog.get(&e.scan_id).unwrap().is_none());
        }
    }

    #[test]
    fn partial_walk_failure_classification() {
        use tidemark_fs::walk::WalkError;
        assert!(recoverable_walk_error(&WalkError::Limit(
            "record bytes".into()
        )));
        assert!(recoverable_walk_error(&WalkError::Io(
            std::io::Error::other("failed directory")
        )));
        assert!(recoverable_walk_error(&WalkError::Cancelled));
        assert!(!recoverable_walk_error(&WalkError::Disconnected));
        assert!(!recoverable_walk_error(&WalkError::Limit(
            "worker panicked".into()
        )));
    }

    #[test]
    fn queue_budget_and_zero_top() {
        assert!((1..=64).contains(&channel_capacity(1280 << 20, 8, 10000, 256 << 10).unwrap()));
        assert!((1..=64).contains(&channel_capacity(1280 << 20, 8, 0, 256 << 10).unwrap()));
        assert!(channel_capacity(1, 8, 10000, 256 << 10).is_err());
        assert!(channel_capacity(u64::MAX, usize::MAX, usize::MAX, u64::MAX).is_err());
    }
    #[test]
    fn scan_ids_retry_without_replacing_and_reject_invalid_years() {
        let state = tempfile::tempdir().unwrap();
        let mut attempts = 0;
        let id = fresh_id(100, state.path(), |candidate| {
            attempts += 1;
            if attempts == 1 {
                std::fs::write(state.path().join(format!("{candidate}.db")), b"preserved")?;
                Ok(true)
            } else {
                Ok(false)
            }
        })
        .unwrap();
        assert_eq!(attempts, 2);
        assert!(!state.path().join(format!("{id}.db")).exists());
        let saved = std::fs::read_dir(state.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(std::fs::read(saved).unwrap(), b"preserved");
        assert!(fresh_id(i64::MAX, state.path(), |_| Ok(false)).is_err());
        assert!(fresh_id(100, state.path(), |_| Ok(true)).is_err());
    }
    #[test]
    fn lifecycle_records_failure_cancellation_and_success() {
        for fail in [true, false] {
            let rec = Recorder::default();
            let mut e = entry();
            let result = recorded(
                &mut e,
                &rec,
                || (102, 2000),
                |e, recorder| {
                    if fail {
                        return Err(ScanError::Cancelled);
                    }
                    e.status = "complete".into();
                    recorder.scan_finished(e)?;
                    recorder.dir_series(&e.root, &e.scan_id, 102, &[])?;
                    Ok(())
                },
            );
            assert_eq!(result.is_err(), fail);
            assert_eq!(
                *rec.0.lock().unwrap(),
                if fail {
                    vec!["start:running", "finish:failed"]
                } else {
                    vec!["start:running", "finish:complete", "series"]
                }
            );
            if fail {
                assert_eq!(e.finished_at, Some(102));
                assert_eq!(e.duration_ms, Some(2000));
            }
        }
    }
    #[test]
    fn injected_space_refusal_creates_no_snapshot() {
        let state = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(state.path(), true).unwrap();
        let result = guard_and_prune(state.path(), &catalog, 4, 100000, || {
            Ok(SpaceVerdict::Refuse {
                reason: "injected".into(),
            })
        });
        assert!(matches!(result, Err(ScanError::NoSpace)));
        assert!(!state.path().join("snapshots").exists());
    }
    #[test]
    fn emergency_prune_protects_recent_and_latest() {
        let state = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(state.path(), true).unwrap();
        let dir = state.path().join("snapshots/root");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..3 {
            let mut e = entry();
            e.scan_id = format!("id{i}");
            e.started_at = i;
            e.status = "complete".into();
            let file = dir.join(format!("id{i}.db"));
            std::fs::write(&file, b"test").unwrap();
            e.file = Some(file);
            catalog.record_start(&e).unwrap();
        }
        guard_and_prune(state.path(), &catalog, 2, 100000, || {
            Ok(if dir.join("id0.db").exists() {
                SpaceVerdict::Refuse {
                    reason: "injected".into(),
                }
            } else {
                SpaceVerdict::Ok
            })
        })
        .unwrap();
        assert!(!dir.join("id0.db").exists());
        assert!(dir.join("id1.db").exists());
        assert!(dir.join("id2.db").exists());
    }
    #[test]
    fn ordinary_catalog_open_preserves_corrupt_bytes_and_explicit_recovery_logs_paths() {
        let state = tempfile::tempdir().unwrap();
        let file = state.path().join("timeseries.db");
        std::fs::write(&file, b"corrupt database").unwrap();
        assert!(Catalog::open(state.path(), true).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"corrupt database");
        let catalog = open_catalog(state.path(), true).unwrap();
        assert_eq!(catalog.recovery_files().len(), 1);
        assert_eq!(
            std::fs::read(&catalog.recovery_files()[0]).unwrap(),
            b"corrupt database"
        );
        drop(catalog);
        assert!(open_catalog(state.path(), false)
            .unwrap()
            .recovery_files()
            .is_empty());
    }
    #[test]
    fn cancellation_drains_bounded_walker() {
        let fixture = test_support::materialize("[[entry]]\npath='file'\nkind='file'\nsize=8\n");
        let root = RawPath::from_path(&fixture.root).unwrap();
        let cfg = tidemark_fs::walk::WalkConfig {
            root: root.clone(),
            threads: 1,
            stat_entries: true,
            exclude: globset::GlobSetBuilder::new().build().unwrap(),
            skip_mounts: vec![],
            big_file_threshold: u64::MAX,
            top_files: 1,
            scan_start: 100,
            getdents_buf: 4096,
            io_idle: false,
            max_record_bytes: 0,
        };
        let agg = crate::agg::Aggregator::new(crate::agg::AggConfig {
            root,
            scan_start: 100,
            top_files: 1,
            big_file_threshold: u64::MAX,
            max_notable: 1,
            link_instances: false,
            max_stored_errors: 10,
        });
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            collect_walk(
                &cfg,
                &tidemark_core::scan::NoopGate,
                &cancel,
                agg,
                1,
                &|_| {},
                None,
                false
            ),
            Err(ScanError::Cancelled)
        ));
        assert!(cancel.load(Ordering::Relaxed));
    }
    #[test]
    fn progress_reports_while_a_worker_is_blocked() {
        struct SlowGate(AtomicBool);
        impl tidemark_core::scan::Gate for SlowGate {
            fn acquire(&self, _: u32) {
                if !self.0.swap(true, Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(2200));
                }
            }
            fn current_rate(&self) -> f64 {
                17.0
            }
        }
        let fixture = test_support::materialize("[[entry]]\npath='file'\nkind='file'\nsize=8\n");
        let root = RawPath::from_path(&fixture.root).unwrap();
        let cfg = tidemark_fs::walk::WalkConfig {
            root: root.clone(),
            threads: 1,
            stat_entries: true,
            exclude: globset::GlobSetBuilder::new().build().unwrap(),
            skip_mounts: vec![],
            big_file_threshold: u64::MAX,
            top_files: 1,
            scan_start: 100,
            getdents_buf: 4096,
            io_idle: false,
            max_record_bytes: 0,
        };
        let agg = crate::agg::Aggregator::new(crate::agg::AggConfig {
            root,
            scan_start: 100,
            top_files: 1,
            big_file_threshold: u64::MAX,
            max_notable: 1,
            link_instances: false,
            max_stored_errors: 10,
        });
        let reports = Mutex::new(Vec::new());
        let (_, result) = collect_walk(
            &cfg,
            &SlowGate(AtomicBool::new(false)),
            &AtomicBool::new(false),
            agg,
            1,
            &|p| reports.lock().unwrap().push((p.elapsed, p.rate)),
            None,
            false,
        )
        .unwrap();
        assert!(result.is_ok());
        let reports = reports.lock().unwrap();
        assert!(!reports.is_empty());
        assert!(reports
            .iter()
            .all(|(elapsed, rate)| *elapsed >= Duration::from_secs(2) && *rate == 17.0));
    }
    #[test]
    fn deletion_refuses_paths_outside_state() {
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir(state.path().join("snapshots")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("preserved.db");
        std::fs::write(&file, b"kept").unwrap();
        assert!(delete_snapshot(state.path(), &file).is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"kept");
    }
    #[test]
    fn cancellation_during_publication_is_recorded_and_never_kept() {
        use std::cell::Cell;
        struct CatalogRecorder<'a>(&'a std::path::Path, &'a Recorder);
        impl ScanRecorder for CatalogRecorder<'_> {
            fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
                Catalog::open(self.0, true)?.record_start(e)?;
                self.1.scan_started(e)
            }
            fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
                Catalog::open(self.0, true)?.record_finish(e)?;
                self.1.scan_finished(e)
            }
            fn dir_series(
                &self,
                _: &RawPath,
                _: &str,
                _: i64,
                _: &[DirRow],
            ) -> Result<(), ScanError> {
                Ok(())
            }
        }
        let fixture = test_support::materialize("[[entry]]\npath='file'\nkind='file'\nsize=8\n");
        let state = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.general.state_dir = state.path().to_owned();
        let req = ScanRequest {
            root: RawPath::from_path(&fixture.root).unwrap(),
            mode: ModeRequest::Walk,
            throttle: Some("max".into()),
            threads: Some(1),
            exclude_extra: vec![],
            keep_partial: false,
            annotations: vec![],
        };
        let outcome = run_scan(
            &cfg,
            &req,
            &Recorder::default(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        let existing = outcome.file.unwrap();
        let bytes = std::fs::read(&existing).unwrap();
        let mut meta = tidemark_store::reader::Snapshot::open(&existing)
            .unwrap()
            .meta()
            .clone();
        for keep_partial in [false, true] {
            meta.scan_id =
                tidemark_core::time::ScanId::new(meta.started_at + 1 + i64::from(keep_partial))
                    .unwrap();
            meta.status = if keep_partial {
                SnapshotStatus::Partial
            } else {
                SnapshotStatus::Complete
            };
            let dir = existing.parent().unwrap();
            let writer = tidemark_store::writer::SnapshotWriter::create(dir, &meta).unwrap();
            let cancel = AtomicBool::new(keep_partial);
            meta.status =
                publication_status(meta.status, ScanMode::Walk, keep_partial, &cancel).unwrap();
            assert!(!cancel.load(Ordering::Acquire));
            let checks = Cell::new(0);
            let recorder = Recorder::default();
            let catalog_dir = tempfile::tempdir().unwrap();
            let catalog = Catalog::open(catalog_dir.path(), true).unwrap();
            let catalog_recorder = CatalogRecorder(catalog_dir.path(), &recorder);
            let mut e = entry();
            let result = recorded(
                &mut e,
                &catalog_recorder,
                || (102, 2000),
                |_, _| {
                    finish_snapshot(writer, &meta, &|| {
                        checks.set(checks.get() + 1);
                        // The first index has completed when cancellation arrives.
                        if checks.get() == 2 {
                            cancel.store(true, Ordering::Release);
                        }
                        cancel.load(Ordering::Acquire)
                    })
                },
            );
            assert!(matches!(result, Err(ScanError::Cancelled)));
            assert_eq!(checks.get(), 2);
            assert_eq!(e.status, "failed");
            let stored = catalog.get(&e.scan_id).unwrap().unwrap();
            assert_eq!(stored.status, "failed");
            assert_eq!(stored.finished_at, Some(102));
            assert!(stored.file.is_none());
            assert!(e.file.is_none());
            assert_eq!(e.finished_at, Some(102));
            assert_eq!(
                *recorder.0.lock().unwrap(),
                ["start:running", "finish:failed"]
            );
            assert!(!dir.join(format!("{}.db", meta.scan_id)).exists());
            assert!(!dir.join(format!("{}.db.tmp", meta.scan_id)).exists());
            assert_eq!(std::fs::read(&existing).unwrap(), bytes);
        }
    }
    #[test]
    fn finish_collision_preserves_existing_snapshot_and_removes_temporary_file() {
        let fixture = test_support::materialize("[[entry]]\npath='file'\nkind='file'\nsize=8\n");
        let state = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.general.state_dir = state.path().to_owned();
        let req = ScanRequest {
            root: RawPath::from_path(&fixture.root).unwrap(),
            mode: ModeRequest::Walk,
            throttle: Some("max".into()),
            threads: Some(1),
            exclude_extra: vec![],
            keep_partial: false,
            annotations: vec![],
        };
        let outcome = run_scan(
            &cfg,
            &req,
            &Recorder::default(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        let snapshot =
            tidemark_store::reader::Snapshot::open(outcome.file.as_ref().unwrap()).unwrap();
        let mut meta = snapshot.meta().clone();
        meta.scan_id = tidemark_core::time::ScanId::new(meta.started_at + 1).unwrap();
        let dir = outcome.file.as_ref().unwrap().parent().unwrap();
        let writer = tidemark_store::writer::SnapshotWriter::create(dir, &meta).unwrap();
        let existing = dir.join(format!("{}.db", meta.scan_id));
        let temporary = dir.join(format!("{}.db.tmp", meta.scan_id));
        std::fs::write(&existing, b"existing snapshot must survive").unwrap();
        assert!(matches!(
            finish_snapshot(writer, &meta, &|| false),
            Err(ScanError::PublicationCollision)
        ));
        assert_eq!(
            std::fs::read(existing).unwrap(),
            b"existing snapshot must survive"
        );
        assert!(!temporary.exists());
        for keep_partial in [false, true] {
            use crate::agg::RowSink;
            meta.scan_id = tidemark_core::time::ScanId::new(meta.started_at + 2).unwrap();
            let tmp = dir.join(format!("{}.db.tmp", meta.scan_id));
            let mut writer = tidemark_store::writer::SnapshotWriter::create(dir, &meta).unwrap();
            let cancel = AtomicBool::new(false);
            {
                let mut sink = WriterSink {
                    writer: &mut writer,
                    series: Default::default(),
                    depth: 0,
                    cancel: &cancel,
                    keep_partial,
                };
                sink.dirs(&[]).unwrap();
                cancel.store(true, std::sync::atomic::Ordering::Release);
                assert_eq!(sink.files(&[]).is_ok(), keep_partial);
            }
            drop(writer);
            assert!(!tmp.exists());
        }
    }
}
