//! Owned by: W5-A
//! Mount-scoped fanotify worker and bounded hot activity state.

use parking_lot::Mutex;
use std::{
    collections::{BTreeSet, VecDeque},
    io,
    os::fd::{AsFd, AsRawFd, OwnedFd},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};
use tidemark_core::{
    live::{Attribution, HotEntry, HotMinute},
    path::RawPath,
};
use tidemark_fs::attribution::AttributionCache;
use tidemark_sys::fanotify::{mask, Fanotify, InitMode, RawEvent};

const FD_EVENT_BYTES: usize = 24;
const MAX_FD_BATCH: usize = 256;
static FD_BATCH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(MAX_FD_BATCH);

/// Set the process-wide fd-mode read budget, including already running watchers.
/// The transport holds between 16 and 256 descriptors per mount per read.
pub fn set_fd_batch(events: usize) {
    FD_BATCH.store(events.clamp(16, MAX_FD_BATCH), Ordering::Release);
}

fn fd_buffer_len() -> usize {
    fd_read_len(FD_BATCH.load(Ordering::Acquire))
}
fn fd_read_len(batch: usize) -> usize {
    batch.clamp(16, MAX_FD_BATCH) * FD_EVENT_BYTES
}
fn read_batch<T>(
    buffer: &mut [u8],
    length: usize,
    read: impl FnOnce(&mut [u8]) -> io::Result<T>,
) -> io::Result<T> {
    read(&mut buffer[..length])
}

fn descriptor_limit_reached() -> bool {
    let limit = rustix::process::getrlimit(rustix::process::Resource::Nofile).current;
    descriptor_limit_with(limit, count_descriptors)
}
fn count_descriptors(limit: u64) -> io::Result<u64> {
    use rustix::fs::{Mode, OFlags, RawDir};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let fd = match rustix::fs::open("/proc/self/fd", flags | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => rustix::fs::open("/proc/self/fd", flags, Mode::empty()),
        result => result,
    }?;
    let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 8192];
    let mut entries = RawDir::new(&fd, &mut buffer);
    let mut count = 0u64;
    while count < limit {
        let Some(entry) = entries.next() else {
            break;
        };
        let entry = entry?;
        if !matches!(entry.file_name().to_bytes(), b"." | b"..") {
            count += 1;
        }
    }
    Ok(count)
}

fn descriptor_limit_with(limit: Option<u64>, count: impl FnOnce(u64) -> io::Result<u64>) -> bool {
    let Some(limit) = limit else {
        return false;
    };
    // The procfs directory descriptor is included, conservatively detecting
    // the last remaining slot. Its open fails if no slot remains at all.
    match count(limit) {
        Ok(count) => count >= limit,
        Err(error) => error.raw_os_error().is_some_and(|code| {
            [
                rustix::io::Errno::MFILE,
                rustix::io::Errno::NFILE,
                rustix::io::Errno::NOMEM,
                rustix::io::Errno::NOBUFS,
            ]
            .iter()
            .any(|errno| code == errno.raw_os_error())
        }),
    }
}

/// Settings for one mount-scoped watcher.
pub struct WatcherOptions {
    /// Contract field.
    pub mount: RawPath,
    /// Contract field.
    pub events: Vec<String>,
    /// Contract field.
    pub hot_depth: usize,
    /// Contract field.
    pub exclude_prefixes: Vec<RawPath>,
    /// Contract field.
    pub lru_entries: usize,
}

/// Reserved for W5-A.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchMode {
    /// Contract variant.
    FdMount,
    /// Worker exited after an unrecoverable transport error.
    Failed,
    /// Contract variant.
    DfidNameMount,
    /// Contract variant.
    DfidNameFilesystem,
}
/// Reserved for W5-A.
#[derive(Clone, Copy)]
pub enum HotScope {
    /// Contract variant.
    Dir,
    /// Contract variant.
    Unit,
    /// Contract variant.
    Comm,
}
/// Counters and recent loss indicators for one watcher.
pub struct WatchStats {
    /// Contract field.
    pub mode: WatchMode,
    /// Contract field.
    pub events_total: u64,
    /// Contract field.
    pub overflows_total: u64,
    /// Contract field.
    pub lossy_minutes_15m: u32,
    /// Contract field.
    pub dropped_keys: u64,
    /// Foreground events discarded when the bounded buffer filled.
    pub dropped_events: u64,
    /// Whether the worker is retrying a transient resource failure.
    pub recovering: bool,
    /// Transport errors observed by the worker.
    pub read_errors: u64,
}
/// One directional foreground event; move events are never correlated into pairs.
#[derive(Clone, Debug)]
pub struct WatchEvent {
    /// Unix timestamp in seconds.
    pub at: i64,
    /// Raw event path using the shared path/path_b64 adapter.
    pub path: RawPath,
    /// Signed allocated-byte change.
    pub delta: i64,
    /// Best-effort process attribution.
    pub attribution: Attribution,
}
/// One completed wall-clock minute of one watcher, emitted once in timestamp order.
///
/// The first eligible minute is the first full minute after watcher startup. Empty
/// minutes are included. Only the retained 60-bucket history is available: missing
/// timestamps identify a gap and the first returned minute after it is lossy.
/// Clock rollback marks affected minutes lossy without repeating a timestamp.
/// Overflow totals never decrease and adjacent minute boundaries share one total.
#[derive(Clone, Debug)]
pub struct CompletedMinute {
    /// Minute start in epoch seconds, a multiple of 60.
    pub started_at: i64,
    /// Cumulative queue overflows at the start boundary.
    pub overflow_total_at_start: u64,
    /// Cumulative queue overflows at the end boundary.
    pub overflow_total_at_end: u64,
    /// Any loss from overflow, recovery, caps, rollback, or a preceding retention gap.
    pub lossy: bool,
    /// A per-minute list or distinct-file count was capped by storage or requested output.
    pub truncated: bool,
    /// Ranked directory rows, bounded by the existing completed-minute cap and top_k.
    pub dirs: Vec<HotEntry>,
    /// Persistence rows the legacy drain would return, sharing its drain cursor.
    /// Empty if those rows have already been handed out by that drain.
    pub persisted: Vec<HotMinute>,
}
/// Owns a bounded background watcher and joins it on stop or drop.
pub struct Watcher {
    mode: WatchMode,
    state: Arc<Mutex<State>>,
    shutdown: Arc<OwnedFd>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    mount: RawPath,
    mount_fd: Option<Arc<OwnedFd>>,
}
// Acknowledgements are FIFO and scoped to the current loss. Eviction is conservative.
const LOSS_ACK_CAP: usize = 1024;
struct State {
    rings: rings::Rings,
    dirty: BTreeSet<RawPath>,
    loss_pending: bool,
    loss_taken: VecDeque<RawPath>,
    last_at: Option<i64>,
    events: u64,
    overflows: u64,
    failed: bool,
    recovering: bool,
    read_errors: u64,
    stream: Option<VecDeque<WatchEvent>>,
    dropped_events: u64,
}
impl State {
    fn mark_loss(&mut self) {
        self.dirty.clear();
        self.loss_pending = true;
        self.loss_taken.clear();
    }
    fn observe_time(&mut self, at: i64) {
        if self.last_at.is_some_and(|last| at < last) {
            self.mark_loss();
            self.rings.overflow(at);
        }
        if self.failed || self.recovering {
            self.rings.recovery(at);
        }
        self.last_at = Some(at);
    }
}
fn now() -> i64 {
    tidemark_core::time::unix_now()
}

