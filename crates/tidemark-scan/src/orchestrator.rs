//! Owned by: W2-C
//! Cross-task interface stubs.

use crate::{agg::AggSummary, mode::ModeRequest, progress::Progress};
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Duration};
use tidemark_core::{
    config::Config,
    path::RawPath,
    scan::{ScanMode, SnapshotStatus},
    snapshot::DirRow,
};
use tidemark_store::catalog::ScanEntry;
/// Reserved for W2-C.
pub struct ScanRequest {
    /// Contract field.
    pub root: RawPath,
    /// Contract field.
    pub mode: ModeRequest,
    /// Contract field.
    pub throttle: Option<String>,
    /// Contract field.
    pub threads: Option<usize>,
    /// Contract field.
    pub exclude_extra: Vec<String>,
    /// Contract field.
    pub keep_partial: bool,
    /// Contract field.
    pub annotations: Vec<(RawPath, String, String)>,
}
/// Reserved for W2-C.
pub struct ScanOutcome {
    /// Contract field.
    pub scan_id: String,
    /// Contract field.
    pub file: Option<PathBuf>,
    /// Contract field.
    pub status: SnapshotStatus,
    /// Contract field.
    pub degraded: bool,
    /// Contract field.
    pub mode: ScanMode,
    /// Contract field.
    pub mode_reason: String,
    /// Contract field.
    pub summary: AggSummary,
    /// Contract field.
    pub duration: Duration,
}

/// Reserved for W2-C.
pub trait ScanRecorder: Send + Sync {
    /// Reserved for W2-C.
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError>;
    /// Reserved for W2-C.
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError>;
    /// Reserved for W2-C.
    fn dir_series(
        &self,
        root: &RawPath,
        scan_id: &str,
        ts: i64,
        rows: &[DirRow],
    ) -> Result<(), ScanError>;
}
/// Reserved for W2-C.
pub fn run_scan(
    cfg: &Config,
    req: &ScanRequest,
    recorder: &dyn ScanRecorder,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&Progress) + Sync),
) -> Result<ScanOutcome, ScanError> {
    let _ = cfg;
    let _ = req;
    let _ = recorder;
    let _ = cancel;
    let _ = progress;
    Err(ScanError::NotYetImplemented)
}

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// Host scan lock is held.
    #[error("Busy")]
    Busy,
    /// State space refused.
    #[error("NoSpace")]
    NoSpace,
}
