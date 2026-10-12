//! Owned by: W1-B
//! Bounded parallel traversal and per-directory records.
use super::{getdents, WalkConfig, WalkError, WalkSummary};
use crossbeam_channel::{SendTimeoutError, Sender};
use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use parking_lot::{Condvar, Mutex};
use rustix::fs::{self, AtFlags, FileType, RawDir, StatxFlags};
use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    io,
    mem::MaybeUninit,
    os::unix::ffi::OsStrExt,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::*,
};

// Caps are deliberately explicit because the frozen WalkConfig has no memory budget.
const MAX_TASKS: usize = 65_536;
const MAX_RECORD_ITEMS: usize = 1_000_000;
const MAX_TASK_BYTES: usize = 64 * 1024 * 1024;
const MAX_THRESHOLD_CANDIDATES: usize = 4096;
const MAX_ERRORS: usize = 10_000;
struct Task {
    id: DirId,
    parent: Option<DirId>,
    name: Option<RawName>,
    path: RawPath,
    ino: Option<Ino>,
}
struct State {
    failure: Option<WalkError>,
}
struct Shared<'a> {
    cfg: &'a WalkConfig,
    gate: &'a dyn Gate,
    cancel: &'a AtomicBool,
    state: Mutex<State>,
    ready: Condvar,
    injector: Injector<Task>,
    stealers: Vec<Stealer<Task>>,
    inflight: AtomicUsize,
    task_bytes: AtomicUsize,
    ids: AtomicU32,
    errors: AtomicUsize,
    dev: DevId,
    mount_id: u64,
    mounts: Vec<RawPath>,
    out: Sender<WalkOutput>,
}
#[derive(Default)]
struct Counts {
    dirs: u64,
    entries: u64,
    errors: u64,
    races: u64,
    falls: u64,
}