impl Watcher {
    /// Build a kernel-free watcher from attributed metadata events for integration tests.
    /// Events are `(timestamp, directory/unit/command keys, signed delta, unit, identity)`.
    /// The first event timestamp establishes startup; an empty source starts now.
    #[doc(hidden)]
    pub fn from_injected_events(
        mount: RawPath,
        events: impl IntoIterator<Item = (i64, [Vec<u8>; 3], i64, Option<String>, (u64, u64))>,
    ) -> io::Result<Self> {
        let mut rings = rings::Rings::new();
        let mut events = events.into_iter().peekable();
        rings.advance(events.peek().map_or_else(now, |event| event.0));
        for (at, keys, delta, unit, identity) in events {
            rings.add(at, keys, delta, unit, identity);
        }
        Ok(Self {
            mode: WatchMode::FdMount,
            state: Arc::new(Mutex::new(State {
                rings,
                dirty: BTreeSet::new(),
                loss_pending: false,
                loss_taken: VecDeque::new(),
                last_at: None,
                events: 0,
                overflows: 0,
                failed: false,
                recovering: false,
                read_errors: 0,
                stream: None,
                dropped_events: 0,
            })),
            shutdown: Arc::new(rustix::event::eventfd(
                0,
                rustix::event::EventfdFlags::NONBLOCK,
            )?),
            stopping: Arc::new(AtomicBool::new(false)),
            thread: None,
            mount,
            mount_fd: None,
        })
    }
    /// Check the host namespace, negotiate reporting, and spawn one worker.
    pub fn start(
        cfg: WatcherOptions,
        attribution: Arc<AttributionCache>,
    ) -> Result<Watcher, WatchError> {
        if !namespace_ok()? {
            return Err(WatchError::Namespace);
        }
        let caps = tidemark_sys::fanotify::probe(cfg.mount.as_path())?;
        let mut event_mask = mask::CLOSE_WRITE;
        for event in &cfg.events {
            match event.as_str() {
                "close_write" => {}
                "modify" => event_mask |= mask::MODIFY,
                _ => return Err(WatchError::Unsupported),
            }
        }
        let mount_fd = Arc::new(
            rustix::fs::open(
                cfg.mount.as_path(),
                rustix::fs::OFlags::PATH
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        let mark_path = mount_mark_path(&mount_fd);
        let mut selected = None;
        if caps.report_dfid_name {
            if let Ok(group) = Fanotify::init(InitMode::DfidName) {
                let rich = event_mask
                    | mask::CREATE
                    | mask::DELETE
                    | mask::MOVED_FROM
                    | mask::MOVED_TO
                    | mask::ONDIR;
                if caps.mark_filesystem && group.mark_filesystem(&mark_path, rich).is_ok() {
                    selected = Some((group, WatchMode::DfidNameFilesystem));
                } else if group.mark_mount(&mark_path, rich).is_ok() {
                    selected = Some((group, WatchMode::DfidNameMount));
                }
            }
        }
        let (group, mode) = select_group(selected, || {
            if !caps.fd_mode {
                return Err(WatchError::Unsupported);
            }
            let group = Fanotify::init(InitMode::FdMode).map_err(|_| WatchError::Unsupported)?;
            group
                .mark_mount(&mark_path, event_mask)
                .map_err(|_| WatchError::Unsupported)?;
            Ok(group)
        })?;
        let shutdown = Arc::new(
            rustix::event::eventfd(
                0,
                rustix::event::EventfdFlags::CLOEXEC | rustix::event::EventfdFlags::NONBLOCK,
            )
            .map_err(io::Error::from)?,
        );
        let state = Arc::new(Mutex::new(State {
            rings: rings::Rings::new(),
            dirty: BTreeSet::new(),
            loss_pending: false,
            loss_taken: VecDeque::new(),
            last_at: None,
            events: 0,
            overflows: 0,
            failed: false,
            recovering: false,
            read_errors: 0,
            stream: None,
            dropped_events: 0,
        }));
        state.lock().rings.advance(now());
        let worker_mount_fd = mount_fd.clone();
        let worker_state = state.clone();
        let worker_shutdown = shutdown.clone();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = stopping.clone();
        let mount = cfg.mount.clone();
        tracing::info!(mode = %mode, mount = %mount.display_lossy(), "watcher started: mount {}, mode {mode}", mount.display_lossy());
        let thread = std::thread::Builder::new()
            .name("tidemark-watch".into())
            .spawn(move || {
                let fd_mode = mode == WatchMode::FdMount;
                let mut buffer = vec![
                    0;
                    if fd_mode {
                        MAX_FD_BATCH * FD_EVENT_BYTES
                    } else {
                        65536
                    }
                ];
                let mut processor = Processor {
                    own_pid: std::process::id() as i32,
                    cfg: &cfg,
                    attribution: &attribution,
                    mount_fd: worker_mount_fd.as_fd(),
                    sizes: fdmode::Sizes::new(cfg.lru_entries, now()),
                    paths: fidmode::PathSizes::new(cfg.lru_entries),
                    handles: fidmode::HandlePaths::new(100_000),
                };
                loop {
                    if worker_stopping.load(Ordering::Relaxed) {
                        break;
                    }
                    let group_fd = group.as_fd();
                    let mut poll = [
                        rustix::event::PollFd::new(&group_fd, rustix::event::PollFlags::IN),
                        rustix::event::PollFd::new(
                            worker_shutdown.as_ref(),
                            rustix::event::PollFlags::IN,
                        ),
                    ];
                    let timeout = rustix::event::Timespec {
                        tv_sec: 0,
                        tv_nsec: 250_000_000,
                    };
                    if let Err(error) = rustix::event::poll(&mut poll, Some(&timeout)) {
                        if error == rustix::io::Errno::INTR {
                            continue;
                        }
                        tracing::warn!(%error,"fanotify poll failed: {error}");
                        if processor.transport_error(&io::Error::from(error), now(), &worker_state)
                            && retry_wait(worker_shutdown.as_ref(), &worker_stopping, 5)
                        {
                            continue;
                        }
                        break;
                    }
                    if !poll[1].revents().is_empty() {
                        break;
                    }
                    {
                        let mut state = worker_state.lock();
                        let at = now();
                        state.observe_time(at);
                        if state.recovering {
                            state.rings.recovery(at);
                        } else {
                            state.rings.advance(at);
                        }
                    }
                    if poll[0].revents().is_empty() {
                        continue;
                    }
                    let Some(events) = read_recover(
                        || {
                            let len = if fd_mode {
                                fd_buffer_len()
                            } else {
                                buffer.len()
                            };
                            read_batch(&mut buffer, len, |bytes| {
                                group.read(bytes).map(|events| events.collect::<Vec<_>>())
                            })
                        },
                        &mut processor,
                        &worker_state,
                        worker_shutdown.as_ref(),
                        &worker_stopping,
                        now,
                    ) else {
                        break;
                    };
                    if fd_mode && descriptor_limit_reached() {
                        processor.loss(now(), &worker_state);
                        let mut state = worker_state.lock();
                        state.read_errors = state.read_errors.saturating_add(1);
                        state.rings.read_failure(now(), true);
                    }
                    for event in events {
                        if worker_stopping.load(Ordering::Relaxed) {
                            break;
                        }
                        let at = now();
                        let Ok(event) = event else {
                            processor.loss(at, &worker_state);
                            continue;
                        };
                        processor.process(event, at, &worker_state);
                    }
                }
            })?;
        Ok(Self {
            mode,
            state,
            shutdown,
            stopping,
            thread: Some(thread),
            mount,
            mount_fd: Some(mount_fd),
        })
    }
    /// Return the reporting mode selected by successful marks.
    pub fn mode(&self) -> WatchMode {
        if self.state.lock().failed {
            WatchMode::Failed
        } else {
            self.mode
        }
    }
    /// Sum the most recent 1–60 minute buckets and return ranked entries.
    pub fn hot(&self, window_min: u32, scope: HotScope, limit: usize) -> Vec<HotEntry> {
        let mut state = self.state.lock();
        let at = now();
        state.observe_time(at);
        state.rings.advance(at);
        state.rings.hot(at, window_min, scope, limit)
    }
    /// Return each retained completed minute once, with at most top_k rows per scope.
    /// Persistence rows share a drain cursor with [`Self::drain_minutes`]: each row
    /// is handed out once, through whichever method drains its minute first.
    pub fn drain_completed_minutes(&self, top_k: usize) -> Vec<HotMinute> {
        self.drain_completed_minutes_at(now(), top_k)
    }
    fn drain_completed_minutes_at(&self, at: i64, top_k: usize) -> Vec<HotMinute> {
        let mut state = self.state.lock();
        state.observe_time(at);
        state.rings.drain(at, &self.mount.display_lossy(), top_k)
    }
    /// Drain completed minutes oldest first, including empty healthy minutes.
    ///
    /// Starts with the first full minute after startup; retained history is bounded
    /// to 60 buckets. A retention gap is visible in timestamps and marks the next
    /// emitted minute lossy. Rollback never repeats an emitted timestamp. Adjacent
    /// boundaries have equal cumulative queue-overflow totals, which never decrease.
    /// Recovery, overflow, rollback and cut lists make hot data unavailable (lossy).
    /// Persistence rows share a cursor with [`Self::drain_completed_minutes`]; a row
    /// is handed out once through whichever method drains its minute first. The
    /// minute facts and directory rows remain available after the legacy row drain.
    pub fn drain_minutes(&self, top_k: usize) -> Vec<CompletedMinute> {
        self.drain_minutes_at(now(), top_k)
    }
    fn drain_minutes_at(&self, at: i64, top_k: usize) -> Vec<CompletedMinute> {
        let mut state = self.state.lock();
        state.observe_time(at);
        if state.failed || state.recovering {
            state.rings.recovery(at);
        }
        state
            .rings
            .drain_minutes(at, &self.mount.display_lossy(), top_k)
    }
    /// Identity of the retained descriptor used to mark the mount: mount ID,
    /// device major and device minor. Uses EL8-compatible procfs fdinfo and fstat.
    /// Injected-event watchers return [`io::ErrorKind::Unsupported`].
    pub fn mount_identity(&self) -> io::Result<(u64, u32, u32)> {
        use std::io::Read;
        let fd = self.mount_fd.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "injected watcher has no mount descriptor",
            )
        })?;
        let stat = rustix::fs::fstat(fd.as_fd())?;
        let file = std::fs::File::open(format!("/proc/self/fdinfo/{}", fd.as_raw_fd()))?;
        let mut text = String::new();
        file.take(16_385).read_to_string(&mut text)?;
        if text.len() > 16_384 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized mount fdinfo",
            ));
        }
        let id = parse_mount_id(&text)?;
        Ok((
            id,
            rustix::fs::major(stat.st_dev),
            rustix::fs::minor(stat.st_dev),
        ))
    }
    /// Enable the optional 1,000-event foreground buffer. Daemon watchers do not allocate it.
    pub fn enable_events(&self) {
        self.state
            .lock()
            .stream
            .get_or_insert_with(|| VecDeque::with_capacity(1000));
    }
    /// Remove and return up to `limit` of the oldest buffered events.
    pub fn drain_events(&self, limit: usize) -> Vec<WatchEvent> {
        let mut state = self.state.lock();
        let Some(stream) = &mut state.stream else {
            return Vec::new();
        };
        stream.drain(..limit.min(stream.len())).collect()
    }
    /// Remove dirty entries at or below `under`, returning them in byte order.
    /// Ancestor entries remain retained and also report `under` as dirty. After loss,
    /// each root within the mount is reported once independently; the oldest of at
    /// most 1,024 acknowledgements is forgotten when full and may be reported again.
    pub fn take_dirty(&self, under: &RawPath) -> Vec<RawPath> {
        let mut state = self.state.lock();
        let ancestor = state
            .dirty
            .iter()
            .any(|p| p != under && under.starts_with(p));
        let lost = under.starts_with(&self.mount)
            && state.loss_pending
            && !state.loss_taken.contains(under);
        if lost {
            if state.loss_taken.len() == LOSS_ACK_CAP {
                state.loss_taken.pop_front();
            }
            state.loss_taken.push_back(under.clone());
        }
        let mut paths: Vec<_> = state
            .dirty
            .iter()
            .filter(|p| p.starts_with(under))
            .cloned()
            .collect();
        for path in &paths {
            state.dirty.remove(path);
        }
        if ancestor || lost {
            paths.push(under.clone());
            paths.sort();
            paths.dedup();
        }
        paths
    }
    /// Explain caps and loss in the retained fifteen-minute history.
    /// Reports at most eight causes, each with its latest affected UTC minute.
    /// Truncation diagnostics do not change legacy lossy flags or alert availability.
    pub fn health_detail(&self) -> String {
        let state = self.state.lock();
        state.rings.detail(now())
    }
    /// Read counters and the loss indicator for the latest fifteen minute buckets.
    pub fn stats(&self) -> WatchStats {
        let mut state = self.state.lock();
        let at = now();
        state.observe_time(at);
        if state.failed || state.recovering {
            state.rings.recovery(at);
        } else {
            state.rings.advance(at);
        }
        WatchStats {
            mode: if state.failed {
                WatchMode::Failed
            } else {
                self.mode
            },
            events_total: state.events,
            overflows_total: state.overflows,
            lossy_minutes_15m: state.rings.lossy(at),
            dropped_keys: state.rings.dropped,
            dropped_events: state.dropped_events,
            recovering: state.recovering,
            read_errors: state.read_errors,
        }
    }
    /// Wake the worker and join it, closing the group and all event descriptors.
    pub fn stop(mut self) {
        self.shutdown_worker();
    }
    fn shutdown_worker(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        let _ = rustix::io::write(self.shutdown.as_ref(), &1u64.to_ne_bytes());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn mount_mark_path(fd: &impl AsRawFd) -> std::path::PathBuf {
    // The mark helper uses FAN_MARK_DONT_FOLLOW. A final `.` names the
    // retained directory itself, rather than the procfs descriptor symlink.
    std::path::PathBuf::from(format!("/proc/self/fd/{}/.", fd.as_raw_fd()))
}
/// Test support for the fdinfo parser; production callers use mount_identity.
#[doc(hidden)]
pub fn parse_mount_id(text: &str) -> io::Result<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix("mnt_id:"))
        .and_then(|id| id.trim().parse().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "missing or invalid fdinfo mount id",
            )
        })
}
fn select_group<T>(
    rich: Option<(T, WatchMode)>,
    fd: impl FnOnce() -> Result<T, WatchError>,
) -> Result<(T, WatchMode), WatchError> {
    match rich {
        Some(selected) => Ok(selected),
        None => fd().map(|group| (group, WatchMode::FdMount)),
    }
}
// Reads are bounded by the mode-specific transport buffer, including collected batches.
fn read_recover<T>(
    mut read: impl FnMut() -> io::Result<T>,
    processor: &mut Processor<'_>,
    state: &Mutex<State>,
    shutdown: &OwnedFd,
    stopping: &AtomicBool,
    clock: impl Fn() -> i64,
) -> Option<T> {
    let mut failures = 0u32;
    while !stopping.load(Ordering::Relaxed) {
        match read() {
            Ok(events) => {
                let mut state = state.lock();
                if state.recovering {
                    state.rings.recovery(clock());
                }
                state.recovering = false;
                return Some(events);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                tracing::warn!(%error, "fanotify read failed: {error}");
                if !processor.transport_error(&error, clock(), state) {
                    return None;
                }
                failures = failures.saturating_add(1);
                if !retry_wait(shutdown, stopping, failures) {
                    return None;
                }
            }
        }
    }
    None
}
// Wait only on shutdown, so a readable group cannot turn resource recovery into a busy loop.
fn retry_wait(shutdown: &OwnedFd, stopping: &AtomicBool, failures: u32) -> bool {
    let millis = (25u64 << failures.min(5)).min(1000);
    let mut poll = [rustix::event::PollFd::new(
        shutdown,
        rustix::event::PollFlags::IN,
    )];
    let timeout = rustix::event::Timespec {
        tv_sec: (millis / 1000) as i64,
        tv_nsec: ((millis % 1000) * 1_000_000) as i64,
    };
    let _ = rustix::event::poll(&mut poll, Some(&timeout));
    poll[0].revents().is_empty() && !stopping.load(Ordering::Relaxed)
}
/// Compare the current and PID 1 mount-namespace identities.
pub fn namespace_ok() -> io::Result<bool> {
    Ok(std::fs::read_link("/proc/self/ns/mnt")? == std::fs::read_link("/proc/1/ns/mnt")?)
}

