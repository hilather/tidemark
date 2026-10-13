//! Owned by: WATCH-bounds
//! Shared fallible admission of retained watcher path and key capacities.
use parking_lot::Mutex;
use std::sync::Arc;
/// Aggregate path/key payload ceiling for one watcher, independent of count caps.
pub(super) const PATH_BYTES: usize = super::WATCH_MEMORY_BYTES;
#[derive(Default)]
struct Account {
    payload: usize,
    nodes: usize,
}
#[derive(Clone)]
pub(super) struct Budget(Arc<Mutex<Account>>, usize);
impl Budget {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Account::default())), PATH_BYTES)
    }
    #[cfg(test)]
    pub fn with_limit(limit: usize) -> Self {
        Self(Arc::new(Mutex::new(Account::default())), limit)
    }
    pub fn used(&self) -> usize {
        self.0.lock().payload
    }
    pub fn committed(&self) -> usize {
        let a = self.0.lock();
        a.payload + a.nodes
    }
    pub fn reserve(&self, bytes: usize) -> bool {
        self.reserve_to(bytes, false, false)
    }
    pub fn reserve_root(&self, bytes: usize) -> bool {
        self.reserve_to(bytes, false, true)
    }
    pub fn reserve_nodes(&self, bytes: usize, emergency: bool) -> bool {
        self.reserve_to(bytes, true, emergency)
    }
    fn reserve_to(&self, bytes: usize, nodes: bool, emergency: bool) -> bool {
        let limit = if emergency {
            self.1
        } else {
            self.1 - (self.1 / 64).min(1024 * 1024)
        };
        let mut a = self.0.lock();
        if a.payload
            .checked_add(a.nodes)
            .and_then(|n| n.checked_add(bytes))
            .is_none_or(|n| n > limit)
        {
            return false;
        }
        if nodes {
            a.nodes += bytes;
        } else {
            a.payload += bytes;
        }
        true
    }
    pub fn release(&self, bytes: usize) {
        self.0.lock().payload -= bytes;
    }
    pub fn release_nodes(&self, bytes: usize) {
        self.0.lock().nodes -= bytes;
    }
}
pub(super) fn copy(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len()).ok()?;
    out.extend_from_slice(bytes);
    Some(out)
}

pub(super) struct Reservation {
    budget: Budget,
    bytes: usize,
    nodes: bool,
}
impl Reservation {
    pub fn normal(budget: &Budget, bytes: usize) -> Option<Self> {
        budget.reserve(bytes).then(|| Self {
            budget: budget.clone(),
            bytes,
            nodes: false,
        })
    }
    pub fn nodes(budget: &Budget, bytes: usize) -> Option<Self> {
        budget.reserve_nodes(bytes, false).then(|| Self {
            budget: budget.clone(),
            bytes,
            nodes: true,
        })
    }
    pub fn emergency_nodes(budget: &Budget, bytes: usize) -> Option<Self> {
        budget.reserve_nodes(bytes, true).then(|| Self {
            budget: budget.clone(),
            bytes,
            nodes: true,
        })
    }
    pub fn new(budget: &Budget, bytes: usize) -> Option<Self> {
        budget.reserve_root(bytes).then(|| Self {
            budget: budget.clone(),
            bytes,
            nodes: false,
        })
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if self.nodes {
            self.budget.release_nodes(self.bytes);
        } else {
            self.budget.release(self.bytes);
        }
    }
}

pub(super) fn copy_string(value: &str) -> Option<String> {
    let mut copy = String::new();
    copy.try_reserve_exact(value.len()).ok()?;
    copy.push_str(value);
    Some(copy)
}
