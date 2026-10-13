//! Owned by: W6-A2a
//! Serialized scan scheduling, acknowledged persistence and first-boot coordination.

use crate::{
    component,
    services::tswriter::{self, Message},
    state::{LiveState, Publication},
    DaemonError, Shutdown,
};
use chrono::{DateTime, Days, Local, LocalResult, NaiveDate, TimeZone, Timelike};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tidemark_core::{
    api::{ScanCancelParams, ScanCancelResult, ScanStartParams, ScanStartResult, ScanStatusParams},
    config::{Config, RootMode, ThrottlePreset},
    live::{LastScan, ScanProgress, ScanStatus},
    path::RawPath,
    snapshot::DirRow,
};
use tidemark_scan::{
    mode::ModeRequest,
    orchestrator::{run_scan, ScanError, ScanRecorder, ScanRequest},
};
use tidemark_store::{
    catalog::{Catalog, ScanEntry},
    reader::Snapshot,
};

const POLL: Duration = Duration::from_millis(50);
const ACK_TIMEOUT: Duration = Duration::from_secs(8);
const GROWTH_CAP: usize = 10_000;

/// Parsed local-time scan cadence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    /// No automatic scans; API scans remain available.
    Off,
    /// Elapsed seconds between complete scans.
    Every(i64),
    /// Local minute of day, once per calendar day.
    Daily(u32),
}
/// Half-open local-time window. Equal endpoints represent a full day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    start: u32,
    end: u32,
}

fn minute(text: &str) -> Result<u32, DaemonError> {
    let b = text.as_bytes();
    if b.len() != 5 || b[2] != b':' || ![b[0], b[1], b[3], b[4]].iter().all(u8::is_ascii_digit) {
        return Err(DaemonError::Invalid("expected HH:MM".into()));
    }
    let h = u32::from(b[0] - b'0') * 10 + u32::from(b[1] - b'0');
    let m = u32::from(b[3] - b'0') * 10 + u32::from(b[4] - b'0');
    if h > 23 || m > 59 {
        return Err(DaemonError::Invalid("invalid clock time".into()));
    }
    Ok(h * 60 + m)
}
/// Parse `off`, `every <positive duration>` or `daily HH:MM`.
pub fn parse_schedule(text: &str) -> Result<Schedule, DaemonError> {
    if text == "off" {
        return Ok(Schedule::Off);
    }
    if let Some(d) = text.strip_prefix("every ") {
        let d = humantime::parse_duration(d).map_err(|e| DaemonError::Invalid(e.to_string()))?;
        if d.is_zero() {
            return Err(DaemonError::Invalid("interval must be positive".into()));
        }
        let seconds = d
            .as_secs()
            .checked_add(u64::from(d.subsec_nanos() != 0))
            .and_then(|n| i64::try_from(n).ok())
            .ok_or_else(|| DaemonError::Invalid("interval too large".into()))?;
        return Ok(Schedule::Every(seconds));
    }
    if let Some(t) = text.strip_prefix("daily ") {
        return minute(t).map(Schedule::Daily);
    }
    Err(DaemonError::Invalid("invalid scan schedule".into()))
}
/// Parse `HH:MM-HH:MM`, permitting midnight wrapping.
pub fn parse_window(text: &str) -> Result<Window, DaemonError> {
    let (a, b) = text
        .split_once('-')
        .ok_or_else(|| DaemonError::Invalid("expected HH:MM-HH:MM".into()))?;
    Ok(Window {
        start: minute(a)?,
        end: minute(b)?,
    })
}
impl Window {
    fn contains(&self, m: u32) -> bool {
        if self.start < self.end {
            m >= self.start && m < self.end
        } else {
            self.start == self.end || m >= self.start || m < self.end
        }
    }
}
fn date_time<T: TimeZone>(tz: &T, now: i64) -> Result<DateTime<T>, DaemonError> {
    tz.timestamp_opt(now, 0)
        .single()
        .ok_or_else(|| DaemonError::Invalid("timestamp out of range".into()))
}
fn daily<T: TimeZone>(tz: &T, date: NaiveDate, m: u32) -> Result<i64, DaemonError> {
    let time = date
        .and_hms_opt(m / 60, m % 60, 0)
        .ok_or_else(|| DaemonError::Invalid("invalid daily time".into()))?;
    // A missing wall time runs at the first valid minute afterwards. A fold
    // runs at its first occurrence, never twice on the same local date.
    for n in 0..=180 {
        let t = time
            .checked_add_signed(chrono::Duration::minutes(n))
            .ok_or_else(|| DaemonError::Invalid("date overflow".into()))?;
        match tz.from_local_datetime(&t) {
            LocalResult::Single(t) => return Ok(t.timestamp()),
            LocalResult::Ambiguous(a, b) => return Ok(a.timestamp().min(b.timestamp())),
            LocalResult::None => {}
        }
    }
    Err(DaemonError::Unavailable(
        "local clock gap exceeds three hours".into(),
    ))
}
/// Compute next due time in the host local time zone, with an injected clock.
pub fn next_run(
    schedule: &Schedule,
    window: Option<&Window>,
    last_complete: Option<i64>,
    now: i64,
) -> Result<Option<i64>, DaemonError> {
    next_run_in_timezone(schedule, window, last_complete, now, &Local)
}
/// Explicit-zone scheduling seam. Overdue scans catch up immediately; windows
/// are checked in wall time across both DST transitions. Daily gaps run at the
/// first valid minute afterwards, and folds use the first occurrence only.
pub fn next_run_in_timezone<T: TimeZone>(
    schedule: &Schedule,
    window: Option<&Window>,
    last_complete: Option<i64>,
    now: i64,
    tz: &T,
) -> Result<Option<i64>, DaemonError> {
    let due = match schedule {
        Schedule::Off => return Ok(None),
        Schedule::Every(n) => last_complete
            .map_or(Ok(now), |last| {
                last.checked_add(*n)
                    .ok_or_else(|| DaemonError::Invalid("due time overflow".into()))
            })?
            .max(now),
        Schedule::Daily(m) => {
            let anchor = last_complete.unwrap_or(now);
            let date = date_time(tz, anchor)?.date_naive();
            let mut candidate = daily(tz, date, *m)?;
            if last_complete.is_some_and(|last| candidate <= last) {
                candidate = daily(
                    tz,
                    date.checked_add_days(Days::new(1))
                        .ok_or_else(|| DaemonError::Invalid("date overflow".into()))?,
                    *m,
                )?;
            }
            candidate.max(now)
        }
    };
    let Some(window) = window else {
        return Ok(Some(due));
    };
    let mut candidate = due;
    // At most three elapsed days, including a repeated hour. No dependence on
    // filesystem or machine time is hidden inside this calculation.
    for _ in 0..=4320 {
        let t = date_time(tz, candidate)?;
        if window.contains(t.hour() * 60 + t.minute()) {
            return Ok(Some(candidate));
        }
        candidate = candidate
            .checked_add(60 - candidate.rem_euclid(60))
            .ok_or_else(|| DaemonError::Invalid("window time overflow".into()))?;
    }
    Err(DaemonError::Unavailable(
        "no scan window within three days".into(),
    ))
}

