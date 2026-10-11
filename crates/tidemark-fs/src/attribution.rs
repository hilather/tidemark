//! Owned by: W1-D
//! Bounded LRU process attribution, validated against kernel starttime.

use crate::procfs;
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tidemark_core::live::Attribution;

struct Entry {
    value: Attribution,
    refreshed: Instant,
}
/// PID/starttime keyed LRU with TTL refresh; no failed lookup is cached.
pub struct AttributionCache {
    ttl: Duration,
    capacity: usize,
    entries: Mutex<VecDeque<Entry>>,
}
impl AttributionCache {
    /// Construct a bounded cache; zero capacity disables storage.
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity,
            entries: Mutex::new(VecDeque::new()),
        }
    }
    /// Read attribution. Unknown fields remain '?' / None; never returns an error.
    /// Starttime is checked on every hit and again after uncached procfs reads.
    pub fn lookup(&self, pid: u32) -> Attribution {
        let start = procfs::read_starttime(pid).ok();
        self.lookup_with(pid, start, Instant::now(), || {
            let mut value = unknown(pid);
            let Some(start) = start else {
                return value;
            };
            value.starttime = start;
            value.comm = procfs::read_comm(pid).unwrap_or_else(|_| "?".into());
            value.unit = procfs::read_cgroup_unit(pid).ok().flatten();
            value.uid = procfs::read_uid(pid).ok();
            if procfs::read_starttime(pid).ok() != Some(start) {
                return unknown(pid);
            }
            value
        })
    }
    fn lookup_with(
        &self,
        pid: u32,
        start: Option<u64>,
        now: Instant,
        load: impl FnOnce() -> Attribution,
    ) -> Attribution {
        let Some(start) = start else {
            self.entries.lock().retain(|e| e.value.pid != pid);
            return unknown(pid);
        };
        {
            let mut entries = self.entries.lock();
            // Evict a reused PID, even if the replacement cannot be read.
            entries.retain(|e| e.value.pid != pid || e.value.starttime == start);
            if let Some(index) = entries.iter().position(|e| {
                e.value.pid == pid && now.saturating_duration_since(e.refreshed) < self.ttl
            }) {
                if let Some(entry) = entries.remove(index) {
                    let result = entry.value.clone();
                    entries.push_back(entry);
                    return result;
                }
            }
        }
        let value = load();
        if self.capacity != 0 && value.starttime == start {
            let mut entries = self.entries.lock();
            entries.retain(|e| e.value.pid != pid);
            while entries.len() >= self.capacity {
                entries.pop_front();
            }
            entries.push_back(Entry {
                value: value.clone(),
                refreshed: now,
            });
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    fn value(pid: u32, starttime: u64) -> Attribution {
        Attribution {
            starttime,
            comm: format!("process-{starttime}"),
            ..unknown(pid)
        }
    }
    #[test]
    fn pid_reuse_ttl_and_lru() {
        let cache = AttributionCache::new(Duration::from_secs(60), 2);
        let now = Instant::now();
        assert_eq!(
            cache.lookup_with(1, Some(10), now, || value(1, 10)).comm,
            "process-10"
        );
        assert_eq!(
            cache
                .lookup_with(1, Some(10), now, || panic!("hit must not reload"))
                .comm,
            "process-10"
        );
        assert_eq!(
            cache.lookup_with(1, Some(20), now, || value(1, 20)).comm,
            "process-20"
        );
        let later = now + Duration::from_secs(60);
        assert_eq!(
            cache
                .lookup_with(1, Some(20), later, || Attribution {
                    comm: "refreshed".into(),
                    ..value(1, 20)
                })
                .comm,
            "refreshed"
        );
        cache.lookup_with(2, Some(2), later, || value(2, 2));
        cache.lookup_with(1, Some(20), later, || panic!("LRU hit"));
        cache.lookup_with(3, Some(3), later, || value(3, 3));
        assert_eq!(
            cache
                .entries
                .lock()
                .iter()
                .map(|e| e.value.pid)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(
            cache
                .lookup_with(1, None, later, || panic!("missing PID"))
                .comm,
            "?"
        );
        assert_eq!(cache.entries.lock().len(), 1);
    }
    #[test]
    fn zero_capacity_and_racing_exit() {
        let now = Instant::now();
        let cache = AttributionCache::new(Duration::ZERO, 0);
        cache.lookup_with(1, Some(10), now, || value(1, 10));
        assert!(cache.entries.lock().is_empty());
        let cache = AttributionCache::new(Duration::from_secs(60), 2);
        cache.lookup_with(1, Some(10), now, || unknown(1));
        assert!(cache.entries.lock().is_empty());
        assert_eq!(cache.lookup(0).comm, "?");
    }
}
