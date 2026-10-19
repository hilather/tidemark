//! Owned by: W7-A
//! Bounded, cancellable path history over immutable snapshot readers.

use super::CatalogRow;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::JoinHandle,
};
use tidemark_core::{
    config::RetentionConfig, path::RawPath, scan::SnapshotStatus, snapshot::SnapshotMeta,
};
use tidemark_store::reader::Snapshot;

pub(super) const SAMPLES: usize = 7;
const CACHE_PATHS: usize = 128;
const CACHE_BYTES: usize = 1024 * 1024;
pub(super) const COOLDOWN: i64 = 5;

/// One catalog identity. The opened entry is always last in a window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub id: String,
    pub file: PathBuf,
    pub started: i64,
}

/// Keep timestamp slots even when their paths or files have disappeared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Sample {
    Known(u64),
    MissingPath,
    Unavailable(&'static str),
}

impl Sample {
    pub fn value(self) -> Option<u64> {
        match self {
            Self::Known(value) => Some(value),
            _ => None,
        }
    }
}

/// Exact comparison identity; never includes a live measurement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Key {
    pub root: RawPath,
    pub path: RawPath,
    pub window: Vec<Entry>,
    pub fs_uuid: String,
}

pub(super) fn window(rows: &[CatalogRow], meta: &SnapshotMeta, file: &Path) -> Vec<Entry> {
    // Keep only six references while selecting, even for a very large catalog.
    let mut earlier: Vec<&CatalogRow> = Vec::with_capacity(SAMPLES);
    for row in rows.iter().filter(|row| row.2 < meta.started_at) {
        let at = earlier
            .partition_point(|other| other.2 > row.2 || (other.2 == row.2 && other.0 < row.0));
        if at < SAMPLES - 1 {
            earlier.insert(at, row);
            earlier.truncate(SAMPLES - 1);
        }
    }
    let mut window: Vec<_> = earlier
        .into_iter()
        .rev()
        .map(|row| Entry {
            id: row.0.clone(),
            file: row.1.clone(),
            started: row.2,
        })
        .collect();
    window.push(Entry {
        id: meta.scan_id.to_string(),
        file: file.to_owned(),
        started: meta.started_at,
    });
    window
}

struct Request {
    generation: u64,
    key: Key,
    seed: Option<u64>,
    // Known and missing immutable samples survive retries of failed slots.
    reuse: Vec<Option<Sample>>,
}

struct ResultRow {
    generation: u64,
    samples: Vec<Sample>,
}

#[derive(Default)]
struct Mailbox {
    pending: Option<Request>,
    result: Option<ResultRow>,
    #[cfg(test)]
    paused: bool,
    #[cfg(test)]
    entered: usize,
}

#[derive(Default)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
    generation: AtomicU64,
    cancel: AtomicBool,
    #[cfg(test)]
    opens: AtomicU64,
    #[cfg(test)]
    lookups: AtomicU64,
    #[cfg(test)]
    discarded: AtomicU64,
    #[cfg(test)]
    readers: AtomicU64,
    retention: RetentionConfig,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Mailbox> {
        self.mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn current(&self, generation: u64) -> bool {
        !self.cancel.load(Ordering::Acquire)
            && self.generation.load(Ordering::Acquire) == generation
    }

    /// The only history-open seam. Tests can hold it until released or cancelled.
    fn open(&self, file: &Path) -> Result<Snapshot, tidemark_store::StoreError> {
        #[cfg(test)]
        {
            self.opens.fetch_add(1, Ordering::Relaxed);
            let mut mailbox = self.lock();
            mailbox.entered += 1;
            self.wake.notify_all();
            while mailbox.paused && !self.cancel.load(Ordering::Acquire) {
                mailbox = self
                    .wake
                    .wait(mailbox)
                    .unwrap_or_else(|error| error.into_inner());
            }
        }
        Snapshot::open_with_retention(file, &self.retention)
    }
}

struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn new(policy: RetentionConfig) -> Self {
        let shared = Arc::new(Shared {
            retention: policy,
            ..Shared::default()
        });
        let worker = shared.clone();
        let thread = std::thread::spawn(move || {
            let mut readers: Vec<(Entry, Snapshot)> = Vec::new();
            loop {
                let request = {
                    let mut mailbox = worker.lock();
                    while mailbox.pending.is_none() && !worker.cancel.load(Ordering::Acquire) {
                        mailbox = worker
                            .wake
                            .wait(mailbox)
                            .unwrap_or_else(|error| error.into_inner());
                    }
                    if worker.cancel.load(Ordering::Acquire) {
                        break;
                    }
                    mailbox.pending.take()
                };
                let Some(request) = request else { continue };
                readers.retain(|(entry, _)| request.key.window.contains(entry));
                let mut samples = Vec::with_capacity(request.key.window.len());
                for (index, entry) in request.key.window.iter().enumerate() {
                    if !worker.current(request.generation) {
                        break;
                    }
                    if let Some(sample) = request.reuse.get(index).copied().flatten() {
                        samples.push(sample);
                        continue;
                    }
                    if index + 1 == request.key.window.len() {
                        if let Some(value) = request.seed {
                            samples.push(Sample::Known(value));
                            continue;
                        }
                    }
                    if !readers.iter().any(|(cached, _)| cached == entry) {
                        match worker.open(&entry.file) {
                            Ok(reader) => readers.push((entry.clone(), reader)),
                            Err(_) => {
                                samples.push(Sample::Unavailable("cannot open snapshot"));
                                continue;
                            }
                        }
                    }
                    if !worker.current(request.generation) {
                        break;
                    }
                    let Some((_, reader)) = readers.iter().find(|(cached, _)| cached == entry)
                    else {
                        continue;
                    };
                    if let Some(reason) = invalid(reader.meta(), entry, &request.key) {
                        samples.push(Sample::Unavailable(reason));
                        // A mismatched file may be repaired; do not pin that reader on retry.
                        readers.retain(|(cached, _)| cached != entry);
                        continue;
                    }
                    #[cfg(test)]
                    worker.lookups.fetch_add(1, Ordering::Relaxed);
                    match reader.dir_by_path(&request.key.path) {
                        Ok(Some(row)) => samples.push(Sample::Known(row.t_disk)),
                        Ok(None) => samples.push(Sample::MissingPath),
                        Err(_) => {
                            samples.push(Sample::Unavailable("cannot read directory"));
                            readers.retain(|(cached, _)| cached != entry);
                        }
                    }
                }
                #[cfg(test)]
                worker
                    .readers
                    .store(readers.len() as u64, Ordering::Relaxed);
                // Posting and publication use the same mutex: an old job cannot overwrite
                // the current result even if a request arrives at the last DB operation.
                let mut mailbox = worker.lock();
                if worker.current(request.generation) && samples.len() == request.key.window.len() {
                    mailbox.result = Some(ResultRow {
                        generation: request.generation,
                        samples,
                    });
                } else {
                    #[cfg(test)]
                    worker.discarded.fetch_add(1, Ordering::Relaxed);
                }
                worker.wake.notify_all();
            }
        });
        Self {
            shared,
            thread: Some(thread),
        }
    }

    fn cancel(&self) {
        let mut mailbox = self.shared.lock();
        self.shared.cancel.store(true, Ordering::Release);
        mailbox.pending = None;
        mailbox.result = None;
        self.shared.wake.notify_all();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn invalid(meta: &SnapshotMeta, entry: &Entry, key: &Key) -> Option<&'static str> {
    if meta.scan_id.as_str() != entry.id || meta.started_at != entry.started {
        Some("scan identity mismatch")
    } else if meta.status != SnapshotStatus::Complete {
        Some("incomplete snapshot")
    } else if meta.root != key.root {
        Some("root mismatch")
    } else if !meta.fs_uuid.is_empty() && !key.fs_uuid.is_empty() && meta.fs_uuid != key.fs_uuid {
        Some("filesystem mismatch")
    } else {
        None
    }
}

struct Cached {
    key: Key,
    samples: Vec<Sample>,
    failed_at: Option<i64>,
    bytes: usize,
}

impl Cached {
    fn new(key: Key, samples: Vec<Sample>, failed_at: Option<i64>) -> Self {
        // Include container storage and owned key buffers, with a conservative allowance
        // for RawPath's normalized allocation (at most twice its retained byte length).
        let bytes = std::mem::size_of::<Self>()
            + 2 * (key.root.as_bytes().len() + key.path.as_bytes().len())
            + key.fs_uuid.capacity()
            + key.window.capacity() * std::mem::size_of::<Entry>()
            + key
                .window
                .iter()
                .map(|e| e.id.capacity() + e.file.capacity())
                .sum::<usize>()
            + samples.capacity() * std::mem::size_of::<Sample>();
        Self {
            key,
            samples,
            failed_at,
            bytes,
        }
    }
}

/// UI-owned result cache and scheduling state; the worker alone owns historical readers.
pub(super) struct History {
    pub window: Vec<Entry>,
    pub samples: Vec<Option<Sample>>,
    pub key: Option<Key>,
    pub generation: u64,
    opened: Option<(String, PathBuf)>,
    visible: bool,
    active: bool,
    failed_at: Option<i64>,
    cache: VecDeque<Cached>,
    bytes: usize,
    worker: Option<Worker>,
    retention: RetentionConfig,
}

impl History {
    /// Cold opens use `retention` for the life of the worker.
    pub(super) fn new(retention: RetentionConfig) -> Self {
        Self {
            window: Vec::new(),
            samples: Vec::new(),
            key: None,
            generation: 0,
            opened: None,
            visible: false,
            active: false,
            failed_at: None,
            cache: VecDeque::new(),
            bytes: 0,
            worker: None,
            retention,
        }
    }

