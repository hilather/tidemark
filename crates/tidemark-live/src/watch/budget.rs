//! Owned by: WATCH-bounds
//! Shared fallible admission of retained watcher path and key capacities.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
/// Aggregate path/key payload ceiling for one watcher, independent of count caps.
pub(super) const PATH_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone)]
pub(super) struct Budget(Arc<AtomicUsize>, usize);
impl Budget {
    pub fn new() -> Self {
        Self(Arc::new(AtomicUsize::new(0)), PATH_BYTES)
    }
    #[cfg(test)]
    pub fn with_limit(limit: usize) -> Self {
        Self(Arc::new(AtomicUsize::new(0)), limit)
    }
    pub fn used(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
    pub fn reserve(&self, bytes: usize) -> bool {
        self.reserve_to(bytes, self.1 - (self.1 / 64).min(1024 * 1024))
    }
    pub fn reserve_root(&self, bytes: usize) -> bool {
        self.reserve_to(bytes, self.1)
    }
    fn reserve_to(&self, bytes: usize, limit: usize) -> bool {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes).filter(|next| *next <= limit)
            })
            .is_ok()
    }
    pub fn release(&self, bytes: usize) {
        self.0.fetch_sub(bytes, Ordering::Relaxed);
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
}
impl Reservation {
    pub fn new(budget: &Budget, bytes: usize) -> Option<Self> {
        budget.reserve_root(bytes).then(|| Self {
            budget: budget.clone(),
            bytes,
        })
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}
