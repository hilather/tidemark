//! Owned by: W1-B
//! Cross-task signatures reserved for W1-B.

/// Stub error reserved for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    /// Cross-task implementation is not available yet.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}
use std::{io, sync::atomic::AtomicBool, time::Duration};
use tidemark_core::{
    path::{RawName, RawPath},
    scan::{DevId, FileKind, Gate, Ino, WalkOutput},
};
/// Reserved for W1-B.
pub struct WalkConfig {
    #[doc = "Field specified by the owning task."]
    pub root: RawPath,
    #[doc = "Field specified by the owning task."]
    pub threads: usize,
    #[doc = "Field specified by the owning task."]
    pub stat_entries: bool,
    #[doc = "Field specified by the owning task."]
    pub exclude: globset::GlobSet,
    #[doc = "Field specified by the owning task."]
    pub skip_mounts: Vec<RawPath>,
    #[doc = "Field specified by the owning task."]
    pub big_file_threshold: u64,
    #[doc = "Field specified by the owning task."]
    pub top_files: usize,
    #[doc = "Field specified by the owning task."]
    pub scan_start: i64,
    #[doc = "Field specified by the owning task."]
    pub getdents_buf: usize,
    #[doc = "Field specified by the owning task."]
    pub io_idle: bool,
}

/// Reserved for W1-B.
pub struct WalkSummary {
    #[doc = "Field specified by the owning task."]
    pub dirs: u64,
    #[doc = "Field specified by the owning task."]
    pub entries: u64,
    #[doc = "Field specified by the owning task."]
    pub errors: u64,
    #[doc = "Field specified by the owning task."]
    pub races: u64,
    #[doc = "Field specified by the owning task."]
    pub noatime_fallbacks: u64,
    #[doc = "Field specified by the owning task."]
    pub elapsed: Duration,
    #[doc = "Field specified by the owning task."]
    pub root_dev: DevId,
    #[doc = "Field specified by the owning task."]
    pub root_ino: Ino,
}
/// Reserved for W1-B.
pub fn walk(
    cfg: &WalkConfig,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    out: crossbeam_channel::Sender<WalkOutput>,
) -> Result<WalkSummary, WalkError> {
    let _ = cfg;
    let _ = gate;
    let _ = cancel;
    let _ = out;
    Err(WalkError::NotYetImplemented)
}

/// Reserved for W1-B.
pub fn read_dir_entries(
    dir: &RawPath,
    gate: &dyn Gate,
) -> io::Result<Vec<(RawName, Ino, FileKind)>> {
    let _ = dir;
    let _ = gate;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}

mod getdents;

mod worker;

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        let raw = tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap();
        assert_eq!(
            walk(
                &WalkConfig {
                    root: raw.clone(),
                    threads: 1,
                    stat_entries: true,
                    exclude: globset::GlobSet::empty(),
                    skip_mounts: Vec::new(),
                    big_file_threshold: 0,
                    top_files: 0,
                    scan_start: 0,
                    getdents_buf: 0,
                    io_idle: false
                },
                &tidemark_core::scan::NoopGate,
                &std::sync::atomic::AtomicBool::new(false),
                crossbeam_channel::bounded(1).0
            )
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            read_dir_entries(&raw, &tidemark_core::scan::NoopGate)
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
