//! Owned by: W5-A
//! Mount-scoped fanotify worker and bounded hot activity state.

use parking_lot::Mutex;
use std::{
    collections::BTreeSet,
    io,
    os::fd::{AsFd, OwnedFd},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};
use tidemark_core::{
    live::{HotEntry, HotMinute},
    path::RawPath,
};
use tidemark_fs::attribution::AttributionCache;
use tidemark_sys::fanotify::{mask, Fanotify, InitMode, RawEvent};

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
}
/// Owns a bounded background watcher and joins it on stop or drop.
pub struct Watcher {
    mode: WatchMode,
    state: Arc<Mutex<State>>,
    shutdown: Arc<OwnedFd>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    mount: RawPath,
}
struct State {
    rings: rings::Rings,
    dirty: BTreeSet<RawPath>,
    events: u64,
    overflows: u64,
}
fn now() -> i64 {
    tidemark_core::time::unix_now()
}

impl Watcher {
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
        let mut selected = None;
        if caps.report_dfid_name {
            if let Ok(group) = Fanotify::init(InitMode::DfidName) {
                let rich = event_mask
                    | mask::CREATE
                    | mask::DELETE
                    | mask::MOVED_FROM
                    | mask::MOVED_TO
                    | mask::ONDIR;
                if caps.mark_filesystem && group.mark_filesystem(cfg.mount.as_path(), rich).is_ok()
                {
                    selected = Some((group, WatchMode::DfidNameFilesystem));
                } else if group.mark_mount(cfg.mount.as_path(), rich).is_ok() {
                    selected = Some((group, WatchMode::DfidNameMount));
                }
            }
        }
        let (group, mode) = match selected {
            Some(v) => v,
            None => {
                if !caps.fd_mode {
                    return Err(WatchError::Unsupported);
                }
                let group =
                    Fanotify::init(InitMode::FdMode).map_err(|_| WatchError::Unsupported)?;
                group
                    .mark_mount(cfg.mount.as_path(), event_mask)
                    .map_err(|_| WatchError::Unsupported)?;
                (group, WatchMode::FdMount)
            }
        };
        let mount_fd = rustix::fs::open(
            cfg.mount.as_path(),
            rustix::fs::OFlags::PATH
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(io::Error::from)?;
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
            events: 0,
            overflows: 0,
        }));
        let worker_state = state.clone();
        let worker_shutdown = shutdown.clone();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = stopping.clone();
        let mount = cfg.mount.clone();
        tracing::info!(mode = %mode, mount = %mount.display_lossy(), "watcher started");
        let thread = std::thread::Builder::new()
            .name("tidemark-watch".into())
            .spawn(move || {
                let mut buffer = vec![0; 65536];
                let mut processor = Processor {
                    cfg: &cfg,
                    attribution: &attribution,
                    mount_fd: mount_fd.as_fd(),
                    sizes: fdmode::Sizes::new(cfg.lru_entries, now()),
                    paths: fidmode::PathSizes::new(cfg.lru_entries),
                    handles: fidmode::HandlePaths::new(100_000),
                };
                loop {
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
                        tracing::warn!(%error,"fanotify poll failed");
                        worker_state.lock().rings.overflow(now());
                        break;
                    }
                    if !poll[1].revents().is_empty() {
                        break;
                    }
                    worker_state.lock().rings.advance(now());
                    if poll[0].revents().is_empty() {
                        continue;
                    }
                    let events = match group.read(&mut buffer) {
                        Ok(events) => events,
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            tracing::warn!(%error,"fanotify read failed");
                            worker_state.lock().rings.overflow(now());
                            break;
                        }
                    };
                    for event in events {
                        if worker_stopping.load(Ordering::Relaxed) {
                            break;
                        }
                        let at = now();
                        let Ok(event) = event else {
                            worker_state.lock().rings.overflow(at);
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
        })
    }
    /// Return the reporting mode selected by successful marks.
    pub fn mode(&self) -> WatchMode {
        self.mode
    }
    /// Sum the most recent 1–60 minute buckets and return ranked entries.
    pub fn hot(&self, window_min: u32, scope: HotScope, limit: usize) -> Vec<HotEntry> {
        let mut state = self.state.lock();
        state.rings.advance(now());
        state.rings.hot(now(), window_min, scope, limit)
    }
    /// Return each retained completed minute once, with at most top_k rows per scope.
    pub fn drain_completed_minutes(&self, top_k: usize) -> Vec<HotMinute> {
        self.state
            .lock()
            .rings
            .drain(now(), &self.mount.display_lossy(), top_k)
    }
    /// Remove and return dirty directories underneath a component-wise root prefix.
    pub fn take_dirty(&self, under: &RawPath) -> Vec<RawPath> {
        let mut state = self.state.lock();
        let paths: Vec<_> = state
            .dirty
            .iter()
            .filter(|p| p.starts_with(under))
            .cloned()
            .collect();
        for path in &paths {
            state.dirty.remove(path);
        }
        paths
    }
    /// Read counters and the loss indicator for the latest fifteen minute buckets.
    pub fn stats(&self) -> WatchStats {
        let mut state = self.state.lock();
        state.rings.advance(now());
        WatchStats {
            mode: self.mode,
            events_total: state.events,
            overflows_total: state.overflows,
            lossy_minutes_15m: state.rings.lossy(now()),
            dropped_keys: state.rings.dropped,
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
/// Compare the current and PID 1 mount-namespace identities.
pub fn namespace_ok() -> io::Result<bool> {
    Ok(std::fs::read_link("/proc/self/ns/mnt")? == std::fs::read_link("/proc/1/ns/mnt")?)
}

struct Processor<'a> {
    cfg: &'a WatcherOptions,
    attribution: &'a AttributionCache,
    mount_fd: std::os::fd::BorrowedFd<'a>,
    sizes: fdmode::Sizes,
    paths: fidmode::PathSizes,
    handles: fidmode::HandlePaths,
}
impl Processor<'_> {
    fn process(&mut self, event: RawEvent, at: i64, state: &Mutex<State>) {
        let cfg = self.cfg;
        let attribution = self.attribution;
        let mount_fd = self.mount_fd;
        let sizes = &mut self.sizes;
        let is_fid = matches!(&event, RawEvent::Fid { .. });
        let event_mask = match &event {
            RawEvent::Fd { mask, .. } | RawEvent::Fid { mask, .. } => *mask,
            RawEvent::Overflow => 0,
        };
        if matches!(event, RawEvent::Overflow) {
            let mut state = state.lock();
            state.overflows = state.overflows.saturating_add(1);
            state.rings.overflow(at);
            return;
        }
        let resolved = match event {
            RawEvent::Fd { pid, fd, .. } => {
                if pid == std::process::id() as i32 {
                    return;
                }
                fdmode::resolve(fd.as_fd()).map(|(p, s, created)| (pid, p, Some(s), created))
            }
            RawEvent::Fid {
                pid,
                handle_type,
                handle,
                name,
                mask,
                ..
            } => {
                if pid == std::process::id() as i32 {
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
                .map(|(p, s, created)| (pid, p, s, created))
            }
            RawEvent::Overflow => return,
        };
        let Ok((pid, path, stat, created)) = resolved else {
            state.lock().rings.overflow(at);
            return;
        };
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
        let who = attribution.lookup(u32::try_from(pid).unwrap_or(0));
        let parent = path
            .parent()
            .filter(|p| p.starts_with(&cfg.mount))
            .unwrap_or_else(|| cfg.mount.clone());
        let key = dir_key(&parent, &cfg.mount, cfg.hot_depth);
        let mut state = state.lock();
        state.events = state.events.saturating_add(1);
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
    fn empty_state() -> Mutex<State> {
        Mutex::new(State {
            rings: rings::Rings::new(),
            dirty: BTreeSet::new(),
            events: 0,
            overflows: 0,
        })
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
        processor.process(RawEvent::Overflow, at, &state);
        assert_eq!(state.lock().overflows, 1);
        assert!(state.lock().rings.hot(at, 1, HotScope::Dir, 1)[0].lossy);
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
            cfg: &cfg,
            attribution: &attribution,
            mount_fd: mount_fd.as_fd(),
            sizes: fdmode::Sizes::new(10, at),
            paths: fidmode::PathSizes::new(10),
            handles: fidmode::HandlePaths::new(100_000),
        };
        let state = empty_state();
        let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
        let before = count();
        for _ in 0..1000 {
            for pid in [0, std::process::id() as i32] {
                processor.process(
                    RawEvent::Fd {
                        mask: mask::CLOSE_WRITE,
                        pid,
                        fd: std::fs::File::open(temp.path()).unwrap().into(),
                    },
                    at,
                    &state,
                );
            }
        }
        assert_eq!(count(), before);
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
