//! Owned by: W1-B
//! Parallel metadata-only traversal with bounded work stealing and output.

/// Fatal walk failures; per-entry I/O errors remain in scan records.
#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    /// Compatibility variant retained from Wave 0; the walker never returns it.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// Cancellation requested by the caller.
    #[error("scan cancelled")]
    Cancelled,
    /// A fatal filesystem or worker error.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Invalid settings or a documented memory cap was reached.
    #[error("walker limit: {0}")]
    Limit(String),
    /// The output consumer disconnected.
    #[error("output consumer disconnected")]
    Disconnected,
}
use std::{io, sync::atomic::AtomicBool, time::Duration};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::{DevId, FileKind, Gate, Ino, WalkOutput},
};
/// Walker settings. Resource caps are 65,536 in-flight tasks, 64 MiB of task
/// paths, one million entries per record, and 10,000 stored errors per scan.
/// A breached cap fails explicitly rather than silently dropping records.
#[derive(Clone)]
pub struct WalkConfig {
    /// Absolute byte-preserving scan root.
    pub root: RawPath,
    /// Worker count, from 1 through 64.
    pub threads: usize,
    /// Collect per-entry attributes when true; otherwise emit namespace records.
    pub stat_entries: bool,
    /// Exclusions matched against full byte paths.
    pub exclude: globset::GlobSet,
    /// Explicit mount targets under the root; augmented from mountinfo.
    pub skip_mounts: Vec<RawPath>,
    /// Allocated-byte threshold for retaining single-link entries.
    pub big_file_threshold: u64,
    /// Worker-local top-file heap capacity, at most 200,000.
    pub top_files: usize,
    /// Injected scan-start Unix seconds used for age buckets.
    pub scan_start: i64,
    /// Per-worker getdents buffer size, 4 KiB through 4 MiB.
    pub getdents_buf: usize,
    /// Request best-effort idle I/O priority on every worker.
    pub io_idle: bool,
    /// Upper bound, in bytes, for one directory record (its entries' link references, names,
    /// skips, error samples and notable candidates). 0 means no bound (tests only).
    pub max_record_bytes: u64,
}

/// Completed scan counters, including errors whose stored details were capped.
pub struct WalkSummary {
    /// Directories emitted (excludes Gone records).
    pub dirs: u64,
    /// All observed direct entries, including skipped entries.
    pub entries: u64,
    /// Total per-directory and per-entry I/O errors, including unstored details.
    pub errors: u64,
    /// Vanished, replaced or changed entries and queued directory races.
    pub races: u64,
    /// Directory opens retried without O_NOATIME after EPERM.
    pub noatime_fallbacks: u64,
    /// Elapsed monotonic time for this walk.
    pub elapsed: Duration,
    /// Encoded device identifier of the scan root.
    pub root_dev: DevId,
    /// Inode number of the scan root.
    pub root_ino: Ino,
}
/// Scan a tree with bounded parallel workers and consumer backpressure.
pub fn walk(
    cfg: &WalkConfig,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    out: crossbeam_channel::Sender<WalkOutput>,
) -> Result<WalkSummary, WalkError> {
    worker::run(cfg, gate, cancel, out)
}

/// Read byte-preserving direct entries, sorted by name, without following links.
pub fn read_dir_entries(
    dir: &RawPath,
    gate: &dyn Gate,
) -> io::Result<Vec<(RawName, Ino, FileKind)>> {
    getdents::read(dir, gate)
}

pub use getdents::{
    open_attempts, open_directory, read_dir_entries_checked, read_dir_entries_checked_cancellable,
};

mod getdents;
mod worker;

/// Decide whether an entry crosses the root device or a known root mount ID.
#[doc(hidden)]
pub fn boundary_reason(
    root_dev: DevId,
    root_mount: u64,
    child_dev: DevId,
    child_mount: u64,
) -> Option<tidemark_core::scan::SkipReason> {
    if child_dev != root_dev || (root_mount != 0 && child_mount != root_mount) {
        Some(tidemark_core::scan::SkipReason::OtherFs)
    } else {
        None
    }
}