struct Active {
    root: RawPath,
    cancel: Shutdown,
}
struct Shared {
    state: LiveState,
    shutdown: Shutdown,
    active: Mutex<Option<Active>>,
    deadline: Arc<Mutex<Option<Instant>>>,
    commands: Sender<Command>,
    clock: crate::Clock,
    monotonic: crate::Clock,
    // Same MAX_SUBJECTS cap as the scan-status inventory.
    failures: Arc<Mutex<std::collections::BTreeMap<RawPath, Retry>>>,
}
#[derive(Clone)]
struct Retry {
    attempts: u32,
    due: i64,
    error: String,
    configured: bool,
}
fn retry_due(now: i64, attempts: u32) -> i64 {
    now.saturating_add((30_i64 << attempts.saturating_sub(1).min(7)).min(3600))
}
fn scheduler_health(
    failures: &Mutex<std::collections::BTreeMap<RawPath, Retry>>,
    message: &str,
) -> tidemark_core::api::ComponentState {
    match failures
        .lock()
        .iter()
        .find(|(_, retry)| !retry.error.is_empty())
    {
        Some((root, retry)) => component(
            "scheduler",
            "degraded",
            format!("{}: {}", root.display_lossy(), retry.error),
        ),
        None => component("scheduler", "ok", message),
    }
}
struct Command {
    params: ScanStartParams,
    scheduled: bool,
    reply: Sender<Result<ScanStartResult, DaemonError>>,
    cancel: Shutdown,
}
/// Bounded single-scan API control; measurement state is held only in LiveState.
#[derive(Clone, Default)]
pub struct ScanControl {
    shared: Option<Arc<Shared>>,
}
impl ScanControl {
    pub(crate) fn begin_shutdown(&self, deadline: Instant) {
        if let Some(shared) = &self.shared {
            *shared.deadline.lock() = Some(deadline);
            if let Some(active) = shared.active.lock().as_ref() {
                active.cancel.store(true, Ordering::Release);
            }
        }
    }

    /// Start one root, acknowledging only after the writer persists
    /// scan_started. Host lock conflicts return `DaemonError::Scan("Busy")`.
    pub fn start(&self, params: &ScanStartParams) -> Result<ScanStartResult, DaemonError> {
        let shared = self
            .shared
            .as_ref()
            .ok_or_else(|| DaemonError::Unavailable("scheduler disabled".into()))?;
        if shared.shutdown.load(Ordering::Acquire) {
            return Err(DaemonError::Shutdown);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let mut active = shared.active.lock();
        if active.is_some() {
            return Err(DaemonError::Scan("Busy".into()));
        }
        // Reserve inventory capacity for configured roots as well as observed
        // manual roots and preflight failures. The temporary set contains at most
        // three times MAX_SUBJECTS.
        let mut labels: std::collections::BTreeSet<String> = shared
            .state
            .view()
            .scans()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.root)
            .collect();
        labels.extend(shared.failures.lock().keys().map(root_label));
        for root in &shared.state.config().0.roots {
            let raw =
                RawPath::from_path(&root.path).map_err(|e| DaemonError::Invalid(e.to_string()))?;
            labels.insert(root_label(&raw));
        }
        if !labels.contains(&root_label(&params.root)) && labels.len() >= crate::state::MAX_SUBJECTS
        {
            return Err(DaemonError::Unavailable(
                "scan root inventory cap reached".into(),
            ));
        }
        shared.state.reserve_scan(root_label(&params.root))?;
        let (tx, rx) = bounded(1);
        *active = Some(Active {
            root: params.root.clone(),
            cancel: cancel.clone(),
        });
        if shared
            .commands
            .try_send(Command {
                params: params.clone(),
                scheduled: false,
                reply: tx,
                cancel: cancel.clone(),
            })
            .is_err()
        {
            *active = None;
            shared.state.release_scan();
            return Err(DaemonError::Unavailable(
                "scheduler queue unavailable".into(),
            ));
        }
        drop(active);
        let end = Instant::now() + ACK_TIMEOUT;
        loop {
            if shared.shutdown.load(Ordering::Acquire)
                || shared.deadline.lock().is_some_and(|d| Instant::now() >= d)
            {
                cancel.store(true, Ordering::Release);
                return Err(DaemonError::Shutdown);
            }
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                cancel.store(true, Ordering::Release);
                return Err(DaemonError::Unavailable(
                    "scan start acknowledgement timed out".into(),
                ));
            }
            match rx.recv_timeout(remaining.min(POLL)) {
                Ok(result) => return result,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    cancel.store(true, Ordering::Release);
                    return Err(DaemonError::Unavailable(
                        "scan start acknowledgement disconnected".into(),
                    ));
                }
            }
        }
    }

    /// Cancel the active scan for exactly this root, keeping no partial output.
    pub fn cancel(&self, params: &ScanCancelParams) -> Result<ScanCancelResult, DaemonError> {
        let shared = self
            .shared
            .as_ref()
            .ok_or_else(|| DaemonError::Unavailable("scheduler disabled".into()))?;
        let active = shared.active.lock();
        let matched = active.as_ref().filter(|a| a.root == params.root);
        if let Some(a) = matched {
            a.cancel.store(true, Ordering::Release);
        }
        Ok(ScanCancelResult {
            cancelled: matched.is_some(),
        })
    }
    /// Read published statuses, optionally filtering by byte-preserving root.
    pub fn status(&self, params: &ScanStatusParams) -> Result<Vec<ScanStatus>, DaemonError> {
        let shared = self
            .shared
            .as_ref()
            .ok_or_else(|| DaemonError::Unavailable("scheduler disabled".into()))?;
        let mut statuses = shared.state.view().scans().unwrap_or_default();
        if let Some(root) = &params.root {
            statuses.retain(|s| s.root == root_label(root));
        }
        Ok(statuses)
    }
}