pub(super) fn run(
    cfg: &WalkConfig,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    out: Sender<WalkOutput>,
) -> Result<WalkSummary, WalkError> {
    if !(1..=64).contains(&cfg.threads)
        || !(4096..=4 * 1024 * 1024).contains(&cfg.getdents_buf)
        || cfg.top_files > 200_000
    {
        return Err(WalkError::Limit(
            "threads 1..64, buffer 4 KiB..4 MiB, top_files <= 200000 required".into(),
        ));
    }
    if out.capacity().is_none_or(|n| n == 0 || n > 1024) {
        return Err(WalkError::Limit("output capacity must be 1..1024".into()));
    }
    if cancel.load(Ordering::Acquire) {
        return Err(WalkError::Cancelled);
    }
    let start = Instant::now();
    let falls = 0;
    let root = getdents::open_path(&cfg.root, gate, cancel).map_err(cancel_error)?;
    getdents::charge(gate, cancel).map_err(cancel_error)?;
    let sx = fs::statx(
        &root,
        &b""[..],
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(io::Error::from)?;
    let dev = libc::makedev(sx.stx_dev_major, sx.stx_dev_minor);
    drop(root);
    let mut mounts = cfg.skip_mounts.clone();
    getdents::charge(gate, cancel).map_err(cancel_error)?;
    match crate::mount::read_mountinfo() {
        Ok(table) => mounts.extend(crate::mount::submounts_under(&table, &cfg.root)),
        Err(e) => {
            if sx.stx_mnt_id == 0 {
                return Err(io::Error::new(io::ErrorKind::Unsupported,
                    format!("cannot identify bind mounts: mountinfo unavailable ({e}) and statx has no mount id")).into());
            }
            tracing::debug!(error = %e, "mount table unavailable; using device and mount ids")
        }
    }
    mounts.sort_unstable();
    mounts.dedup();
    let locals: Vec<_> = (0..cfg.threads).map(|_| Worker::new_lifo()).collect();
    let injector = Injector::new();
    injector.push(Task {
        id: DirId(0),
        parent: None,
        name: None,
        path: cfg.root.clone(),
        ino: Some(sx.stx_ino),
    });
    let shared = Shared {
        cfg,
        gate,
        cancel,
        state: Mutex::new(State { failure: None }),
        injector,
        stealers: locals.iter().map(Worker::stealer).collect(),
        inflight: AtomicUsize::new(1),
        task_bytes: AtomicUsize::new(cfg.root.as_bytes().len()),
        ready: Condvar::new(),
        ids: AtomicU32::new(1),
        errors: AtomicUsize::new(0),
        dev,
        mount_id: sx.stx_mnt_id,
        mounts,
        out,
    };
    let counts = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for local in locals {
            let shared_ref = &shared;
            handles.push(scope.spawn(move || worker(shared_ref, local)));
        }
        let mut total = Counts {
            falls,
            ..Counts::default()
        };
        for handle in handles {
            match handle.join() {
                Ok(c) => {
                    total.dirs += c.dirs;
                    total.entries += c.entries;
                    total.errors += c.errors;
                    total.races += c.races;
                    total.falls += c.falls;
                }
                Err(_) => {
                    shared.state.lock().failure = Some(WalkError::Limit("worker panicked".into()));
                }
            }
        }
        total
    });
    if let Some(e) = shared.state.lock().failure.take() {
        return Err(e);
    }
    if cancel.load(Ordering::Acquire) {
        return Err(WalkError::Cancelled);
    }
    Ok(WalkSummary {
        dirs: counts.dirs,
        entries: counts.entries,
        errors: counts.errors,
        races: counts.races,
        noatime_fallbacks: counts.falls,
        elapsed: start.elapsed(),
        root_dev: dev,
        root_ino: sx.stx_ino,
    })
}
fn worker(s: &Shared<'_>, local: Worker<Task>) -> Counts {
    if s.cfg.io_idle {
        if let Err(e) = tidemark_sys::ioprio::set_current_thread_idle() {
            tracing::debug!(error=%e, "idle priority unavailable");
        }
    }
    let mut c = Counts::default();
    let mut heap = BinaryHeap::<Reverse<u64>>::new();
    let mut buffer = vec![MaybeUninit::uninit(); s.cfg.getdents_buf];
    loop {
        if s.cancel.load(Ordering::Acquire) || s.state.lock().failure.is_some() {
            return c;
        }
        let mut task = local.pop();
        if task.is_none() {
            match s.injector.steal_batch_and_pop(&local) {
                Steal::Success(t) => task = Some(t),
                Steal::Retry => continue,
                Steal::Empty => {}
            }
        }
        if task.is_none() {
            for stealer in &s.stealers {
                match stealer.steal() {
                    Steal::Success(t) => {
                        task = Some(t);
                        break;
                    }
                    Steal::Retry => {
                        break;
                    }
                    Steal::Empty => {}
                }
            }
        }
        let Some(task) = task else {
            if s.inflight.load(Ordering::Acquire) == 0 {
                return c;
            }
            let mut q = s.state.lock();
            s.ready.wait_for(&mut q, Duration::from_millis(10));
            continue;
        };
        let active = ActiveTask {
            shared: s,
            bytes: task.path.as_bytes().len(),
        };
        let result = directory(s, task, &local, &mut c, &mut heap, &mut buffer)
            .and_then(|record| send(s, record));
        if let Err(e) = result {
            let mut q = s.state.lock();
            if q.failure.is_none() {
                q.failure = Some(e);
            }
        }
        drop(active);
    }
}
// A panicking Gate must not leave the other workers waiting for a lost task.
struct ActiveTask<'a, 'b> {
    shared: &'a Shared<'b>,
    bytes: usize,
}
impl Drop for ActiveTask<'_, '_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let mut q = self.shared.state.lock();
            if q.failure.is_none() {
                q.failure = Some(WalkError::Limit("worker panicked".into()));
            }
        }
        self.shared
            .task_bytes
            .fetch_sub(self.bytes, Ordering::Relaxed);
        self.shared.inflight.fetch_sub(1, Ordering::AcqRel);
        self.shared.ready.notify_all();
    }
}
fn send(s: &Shared<'_>, mut record: WalkOutput) -> Result<(), WalkError> {
    loop {
        match s.out.send_timeout(record, Duration::from_millis(20)) {
            Ok(()) => return Ok(()),
            Err(SendTimeoutError::Disconnected(_)) => return Err(WalkError::Disconnected),
            Err(SendTimeoutError::Timeout(r)) => record = r,
        }
        if s.cancel.load(Ordering::Acquire) {
            return Err(WalkError::Cancelled);
        }
        if s.state.lock().failure.is_some() {
            return Err(WalkError::Disconnected);
        }
    }
}
fn race(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP | libc::ESTALE)
    )
}
fn gone(t: Task, reason: GoneReason, c: &mut Counts) -> Result<WalkOutput, WalkError> {
    c.races += 1;
    match (t.parent, t.name) {
        (Some(parent), Some(name)) => Ok(WalkOutput::Gone(GoneRecord {
            id: t.id,
            parent,
            name,
            reason,
        })),
        _ => Err(WalkError::Io(io::Error::other(
            "root vanished or was replaced",
        ))),
    }
}
fn error(
    s: &Shared<'_>,
    r: &mut DirRecord,
    c: &mut Counts,
    path: RawPath,
    op: ErrOp,
    e: &io::Error,
) {
    c.errors += 1;
    if s.errors.fetch_add(1, Ordering::Relaxed) < MAX_ERRORS {
        r.errors.push(ScanErrorRec {
            path,
            op,
            errno: e.raw_os_error().unwrap_or(libc::EIO),
        });
    }
}
fn set_error_total(record: &mut DirRecord, before: u64, after: u64) -> Result<(), WalkError> {
    record.errors_total = u32::try_from(after - before)
        .map_err(|_| WalkError::Limit("directory error count".into()))?;
    Ok(())
}
fn directory(
    s: &Shared<'_>,
    t: Task,
    local: &Worker<Task>,
    c: &mut Counts,
    heap: &mut BinaryHeap<Reverse<u64>>,
    buffer: &mut [MaybeUninit<u8>],
) -> Result<WalkOutput, WalkError> {
    let errors_before = c.errors;
    let opened = getdents::open_directory(&t.path, s.gate, &mut c.falls, s.cancel);
    if s.cancel.load(Ordering::Acquire) {
        return Err(WalkError::Cancelled);
    }
    if let Err(e) = &opened {
        if race(e) {
            return gone(
                t,
                if e.raw_os_error() == Some(libc::ENOENT) {
                    GoneReason::Vanished
                } else {
                    GoneReason::Replaced
                },
                c,
            );
        }
    }
    let sx_result = match &opened {
        Ok(fd) => {
            getdents::charge(s.gate, s.cancel).map_err(cancel_error)?;
            fs::statx(
                fd,
                &b""[..],
                AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
                StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
            )
            .map_err(io::Error::from)
        }
        Err(_) => (|| -> io::Result<fs::Statx> {
            // Stat through a verified parent handle; never follow ancestors during recovery.
            let parent = t
                .path
                .parent()
                .ok_or_else(|| io::Error::other("cannot stat inaccessible root"))?;
            let name = t
                .path
                .file_name()
                .ok_or_else(|| io::Error::other("missing root name"))?;
            let fd = getdents::open_path(&parent, s.gate, s.cancel)?;
            getdents::stat(&fd, name.as_bytes(), s.gate, s.cancel)
        })(),
    };
    let sx = match sx_result {
        Ok(sx) => sx,
        Err(e) if race(&e) => {
            return gone(
                t,
                if e.raw_os_error() == Some(libc::ENOENT) {
                    GoneReason::Vanished
                } else {
                    GoneReason::Replaced
                },
                c,
            )
        }
        Err(e) => return Err(e.into()),
    };
    let dev = libc::makedev(sx.stx_dev_major, sx.stx_dev_minor);
    if super::boundary_reason(
        s.dev,
        if t.parent.is_some() { s.mount_id } else { 0 },
        dev,
        sx.stx_mnt_id,
    )
    .is_some()
    {
        return gone(t, GoneReason::OtherFs, c);
    }
    if t.ino.is_some_and(|ino| ino != sx.stx_ino)
        || getdents::kind(FileType::from_raw_mode(sx.stx_mode.into())) != FileKind::Dir
    {
        return gone(t, GoneReason::Replaced, c);
    }
    let mut r = DirRecord {
        id: t.id,
        parent: t.parent,
        name: t.name,
        ino: sx.stx_ino,
        dev,
        mtime: sx.stx_mtime.tv_sec,
        dir_disk: sx.stx_blocks.saturating_mul(512),
        dir_apparent: sx.stx_size,
        projid: None,
        entries: 0,
        own: OwnStats::default(),
        links: Vec::new(),
        notable: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
        errors_total: 0,
        races: 0,
    };
    let mut record_bytes =
        std::mem::size_of::<DirRecord>() as u64 + 1024 + t.path.as_bytes().len() as u64;
    charge_record(s.cfg, &t.path, 0, &mut record_bytes, 0)?;
    let mut children = Vec::new();
    let mut top = Vec::<NotableFile>::new();
    let mut directory_heap = BinaryHeap::<(Reverse<u64>, u64, usize)>::new();
    match opened {
        Err(e) => {
            charge_record(
                s.cfg,
                &t.path,
                r.entries,
                &mut record_bytes,
                (2 * std::mem::size_of::<ScanErrorRec>() + t.path.as_bytes().len() + 32) as u64,
            )?;
            error(s, &mut r, c, t.path.clone(), ErrOp::Open, &e);
        }
        Ok(fd) => {
            let mut iter = RawDir::new(&fd, buffer);
            let mut path_bytes = t.path.as_bytes().to_vec();
            if path_bytes.len() > 1 {
                path_bytes.push(b'/');
            }
            let prefix_len = path_bytes.len();
            loop {
                if s.cancel.load(Ordering::Acquire) {
                    return Err(WalkError::Cancelled);
                }
                if iter.is_buffer_empty() {
                    getdents::charge(s.gate, s.cancel).map_err(cancel_error)?;
                }
                let Some(entry) = iter.next() else { break };
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        charge_record(
                            s.cfg,
                            &t.path,
                            r.entries,
                            &mut record_bytes,
                            (2 * std::mem::size_of::<ScanErrorRec>() + t.path.as_bytes().len() + 32)
                                as u64,
                        )?;
                        error(s, &mut r, c, t.path.clone(), ErrOp::Getdents, &e.into());
                        break;
                    }
                };
                let bytes = entry.file_name().to_bytes();
                if bytes == b"." || bytes == b".." {
                    continue;
                }
                r.entries = r
                    .entries
                    .checked_add(1)
                    .ok_or(WalkError::Limit("entry count overflow".into()))?;
                if r.entries as usize > MAX_RECORD_ITEMS {
                    return Err(WalkError::Limit("million-entry directory cap".into()));
                }
                path_bytes.truncate(prefix_len);
                path_bytes.extend_from_slice(bytes);
                let make_name = || RawName::new(bytes.to_vec()).map_err(io::Error::other);
                let make_path = || RawPath::new(path_bytes.clone()).map_err(io::Error::other);
                if s.mounts
                    .binary_search_by(|p| p.as_bytes().cmp(&path_bytes))
                    .is_ok()
                {
                    charge_record(
                        s.cfg,
                        &t.path,
                        r.entries,
                        &mut record_bytes,
                        (2 * std::mem::size_of::<Skip>() + bytes.len() + 32) as u64,
                    )?;
                    r.skipped.push(Skip {
                        name: make_name()?,
                        reason: SkipReason::Mount,
                    });
                    continue;
                }
                if s.cfg
                    .exclude
                    .is_match(std::path::Path::new(std::ffi::OsStr::from_bytes(
                        &path_bytes,
                    )))
                {
                    charge_record(
                        s.cfg,
                        &t.path,
                        r.entries,
                        &mut record_bytes,
                        (2 * std::mem::size_of::<Skip>() + bytes.len() + 32) as u64,
                    )?;
                    r.skipped.push(Skip {
                        name: make_name()?,
                        reason: SkipReason::Excluded,
                    });
                    continue;
                }
                let mut attrs = None;
                let kind = if entry.file_type() == FileType::Unknown {
                    match getdents::stat(&fd, bytes, s.gate, s.cancel) {
                        Ok(st) => {
                            let k = getdents::kind(FileType::from_raw_mode(st.stx_mode.into()));
                            attrs = Some(st);
                            k
                        }
                        Err(e) => {
                            if race(&e) {
                                r.races += 1;
                            } else {
                                charge_record(
                                    s.cfg,
                                    &t.path,
                                    r.entries,
                                    &mut record_bytes,
                                    (2 * std::mem::size_of::<ScanErrorRec>()
                                        + path_bytes.len()
                                        + 32) as u64,
                                )?;
                                error(s, &mut r, c, make_path()?, ErrOp::Statx, &e);
                            }
                            continue;
                        }
                    }
                } else {
                    getdents::kind(entry.file_type())
                };
                if kind == FileKind::Dir {
                    let name = make_name()?;
                    let path = make_path()?;
                    let id = s
                        .ids
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                        .map_err(|_| WalkError::Limit("directory id overflow".into()))?;
                    s.inflight
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                            (n < MAX_TASKS).then_some(n + 1)
                        })
                        .map_err(|_| WalkError::Limit("65536 in-flight directories".into()))?;
                    let bytes = path.as_bytes().len();
                    if s.task_bytes
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                            n.checked_add(bytes).filter(|n| *n <= MAX_TASK_BYTES)
                        })
                        .is_err()
                    {
                        s.inflight.fetch_sub(1, Ordering::AcqRel);
                        return Err(WalkError::Limit("64 MiB pending path bytes".into()));
                    }
                    local.push(Task {
                        id: DirId(id),
                        parent: Some(r.id),
                        name: Some(name),
                        path,
                        ino: Some(entry.ino()),
                    });
                    s.ready.notify_one();
                    continue;
                }
                if !s.cfg.stat_entries {
                    charge_record(
                        s.cfg,
                        &t.path,
                        r.entries,
                        &mut record_bytes,
                        (2 * std::mem::size_of::<Ino>()) as u64,
                    )?;
                    children.push(entry.ino());
                    continue;
                }
                let st = match attrs
                    .map(Ok)
                    .unwrap_or_else(|| getdents::stat(&fd, bytes, s.gate, s.cancel))
                {
                    Ok(st) => st,
                    Err(e) => {
                        if race(&e) {
                            r.races += 1;
                        } else {
                            charge_record(
                                s.cfg,
                                &t.path,
                                r.entries,
                                &mut record_bytes,
                                (2 * std::mem::size_of::<ScanErrorRec>() + path_bytes.len() + 32)
                                    as u64,
                            )?;
                            error(s, &mut r, c, make_path()?, ErrOp::Statx, &e);
                        }
                        continue;
                    }
                };
                let entry_dev = libc::makedev(st.stx_dev_major, st.stx_dev_minor);
                if super::boundary_reason(s.dev, s.mount_id, entry_dev, st.stx_mnt_id).is_some() {
                    charge_record(
                        s.cfg,
                        &t.path,
                        r.entries,
                        &mut record_bytes,
                        (2 * std::mem::size_of::<Skip>() + bytes.len() + 32) as u64,
                    )?;
                    r.skipped.push(Skip {
                        name: make_name()?,
                        reason: SkipReason::OtherFs,
                    });
                    continue;
                }
                let actual = getdents::kind(FileType::from_raw_mode(st.stx_mode.into()));
                if actual != kind || st.stx_ino != entry.ino() {
                    r.races += 1;
                    continue;
                }
                let disk = st.stx_blocks.saturating_mul(512);
                let mtime = st.stx_mtime.tv_sec;
                if st.stx_nlink > 1 {
                    charge_record(
                        s.cfg,
                        &t.path,
                        r.entries,
                        &mut record_bytes,
                        (2 * std::mem::size_of::<LinkRef>() + bytes.len() + 32) as u64,
                    )?;
                    r.links.push(LinkRef {
                        ino: st.stx_ino,
                        nlink: st.stx_nlink,
                        kind: actual,
                        disk,
                        apparent: st.stx_size,
                        mtime,
                        name: Some(make_name()?),
                    });
                } else {
                    r.own.files += 1;
                    r.own.symlinks += u64::from(actual == FileKind::Symlink);
                    r.own.disk += disk;
                    r.own.apparent += st.stx_size;
                    r.own.age.0[tidemark_core::time::age_bucket(s.cfg.scan_start, mtime)] += disk;
                    if r.own.files == 1 || mtime > r.own.newest_mtime {
                        r.own.newest_mtime = mtime;
                    }
                    // The heap tracks sizes only: discovery IDs and names cannot
                    // reproduce final directory preorder / inode tie ordering.
                    // Cross-directory cutoff ties survive the per-directory top-N bound.
                    let admitted = s.cfg.top_files > 0
                        && (heap.len() < s.cfg.top_files
                            || heap
                                .peek()
                                .is_some_and(|Reverse(least_disk)| disk >= *least_disk));
                    if admitted
                        && (heap.len() < s.cfg.top_files
                            || heap
                                .peek()
                                .is_some_and(|Reverse(least_disk)| disk > *least_disk))
                    {
                        if heap.len() == s.cfg.top_files {
                            heap.pop();
                        }
                        heap.push(Reverse(disk));
                    }
                    let qualifies = s.cfg.top_files > 0
                        && (directory_heap.len() < s.cfg.top_files
                            || directory_heap
                                .peek()
                                .is_some_and(|(Reverse(size), ino, _)| {
                                    (Reverse(disk), st.stx_ino) < (Reverse(*size), *ino)
                                }));
                    if qualifies || disk >= s.cfg.big_file_threshold {
                        let candidate_bytes =
                            (2 * std::mem::size_of::<NotableFile>() + bytes.len() + 32) as u64;
                        // Both threshold and top collections may own a copy.
                        if disk >= s.cfg.big_file_threshold {
                            charge_record(
                                s.cfg,
                                &t.path,
                                r.entries,
                                &mut record_bytes,
                                candidate_bytes,
                            )?;
                        }
                        if qualifies {
                            if top.len() == s.cfg.top_files {
                                if let Some((_, _, index)) = directory_heap.peek() {
                                    let old_name = top[*index]
                                        .name
                                        .as_ref()
                                        .map(|n| n.as_bytes().len())
                                        .unwrap_or(0);
                                    record_bytes = record_bytes.saturating_sub(old_name as u64);
                                    charge_record(
                                        s.cfg,
                                        &t.path,
                                        r.entries,
                                        &mut record_bytes,
                                        bytes.len() as u64,
                                    )?;
                                }
                            } else {
                                charge_record(
                                    s.cfg,
                                    &t.path,
                                    r.entries,
                                    &mut record_bytes,
                                    candidate_bytes
                                        + (2 * std::mem::size_of::<(Reverse<u64>, u64, usize)>())
                                            as u64,
                                )?;
                            }
                        }
                        let candidate = NotableFile {
                            ino: st.stx_ino,
                            name: Some(make_name()?),
                            disk,
                            apparent: st.stx_size,
                            mtime,
                        };
                        if disk >= s.cfg.big_file_threshold {
                            if r.notable.len() == MAX_THRESHOLD_CANDIDATES {
                                return Err(WalkError::Limit(
                                    "4096 threshold candidates per directory".into(),
                                ));
                            }
                            r.notable.push(candidate.clone());
                        }
                        if qualifies {
                            let index = if top.len() == s.cfg.top_files {
                                // The heap is nonempty because top_files > 0 here.
                                let Some((_, _, index)) = directory_heap.pop() else {
                                    return Err(WalkError::Limit(
                                        "missing directory candidate".into(),
                                    ));
                                };
                                top[index] = candidate;
                                index
                            } else {
                                top.push(candidate);
                                top.len() - 1
                            };
                            directory_heap.push((Reverse(disk), st.stx_ino, index));
                        }
                    }
                }
            }
        }
    }
    // Preserve worker cutoff ties across directories, but retain only the exact
    // first top_files within each directory (final directory ID is constant).
    merge_notable(
        s.cfg,
        &t.path,
        r.entries,
        &mut record_bytes,
        &mut r.notable,
        top,
        directory_heap,
        heap.peek().map(|Reverse(cutoff)| *cutoff),
        &|_| {},
    )?;
    set_error_total(&mut r, errors_before, c.errors)?;
    c.dirs += 1;
    c.entries += u64::from(r.entries);
    c.races += u64::from(r.races);
    r.links.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    r.notable.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    r.skipped.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    children.sort_unstable();
    r.errors
        .sort_unstable_by(|a, b| a.path.cmp(&b.path).then(a.errno.cmp(&b.errno)));
    if s.cfg.stat_entries {
        Ok(WalkOutput::Dir(r))
    } else {
        Ok(WalkOutput::Ns(NsRecord {
            id: r.id,
            parent: r.parent,
            name: r.name,
            ino: r.ino,
            dev: r.dev,
            entries: r.entries,
            children,
            skipped: r.skipped,
            errors: r.errors,
            errors_total: r.errors_total,
            races: r.races,
        }))
    }
}