    pub fn prepare_window(&mut self, meta: &SnapshotMeta, file: &Path, catalog: &[CatalogRow]) {
        if self
            .opened
            .as_ref()
            .is_none_or(|(id, path)| id != meta.scan_id.as_str() || path != file)
        {
            self.window = window(catalog, meta, file);
            self.opened = Some((meta.scan_id.to_string(), file.to_owned()));
        }
    }

    pub fn update(&mut self, key: Key, seed: Option<u64>, visible: bool, refresh: bool, now: i64) {
        let changed = self.key.as_ref() != Some(&key);
        let window_changed = self.key.as_ref().is_none_or(|old| old.window != key.window);
        let appeared = visible && !self.visible;
        let retry = visible
            && !self.active
            && self
                .failed_at
                .is_some_and(|at| now.saturating_sub(at) >= COOLDOWN);
        if changed || appeared || refresh || retry || (!visible && self.visible) {
            self.generation += 1;
            if let Some(worker) = &self.worker {
                let mut mailbox = worker.shared.lock();
                worker
                    .shared
                    .generation
                    .store(self.generation, Ordering::Release);
                mailbox.pending = None;
                mailbox.result = None;
            }
            self.active = false;
            self.failed_at = None;
            self.samples = vec![None; key.window.len()];
            if let Some(index) = self.cache.iter().position(|cached| cached.key == key) {
                if let Some(cached) = self.cache.remove(index) {
                    let expired = cached
                        .failed_at
                        .is_some_and(|at| now.saturating_sub(at) >= COOLDOWN);
                    self.samples = cached
                        .samples
                        .iter()
                        .map(|sample| {
                            if expired && matches!(sample, Sample::Unavailable(_)) {
                                None
                            } else {
                                Some(*sample)
                            }
                        })
                        .collect();
                    self.failed_at = if expired { None } else { cached.failed_at };
                    self.cache.push_back(cached);
                }
            }
            if let (Some(value), Some(last)) = (seed, self.samples.last_mut()) {
                *last = Some(Sample::Known(value));
            }
            self.key = Some(key.clone());
            if visible
                && (self.samples.iter().any(Option::is_none)
                    || (window_changed && self.worker.is_some()))
            {
                let policy = self.retention.clone();
                let worker = self.worker.get_or_insert_with(|| Worker::new(policy));
                let mut mailbox = worker.shared.lock();
                worker
                    .shared
                    .generation
                    .store(self.generation, Ordering::Release);
                mailbox.pending = Some(Request {
                    generation: self.generation,
                    key,
                    seed,
                    reuse: self.samples.clone(),
                });
                self.active = true;
                worker.shared.wake.notify_all();
            }
        }
        self.visible = visible;
        self.poll(now);
    }

    #[cfg(test)]
    pub fn settle(&mut self, now: i64) {
        if self.active {
            if let Some(worker) = &self.worker {
                let mailbox = worker.shared.lock();
                let (_mailbox, timeout) = worker
                    .shared
                    .wake
                    .wait_timeout_while(mailbox, std::time::Duration::from_secs(10), |mailbox| {
                        mailbox.result.is_none() && worker.shared.current(self.generation)
                    })
                    .unwrap();
                assert!(!timeout.timed_out(), "history worker did not settle");
            }
        }
        self.poll(now);
    }

    pub fn poll(&mut self, now: i64) {
        let result = self
            .worker
            .as_ref()
            .and_then(|worker| worker.shared.lock().result.take());
        let Some(result) = result else { return };
        if result.generation != self.generation {
            return;
        }
        self.active = false;
        self.failed_at = result
            .samples
            .iter()
            .any(|sample| matches!(sample, Sample::Unavailable(_)))
            .then_some(now);
        self.samples = result.samples.iter().copied().map(Some).collect();
        if let Some(key) = &self.key {
            let cached = Cached::new(key.clone(), result.samples, self.failed_at);
            if let Some(index) = self.cache.iter().position(|old| old.key == cached.key) {
                if let Some(old) = self.cache.remove(index) {
                    self.bytes -= old.bytes;
                }
            }
            if cached.bytes <= CACHE_BYTES {
                while self.cache.len() >= CACHE_PATHS || self.bytes + cached.bytes > CACHE_BYTES {
                    if let Some(old) = self.cache.pop_front() {
                        self.bytes -= old.bytes;
                    } else {
                        break;
                    }
                }
                self.bytes += cached.bytes;
                self.cache.push_back(cached);
            }
        }
    }

    /// Signal before terminal restoration. Drop joins only after the guard is gone.
    pub fn cancel(&self) {
        if let Some(worker) = &self.worker {
            worker.cancel();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/w7a/history.rs"]
mod tests;