struct Processor<'a> {
    own_pid: i32,
    cfg: &'a WatcherOptions,
    attribution: &'a AttributionCache,
    mount_fd: std::os::fd::BorrowedFd<'a>,
    sizes: fdmode::Sizes,
    paths: fidmode::PathSizes,
    handles: fidmode::HandlePaths,
}
impl Processor<'_> {
    fn loss(&mut self, at: i64, state: &Mutex<State>) {
        self.handles.clear();
        self.paths.clear();
        let mut state = state.lock();
        state.observe_time(at);
        state.rings.overflow(at);
        state.mark_loss();
    }
    fn transport_error(&mut self, error: &io::Error, at: i64, state: &Mutex<State>) -> bool {
        self.loss(at, state);
        let retry = error.raw_os_error().is_some_and(|code| {
            [
                rustix::io::Errno::NOMEM,
                rustix::io::Errno::NFILE,
                rustix::io::Errno::MFILE,
                rustix::io::Errno::NOBUFS,
            ]
            .iter()
            .any(|errno| errno.raw_os_error() == code)
        }) || error.kind() == io::ErrorKind::WouldBlock;
        let mut state = state.lock();
        state.rings.read_failure(
            at,
            error.raw_os_error().is_some_and(|code| {
                code == rustix::io::Errno::MFILE.raw_os_error()
                    || code == rustix::io::Errno::NFILE.raw_os_error()
            }),
        );
        state.read_errors = state.read_errors.saturating_add(1);
        state.recovering = retry;
        state.failed = !retry;
        retry
    }
    fn process(&mut self, event: RawEvent, at: i64, state: &Mutex<State>) {
        let mount_fd = self.mount_fd;
        let is_fid = matches!(&event, RawEvent::Fid { .. });
        let event_mask = match &event {
            RawEvent::Fd { mask, .. } | RawEvent::Fid { mask, .. } => *mask,
            RawEvent::Overflow => 0,
        };
        if matches!(event, RawEvent::Overflow) {
            self.loss(at, state);
            let mut state = state.lock();
            state.overflows = state.overflows.saturating_add(1);
            state.rings.queue_overflow(at);
            return;
        }
        let resolved = match event {
            RawEvent::Fd { pid, fd, .. } => {
                if pid == self.own_pid {
                    return;
                }
                fdmode::resolve_event(fd.as_fd()).map(|(p, s, created)| (pid, p, Some(s), created))
            }
            RawEvent::Fid {
                pid,
                handle_type,
                handle,
                name,
                mask,
                ..
            } => {
                if pid == self.own_pid {
                    return;
                }
                fidmode::resolve(
                    mount_fd.as_fd(),
                    handle_type,
                    &handle,
                    name.as_deref(),
                    mask,
                    &mut self.handles,
                )
                .map(|(p, s, created)| (pid, p, s.map(Into::into), created))
            }
            RawEvent::Overflow => return,
        };
        let Ok(resolved) = resolved else {
            self.loss(at, state);
            return;
        };
        self.record(resolved, event_mask, is_fid, at, state);
    }
    fn record(
        &mut self,
        resolved: (i32, RawPath, Option<fdmode::EventStat>, i64),
        event_mask: u64,
        is_fid: bool,
        at: i64,
        state: &Mutex<State>,
    ) {
        let (pid, path, stat, created) = resolved;
        let cfg = self.cfg;
        let attribution = self.attribution;
        let sizes = &mut self.sizes;
        if !path.starts_with(&cfg.mount) || cfg.exclude_prefixes.iter().any(|p| path.starts_with(p))
        {
            return;
        }
        let (delta, identity) = if let Some(stat) = stat {
            let id = (stat.st_dev, stat.st_ino);
            let disk = stat.st_blocks.saturating_mul(512);
            let delta = sizes.delta(id, disk, created, at);
            if is_fid {
                self.paths.remember(&path, id, disk);
            }
            (
                if event_mask & mask::MOVED_TO != 0 {
                    disk
                } else {
                    delta
                },
                id,
            )
        } else {
            self.paths
                .remove(&path)
                .map_or((0, (0, path.hash64() as u64)), |(id, disk)| {
                    (disk.saturating_neg(), id)
                })
        };
        let who = attribution.lookup_watcher(u32::try_from(pid).unwrap_or(0));
        let parent = path
            .parent()
            .filter(|p| p.starts_with(&cfg.mount))
            .unwrap_or_else(|| cfg.mount.clone());
        let key = dir_key(&parent, &cfg.mount, cfg.hot_depth);
        let mut state = state.lock();
        state.observe_time(at);
        state.events = state.events.saturating_add(1);
        if let Some(stream) = &mut state.stream {
            let full = stream.len() == 1000;
            if full {
                stream.pop_front();
            }
            stream.push_back(WatchEvent {
                at,
                path,
                delta,
                attribution: who.clone(),
            });
            if full {
                state.dropped_events = state.dropped_events.saturating_add(1);
            }
        }
        state.rings.add(
            at,
            [
                key.as_bytes().to_vec(),
                who.unit.clone().unwrap_or_else(|| "?".into()).into_bytes(),
                who.comm.into_bytes(),
            ],
            delta,
            who.unit,
            identity,
        );
        if state.dirty.len() < 200_000 {
            state.dirty.insert(parent);
        } else {
            state.rings.overflow(at);
        }
    }
}