struct Recorder {
    cancel: Shutdown,
    shutdown: Shutdown,
    deadline: Arc<Mutex<Option<Instant>>>,
    state: LiveState,
    writer: tswriter::Sender,
    config: Arc<Config>,
    reply: Mutex<Option<Sender<Result<ScanStartResult, DaemonError>>>>,
    previous: Mutex<Option<ScanEntry>>,
    clock: crate::Clock,
    failures: Arc<Mutex<std::collections::BTreeMap<RawPath, Retry>>>,
}
fn scan_error(e: impl std::fmt::Display) -> ScanError {
    ScanError::Resource(e.to_string())
}
impl Recorder {
    fn persist(&self, entry: &ScanEntry, finish: bool) -> Result<(), ScanError> {
        let (tx, rx) = bounded(1);
        let message = if finish {
            Message::ScanFinished(copy_entry(entry), tx)
        } else {
            Message::ScanStarted(copy_entry(entry), tx)
        };
        self.send(message)?;
        self.ack(&rx)?.map_err(scan_error)
    }
    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
            || self.shutdown.load(Ordering::Acquire)
            || self.deadline.lock().is_some_and(|d| Instant::now() >= d)
    }
    fn send(&self, mut message: Message) -> Result<(), ScanError> {
        let end = Instant::now() + ACK_TIMEOUT;
        loop {
            if self.stopped() {
                return Err(ScanError::Cancelled);
            }
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(scan_error("writer queue timed out; message not committed"));
            }
            match self.writer.send_timeout(message, remaining.min(POLL)) {
                Ok(()) => return Ok(()),
                Err(crossbeam_channel::SendTimeoutError::Timeout(m)) => message = m,
                Err(e) => return Err(scan_error(e)),
            }
        }
    }
    fn ack<T>(&self, rx: &Receiver<T>) -> Result<T, ScanError> {
        let end = Instant::now() + ACK_TIMEOUT;
        loop {
            // Once enqueued, interruption cannot prove the writer did not commit.
            if self.stopped() {
                return Err(scan_error(
                    "persistence uncertain: acknowledgement interrupted",
                ));
            }
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(scan_error(
                    "persistence uncertain: acknowledgement timed out",
                ));
            }
            match rx.recv_timeout(remaining.min(POLL)) {
                Ok(value) => return Ok(value),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(e) => return Err(scan_error(format!("persistence uncertain: {e}"))),
            }
        }
    }

    fn status(&self, entry: &ScanEntry, running: bool) -> Result<(), ScanError> {
        let root = root_label(&entry.root);
        let previous = self.previous.lock();
        let last = if entry.status == "complete" {
            Some(entry)
        } else {
            previous.as_ref()
        };
        let cfg = self
            .config
            .roots
            .iter()
            .find(|r| r.path == entry.root.as_path());
        let next_due = cfg
            .map(|r| {
                next_run(
                    &parse_schedule(&r.schedule)?,
                    r.window.as_deref().map(parse_window).transpose()?.as_ref(),
                    last.and_then(|e| e.finished_at),
                    (self.clock)(),
                )
            })
            .transpose()
            .map_err(scan_error)?
            .flatten();
        self.state
            .publish(
                Publication::Scan(ScanStatus {
                    root,
                    running: running.then(|| ScanProgress {
                        scan_id: entry.scan_id.clone(),
                        phase: "scan".into(),
                        dirs: 0,
                        entries: 0,
                        started_at: entry.started_at,
                        rate: 0.0,
                    }),
                    last_complete: last.map(last_scan),
                    next_due,
                }),
                scheduler_health(&self.failures, "scan state published"),
            )
            .map_err(scan_error)
    }
    fn post_scan(&self, entry: &ScanEntry) -> Result<(), DaemonError> {
        self.post_scan_with(entry, &crate::metrics::SnapshotMetrics::refresh)
    }
    fn post_scan_with(
        &self,
        entry: &ScanEntry,
        refresh: &dyn Fn(
            &crate::metrics::SnapshotMetrics,
            &Snapshot,
            u32,
        ) -> Result<(), DaemonError>,
    ) -> Result<(), DaemonError> {
        let default = tidemark_core::config::RootConfig::default();
        let cfg = self
            .config
            .roots
            .iter()
            .find(|r| r.path == entry.root.as_path())
            .unwrap_or(&default);
        let file = entry
            .file
            .as_ref()
            .ok_or_else(|| DaemonError::Scan("complete scan has no file".into()))?;
        let snapshot = Snapshot::open(file).map_err(|e| DaemonError::Store(e.to_string()))?;
        let previous = self.previous.lock();
        if let Some(old) = previous.as_ref().and_then(|e| e.file.as_ref()) {
            let rows = tidemark_store::diff::diff(
                &snapshot,
                old,
                &entry.root,
                tidemark_store::diff::DiffMetric::Disk,
                cfg.series_depth,
                GROWTH_CAP + 1,
            )
            .map_err(|e| DaemonError::Store(e.to_string()))?;
            if rows.len() > GROWTH_CAP {
                return Err(DaemonError::Unavailable("directory growth exceeds complete inventory cap; growth invalidation requires coordinator interface".into()));
            }
            // Publish each supported depth without claiming a deeper list is
            // complete. Total temporary storage remains bounded by GROWTH_CAP.
            if cfg.series_depth > 128 {
                return Err(DaemonError::Unavailable(
                    "growth depth exceeds scheduler cap 128".into(),
                ));
            }
            for depth in 0..=cfg.series_depth {
                let inventory = rows
                    .iter()
                    .filter(|r| r.depth_rel <= depth)
                    .map(|r| (r.path.clone(), r.delta))
                    .collect();
                self.state.publish(
                    Publication::DirGrowth((root_label(&entry.root), depth, inventory)),
                    component("growth", "ok", "complete snapshot diff"),
                )?;
            }
        }
        if let Some(metrics) = self.state.view().snapshot_metrics_handle() {
            refresh(&metrics, &snapshot, self.config.metrics.dir_metrics_depth)?;
            self.state.publish_component(component(
                "snapshot_metrics",
                "ok",
                "shared cache refreshed after scan",
            ))?;
        } else {
            // Keep construction compatible when W6-B replaces its unit stub.
            #[allow(clippy::default_constructed_unit_structs)]
            let metrics = crate::metrics::SnapshotMetrics::default();
            refresh(&metrics, &snapshot, self.config.metrics.dir_metrics_depth)?;
            self.state.publish(
                Publication::SnapshotMetrics(metrics),
                component("snapshot_metrics", "ok", "refreshed after scan"),
            )?;
        }
        self.state.view().clear_watch_dirty(&entry.root)?;
        Ok(())
    }
}
fn copy_entry(e: &ScanEntry) -> ScanEntry {
    ScanEntry {
        scan_id: e.scan_id.clone(),
        root: e.root.clone(),
        mode: e.mode,
        started_at: e.started_at,
        finished_at: e.finished_at,
        status: e.status.clone(),
        degraded: e.degraded,
        file: e.file.clone(),
        t_disk: e.t_disk,
        t_inodes: e.t_inodes,
        t_dirs: e.t_dirs,
        errors: e.errors,
        duration_ms: e.duration_ms,
    }
}
// String-only DTO roots are exact configured UTF-8 paths (SCR-002).
// Byte-only request paths use the normative escaped display fallback.
fn root_label(root: &RawPath) -> String {
    root.as_path()
        .to_str()
        .map_or_else(|| root.display_lossy(), str::to_owned)
}
fn last_scan(e: &ScanEntry) -> LastScan {
    LastScan {
        scan_id: e.scan_id.clone(),
        finished_at: e.finished_at.unwrap_or(e.started_at),
        mode: format!("{:?}", e.mode).to_lowercase(),
        duration_ms: e.duration_ms.unwrap_or(0),
    }
}
impl ScanRecorder for Recorder {
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
        // run_scan holds the host lock here. Capture the actual predecessor,
        // rather than a catalog observation made before lock acquisition.
        *self.previous.lock() =
            Catalog::open(&self.config.general.state_dir, false)?.latest(&e.root)?;
        self.persist(e, false)?;
        // Admission has passed the orchestrator's space guard. Busy or failed
        // preflight must not fabricate a recovered state-space measurement.
        self.state
            .publish(
                Publication::StateSpaceRefused(false),
                scheduler_health(&self.failures, "space guard admitted scan"),
            )
            .map_err(scan_error)?;
        self.status(e, true)?;
        tracing::info!(root = %e.root.display_lossy(), scan_id = %e.scan_id, "scan started");
        if let Some(reply) = self.reply.lock().take() {
            let _ = reply.try_send(Ok(ScanStartResult {
                scan_id: e.scan_id.clone(),
            }));
        }
        Ok(())
    }
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
        self.persist(e, true)?;
        if e.status == "complete" {
            // Persistence succeeded: post-scan failure must not reclassify an
            // immutable complete snapshot as failed. Leave dirty state intact.
            if let Err(error) = self.post_scan(e) {
                self.state
                    .publish_component(component("post_scan", "degraded", error.to_string()))
                    .map_err(scan_error)?;
                tracing::warn!(%error, "post-scan publication incomplete; dirty state retained");
            }
        }
        self.status(e, false)
    }
    fn dir_series(
        &self,
        root: &RawPath,
        scan_id: &str,
        ts: i64,
        rows: &[DirRow],
    ) -> Result<(), ScanError> {
        // The root row is present in every nonempty scan series. Checking its
        // primary-key second coalesces whole observations, including restart.
        let ts_reader = tidemark_store::timeseries::TimeSeries::open(
            &self.config.general.state_dir.join("timeseries.db"),
            false,
        )?;
        if !ts_reader.query_dir_series(root, root, ts, ts)?.is_empty() {
            return Ok(());
        }
        drop(ts_reader);
        if rows.len() > 20_000 {
            return Err(scan_error("directory series exceeds 20,000 rows"));
        }
        self.send(Message::DirSeries((
            root.clone(),
            scan_id.into(),
            ts,
            rows.to_vec(),
        )))?;
        // Barrier preserves series-before-retention ordering on the one writer.
        let (tx, rx) = bounded(1);
        self.send(Message::Flush(tx)).map_err(|e| {
            scan_error(format!(
                "persistence uncertain: series queued but barrier not acknowledged: {e}"
            ))
        })?;
        self.ack(&rx)?;
        Ok(())
    }
}

