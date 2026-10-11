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
/// Reserved for W2-C.
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
/// Reserved for W2-C.
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

/// Reserved for W2-C.
pub trait ScanRecorder: Send + Sync {
    /// Reserved for W2-C.
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError>;
    /// Reserved for W2-C.
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
/// Reserved for W2-C.
pub fn run_scan(
    cfg: &Config,
    req: &ScanRequest,
    recorder: &dyn ScanRecorder,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&Progress) + Sync),
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
    let _lock = ScanLock::acquire(&cfg.general.state_dir)?;
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
    let gate = scan_gate.gate();
    gate.acquire(1);
    let (root_fd, setup_noatime) = open_directory(req.root.as_path())?;
    gate.acquire(1);
    let root_stat = rustix::fs::fstat(&root_fd)?;
    if root_stat.st_dev != mount.dev {
        return Err(ScanError::Precondition(
            "root mount changed during setup".into(),
        ));
    }
    gate.acquire(1);
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
        || {
            Ok(tidemark_store::space_guard::space_check(
                &cfg.general.state_dir,
                estimate,
                &cfg.retention,
            )?)
        },
    )?;
    gate.acquire(1);
    let is_xfs = tidemark_sys::xfs::is_xfs(root_fd.as_fd())?;
    let geometry = if is_xfs {
        gate.acquire(1);
        tidemark_sys::xfs::geometry(root_fd.as_fd()).ok()
    } else {
        None
    };
    let v5 = if is_xfs {
        gate.acquire(2);
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
    let (mode, reason) = crate::mode::choose_mode(&facts, &req.mode, cfg.scan.memory_budget.0);
    if matches!(req.mode, ModeRequest::Bulk) && mode != ScanMode::Bulk {
        return Err(ScanError::Precondition(format!(
            "bulk mode unavailable: {reason}"
        )));
    }
    let threads = req.threads.unwrap_or(cfg.scan.threads as usize);
    if !(1..=64).contains(&threads) {
        return Err(ScanError::Precondition("threads must be 1..=64".into()));
    }
    let capacity = channel_capacity(
        cfg.scan.memory_budget.0,
        threads,
        root_cfg.top_files,
        cfg.scan.getdents_buf.0,
    )?;
    let now = tidemark_core::time::unix_now();
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
        || {
            (
                tidemark_core::time::unix_now(),
                milliseconds(start.elapsed()),
            )
        },
        |entry| {
            let mut exclude = root_cfg.exclude.clone();
            exclude.extend(req.exclude_extra.clone());
            let mut globs = globset::GlobSetBuilder::new();
            for pattern in &exclude {
                globs.add(
                    globset::Glob::new(pattern)
                        .map_err(|e| ScanError::Precondition(e.to_string()))?,
                );
            }
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
            let walk_cfg = tidemark_fs::walk::WalkConfig {
                root: req.root.clone(),
                threads,
                stat_entries: mode == ScanMode::Walk,
                exclude: globs
                    .build()
                    .map_err(|e| ScanError::Precondition(e.to_string()))?,
                skip_mounts: skips,
                big_file_threshold: root_cfg.big_file_threshold.0,
                top_files: root_cfg.top_files,
                scan_start: now,
                getdents_buf: usize::try_from(cfg.scan.getdents_buf.0)
                    .map_err(|_| ScanError::Resource("getdents buffer overflow".into()))?,
                io_idle: true,
            };
            let agg = crate::agg::Aggregator::new(crate::agg::AggConfig {
                root: req.root.clone(),
                scan_start: now,
                top_files: root_cfg.top_files,
                big_file_threshold: root_cfg.big_file_threshold.0,
                max_notable: cfg.scan.max_notable,
                link_instances: cfg.scan.link_instances
                    == tidemark_core::config::LinkInstances::All,
                max_stored_errors: cfg.scan.max_stored_errors,
            });
            let mut phases = std::collections::BTreeMap::new();
            let mut walk_totals = None;
            let (mut prepared, noatime, partial) = if mode == ScanMode::Walk {
                let (agg, walk_result) = collect_walk(
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
                    Err(tidemark_fs::walk::WalkError::Cancelled) if req.keep_partial => (0, true),
                    Err(e) => return Err(e.into()),
                };
                phases.insert("walk".into(), milliseconds(start.elapsed()));
                (agg.prepare()?, noatime, partial)
            } else {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ScanError::Cancelled);
                }
                let geo = geometry
                    .as_ref()
                    .ok_or_else(|| ScanError::Precondition("XFS geometry unavailable".into()))?;
                let params = crate::bulk::BulkParams {
                    walk: walk_cfg,
                    batch: cfg.scan.bulk_batch as usize,
                    threads,
                };
                let (prepared, stats) = crate::bulk::run_bulk(
                    &params,
                    root_fd.as_fd(),
                    geo,
                    gate.as_ref(),
                    cancel,
                    agg,
                )?;
                for (name, ms) in ["namespace", "bulkstat", "names"]
                    .into_iter()
                    .zip(stats.phase_ms)
                {
                    phases.insert(name.into(), ms);
                }
                (prepared, 0, false)
            };
            if cancel.load(Ordering::Relaxed) && !req.keep_partial {
                return Err(ScanError::Cancelled);
            }
            for (path, key, value) in &req.annotations {
                prepared.annotate(path, key, value);
            }
            let mut summary = prepared.summary();
            if let Some((errors, races)) = walk_totals {
                summary.errors = errors;
                summary.races = races;
                summary.degraded = errors > 0;
            }
            let status = if partial || cancel.load(Ordering::Relaxed) {
                SnapshotStatus::Partial
            } else {
                SnapshotStatus::Complete
            };
            let stats = scan_gate.stop();
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
                finished_at: tidemark_core::time::unix_now(),
                host: rustix::system::uname()
                    .nodename()
                    .to_string_lossy()
                    .into_owned(),
                kernel: rustix::system::uname()
                    .release()
                    .to_string_lossy()
                    .into_owned(),
                os_release: String::new(),
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
            let catalog = tidemark_store::catalog::Catalog::open(&cfg.general.state_dir, false)?;
            let mut writer = create_writer(&dir, &catalog, &mut meta, entry, recorder)?;
            drop(catalog);
            let state_dir = cfg.general.state_dir.clone();
            let policy = cfg.retention.clone();
            writer.space_check_every(
                100_000,
                Box::new(move || {
                    tidemark_store::space_guard::space_check(&state_dir, 0, &policy).unwrap_or_else(
                        |e| tidemark_store::space_guard::SpaceVerdict::Refuse {
                            reason: e.to_string(),
                        },
                    )
                }),
            );
            let mut sink = WriterSink {
                writer: &mut writer,
                series: std::collections::BinaryHeap::new(),
                depth: root_cfg.series_depth,
            };
            let writing = Instant::now();
            let mut summary = prepared.write(&mut sink)?;
            if let Some((errors, races)) = walk_totals {
                summary.errors = errors;
                summary.races = races;
                summary.degraded = errors > 0;
            }
            meta.phase_ms
                .insert("write".into(), milliseconds(writing.elapsed()));
            let series: Vec<_> = sink
                .series
                .into_sorted_vec()
                .into_iter()
                .map(|r| r.0 .0)
                .collect();
            let file = writer.finish(&meta)?;
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
            let catalog = tidemark_store::catalog::Catalog::open(&cfg.general.state_dir, false)?;
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
}

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
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
/// Open without destructive recovery; preserve corrupt files only after a reported corruption.
/// Caller must hold the host lock and have no other sample/catalog connections.
pub fn open_catalog(
    state: &std::path::Path,
    rw: bool,
) -> Result<tidemark_store::catalog::Catalog, ScanError> {
    match tidemark_store::catalog::Catalog::open(state, rw) {
        Ok(c) => Ok(c),
        Err(tidemark_store::StoreError::Sql(ref e))
            if e.sqlite_error()
                .is_some_and(|e| matches!(e.extended_code & 255, 11 | 26))
                && rw =>
        {
            let c = tidemark_store::catalog::recover_catalog(state)?;
            for p in c.recovery_files() {
                tracing::warn!(path=%p.display(), "preserved catalog recovery file");
            }
            Ok(c)
        }
        Err(e) => Err(e.into()),
    }
}
fn open_directory(path: &std::path::Path) -> Result<(rustix::fd::OwnedFd, u64), ScanError> {
    use rustix::fs::{open, Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match open(path, flags | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => Ok((open(path, flags, Mode::empty())?, 1)),
        value => Ok((value?, 0)),
    }
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
/// Compute queue capacity with explicit producer and threshold-candidate reserves.
/// Names reserve 255 bytes plus allocation/capacity overhead. Threshold cap is
/// W1-B's documented 4096; queue capacity is always at most 64.
pub fn channel_capacity(
    budget: u64,
    threads: usize,
    top: usize,
    buffer: u64,
) -> Result<usize, ScanError> {
    let candidate = std::mem::size_of::<tidemark_core::scan::NotableFile>() as u128 + 255 + 64;
    let record = (top as u128 + 4096) * candidate + (1 << 20);
    let producer = (threads as u128).saturating_mul(record + buffer as u128 + top as u128 * 8);
    let reserved = producer.saturating_add(64 << 20);
    let available = u128::from(budget).checked_sub(reserved).ok_or_else(|| {
        ScanError::Resource("memory budget cannot fit producer candidates and scan state".into())
    })?;
    let capacity = (available / 4 / record).min(64) as usize;
    if capacity == 0 {
        return Err(ScanError::Resource(
            "memory budget cannot fit one queued record".into(),
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
                        counts.errors += rec.errors.len() as u64;
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
}
impl crate::agg::RowSink for WriterSink<'_> {
    fn dirs(&mut self, rows: &[DirRow]) -> Result<(), crate::agg::SinkError> {
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
        Ok(self.writer.put_files(r)?)
    }
    fn link_sets(
        &mut self,
        r: &[tidemark_core::snapshot::LinkSetRow],
    ) -> Result<(), crate::agg::SinkError> {
        Ok(self.writer.put_link_sets(r)?)
    }
    fn link_instances(
        &mut self,
        r: &[tidemark_core::snapshot::LinkInstanceRow],
    ) -> Result<(), crate::agg::SinkError> {
        Ok(self.writer.put_link_instances(r)?)
    }
    fn skipped(
        &mut self,
        r: &[tidemark_core::snapshot::SkipRow],
    ) -> Result<(), crate::agg::SinkError> {
        Ok(self.writer.put_skipped(r)?)
    }
    fn errors(
        &mut self,
        r: &[tidemark_core::snapshot::ErrorRow],
    ) -> Result<(), crate::agg::SinkError> {
        Ok(self.writer.put_errors(r)?)
    }
    fn annotations(
        &mut self,
        r: &[tidemark_core::snapshot::AnnotationRow],
    ) -> Result<(), crate::agg::SinkError> {
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
    mut check: impl FnMut() -> Result<tidemark_store::space_guard::SpaceVerdict, ScanError>,
) -> Result<(), ScanError> {
    use tidemark_store::space_guard::SpaceVerdict;
    if matches!(check()?, SpaceVerdict::Ok) {
        return Ok(());
    }
    let entries = catalog.list(None)?;
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
    work: impl FnOnce(&mut ScanEntry) -> Result<T, ScanError>,
) -> Result<T, ScanError> {
    recorder.scan_started(entry)?;
    let result = work(entry);
    if result.is_err() && entry.status == "running" {
        let (now, duration) = finish();
        entry.status = "failed".into();
        entry.finished_at = Some(now);
        entry.duration_ms = Some(duration);
        recorder.scan_finished(entry)?;
    }
    result
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
                |e| {
                    if fail {
                        return Err(ScanError::Cancelled);
                    }
                    e.status = "complete".into();
                    rec.scan_finished(e)?;
                    rec.dir_series(&e.root, &e.scan_id, 102, &[])?;
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
        let result = guard_and_prune(state.path(), &catalog, 4, || {
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
        guard_and_prune(state.path(), &catalog, 2, || {
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
}