// The observer sees every live allocation at the merge boundary; production uses a no-op.
#[allow(clippy::too_many_arguments)]
fn merge_notable(
    cfg: &WalkConfig,
    path: &RawPath,
    entries: u32,
    used: &mut u64,
    output: &mut Vec<NotableFile>,
    mut top: Vec<NotableFile>,
    directory_heap: BinaryHeap<(Reverse<u64>, u64, usize)>,
    cutoff: Option<u64>,
    observe: &dyn Fn(usize),
) -> Result<(), WalkError> {
    let bytes = |v: &Vec<NotableFile>| {
        v.capacity() * std::mem::size_of::<NotableFile>()
            + v.iter()
                .map(|f| f.name.as_ref().map_or(0, |n| n.as_bytes().len()))
                .sum::<usize>()
    };
    observe(
        bytes(output)
            + bytes(&top)
            + directory_heap.capacity() * std::mem::size_of::<(Reverse<u64>, u64, usize)>(),
    );
    drop(directory_heap);
    top.retain(|f| f.disk < cfg.big_file_threshold && cutoff.is_some_and(|c| f.disk >= c));
    let needed = top
        .len()
        .checked_add(output.len())
        .ok_or_else(|| WalkError::Limit("candidate count overflow".into()))?;
    if needed > top.capacity() {
        // reserve_exact may temporarily keep the old buffer alive. Charge the
        // entire new allocation, not just its incremental payload, before reserve.
        let allocation = needed
            .checked_mul(std::mem::size_of::<NotableFile>())
            .ok_or_else(|| WalkError::Limit("candidate byte overflow".into()))?;
        charge_record(cfg, path, entries, used, allocation as u64)?;
        observe(bytes(output) + bytes(&top) + allocation);
        top.reserve_exact(needed - top.len());
    }
    observe(bytes(output) + bytes(&top));
    top.append(output);
    // Assignment drops the old threshold buffer and transfers top's allocation.
    *output = top;
    observe(bytes(output));
    Ok(())
}