struct ReleaseActive(Arc<Shared>);
impl Drop for ReleaseActive {
    fn drop(&mut self) {
        let mut active = self.0.active.lock();
        self.0.state.release_scan();
        *active = None;
    }
}

fn launch(
    command: Command,
    shared: Arc<Shared>,
    writer: tswriter::Sender,
) -> Result<crate::ServiceHandle, DaemonError> {
    std::thread::Builder::new()
        .name("tidemark-scan".into())
        .spawn(move || {
            let _release = ReleaseActive(shared.clone());
            let (config, _) = shared.state.config();
            let mut config = (*config).clone();
            config.general.state_dir = shared.state.storage_path();
            let config = Arc::new(config);
            let cfg = config
                .roots
                .iter()
                .find(|r| r.path == command.params.root.as_path());
            let mode = command
                .params
                .mode
                .as_ref()
                .or_else(|| cfg.map(|r| &r.mode));
            let mode = match mode {
                Some(RootMode::Walk) => ModeRequest::Walk,
                Some(RootMode::Bulk) => ModeRequest::Bulk,
                _ => ModeRequest::Auto,
            };
            let throttle = command.params.throttle.map(|t| {
                match t {
                    ThrottlePreset::Gentle => "gentle",
                    ThrottlePreset::Normal => "normal",
                    ThrottlePreset::Max => "max",
                }
                .to_owned()
            });
            let req = ScanRequest {
                root: command.params.root,
                mode,
                throttle,
                threads: None,
                exclude_extra: vec![],
                keep_partial: false,
                annotations: vec![],
            };
            let recorder = Recorder {
                cancel: command.cancel.clone(),
                shutdown: shared.shutdown.clone(),
                deadline: shared.deadline.clone(),
                state: shared.state.clone(),
                writer,
                config: config.clone(),
                reply: Mutex::new(Some(command.reply)),
                previous: Mutex::new(None),
                failures: shared.failures.clone(),
                clock: shared.clock.clone(),
            };
            let presence = crate::mounts::read()
                .and_then(|mounts| crate::mounts::check_scan(&config, &req.root, &mounts))
                .map_err(scan_error);
            let result = presence.and_then(|()| run_scan(&config, &req, &recorder, &command.cancel, &|p| {
                if let Some(mut s) = shared
                    .state
                    .view()
                    .scans()
                    .and_then(|v| v.into_iter().find(|s| s.root == root_label(&req.root)))
                {
                    if let Some(r) = s.running.as_mut() {
                        r.phase = p.phase.into();
                        r.dirs = p.dirs;
                        r.entries = p.entries;
                        r.rate = p.rate;
                    }
                    let _ = shared.state.publish(
                        Publication::Scan(s),
                        scheduler_health(&shared.failures, "scan running"),
                    );
                }
            }));
            if let Err(e) = result {
                if let Some(reply) = recorder.reply.lock().take() {
                    let _ = reply.try_send(Err(DaemonError::Scan(e.to_string())));
                }
                if !matches!(e, ScanError::Cancelled | ScanError::Busy) {
                    let mut failures = shared.failures.lock();
                    let attempts = failures
                        .get(&req.root)
                        .map_or(1, |r| r.attempts.saturating_add(1));
                    let due = retry_due((shared.monotonic)(), attempts);
                    if failures.len() == crate::state::MAX_SUBJECTS
                        && !failures.contains_key(&req.root)
                    {
                        failures.pop_first();
                    }
                    failures.insert(
                        req.root.clone(),
                        Retry { attempts, due, error: e.to_string(), configured: cfg.is_some() },
                    );
                    let mode = match req.mode {
                        ModeRequest::Walk => "walk",
                        ModeRequest::Bulk => "bulk",
                        ModeRequest::Auto => "auto",
                    };
                    tracing::warn!(
                        root = %req.root.display_lossy(), mode, scheduled = command.scheduled,
                        error = %e, attempt = attempts, next_due = due, "scan failed"
                    );
                } else {
                    // Preserve the existing 30-second scheduled deferral without
                    // counting Busy/cancellation as a failure or claiming recovery.
                    let mut failures = shared.failures.lock();
                    let due = (shared.monotonic)().saturating_add(30);
                    if failures.len() == crate::state::MAX_SUBJECTS
                        && !failures.contains_key(&req.root)
                    {
                        failures.pop_first();
                    }
                    failures.entry(req.root.clone()).and_modify(|r| r.due = due).or_insert(Retry {
                        attempts: 0, due, error: String::new(), configured: cfg.is_some(),
                    });
                }
                let health = scheduler_health(&shared.failures, &e.to_string());
                let retry_due = if cfg.is_some_and(|r| r.schedule.trim() != "off") {
                    shared.failures.lock().get(&req.root).map(|r| r.due)
                } else {
                    None
                };
                if let Some(due) = retry_due {
                    if let Some(mut status) = shared.state.view().scans().and_then(|v| {
                        v.into_iter().find(|s| s.root == root_label(&req.root))
                    }) {
                        status.next_due = Some((shared.clock)().saturating_add(due.saturating_sub((shared.monotonic)())));
                        shared.state.publish(Publication::Scan(status), health.clone())?;
                    }
                }
                if matches!(e, ScanError::NoSpace) {
                    shared
                        .state
                        .publish(Publication::StateSpaceRefused(true), health)?;
                } else {
                    shared.state.publish_component(health)?;
                }
                return Err(DaemonError::Scan(e.to_string()));
            } else {
                shared.failures.lock().remove(&req.root);
                if let Ok(outcome) = &result {
                    tracing::info!(root = %req.root.display_lossy(), scan_id = %outcome.scan_id, "scan completed");
                }
                shared.state.publish(
                    Publication::StateSpaceRefused(false),
                    scheduler_health(&shared.failures, "scan complete"),
                )?;
            }

            Ok(())
        })
        .map_err(|source| DaemonError::Io {
            context: "spawn scan".into(),
            source,
        })
}

#[cfg(test)]
fn report_firstboot(state: &LiveState, now: i64) -> Result<bool, DaemonError> {
    let elapsed = state
        .view()
        .firstboot_inputs()
        .map_or(0, |i| now.saturating_sub(i.daemon_started_at));
    report_firstboot_elapsed(state, now, elapsed)
}
fn report_firstboot_elapsed(
    state: &LiveState,
    now: i64,
    elapsed: i64,
) -> Result<bool, DaemonError> {
    let Some(inputs) = state.view().firstboot_inputs() else {
        return Ok(false);
    };
    let path = state.config().0.general.state_dir.clone();
    if !crate::firstboot::needs_report(&path, &inputs.config_sha256) {
        return Ok(false);
    }
    let (config, _) = state.config();
    let watcher_reported = inputs
        .components
        .iter()
        .any(|c| c.name == "watcher" && (c.state == "disabled" || c.state == "error"))
        || (!config.watch.mounts.is_empty()
            && config.watch.mounts.iter().all(|mount| {
                let name = format!("watcher:{}", mount.to_string_lossy());
                inputs.components.iter().any(|c| c.name == name)
            }));
    let ready = watcher_reported
        && [
            "sampler",
            "lvm",
            "procscan",
            "alerts",
            "tswriter",
            "scheduler",
            "api",
            "metrics",
        ]
        .iter()
        .all(|name| inputs.components.iter().any(|c| c.name == *name));
    if !ready && elapsed < 120 {
        return Ok(false);
    }
    let report = crate::firstboot::build(&inputs, now);
    crate::firstboot::write(&path, &report).map_err(|source| DaemonError::Io {
        context: "firstboot report".into(),
        source,
    })?;
    Ok(true)
}

