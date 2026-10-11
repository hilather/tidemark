//! Owned by: W5-B
//! Cross-task interface stubs.

use std::{io, sync::Arc, time::Duration};
use tidemark_core::{
    live::{OpenWriter, PhantomSample, PhantomSummary, WriterSample},
    path::RawPath,
    scan::DevId,
};
use tidemark_fs::attribution::AttributionCache;
/// Reserved for W5-B.
pub struct ProcScannerOptions {
    /// Contract field.
    pub mounts: Vec<(RawPath, DevId)>,
    /// Contract field.
    pub open_writers: bool,
    /// Contract field.
    pub exclude_prefixes: Vec<RawPath>,
}
/// Reserved for W5-B.
pub struct ProcScanResult {
    /// Contract field.
    pub at: i64,
    /// Contract field.
    pub fds_seen: u64,
    /// Contract field.
    pub duration: Duration,
}
/// Reserved for W5-B.
pub struct ProcScanner;
impl ProcScanner {
    /// Reserved for W5-B.
    pub fn new(cfg: ProcScannerOptions, attribution: Arc<AttributionCache>) -> Self {
        let _ = cfg;
        let _ = attribution;
        Self
    }
    /// Reserved for W5-B.
    pub fn scan_once(&mut self, now: i64) -> io::Result<ProcScanResult> {
        let _ = now;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "NotYetImplemented",
        ))
    }
    /// Reserved for W5-B.
    pub fn phantom(&self, mount: Option<&RawPath>) -> Vec<PhantomSummary> {
        let _ = mount;
        Vec::new()
    }
    /// Reserved for W5-B.
    pub fn writers(
        &self,
        mount: Option<&RawPath>,
        window: Duration,
        limit: usize,
    ) -> Vec<OpenWriter> {
        let _ = mount;
        let _ = window;
        let _ = limit;
        Vec::new()
    }
    /// Reserved for W5-B.
    pub fn take_samples(&mut self) -> (Vec<PhantomSample>, Vec<WriterSample>) {
        (Vec::new(), Vec::new())
    }
}
