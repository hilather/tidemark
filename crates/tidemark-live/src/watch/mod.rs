//! Owned by: W5-A
//! Cross-task interface stubs.

use std::{io, sync::Arc};
use tidemark_core::{
    live::{HotEntry, HotMinute},
    path::RawPath,
};
use tidemark_fs::attribution::AttributionCache;

/// Reserved for W5-A.
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
pub enum WatchMode {
    /// Contract variant.
    FdMount,
    /// Contract variant.
    DfidNameMount,
    /// Contract variant.
    DfidNameFilesystem,
}
/// Reserved for W5-A.
pub enum HotScope {
    /// Contract variant.
    Dir,
    /// Contract variant.
    Unit,
    /// Contract variant.
    Comm,
}
/// Reserved for W5-A.
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
/// Reserved for W5-A.
pub struct Watcher;
impl Watcher {
    /// Reserved for W5-A.
    pub fn start(
        cfg: WatcherOptions,
        attribution: Arc<AttributionCache>,
    ) -> Result<Watcher, WatchError> {
        let _ = cfg;
        let _ = attribution;
        Err(WatchError::NotYetImplemented)
    }
    /// Reserved for W5-A.
    pub fn mode(&self) -> WatchMode {
        WatchMode::FdMount
    }
    /// Reserved for W5-A.
    pub fn hot(&self, window_min: u32, scope: HotScope, limit: usize) -> Vec<HotEntry> {
        let _ = window_min;
        let _ = scope;
        let _ = limit;
        Vec::new()
    }
    /// Reserved for W5-A.
    pub fn drain_completed_minutes(&self, top_k: usize) -> Vec<HotMinute> {
        let _ = top_k;
        Vec::new()
    }
    /// Reserved for W5-A.
    pub fn take_dirty(&self, under: &RawPath) -> Vec<RawPath> {
        let _ = under;
        Vec::new()
    }
    /// Reserved for W5-A.
    pub fn stats(&self) -> WatchStats {
        WatchStats {
            mode: WatchMode::FdMount,
            events_total: 0,
            overflows_total: 0,
            lossy_minutes_15m: 0,
            dropped_keys: 0,
        }
    }
    /// Reserved for W5-A.
    pub fn stop(self) {}
}
/// Reserved for W5-A.
pub fn namespace_ok() -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}

mod fdmode;
mod fidmode;
mod rings;

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
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