mod fdmode;
mod fidmode;
mod rings;

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// System call or worker creation failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// Unsupported fanotify.
    #[error("Unsupported")]
    Unsupported,
    /// Incorrect mount namespace.
    #[error("Namespace")]
    Namespace,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.shutdown_worker();
    }
}
fn dir_key(parent: &RawPath, mount: &RawPath, depth: usize) -> RawPath {
    if !parent.starts_with(mount) {
        return mount.clone();
    }
    let mut result = mount.clone();
    for part in parent.as_bytes()[mount.as_bytes().len()..]
        .split(|b| *b == b'/')
        .filter(|p| !p.is_empty())
        .take(depth)
    {
        if let Ok(name) = tidemark_core::path::RawName::new(part.to_vec()) {
            result = result.join(&name);
        }
    }
    result
}
impl std::fmt::Display for WatchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Failed => "failed",
            Self::FdMount => "fd/mount",
            Self::DfidNameMount => "dfid-name/mount",
            Self::DfidNameFilesystem => "dfid-name/filesystem",
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_limit_counts_and_unreadable_pressure_are_lossy() {
        assert_eq!(count_descriptors(1).unwrap(), 1);
        assert!(count_descriptors(65536).unwrap() > 0);
        assert!(descriptor_limit_with(Some(64), |_| Ok(64)));
        assert!(!descriptor_limit_with(Some(64), |_| Ok(63)));
        assert!(!descriptor_limit_with(None, |_| panic!("unlimited")));
        for errno in [24, 23, 12, 105] {
            assert!(descriptor_limit_with(Some(64), |_| Err(
                io::Error::from_raw_os_error(errno)
            )));
        }
        assert!(!descriptor_limit_with(Some(64), |_| Err(
            io::Error::from_raw_os_error(2)
        )));
    }
    #[test]
    fn fd_reader_drains_five_thousand_in_bounded_batches() {
        for batch in [256, 32, 16] {
            let mut remaining = 5000;
            let mut buffer = vec![0; 65536];
            let mut reads = 0;
            while remaining > 0 {
                let count = read_batch(&mut buffer, fd_read_len(batch), |bytes| {
                    let count = remaining.min(bytes.len() / FD_EVENT_BYTES);
                    assert!(count <= batch);
                    Ok(count)
                })
                .unwrap();
                remaining -= count;
                reads += 1;
            }
            assert_eq!(reads, 5000usize.div_ceil(batch));
        }
    }
    #[test]
    fn distinct_file_cap_marks_truncation_and_preserves_event_totals() {
        let at = now().div_euclid(60) * 60;
        let events = std::iter::once((at - 60, [vec![], vec![], vec![]], 0, None, (1, 0))).chain(
            (1..=100_000).map(|n| {
                (
                    at,
                    [b"d".to_vec(), b"u".to_vec(), b"c".to_vec()],
                    1,
                    None,
                    (1, n),
                )
            }),
        );
        let watcher = Watcher::from_injected_events(raw("/fixture"), events).unwrap();
        assert!(watcher
            .health_detail()
            .contains("truncated: distinct-file cap reached in the minute"));
        let minutes = watcher.drain_minutes_at(at + 60, 1000);
        let minute = minutes.last().unwrap();
        assert!(minute.truncated && minute.lossy);
        assert_eq!(minute.overflow_total_at_end, 0);
        assert_eq!(minute.dirs[0].events, 100_000);
        assert_eq!(minute.dirs[0].files, 66_667);
    }
    #[test]
    fn injected_watcher_minutes_and_shared_persistence() {
        for legacy_first in [false, true] {
            let watcher = Watcher::from_injected_events(
                raw("/fixture"),
                [
                    (
                        17,
                        [b"d".to_vec(), b"u".to_vec(), b"c".to_vec()],
                        1,
                        Some("u".into()),
                        (1, 1),
                    ),
                    (
                        60,
                        [b"d".to_vec(), b"u".to_vec(), b"c".to_vec()],
                        2,
                        Some("u".into()),
                        (1, 1),
                    ),
                ],
            )
            .unwrap();
            assert_eq!(
                watcher.mount_identity().unwrap_err().kind(),
                io::ErrorKind::Unsupported
            );
            if legacy_first {
                assert_eq!(watcher.drain_completed_minutes_at(180, 1000).len(), 6);
            }
            let minutes = watcher.drain_minutes_at(180, 1000);
            assert_eq!(minutes.len(), 2);
            assert_eq!(minutes[0].started_at, 60);
            assert_eq!(minutes[0].dirs[0].bytes_delta, 2);
            assert_eq!(minutes[0].dirs[0].top_unit.as_deref(), Some("u"));
            assert_eq!(minutes[0].persisted.len(), if legacy_first { 0 } else { 3 });
            assert!(minutes[1].dirs.is_empty() && minutes[1].persisted.is_empty());
            assert!(!minutes[1].lossy);
            assert!(watcher.drain_minutes_at(180, 1000).is_empty());
            // The legacy drain also owns startup's partial-minute rows.
            assert_eq!(
                watcher.drain_completed_minutes_at(180, 1000).len(),
                if legacy_first { 0 } else { 3 }
            );
        }
    }
    #[test]
    fn injected_watcher_overflow_and_recovery_facts() {
        let watcher = Watcher::from_injected_events(
            raw("/fixture"),
            [(17, [vec![], vec![], vec![]], 0, None, (1, 1))],
        )
        .unwrap();
        let mut state = watcher.state.lock();
        state.rings.queue_overflow(130);
        state.overflows = 1;
        state.recovering = true;
        drop(state);
        let minutes = watcher.drain_minutes_at(240, 1000);
        assert_eq!(
            minutes
                .iter()
                .map(|m| (m.overflow_total_at_start, m.overflow_total_at_end))
                .collect::<Vec<_>>(),
            vec![(0, 0), (0, 1), (1, 1)]
        );
        assert!(minutes[1].lossy);
        assert!(minutes[2].lossy);
        // Ordinary queries must not make recovery intervals look healthy.
        {
            let mut state = watcher.state.lock();
            state.observe_time(360);
            state.rings.advance(360);
        }
        let recovering = watcher.drain_minutes_at(420, 1000);
        assert!(recovering.iter().all(|m| m.lossy));
        watcher.state.lock().recovering = false;
    }
    #[test]
    fn retained_mount_descriptor_survives_path_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("mount");
        std::fs::create_dir(&original).unwrap();
        let fd = Arc::new(
            rustix::fs::open(
                &original,
                rustix::fs::OFlags::PATH
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .unwrap(),
        );
        let mut watcher = Watcher::from_injected_events(raw("/fixture"), []).unwrap();
        watcher.mount_fd = Some(fd.clone());
        let identity = watcher.mount_identity().unwrap();
        let mounts = tidemark_fs::mount::read_mountinfo().unwrap();
        let raw_path = RawPath::from_path(&original).unwrap();
        let mount = mounts
            .iter()
            .filter(|m| raw_path.starts_with(&m.mount_point))
            .max_by_key(|m| m.mount_point.as_bytes().len())
            .unwrap();
        assert_eq!(identity, (u64::from(mount.id), mount.major, mount.minor));
        std::fs::rename(&original, temp.path().join("moved")).unwrap();
        std::fs::create_dir(&original).unwrap();
        let retained = rustix::fs::fstat(fd.as_fd()).unwrap();
        // FAN_MARK_DONT_FOLLOW must resolve the final component to the retained
        // directory, even after replacement of the originally configured path.
        let target = rustix::fs::statat(
            rustix::fs::CWD,
            mount_mark_path(&fd),
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .unwrap();
        let replacement = rustix::fs::stat(&original).unwrap();
        assert_eq!(
            (target.st_dev, target.st_ino),
            (retained.st_dev, retained.st_ino)
        );
        assert_ne!(target.st_ino, replacement.st_ino);
        assert_eq!(watcher.mount_identity().unwrap(), identity);
    }
    #[test]
    fn fdinfo_mount_id_parser() {
        assert_eq!(
            parse_mount_id("pos:\t0\nflags:\t010000000\nmnt_id:\t123\n").unwrap(),
            123
        );
        for text in [
            "",
            "mnt_id: garbage",
            "mnt_id: -1",
            "mnt_id: 18446744073709551616",
        ] {
            assert_eq!(
                parse_mount_id(text).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }
    fn empty_state() -> Mutex<State> {
        Mutex::new(State {
            rings: rings::Rings::new(),
            dirty: BTreeSet::new(),
            loss_pending: false,
            loss_taken: VecDeque::new(),
            last_at: None,
            events: 0,
            overflows: 0,
            failed: false,
            recovering: false,
            read_errors: 0,
            stream: None,
            dropped_events: 0,
        })
    }
    fn dirty_watcher() -> Watcher {
        Watcher {
            mode: WatchMode::FdMount,
            state: Arc::new(empty_state()),
            shutdown: Arc::new(
                rustix::event::eventfd(0, rustix::event::EventfdFlags::NONBLOCK).unwrap(),
            ),
            stopping: Arc::new(AtomicBool::new(false)),
            thread: None,
            mount: raw("/data"),
            mount_fd: None,
        }
    }
    fn raw(path: &str) -> RawPath {
        RawPath::new(path.as_bytes().to_vec()).unwrap()
    }
    fn overflow(watcher: &Watcher) {
        let temp = tempfile::tempdir().unwrap();
        let mount_fd = std::fs::File::open(temp.path()).unwrap();
        let cfg = WatcherOptions {
            mount: watcher.mount.clone(),
            events: vec![],
            hot_depth: 6,
            exclude_prefixes: vec![],
            lru_entries: 10,
        };
        let attribution = AttributionCache::new(std::time::Duration::from_secs(60), 10);
        let mut processor = Processor {
            own_pid: std::process::id() as i32,
            cfg: &cfg,
            attribution: &attribution,
            mount_fd: mount_fd.as_fd(),
            sizes: fdmode::Sizes::new(10, 120),
            paths: fidmode::PathSizes::new(10),
            handles: fidmode::HandlePaths::new(10),
        };
        processor.process(RawEvent::Overflow, 120, &watcher.state);
    }
    #[test]
    fn overflow_keeps_nested_scan_root_dirty() {
        let watcher = dirty_watcher();
        let root = raw("/data/reports");
        watcher.state.lock().dirty.insert(root.clone());
        overflow(&watcher);
        assert_eq!(watcher.take_dirty(&root), vec![root.clone()]);
        assert!(watcher.take_dirty(&root).is_empty());
    }
    #[test]
    fn loss_acknowledgements_are_independent_in_both_orders() {
        for (a, b) in [
            ("/data/reports", "/data/logs"),
            ("/data", "/data/reports"),
            ("/data/reports", "/data/reports/nested"),
        ] {
            for (first, second) in [(a, b), (b, a)] {
                let watcher = dirty_watcher();
                overflow(&watcher);
                for root in [raw(first), raw(second)] {
                    assert_eq!(watcher.take_dirty(&root), vec![root.clone()]);
                    assert!(watcher.take_dirty(&root).is_empty());
                }
                assert!(watcher.take_dirty(&raw(first)).is_empty());
                overflow(&watcher);
                assert_eq!(watcher.take_dirty(&raw(first)), vec![raw(first)]);
                assert_eq!(watcher.take_dirty(&raw(second)), vec![raw(second)]);
            }
        }
    }
    #[test]
    fn ancestor_dirty_entry_is_retained_and_descendants_are_removed() {
        let watcher = dirty_watcher();
        let ancestor = raw("/data/reports");
        let root = raw("/data/reports/nested");
        let child = raw("/data/reports/nested/z");
        watcher
            .state
            .lock()
            .dirty
            .extend([ancestor.clone(), root.clone(), child.clone()]);
        assert_eq!(watcher.take_dirty(&root), vec![root.clone(), child]);
        assert_eq!(watcher.take_dirty(&root), vec![root]);
        assert_eq!(
            watcher.take_dirty(&raw("/data/reports/other")),
            vec![raw("/data/reports/other")]
        );
        assert_eq!(watcher.take_dirty(&ancestor), vec![ancestor]);
        assert!(watcher.state.lock().dirty.is_empty());
    }
    #[test]
    fn loss_acknowledgements_are_bounded_and_forgetting_is_conservative() {
        let watcher = dirty_watcher();
        overflow(&watcher);
        assert!(watcher.take_dirty(&raw("/database")).is_empty());
        for n in 0..=LOSS_ACK_CAP {
            let root = raw(&format!("/data/{n}"));
            assert_eq!(watcher.take_dirty(&root), vec![root.clone()]);
            assert!(watcher.take_dirty(&root).is_empty());
            assert!(watcher.state.lock().loss_taken.len() <= LOSS_ACK_CAP);
        }
        assert_eq!(
            watcher.state.lock().loss_taken.front(),
            Some(&raw("/data/1"))
        );
        assert_eq!(watcher.take_dirty(&raw("/data/0")), vec![raw("/data/0")]);
        for _ in 0..10 {
            overflow(&watcher);
            assert!(watcher.state.lock().loss_taken.is_empty());
            assert_eq!(watcher.take_dirty(&raw("/data/0")), vec![raw("/data/0")]);
        }
    }
    #[test]
    fn no_loss_preserves_sorted_descendant_drain() {
        let watcher = dirty_watcher();
        let root = raw("/data/reports");
        let sibling = raw("/data/reports-other");
        let paths = vec![root.clone(), raw("/data/reports/a"), raw("/data/reports/z")];
        watcher
            .state
            .lock()
            .dirty
            .extend(paths.iter().cloned().chain([sibling.clone()]));
        assert_eq!(watcher.take_dirty(&root), paths);
        assert!(watcher.take_dirty(&root).is_empty());
        assert_eq!(watcher.take_dirty(&raw("/data")), vec![sibling]);
        assert!(watcher.state.lock().loss_taken.is_empty());
    }
    #[test]
    fn rollback_invalidates_each_root_again() {
        let watcher = dirty_watcher();
        overflow(&watcher);
        let root = raw("/data/reports");
        assert_eq!(watcher.take_dirty(&root), vec![root.clone()]);
        watcher.state.lock().observe_time(60);
        assert_eq!(watcher.take_dirty(&root), vec![root.clone()]);
        assert!(watcher.take_dirty(&root).is_empty());
        assert_eq!(
            watcher.take_dirty(&raw("/data/logs")),
            vec![raw("/data/logs")]
        );
    }
    #[test]
    fn synthetic_event_processing_exclusions_attribution_and_cleanup() {
        use std::os::unix::fs::MetadataExt;
        let temp = tempfile::tempdir().unwrap();
        let mount_fd = std::fs::File::open(temp.path()).unwrap();
        let mount = RawPath::from_path(temp.path()).unwrap();
        let excluded = temp.path().join("skip");
        std::fs::create_dir(&excluded).unwrap();
        let included = temp.path().join("skipper");
        std::fs::create_dir(&included).unwrap();
        let file = included.join("raw-file");
        std::fs::write(&file, vec![1; 8192]).unwrap();
        let disk = (std::fs::metadata(&file).unwrap().blocks() * 512) as i64;
        let at = std::fs::metadata(&file).unwrap().ctime();
        let cfg = WatcherOptions {
            mount: mount.clone(),
            events: vec![],
            hot_depth: 6,
            exclude_prefixes: vec![RawPath::from_path(&excluded).unwrap()],
            lru_entries: 10,
        };
        let attribution = AttributionCache::new(std::time::Duration::from_secs(60), 10);
        let mut processor = Processor {
            own_pid: std::process::id() as i32,
            cfg: &cfg,
            attribution: &attribution,
            mount_fd: mount_fd.as_fd(),
            sizes: fdmode::Sizes::new(10, at - 1),
            paths: fidmode::PathSizes::new(10),
            handles: fidmode::HandlePaths::new(100_000),
        };
        let state = empty_state();
        for _ in 0..2 {
            processor.process(
                RawEvent::Fd {
                    mask: mask::CLOSE_WRITE,
                    pid: 0,
                    fd: std::fs::File::open(&file).unwrap().into(),
                },
                at,
                &state,
            );
        }
        let rows = state.lock().rings.hot(at, 1, HotScope::Dir, 1);
        let row = &rows[0];
        assert_eq!((row.bytes_delta, row.events, row.files), (disk, 2, 1));
        assert_eq!(
            state.lock().rings.hot(at, 1, HotScope::Comm, 1)[0].key,
            b"?"
        );
        let excluded_file = excluded.join("file");
        std::fs::write(&excluded_file, b"fixture").unwrap();
        processor.process(
            RawEvent::Fd {
                mask: mask::CLOSE_WRITE,
                pid: 0,
                fd: std::fs::File::open(&excluded_file).unwrap().into(),
            },
            at,
            &state,
        );
        processor.process(
            RawEvent::Fd {
                mask: mask::CLOSE_WRITE,
                pid: std::process::id() as i32,
                fd: std::fs::File::open(&file).unwrap().into(),
            },
            at,
            &state,
        );
        assert_eq!(state.lock().events, 2);
        assert_eq!(state.lock().dirty.len(), 1);
        // Independent direction records carry their own paths and measured deltas.
        let directions = empty_state();
        directions.lock().stream = Some(VecDeque::new());
        let from_path = RawPath::from_path(&file).unwrap();
        let to_path = RawPath::from_path(&included.join("destination")).unwrap();
        processor.paths.remember(&from_path, (1, 1), disk);
        processor.record(
            (0, from_path.clone(), None, 0),
            mask::MOVED_FROM,
            true,
            at,
            &directions,
        );
        let stat = rustix::fs::stat(&file).unwrap();
        processor.record(
            (0, to_path.clone(), Some(stat.into()), at),
            mask::MOVED_TO,
            true,
            at,
            &directions,
        );
        {
            let directions = directions.lock();
            let events = directions.stream.as_ref().unwrap();
            assert_eq!(
                (events[0].path.clone(), events[0].delta),
                (from_path, -disk)
            );
            assert_eq!((events[1].path.clone(), events[1].delta), (to_path, disk));
            assert_eq!(directions.events, 2);
        }
        let renamed_fd = std::fs::File::open(&included).unwrap();
        processor
            .handles
            .put((1, vec![1]), RawPath::from_path(&included).unwrap());
        processor
            .paths
            .remember(&RawPath::from_path(&file).unwrap(), (1, 1), disk);
        std::fs::rename(&included, excluded.join("moved")).unwrap();
        processor.process(RawEvent::Overflow, at, &state);
        assert!(processor.handles.get(&(1, vec![1])).is_none());
        let (fresh, _, _) = fidmode::resolve_fd(
            renamed_fd.as_fd(),
            1,
            &[1],
            Some(b"raw-file"),
            mask::CLOSE_WRITE,
            &mut processor.handles,
        )
        .unwrap();
        assert!(fresh.starts_with(&cfg.exclude_prefixes[0]));
        assert_eq!(fresh.as_path(), excluded.join("moved/raw-file"));
        assert!(processor
            .paths
            .remove(&RawPath::from_path(&file).unwrap())
            .is_none());
        processor.process(
            RawEvent::Fd {
                mask: mask::CLOSE_WRITE,
                pid: 0,
                fd: std::fs::File::open(excluded.join("moved/raw-file"))
                    .unwrap()
                    .into(),
            },
            at,
            &state,
        );
        assert_eq!(state.lock().events, 2);
        let live = temp.path().join("resumed");
        std::fs::write(&live, b"fixture").unwrap();
        state.lock().stream = Some(VecDeque::new());
        let shutdown = rustix::event::eventfd(0, rustix::event::EventfdFlags::NONBLOCK).unwrap();
        let stopping = AtomicBool::new(false);
        let mut reads = 0;
        let event = read_recover(
            || {
                reads += 1;
                if reads == 1 {
                    Err(io::Error::from_raw_os_error(24))
                } else {
                    Ok(RawEvent::Fd {
                        mask: mask::CLOSE_WRITE,
                        pid: 0,
                        fd: std::fs::File::open(&live).unwrap().into(),
                    })
                }
            },
            &mut processor,
            &state,
            &shutdown,
            &stopping,
            || at,
        )
        .unwrap();
        processor.process(event, at, &state);
        assert_eq!(reads, 2);
        assert_eq!(state.lock().events, 3);
        assert!(!state.lock().recovering);
        assert_eq!(state.lock().read_errors, 1);
        for n in 0..1001 {
            processor.process(
                RawEvent::Fd {
                    mask: mask::CLOSE_WRITE,
                    pid: 0,
                    fd: std::fs::File::open(&live).unwrap().into(),
                },
                at + n,
                &state,
            );
        }
        assert_eq!(state.lock().stream.as_ref().unwrap().len(), 1000);
        assert_eq!(state.lock().dropped_events, 2);
        assert!(!processor.transport_error(&io::Error::from_raw_os_error(9), at, &state));
        assert!(state.lock().failed);
        stopping.store(true, Ordering::Relaxed);
        assert!(!retry_wait(&shutdown, &stopping, 30));
        stopping.store(false, Ordering::Relaxed);
        rustix::io::write(&shutdown, &1u64.to_ne_bytes()).unwrap();
        let started = std::time::Instant::now();
        assert!(!retry_wait(&shutdown, &stopping, 30));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert_eq!(state.lock().overflows, 1);
        assert!(state.lock().rings.lossy(at) > 0);
        let watcher = Watcher {
            mode: WatchMode::FdMount,
            state: Arc::new(state),
            shutdown: Arc::new(shutdown),
            stopping: Arc::new(stopping),
            thread: None,
            mount: cfg.mount.clone(),
            mount_fd: None,
        };
        assert_eq!(watcher.mode(), WatchMode::Failed);
        let drained = watcher.drain_events(3);
        assert_eq!(
            drained.iter().map(|e| e.at).collect::<Vec<_>>(),
            vec![at + 1, at + 2, at + 3]
        );
        watcher.state.lock().rings.advance(at + 2000);
        assert_eq!(watcher.stats().mode, WatchMode::Failed);
        assert!(watcher.stats().lossy_minutes_15m > 0);
        assert_eq!(watcher.stats().read_errors, 2);
        assert_eq!(watcher.drain_events(usize::MAX).len(), 997);
    }
    #[cfg(feature = "privileged")]
    #[test]
    fn kernel_overflow_worker_survives_and_resumes() {
        use std::time::{Duration, Instant};
        let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
        let watcher = Watcher::start(
            WatcherOptions {
                mount: RawPath::from_path(&fs.mountpoint).unwrap(),
                events: vec![],
                hot_depth: 6,
                exclude_prefixes: vec![],
                lru_entries: 1000,
            },
            Arc::new(AttributionCache::new(Duration::from_secs(60), 10)),
        )
        .unwrap();
        // Hold the worker's processing lock so its bounded kernel queue must overflow.
        let held = watcher.state.lock();
        let status = std::process::Command::new("sh")
            .args([
                "-c",
                "i=0; while [ $i -lt 100000 ]; do : > \"$1/$i\"; i=$((i+1)); done",
                "w5a",
            ])
            .arg(&fs.mountpoint)
            .status()
            .unwrap();
        assert!(status.success());
        drop(held);
        let deadline = Instant::now() + Duration::from_secs(30);
        while watcher.stats().overflows_total == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(watcher.stats().overflows_total > 0);
        assert!(watcher.stats().lossy_minutes_15m > 0);
        watcher.enable_events();
        let path = fs.mountpoint.join("after-overflow");
        assert!(std::process::Command::new("sh")
            .args(["-c", "echo resumed > \"$1\"", "w5a"])
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while Instant::now() < deadline && !seen {
            seen = watcher
                .drain_events(1000)
                .iter()
                .any(|event| event.path.as_path() == path);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seen);
        assert_ne!(watcher.mode(), WatchMode::Failed);
        watcher.stop();
    }
    #[test]
    fn synthetic_batch_fd_count_stable() {
        const CHILD: &str = "TIDEMARK_W5A_FD_COUNT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "watch::tests::synthetic_batch_fd_count_stable",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let mount_fd = std::fs::File::open(temp.path()).unwrap();
        let cfg = WatcherOptions {
            mount: RawPath::from_path(temp.path()).unwrap(),
            events: vec![],
            hot_depth: 6,
            exclude_prefixes: vec![],
            lru_entries: 10,
        };
        let attribution = AttributionCache::new(std::time::Duration::from_secs(60), 10);
        let at = now();
        let mut processor = Processor {
            own_pid: std::process::id() as i32,
            cfg: &cfg,
            attribution: &attribution,
            mount_fd: mount_fd.as_fd(),
            sizes: fdmode::Sizes::new(10, at),
            paths: fidmode::PathSizes::new(10),
            handles: fidmode::HandlePaths::new(100_000),
        };
        let state = empty_state();
        let fixture = temp.path().join("fixture");
        std::fs::write(&fixture, b"metadata-only fixture").unwrap();
        let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
        let before = count();
        for _ in 0..1000 {
            for pid in [0, std::process::id() as i32] {
                processor.process(
                    RawEvent::Fd {
                        mask: mask::CLOSE_WRITE,
                        pid,
                        fd: std::fs::File::open(&fixture).unwrap().into(),
                    },
                    at,
                    &state,
                );
            }
        }
        assert_eq!(state.lock().events, 1000);
        assert!(!state.lock().failed);
        assert_eq!(count(), before);
    }
    #[test]
    fn rich_mark_failure_selects_fd_fallback() {
        let mut fallback_called = false;
        let (group, mode) = select_group(None, || {
            fallback_called = true;
            Ok(42)
        })
        .unwrap();
        assert!(fallback_called);
        assert_eq!(group, 42);
        assert_eq!(mode, WatchMode::FdMount);
        assert!(matches!(
            select_group::<u8>(None, || Err(WatchError::Unsupported)),
            Err(WatchError::Unsupported)
        ));
        assert_eq!(
            select_group(
                Some((7, WatchMode::DfidNameMount)),
                || -> Result<u8, WatchError> { panic!("unnecessary fallback") }
            )
            .unwrap()
            .1,
            WatchMode::DfidNameMount
        );
    }
    #[test]
    fn component_prefixes_and_hot_depth() {
        let mount = RawPath::new(b"/data".to_vec()).unwrap();
        let path = RawPath::new(b"/data/a/b/c\xff".to_vec()).unwrap();
        assert_eq!(dir_key(&path, &mount, 2).as_bytes(), b"/data/a/b");
        assert_eq!(dir_key(&path, &mount, 0), mount);
        assert!(!path.starts_with(&RawPath::new(b"/data/ab".to_vec()).unwrap()));
        let root = RawPath::new(b"/".to_vec()).unwrap();
        assert_eq!(dir_key(&path, &root, 1).as_bytes(), b"/data");
    }
}
