//! Owned by: W2-B
//! Cross-task interface stubs.

use crate::agg::{Aggregator, Prepared};
use std::{os::fd::BorrowedFd, sync::atomic::AtomicBool};
use tidemark_core::scan::Gate;
use tidemark_fs::walk::WalkConfig;
use tidemark_sys::xfs::XfsGeometry;
/// Reserved for W2-B.
pub struct BulkParams {
    /// Contract field.
    pub walk: WalkConfig,
    /// Contract field.
    pub batch: usize,
    /// Contract field.
    pub threads: usize,
}
/// Reserved for W2-B.
pub struct BulkStats {
    /// Contract field.
    pub phase_ms: [u64; 3],
    /// Contract field.
    pub dents: u64,
    /// Contract field.
    pub inodes_streamed: u64,
    /// Contract field.
    pub joined: u64,
    /// Contract field.
    pub races: u64,
    /// Contract field.
    pub ags: u32,
    /// Contract field.
    pub names_resolved: u64,
    /// Contract field.
    pub names_missing: u64,
}
/// Reserved for W2-B.
pub fn estimate_memory(used_inodes: u64, dirs_estimate: u64) -> u64 {
    let _ = used_inodes;
    let _ = dirs_estimate;
    0
}
/// Reserved for W2-B.
pub fn run_bulk(
    p: &BulkParams,
    mount_fd: BorrowedFd<'_>,
    geo: &XfsGeometry,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    agg: Aggregator,
) -> Result<(Prepared, BulkStats), BulkError> {
    let _ = p;
    let _ = mount_fd;
    let _ = geo;
    let _ = gate;
    let _ = cancel;
    let _ = agg;
    Err(BulkError::NotYetImplemented)
}

mod join;
mod names;

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum BulkError {
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// Scan was cancelled.
    #[error("Cancelled")]
    Cancelled,
}