/// Start the scheduler and first-boot coordinator. Reloads are observed at each
/// idle boundary; a running scan retains its admitted configuration. run_scan
/// exclusively owns the host lock, gate, space guard and retention.
pub fn start(
    config: Arc<Config>,
    state: LiveState,
    writer: tswriter::Sender,
    shutdown: Shutdown,
) -> Result<(ScanControl, crate::ServiceHandle), DaemonError> {
    start_with_clock(config, state, writer, shutdown, Arc::new(crate::unix_now))
}
/// Inject wall time for calendar schedules; operational intervals use real elapsed time.
/// Scan metadata timestamps remain owned by the orchestrator.
pub fn start_with_clock(
    config: Arc<Config>,
    state: LiveState,
    writer: tswriter::Sender,
    shutdown: Shutdown,
    clock: crate::Clock,
) -> Result<(ScanControl, crate::ServiceHandle), DaemonError> {
    start_with_clocks(
        config,
        state,
        writer,
        shutdown,
        clock,
        crate::monotonic_clock(),
    )
}
/// Start with independent wall and elapsed-seconds clocks.
pub fn start_with_clocks(
    config: Arc<Config>,
    state: LiveState,
    writer: tswriter::Sender,
    shutdown: Shutdown,
    clock: crate::Clock,
    monotonic: crate::Clock,
) -> Result<(ScanControl, crate::ServiceHandle), DaemonError> {
    start_inner(
        config,
        state,
        writer,
        shutdown,
        clock,
        monotonic,
        Arc::new(Mutex::new(None)),
    )
}
pub(crate) fn start_with_deadline(
    config: Arc<Config>,
    state: LiveState,
    writer: tswriter::Sender,
    shutdown: Shutdown,
    deadline: Arc<Mutex<Option<Instant>>>,
) -> Result<(ScanControl, crate::ServiceHandle), DaemonError> {
    start_inner(
        config,
        state,
        writer,
        shutdown,
        Arc::new(crate::unix_now),
        crate::monotonic_clock(),
        deadline,
    )
}
fn start_inner(
    config: Arc<Config>,
    state: LiveState,
    writer: tswriter::Sender,
    shutdown: Shutdown,
    clock: crate::Clock,
    monotonic: crate::Clock,
    deadline: Arc<Mutex<Option<Instant>>>,
) -> Result<(ScanControl, crate::ServiceHandle), DaemonError> {
    crate::state::validate_bounds(&config)?;
    for root in &config.roots {
        parse_schedule(&root.schedule)?;
        root.window.as_deref().map(parse_window).transpose()?;
    }
    let (tx, rx) = bounded(1);
    let shared = Arc::new(Shared {
        state,
        shutdown,
        active: Mutex::new(None),
        deadline,
        commands: tx,
        clock,
        monotonic,
        failures: Arc::new(Mutex::new(std::collections::BTreeMap::new())),
    });
    let control = ScanControl {
        shared: Some(shared.clone()),
    };
    let handle = std::thread::Builder::new()
        .name("tidemark-scheduler".into())
        .spawn(move || scheduler(shared, rx, writer))
        .map_err(|source| DaemonError::Io {
            context: "spawn scheduler".into(),
            source,
        })?;
    Ok((control, handle))
}
// Firstboot can become ready immediately when the lifecycle reports disabled
// optional services. Populate every root before that first readiness check,
// even when the first due scan will occupy the host slot for a long time.
fn initialize_schedules(state: &LiveState, now: i64) -> Result<(), DaemonError> {
    let (config, generation) = state.config();
    let catalog = Catalog::open(&state.storage_path(), false)
        .map_err(|e| DaemonError::Store(e.to_string()))?;
    for root in &config.roots {
        let raw =
            RawPath::from_path(&root.path).map_err(|e| DaemonError::Invalid(e.to_string()))?;
        let last = catalog
            .latest(&raw)
            .map_err(|e| DaemonError::Store(e.to_string()))?;
        let next_due = next_run(
            &parse_schedule(&root.schedule)?,
            root.window
                .as_deref()
                .map(parse_window)
                .transpose()?
                .as_ref(),
            last.as_ref().and_then(|e| e.finished_at),
            now,
        )?;
        state.publish_scheduled(
            ScanStatus {
                root: root_label(&raw),
                running: None,
                last_complete: last.as_ref().map(last_scan),
                next_due,
            },
            generation,
            component("scheduler", "ok", "initial schedules published"),
        )?;
    }
    Ok(())
}
pub(crate) fn preflight_reload(state: &LiveState, config: &Config) -> Result<(), DaemonError> {
    let catalog = Catalog::open(&state.storage_path(), false)
        .map_err(|e| DaemonError::Store(e.to_string()))?;
    for root in &config.roots {
        let raw =
            RawPath::from_path(&root.path).map_err(|e| DaemonError::Invalid(e.to_string()))?;
        let last = catalog
            .latest(&raw)
            .map_err(|e| DaemonError::Store(e.to_string()))?;
        next_run(
            &parse_schedule(&root.schedule)?,
            root.window
                .as_deref()
                .map(parse_window)
                .transpose()?
                .as_ref(),
            last.as_ref().and_then(|e| e.finished_at),
            crate::unix_now(),
        )?;
    }
    Ok(())
}
struct SchedulerExit(Arc<Shared>);
impl Drop for SchedulerExit {
    fn drop(&mut self) {
        self.0.shutdown.store(true, Ordering::Release);
        if let Some(active) = self.0.active.lock().as_ref() {
            active.cancel.store(true, Ordering::Release);
        }
    }
}
struct Job {
    handle: Option<crate::ServiceHandle>,
    shared: Arc<Shared>,
}
impl Job {
    fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_none_or(|handle| handle.is_finished())
    }
    fn join(mut self) -> Result<Result<(), DaemonError>, DaemonError> {
        match self.handle.take() {
            Some(handle) => handle
                .join()
                .map_err(|_| DaemonError::Internal("scan worker panicked".into())),
            None => Ok(Ok(())),
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            if let Some(active) = self.shared.active.lock().as_ref() {
                active.cancel.store(true, Ordering::Release);
            }
            // Also join on scheduler errors and unwinding, before releasing the writer.
            if let Err(error) = handle.join() {
                tracing::error!(?error, "scan worker panicked during scheduler cleanup");
            }
        }
    }
}
fn scheduler(
    shared: Arc<Shared>,
    rx: Receiver<Command>,
    writer: tswriter::Sender,
) -> Result<(), DaemonError> {
    let _exit = SchedulerExit(shared.clone());
    let mut job: Option<Job> = None;
    let mut reported = false;
    let mut report_at = i64::MIN;
    let mut schedule_at = i64::MIN;
    let mut generation = u64::MAX;
    let started = (shared.monotonic)();
    let started_wall = (shared.clock)();
    // At most one completion anchor per current configured root. Only startup
    // history is translated from Unix timestamps; runtime completions use elapsed time.
    let mut completions = std::collections::BTreeMap::new();
    shared
        .state
        .publish_component(component("scheduler", "ok", "scheduler started"))?;
    initialize_schedules(&shared.state, started_wall)?;
    // Seed startup history before accepting commands, including for off roots.
    // A manual scan admitted immediately must be recognized as a runtime
    // completion rather than translated from its later wall timestamp.
    let initial_statuses = shared.state.view().scans().unwrap_or_default();
    for root in &shared.state.config().0.roots {
        let raw =
            RawPath::from_path(&root.path).map_err(|e| DaemonError::Invalid(e.to_string()))?;
        let label = root_label(&raw);
        let last = initial_statuses
            .iter()
            .find(|s| s.root == label)
            .and_then(|s| s.last_complete.as_ref());
        completions.insert(
            raw,
            (
                last.map(|e| e.scan_id.clone()),
                last.map(|e| started.saturating_add(e.finished_at.saturating_sub(started_wall))),
            ),
        );
    }
    while !shared.shutdown.load(Ordering::Acquire) {
        if job.as_ref().is_some_and(|j| j.is_finished()) {
            if let Some(j) = job.take() {
                match j.join()? {
                    Ok(()) | Err(DaemonError::Scan(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        let now = (shared.clock)();
        let elapsed = (shared.monotonic)();
        if !reported && elapsed >= report_at {
            report_at = elapsed.saturating_add(1);
            match report_firstboot_elapsed(&shared.state, now, elapsed.saturating_sub(started)) {
                Ok(done) => {
                    reported = done
                        || shared.state.view().firstboot_inputs().is_some_and(|i| {
                            !crate::firstboot::needs_report(
                                &shared.state.config().0.general.state_dir,
                                &i.config_sha256,
                            )
                        });
                }
                Err(e) => {
                    shared.state.publish_component(component(
                        "firstboot",
                        "error",
                        e.to_string(),
                    ))?;
                    reported = true;
                }
            }
        }
        match rx.recv_timeout(POLL) {
            Ok(command) => {
                if job.is_some() {
                    let _ = command
                        .reply
                        .try_send(Err(DaemonError::Scan("Busy".into())));
                    *shared.active.lock() = None;
                } else {
                    job = Some(Job {
                        handle: Some(launch(command, shared.clone(), writer.clone())?),
                        shared: shared.clone(),
                    });
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        if job.is_some() || shared.active.lock().is_some() {
            continue;
        }
        let (config, current_generation) = shared.state.config();
        if elapsed < schedule_at && generation == current_generation {
            continue;
        }
        schedule_at = elapsed.saturating_add(1);
        if generation != current_generation {
            completions
                .retain(|root: &RawPath, _| config.roots.iter().any(|r| r.path == root.as_path()));
            // Reload resets delays and attempts, but cannot claim a failed root recovered.
            shared.failures.lock().values_mut().for_each(|r| {
                r.attempts = 0;
                r.due = elapsed;
            });
        }
        generation = current_generation;
        shared.failures.lock().retain(|root, retry| {
            !retry.configured || config.roots.iter().any(|r| r.path == root.as_path())
        });
        shared.state.publish_component(scheduler_health(
            &shared.failures,
            "waiting for scheduled scan",
        ))?;
        let catalog = Catalog::open(&shared.state.storage_path(), false)
            .map_err(|e| DaemonError::Store(e.to_string()))?;
        for root in &config.roots {
            let raw =
                RawPath::from_path(&root.path).map_err(|e| DaemonError::Invalid(e.to_string()))?;
            let last = catalog
                .latest(&raw)
                .map_err(|e| DaemonError::Store(e.to_string()))?;
            let last_finished = last.as_ref().and_then(|e| e.finished_at);
            let last_id = last.as_ref().map(|e| e.scan_id.as_str());
            let completion = completions.entry(raw.clone()).or_insert_with(|| {
                (
                    last_id.map(str::to_owned),
                    last_finished.map(|finished| {
                        started.saturating_add(finished.saturating_sub(started_wall))
                    }),
                )
            });
            if completion.0.as_deref() != last_id {
                *completion = (last_id.map(str::to_owned), last_finished.map(|_| elapsed));
            }
            let schedule = parse_schedule(&root.schedule)?;
            let window = root.window.as_deref().map(parse_window).transpose()?;
            let operational_due = match schedule {
                Schedule::Every(interval) => Some(
                    completion
                        .1
                        .map_or(elapsed, |done| done.saturating_add(interval)),
                ),
                _ => None,
            };
            let due = if let Some(deadline) = operational_due {
                // Windows remain calendar-based even when cadence uses elapsed time.
                next_run(
                    &schedule,
                    window.as_ref(),
                    None,
                    now.saturating_add(deadline.saturating_sub(elapsed).max(0)),
                )?
            } else {
                next_run(&schedule, window.as_ref(), last_finished, now)?
            };
            let next_due = due.map(|scheduled| {
                shared.failures.lock().get(&raw).map_or(scheduled, |r| {
                    now.saturating_add(r.due.saturating_sub(elapsed).max(0))
                })
            });
            if let Err(error) = shared.state.publish_scheduled(
                ScanStatus {
                    root: root_label(&raw),
                    running: None,
                    last_complete: last.as_ref().map(last_scan),
                    next_due,
                },
                current_generation,
                scheduler_health(&shared.failures, "waiting for scheduled scan"),
            ) {
                shared
                    .state
                    .publish_component(component("config", "error", error.to_string()))?;
                tracing::error!(%error, "scan inventory publication rejected; scheduler continuing");
                continue;
            }
            if due.is_some_and(|d| d <= now)
                && operational_due.is_none_or(|d| d <= elapsed)
                && shared
                    .failures
                    .lock()
                    .get(&raw)
                    .is_none_or(|r| r.due <= elapsed)
            {
                let mut active = shared.active.lock();
                if active.is_some() {
                    break;
                }
                match shared
                    .state
                    .reserve_scheduled(root_label(&raw), current_generation)
                {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) => {
                        shared.state.publish_component(component(
                            "config",
                            "error",
                            error.to_string(),
                        ))?;
                        tracing::error!(%error, "scheduled scan capacity rejected; scheduler continuing");
                        continue;
                    }
                }
                let cancel = Arc::new(AtomicBool::new(false));
                *active = Some(Active {
                    root: raw.clone(),
                    cancel: cancel.clone(),
                });
                drop(active);
                let (tx, _) = bounded(1);
                job = Some(Job {
                    handle: Some(launch(
                        Command {
                            params: ScanStartParams {
                                root: raw,
                                mode: None,
                                throttle: None,
                            },
                            scheduled: true,
                            reply: tx,
                            cancel,
                        },
                        shared.clone(),
                        writer.clone(),
                    )?),
                    shared: shared.clone(),
                });
                break;
            }
        }
    }
    if let Some(active) = shared.active.lock().as_ref() {
        active.cancel.store(true, Ordering::Release);
    }
    if let Some(job) = job {
        tracing::info!(
            "waiting for scan cancellation; daemon lifecycle enforces the shutdown deadline"
        );
        match job.join()? {
            Ok(()) | Err(DaemonError::Scan(_)) => {}
            Err(e) => return Err(e),
        }
    }
    while let Ok(command) = rx.try_recv() {
        let _ = command.reply.try_send(Err(DaemonError::Shutdown));
    }
    *shared.active.lock() = None;
    shared
        .state
        .publish_component(component("scheduler", "disabled", "scheduler stopped"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidemark_live::alerts::AlertSource;
    fn config(root: &std::path::Path, storage: &std::path::Path) -> Config {
        let mut cfg = Config::default();
        cfg.general.state_dir = storage.to_owned();
        cfg.roots = vec![tidemark_core::config::RootConfig {
            path: root.to_owned(),
            schedule: "off".into(),
            window: None,
            mode: RootMode::Walk,
            throttle: "max".into(),
            ..Default::default()
        }];
        cfg.scan.threads = 1;
        cfg
    }
    fn recorder(
        cfg: Arc<Config>,
        state: LiveState,
        writer: tswriter::Sender,
        previous: Option<ScanEntry>,
    ) -> Recorder {
        Recorder {
            cancel: Shutdown::default(),
            shutdown: Shutdown::default(),
            deadline: Arc::new(Mutex::new(None)),
            config: cfg,
            state,
            writer,
            reply: Mutex::new(None),
            previous: Mutex::new(previous),
            clock: Arc::new(crate::unix_now),
            failures: Arc::new(Mutex::new(std::collections::BTreeMap::new())),
        }
    }
    #[test]
    fn retry_sequence_uses_injected_time_and_caps_at_one_hour() {
        let mut now = 100;
        for (attempt, delay) in [30, 60, 120, 240, 480, 960, 1920, 3600, 3600]
            .into_iter()
            .enumerate()
        {
            let due = retry_due(now, attempt as u32 + 1);
            assert_eq!(due, now + delay);
            now = due;
        }
        assert_eq!(retry_due(now, 1), now + 30);
        assert_eq!(retry_due(i64::MAX, u32::MAX), i64::MAX);
    }
    #[test]
    fn callbacks_wait_for_persistence_and_propagate_writer_error() {
        let root = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let cfg = Arc::new(config(root.path(), storage.path()));
        drop(Catalog::open(storage.path(), true).unwrap());
        let state = LiveState::new(&cfg, 0);
        let (writer, rx) = bounded(1);
        let rec = recorder(cfg, state, writer, None);
        let entry = ScanEntry {
            scan_id: "test".into(),
            root: RawPath::from_path(root.path()).unwrap(),
            mode: tidemark_core::scan::ScanMode::Walk,
            started_at: 0,
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
        let (done, complete) = bounded(1);
        let worker = std::thread::spawn(move || {
            let r = rec.scan_started(&entry);
            done.send(r).unwrap();
        });
        let Message::ScanStarted(_, ack) = rx.recv_timeout(ACK_TIMEOUT).unwrap() else {
            panic!("wrong message");
        };
        assert!(complete.try_recv().is_err());
        ack.send(Err(DaemonError::Store(
            "injected persistence failure".into(),
        )))
        .unwrap();
        assert!(complete
            .recv_timeout(ACK_TIMEOUT)
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("injected persistence failure"));
        worker.join().unwrap();
    }
    #[test]
    fn recorder_interrupts_stalled_queue_and_uncertain_ack() {
        for (enqueued, daemon_stop) in [(false, false), (true, false), (false, true), (true, true)]
        {
            let root = tempfile::tempdir().unwrap();
            let storage = tempfile::tempdir().unwrap();
            let cfg = Arc::new(config(root.path(), storage.path()));
            let state = LiveState::new(&cfg, 0);
            let (writer, rx) = bounded(1);
            let rec = recorder(cfg, state, writer.clone(), None);
            let cancel = if daemon_stop {
                rec.shutdown.clone()
            } else {
                rec.cancel.clone()
            };
            if !enqueued {
                let (tx, _) = bounded(1);
                writer.send(Message::Flush(tx)).unwrap();
            }
            let entry = ScanEntry {
                scan_id: "stalled".into(),
                root: RawPath::from_path(root.path()).unwrap(),
                mode: tidemark_core::scan::ScanMode::Walk,
                started_at: 0,
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
            let (done, result) = bounded(1);
            let worker = std::thread::spawn(move || done.send(rec.persist(&entry, false)).unwrap());
            // Keep acknowledgement sender alive, so disconnect cannot mask cancellation.
            let held = if enqueued {
                Some(rx.recv_timeout(Duration::from_secs(1)).unwrap())
            } else {
                None
            };
            cancel.store(true, Ordering::Release);
            let error = result
                .recv_timeout(Duration::from_millis(500))
                .unwrap()
                .unwrap_err();
            if enqueued {
                assert!(
                    error.to_string().contains("persistence uncertain"),
                    "{error}"
                );
            } else {
                assert!(matches!(error, ScanError::Cancelled));
                assert!(matches!(rx.try_recv().unwrap(), Message::Flush(_)));
                assert!(rx.try_recv().is_err());
            }
            worker.join().unwrap();
            drop(held);
        }
    }
    #[test]
    fn recorder_shared_deadline_interrupts_series_queue_and_barrier_ack() {
        let root = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let cfg = Arc::new(config(root.path(), storage.path()));
        let state = LiveState::new(&cfg, 0);
        let (writer, rx) = bounded(1);
        let rec = recorder(cfg, state, writer, None);
        *rec.deadline.lock() = Some(Instant::now());
        let (tx, ack) = bounded(1);
        assert!(matches!(
            rec.send(Message::Flush(tx)),
            Err(ScanError::Cancelled)
        ));
        assert!(rx.try_recv().is_err());
        assert!(rec
            .ack(&ack)
            .unwrap_err()
            .to_string()
            .contains("persistence uncertain"));
    }
    #[test]
    fn series_cancellation_interrupts_each_queue_and_ack_wait() {
        for phase in 0..3 {
            let root = tempfile::tempdir().unwrap();
            let storage = tempfile::tempdir().unwrap();
            let cfg = Arc::new(config(root.path(), storage.path()));
            drop(Catalog::open(storage.path(), true).unwrap());
            let state = LiveState::new(&cfg, 0);
            let (writer, rx) = bounded(1);
            let rec = recorder(cfg, state, writer.clone(), None);
            let cancel = rec.cancel.clone();
            if phase == 0 {
                let (tx, _) = bounded(1);
                writer.send(Message::Flush(tx)).unwrap();
            }
            let raw = RawPath::from_path(root.path()).unwrap();
            let (done, result) = bounded(1);
            let worker = std::thread::spawn(move || {
                done.send(rec.dir_series(&raw, "series", 1, &[])).unwrap()
            });
            let mut held = Vec::new();
            if phase == 1 {
                let deadline = Instant::now() + Duration::from_secs(1);
                while writer.is_empty() {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                // Leave DirSeries occupying the slot, so Flush cannot enqueue.
            } else if phase == 2 {
                held.push(rx.recv_timeout(Duration::from_secs(1)).unwrap());
                held.push(rx.recv_timeout(Duration::from_secs(1)).unwrap());
                assert!(matches!(held[0], Message::DirSeries(_)));
                assert!(matches!(held[1], Message::Flush(_)));
            }
            cancel.store(true, Ordering::Release);
            let error = result
                .recv_timeout(Duration::from_millis(500))
                .unwrap()
                .unwrap_err();
            if phase == 0 {
                assert!(matches!(error, ScanError::Cancelled));
                assert!(matches!(rx.try_recv().unwrap(), Message::Flush(_)));
            } else {
                assert!(
                    error.to_string().contains("persistence uncertain"),
                    "{error}"
                );
            }
            worker.join().unwrap();
        }
    }
    #[test]
    fn firstboot_waits_for_components_or_deadline_and_writes_once() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        std::fs::set_permissions(storage.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(storage.path().join("deploy")).unwrap();
        std::fs::set_permissions(
            storage.path().join("deploy"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::write(storage.path().join("deploy/staged.json"), b"{}").unwrap();
        let cfg = config(root.path(), storage.path());
        let state = LiveState::new(&cfg, 10);
        state
            .publish(
                Publication::Deployment {
                    config_sha256: "test-config".into(),
                    started_at: 10,
                },
                component("startup", "ok", "test"),
            )
            .unwrap();
        assert!(!report_firstboot_elapsed(&state, 7200, 119).unwrap());
        assert!(!report_firstboot_elapsed(&state, 0, 119).unwrap());
        assert!(!report_firstboot(&state, 129).unwrap());
        assert!(report_firstboot_elapsed(&state, 0, 120).unwrap());
        let path = storage.path().join("deploy/firstboot.json");
        let bytes = std::fs::read(&path).unwrap();
        assert!(!report_firstboot(&state, 140).unwrap());
        assert_eq!(bytes, std::fs::read(&path).unwrap());
        std::fs::remove_file(&path).unwrap();
        for name in [
            "sampler",
            "lvm",
            "procscan",
            "alerts",
            "tswriter",
            "scheduler",
            "api",
            "metrics",
        ] {
            state
                .publish_component(component(name, "ok", "reported"))
                .unwrap();
        }
        for mount in &cfg.watch.mounts {
            state
                .publish_component(component(
                    &format!("watcher:{}", mount.to_string_lossy()),
                    "ok",
                    "reported",
                ))
                .unwrap();
        }
        assert!(report_firstboot(&state, 20).unwrap());
    }
    #[test]
    fn growth_precedes_metrics_publication_and_series_coalesces_across_restart() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        let storage = tempfile::tempdir().unwrap();
        let cfg = Arc::new(config(root.path(), storage.path()));
        let state = LiveState::new(&cfg, 0);
        let stop = Arc::new(AtomicBool::new(false));
        let (writer, wh) = tswriter::start(cfg.clone(), state.clone(), stop.clone()).unwrap();
        let req = ScanRequest {
            root: RawPath::from_path(root.path()).unwrap(),
            mode: ModeRequest::Walk,
            throttle: Some("max".into()),
            threads: Some(1),
            exclude_extra: vec![],
            keep_partial: false,
            annotations: vec![],
        };
        let first = recorder(cfg.clone(), state.clone(), writer.clone(), None);
        let cancel = AtomicBool::new(false);
        run_scan(&cfg, &req, &first, &cancel, &|_| {}).unwrap();
        let previous = Catalog::open(storage.path(), false)
            .unwrap()
            .latest(&req.root)
            .unwrap()
            .unwrap();
        std::fs::write(root.path().join("child/new"), b"new allocation").unwrap();
        let second = recorder(
            cfg.clone(),
            state.clone(),
            writer.clone(),
            Some(copy_entry(&previous)),
        );
        let outcome = run_scan(&cfg, &req, &second, &cancel, &|_| {}).unwrap();
        let current = Catalog::open(storage.path(), false)
            .unwrap()
            .get(&outcome.scan_id)
            .unwrap()
            .unwrap();
        // Inject only the not-yet-owned cache implementation; assert ordering
        // against the actual published diff and immutable snapshots.
        second
            .post_scan_with(&current, &|_, _, _| {
                assert!(state.view().dir_growth(&root_label(&req.root), 2).is_some());
                Ok(())
            })
            .unwrap();
        assert!(state.view().snapshot_metrics_handle().is_some());
        let snap = Snapshot::open(current.file.as_ref().unwrap()).unwrap();
        let rows = vec![snap.root().unwrap()];
        second
            .dir_series(&req.root, "first-observation", 123, &rows)
            .unwrap();
        // A fresh Recorder simulates restart; the root/second query preserves
        // the first persisted observation without a second writer connection.
        let restarted = recorder(cfg, state, writer.clone(), None);
        restarted
            .dir_series(&req.root, "second-observation", 123, &rows)
            .unwrap();
        let reader = tidemark_store::timeseries::TimeSeries::open(
            &storage.path().join("timeseries.db"),
            false,
        )
        .unwrap();
        assert_eq!(
            reader
                .query_dir_series(&req.root, &req.root, 123, 123)
                .unwrap()
                .len(),
            1
        );
        drop(writer);
        stop.store(true, Ordering::Release);
        wh.join().unwrap().unwrap();
    }
    #[test]
    fn predecessor_survives_growth_and_series_then_retention_removes_it() {
        struct Observe<'a> {
            inner: &'a Recorder,
            old: &'a std::path::Path,
            finished: AtomicBool,
        }
        impl ScanRecorder for Observe<'_> {
            fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
                self.inner.scan_started(e)
            }
            fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
                self.inner.scan_finished(e)?;
                if e.status == "complete" {
                    assert!(self.old.exists());
                    assert!(self
                        .inner
                        .state
                        .view()
                        .dir_growth(&root_label(&e.root), 2)
                        .is_some());
                    self.finished.store(true, Ordering::Release);
                }
                Ok(())
            }
            fn dir_series(
                &self,
                root: &RawPath,
                id: &str,
                ts: i64,
                rows: &[DirRow],
            ) -> Result<(), ScanError> {
                assert!(self.finished.load(Ordering::Acquire));
                assert!(self.old.exists());
                self.inner.dir_series(root, id, ts, rows)
            }
        }
        let root = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let mut cfg = config(root.path(), storage.path());
        cfg.retention.snapshots = tidemark_core::config::SnapshotRetention {
            recent: 1,
            daily: 0,
            weekly: 0,
        };
        let cfg = Arc::new(cfg);
        let state = LiveState::new(&cfg, 0);
        let stop = Arc::new(AtomicBool::new(false));
        let (writer, wh) = tswriter::start(cfg.clone(), state.clone(), stop.clone()).unwrap();
        let req = ScanRequest {
            root: RawPath::from_path(root.path()).unwrap(),
            mode: ModeRequest::Walk,
            throttle: Some("max".into()),
            threads: Some(1),
            exclude_extra: vec![],
            keep_partial: false,
            annotations: vec![],
        };
        let rec = recorder(cfg.clone(), state, writer.clone(), None);
        let cancel = AtomicBool::new(false);
        let old = run_scan(&cfg, &req, &rec, &cancel, &|_| {})
            .unwrap()
            .file
            .unwrap();
        // Seed an explicitly older catalog observation through the same writer.
        // Real scans use a wall clock with one-second IDs; no elapsed-time wait
        // or random ID tie may decide this retention assertion.
        let mut older = Catalog::open(storage.path(), false)
            .unwrap()
            .latest(&req.root)
            .unwrap()
            .unwrap();
        older.started_at -= 86400;
        older.finished_at = older.finished_at.map(|t| t - 86400);
        rec.persist(&older, true).unwrap();
        std::fs::write(root.path().join("new"), b"fixture").unwrap();
        let observe = Observe {
            inner: &rec,
            old: &old,
            finished: AtomicBool::new(false),
        };
        let new = run_scan(&cfg, &req, &observe, &cancel, &|_| {})
            .unwrap()
            .file
            .unwrap();
        assert!(!old.exists());
        assert!(new.exists());
        drop(writer);
        stop.store(true, Ordering::Release);
        wh.join().unwrap().unwrap();
    }
    #[test]
    fn firstboot_has_every_roots_next_due_before_first_admission() {
        let root = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let mut cfg = config(root.path(), storage.path());
        cfg.roots[0].schedule = "every 6h".into();
        let mut second = cfg.roots[0].clone();
        second.path = root.path().join("second");
        cfg.roots.push(second);
        let state = LiveState::new(&cfg, 100);
        drop(Catalog::open(storage.path(), true).unwrap());
        state
            .publish(
                Publication::Deployment {
                    config_sha256: "test".into(),
                    started_at: 100,
                },
                component("startup", "ok", "reported"),
            )
            .unwrap();
        initialize_schedules(&state, 100).unwrap();
        let inputs = state.view().firstboot_inputs().unwrap();
        assert_eq!(inputs.next_scans.len(), 2);
        assert!(inputs.next_scans.iter().all(|(_, next)| *next == 100));
    }
}
