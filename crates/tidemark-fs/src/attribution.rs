//! Owned by: WATCH-perf
//! Bounded constant-time LRU attribution with exact and watcher validation.
use crate::procfs;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tidemark_core::live::Attribution;

#[cfg(not(test))]
type CacheHasher = std::collections::hash_map::RandomState;
#[cfg(test)]
#[derive(Default)]
struct CacheHasher {
    hashes: std::sync::atomic::AtomicUsize,
    inner: std::collections::hash_map::RandomState,
}
#[cfg(test)]
impl std::hash::BuildHasher for CacheHasher {
    type Hasher = std::collections::hash_map::DefaultHasher;
    fn build_hasher(&self) -> Self::Hasher {
        self.hashes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::hash::BuildHasher::build_hasher(&self.inner)
    }
}

struct Entry {
    value: Attribution,
    refreshed: Instant,
    validated: Instant,
    prev: Option<u32>,
    next: Option<u32>,
}
#[derive(Default)]
struct Entries {
    map: HashMap<u32, Entry, CacheHasher>,
    first: Option<u32>,
    last: Option<u32>,
}
impl Entries {
    fn remove(&mut self, pid: u32) -> Option<Entry> {
        let entry = self.map.remove(&pid)?;
        if let Some(prev) = entry.prev {
            if let Some(e) = self.map.get_mut(&prev) {
                e.next = entry.next;
            }
        } else {
            self.first = entry.next;
        }
        if let Some(next) = entry.next {
            if let Some(e) = self.map.get_mut(&next) {
                e.prev = entry.prev;
            }
        } else {
            self.last = entry.prev;
        }
        Some(entry)
    }
    fn insert(&mut self, pid: u32, mut entry: Entry) {
        entry.prev = self.last;
        entry.next = None;
        if let Some(last) = self.last {
            if let Some(e) = self.map.get_mut(&last) {
                e.next = Some(pid);
            }
        } else {
            self.first = Some(pid);
        }
        self.last = Some(pid);
        self.map.insert(pid, entry);
    }
}
/// PID/starttime validated LRU with TTL refresh; failed lookups are not cached.
pub struct AttributionCache {
    ttl: Duration,
    capacity: usize,
    entries: Mutex<Entries>,
}
impl AttributionCache {
    /// Construct a bounded cache; zero capacity disables storage.
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity,
            entries: Mutex::new(Entries::default()),
        }
    }
    /// Read attribution with exact starttime validation on every hit.
    /// Uncached procfs reads are checked again before storing attribution.
    pub fn lookup(&self, pid: u32) -> Attribution {
        self.read(pid, Duration::ZERO)
    }
    /// Read watcher attribution, reusing starttime validation for less than one second.
    /// A recycled PID can therefore retain its predecessor's attribution for at most
    /// one second. TTL refresh and failed-lookup handling match `lookup`.
    pub fn lookup_watcher(&self, pid: u32) -> Attribution {
        self.read(pid, Duration::from_secs(1))
    }
    fn read(&self, pid: u32, validation: Duration) -> Attribution {
        self.lookup_with(
            pid,
            Instant::now(),
            validation,
            || procfs::read_starttime(pid).ok(),
            |start| {
                let mut value = unknown(pid);
                value.starttime = start;
                value.comm = procfs::read_comm(pid).unwrap_or_else(|_| "?".into());
                value.unit = procfs::read_cgroup_unit(pid).ok().flatten();
                value.uid = procfs::read_uid(pid).ok();
                if procfs::read_starttime(pid).ok() != Some(start) {
                    return unknown(pid);
                }
                value
            },
        )
    }
    fn lookup_with(
        &self,
        pid: u32,
        now: Instant,
        validation: Duration,
        start: impl FnOnce() -> Option<u64>,
        load: impl FnOnce(u64) -> Attribution,
    ) -> Attribution {
        // Serialize validation/load as well as LRU edits: concurrent exact and
        // watcher lookups cannot overwrite a newer process identity with an old one.
        let mut entries = self.entries.lock();
        let cached = entries.map.get(&pid);
        let fresh = cached.is_some_and(|e| now.saturating_duration_since(e.refreshed) < self.ttl);
        let validated = fresh
            && cached.is_some_and(|e| now.saturating_duration_since(e.validated) < validation);
        let identity = if validated {
            cached.map(|e| e.value.starttime)
        } else {
            start()
        };
        let Some(identity) = identity else {
            entries.remove(pid);
            return unknown(pid);
        };
        if let Some(mut entry) = entries.remove(pid) {
            if fresh && entry.value.starttime == identity {
                if !validated {
                    entry.validated = now;
                }
                let value = entry.value.clone();
                entries.insert(pid, entry);
                return value;
            }
        }
        let value = load(identity);
        if self.capacity != 0 && value.starttime == identity {
            if entries.map.len() >= self.capacity {
                if let Some(first) = entries.first {
                    entries.remove(first);
                }
            }
            entries.insert(
                pid,
                Entry {
                    value: value.clone(),
                    refreshed: now,
                    validated: now,
                    prev: None,
                    next: None,
                },
            );
        }
        value
    }
}
fn unknown(pid: u32) -> Attribution {
    Attribution {
        pid,
        starttime: 0,
        comm: "?".into(),
        unit: None,
        uid: None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn value(pid: u32, starttime: u64) -> Attribution {
        Attribution {
            starttime,
            comm: format!("process-{starttime}"),
            ..unknown(pid)
        }
    }
    #[test]
    fn validation_reads_reuse_ttl_and_failures() {
        let cache = AttributionCache::new(Duration::from_secs(60), 2);
        let now = Instant::now();
        let reads = Cell::new(0);
        let loads = Cell::new(0);
        let identity = Cell::new(Some(10));
        let lookup = |at, validation| {
            cache.lookup_with(
                1,
                at,
                validation,
                || {
                    reads.set(reads.get() + 1);
                    identity.get()
                },
                |start| {
                    loads.set(loads.get() + 1);
                    value(1, start)
                },
            )
        };
        let second = Duration::from_secs(1);
        assert_eq!(lookup(now, second).starttime, 10);
        assert_eq!(
            lookup(now + Duration::from_millis(999), second).starttime,
            10
        );
        assert_eq!(reads.get(), 1);
        lookup(now + second, second);
        assert_eq!(reads.get(), 2);
        assert_eq!(loads.get(), 1);
        identity.set(Some(20));
        assert_eq!(
            lookup(now + Duration::from_millis(1999), second).starttime,
            10
        );
        assert_eq!(reads.get(), 2);
        assert_eq!(lookup(now + Duration::from_secs(2), second).starttime, 20);
        assert_eq!(reads.get(), 3);
        assert_eq!(
            lookup(now + Duration::from_secs(2), Duration::ZERO).starttime,
            20
        );
        assert_eq!(reads.get(), 4); // exact API never trusts the watcher window
        assert_eq!(lookup(now + Duration::from_secs(62), second).starttime, 20);
        assert_eq!(reads.get(), 5);
        assert_eq!(loads.get(), 3);
        identity.set(None);
        assert_eq!(lookup(now + Duration::from_secs(63), second).comm, "?");
        assert!(cache.entries.lock().map.is_empty());
        lookup(now + Duration::from_secs(63), second);
        assert_eq!(reads.get(), 7);
        cache.lookup_with(2, now, second, || Some(30), |_| unknown(2));
        assert!(cache.entries.lock().map.is_empty());
    }
    #[test]
    fn ten_thousand_entries_constant_work_and_lru() {
        let cache = AttributionCache::new(Duration::from_secs(60), 10_000);
        let now = Instant::now();
        let second = Duration::from_secs(1);
        for pid in 1..=10_000 {
            cache.lookup_with(pid, now, second, || Some(u64::from(pid)), |s| value(pid, s));
        }
        let operations = || {
            cache
                .entries
                .lock()
                .map
                .hasher()
                .hashes
                .load(std::sync::atomic::Ordering::Relaxed)
        };
        let before = operations();
        for pid in 1..=10_000 {
            cache.lookup_with(
                pid,
                now,
                second,
                || panic!("hit read procfs"),
                |_| panic!("hit reload"),
            );
        }
        let work = operations() - before;
        // Each hit uses at most six keyed map operations regardless of capacity.
        // Count keyed LRU work; linked-order invariants are checked below.
        assert!(work <= 6 * 10_000, "{work} map operations");
        cache.lookup_with(1, now, second, || panic!("hit"), |_| panic!("hit"));
        cache.lookup_with(10_001, now, second, || Some(1), |s| value(10_001, s));
        let entries = cache.entries.lock();
        assert_eq!(entries.map.len(), 10_000);
        assert!(!entries.map.contains_key(&2));
        assert_eq!(entries.first, Some(3));
        assert_eq!(entries.last, Some(10_001));
        let mut pid = entries.first;
        let mut count = 0;
        let mut previous = None;
        while let Some(id) = pid {
            let entry = &entries.map[&id];
            assert_eq!(entry.prev, previous);
            previous = pid;
            pid = entry.next;
            count += 1;
            assert!(count <= 10_000);
        }
        assert_eq!(count, 10_000);
    }
    #[test]
    fn zero_capacity_and_racing_exit() {
        let cache = AttributionCache::new(Duration::ZERO, 0);
        cache.lookup_with(
            1,
            Instant::now(),
            Duration::ZERO,
            || Some(10),
            |s| value(1, s),
        );
        assert!(cache.entries.lock().map.is_empty());
        assert_eq!(cache.lookup(0).comm, "?");
    }
}