// Charges conservatively include Vec growth, allocator padding and candidate clones.
fn charge_record(
    cfg: &WalkConfig,
    path: &RawPath,
    entries: u32,
    used: &mut u64,
    extra: u64,
) -> Result<(), WalkError> {
    *used = used
        .checked_add(extra)
        .ok_or_else(|| WalkError::Limit("record byte overflow".into()))?;
    if cfg.max_record_bytes != 0 && *used > cfg.max_record_bytes {
        return Err(WalkError::Limit(format!(
            "directory {} with {} entries exceeds record bound {} bytes; raise scan.memory_budget",
            path.display_lossy(),
            entries,
            cfg.max_record_bytes,
        )));
    }
    Ok(())
}

fn cancel_error(error: io::Error) -> WalkError {
    if error.kind() == io::ErrorKind::Interrupted {
        WalkError::Cancelled
    } else {
        WalkError::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notable_merge_charges_coexisting_reserve_before_allocation() {
        let root = RawPath::new(b"/fixture".to_vec()).unwrap();
        let cfg = WalkConfig {
            root: root.clone(),
            threads: 1,
            stat_entries: true,
            exclude: globset::GlobSet::empty(),
            skip_mounts: vec![],
            big_file_threshold: 2,
            top_files: 1,
            scan_start: 0,
            getdents_buf: 4096,
            io_idle: false,
            max_record_bytes: 4096,
        };
        let candidate = |ino, disk| NotableFile {
            ino,
            disk,
            apparent: 0,
            mtime: 0,
            name: None,
        };
        let mut output = vec![candidate(2, 2); 100];
        let original_capacity = output.capacity();
        let top = vec![candidate(1, 1)];
        let mut used = 4000;
        let observations = std::cell::Cell::new(0);
        let result = merge_notable(
            &cfg,
            &root,
            101,
            &mut used,
            &mut output,
            top,
            BinaryHeap::new(),
            Some(1),
            &|_| observations.set(observations.get() + 1),
        );
        assert!(matches!(result, Err(WalkError::Limit(_))));
        assert_eq!(output.capacity(), original_capacity);
        assert_eq!(output.len(), 100);
        assert_eq!(observations.get(), 1, "refusal must precede reserve");
    }

    #[test]
    fn notable_merge_power_of_two_plus_one_peak() {
        for threshold_count in [0, 4096] {
            let root = RawPath::new(b"/fixture".to_vec()).unwrap();
            let cfg = WalkConfig {
                root: root.clone(),
                threads: 1,
                stat_entries: true,
                exclude: globset::GlobSet::empty(),
                skip_mounts: vec![],
                big_file_threshold: 2,
                top_files: 65_537,
                scan_start: 0,
                getdents_buf: 4096,
                io_idle: false,
                max_record_bytes: 16 * 1024 * 1024,
            };
            let candidate = |ino, disk| NotableFile {
                ino,
                disk,
                apparent: 0,
                mtime: 0,
                name: Some(RawName::new(b"1234567".to_vec()).unwrap()),
            };
            let mut top = Vec::new();
            let mut heap = BinaryHeap::new();
            let mut output = Vec::new();
            let mut used = 2048;
            for i in 0..65_537 {
                charge_record(
                    &cfg,
                    &root,
                    i,
                    &mut used,
                    (2 * std::mem::size_of::<NotableFile>()
                        + 7
                        + 32
                        + 2 * std::mem::size_of::<(Reverse<u64>, u64, usize)>())
                        as u64,
                )
                .unwrap();
                top.push(candidate(u64::from(i), 1));
                heap.push((Reverse(1), u64::from(i), i as usize));
            }
            for i in 0..threshold_count {
                charge_record(
                    &cfg,
                    &root,
                    65_537,
                    &mut used,
                    (2 * std::mem::size_of::<NotableFile>() + 7 + 32) as u64,
                )
                .unwrap();
                output.push(candidate(100_000 + i, 2));
            }
            let peak = std::cell::Cell::new(0);
            merge_notable(
                &cfg,
                &root,
                65_537,
                &mut used,
                &mut output,
                top,
                heap,
                Some(1),
                &|bytes| peak.set(peak.get().max(bytes + 2048)),
            )
            .unwrap();
            assert!(
                peak.get() as u64 <= cfg.max_record_bytes,
                "peak={} bound={}",
                peak.get(),
                cfg.max_record_bytes
            );
            assert!(peak.get() as u64 <= used);
            assert_eq!(output.len(), 65_537 + threshold_count as usize);
        }
    }

    #[test]
    fn record_byte_bound_names_directory_and_preserves_smaller_records() {
        let fixture = test_support::materialize("[[entry]]\npath='file'\nkind='file'\nsize=8\n[[entry]]\npath='link'\nkind='hardlink'\ntarget='file'\n");
        let root = RawPath::from_path(&fixture.root).unwrap();
        let mut cfg = WalkConfig {
            root: root.clone(),
            threads: 1,
            stat_entries: true,
            exclude: globset::GlobSet::empty(),
            skip_mounts: vec![],
            big_file_threshold: u64::MAX,
            top_files: 0,
            scan_start: 100,
            getdents_buf: 4096,
            io_idle: false,
            max_record_bytes: 0,
        };
        let collect = |cfg: &WalkConfig| {
            let (tx, rx) = crossbeam_channel::bounded(2);
            let result = run(cfg, &NoopGate, &AtomicBool::new(false), tx);
            (result, rx.try_iter().collect::<Vec<_>>())
        };
        let (baseline, rows) = collect(&cfg);
        assert!(baseline.is_ok());
        cfg.max_record_bytes = 1 << 20;
        let (bounded, bounded_rows) = collect(&cfg);
        assert!(bounded.is_ok());
        assert_eq!(format!("{rows:?}"), format!("{bounded_rows:?}"));
        cfg.max_record_bytes =
            std::mem::size_of::<DirRecord>() as u64 + 1024 + root.as_bytes().len() as u64;
        let (limited, rows) = collect(&cfg);
        let error = limited.err().unwrap();
        assert!(matches!(error, WalkError::Limit(_)));
        let message = error.to_string();
        assert!(message.contains(&root.display_lossy()));
        assert!(message.contains("1 entries"));
        assert!(message.contains(&cfg.max_record_bytes.to_string()));
        assert!(message.contains("raise scan.memory_budget"));
        assert!(rows.is_empty());
    }

    #[test]
    fn stored_errors_are_capped_but_totals_are_not() {
        let model = test_support::golden::link_semantics_basic();
        let cfg = WalkConfig {
            root: model.root.clone(),
            threads: 1,
            stat_entries: true,
            exclude: globset::GlobSet::empty(),
            skip_mounts: Vec::new(),
            big_file_threshold: 0,
            top_files: 0,
            scan_start: model.now,
            getdents_buf: 4096,
            io_idle: false,
            max_record_bytes: 0,
        };
        let cancel = AtomicBool::new(false);
        let (out, _rx) = crossbeam_channel::bounded(1);
        let s = Shared {
            cfg: &cfg,
            gate: &NoopGate,
            cancel: &cancel,
            state: Mutex::new(State { failure: None }),
            ready: Condvar::new(),
            injector: Injector::new(),
            stealers: Vec::new(),
            inflight: AtomicUsize::new(0),
            task_bytes: AtomicUsize::new(0),
            ids: AtomicU32::new(1),
            errors: AtomicUsize::new(0),
            dev: 0,
            mount_id: 0,
            mounts: Vec::new(),
            out,
        };
        let mut record = model.to_dir_records().remove(0);
        let mut counts = Counts::default();
        let e = io::Error::from_raw_os_error(libc::EACCES);
        for _ in 0..MAX_ERRORS + 17 {
            error(
                &s,
                &mut record,
                &mut counts,
                cfg.root.clone(),
                ErrOp::Statx,
                &e,
            );
        }
        set_error_total(&mut record, 0, counts.errors).unwrap();
        assert_eq!(record.errors_total, (MAX_ERRORS + 17) as u32);
        let before = counts.errors;
        let mut next = model.to_dir_records().remove(1);
        for _ in 0..23 {
            error(
                &s,
                &mut next,
                &mut counts,
                cfg.root.clone(),
                ErrOp::Open,
                &e,
            );
        }
        set_error_total(&mut next, before, counts.errors).unwrap();
        assert_eq!(next.errors_total, 23);
        assert!(next.errors.is_empty());
        assert_eq!(
            u64::from(record.errors_total) + u64::from(next.errors_total),
            counts.errors
        );
        assert!(matches!(
            set_error_total(&mut next, 0, u64::from(u32::MAX) + 1),
            Err(WalkError::Limit(_))
        ));
        assert_eq!(record.errors.len(), MAX_ERRORS);
        assert_eq!(counts.errors, (MAX_ERRORS + 40) as u64);
        assert!(record
            .errors
            .iter()
            .all(|e| e.errno == libc::EACCES && e.op == ErrOp::Statx));
    }
}
