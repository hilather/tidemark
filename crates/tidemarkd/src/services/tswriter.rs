//! Owned by: W6-A1
//! Single bounded time-series writer interface.

use tidemark_core::live::*;
/// Persistence work; producers must bound batches by configured caps. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub enum Message {
    /// Record scan start and acknowledge persistence to the scheduler recorder.
    ScanStarted(
        tidemark_store::catalog::ScanEntry,
        crossbeam_channel::Sender<Result<(), crate::DaemonError>>,
    ),
    /// Record scan completion and acknowledge persistence to the scheduler recorder.
    ScanFinished(
        tidemark_store::catalog::ScanEntry,
        crossbeam_channel::Sender<Result<(), crate::DaemonError>>,
    ),
    /// Persist fs records.
    Fs(Vec<FsSample>),
    /// Persist lvm records.
    Lvm(Vec<LvmSample>),
    /// Persist hot records.
    Hot(Vec<HotMinute>),
    /// Persist phantom records.
    Phantom(Vec<PhantomSample>),
    /// Persist writers records.
    Writers(Vec<WriterSample>),
    /// Persist alert records.
    Alert(AlertEvent),
    /// Persist dirseries records.
    DirSeries(
        (
            tidemark_core::path::RawPath,
            String,
            i64,
            Vec<tidemark_core::snapshot::DirRow>,
        ),
    ),
    /// Flush queued inserts before acknowledging shutdown.
    Flush(crossbeam_channel::Sender<()>),
}
/// Sender from a bounded channel created by the writer. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub type Sender = crossbeam_channel::Sender<Message>;
/// Start the writer; returned sender has bounded capacity from configuration.
/// Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn start(
    _config: std::sync::Arc<tidemark_core::config::Config>,
    _state: crate::state::LiveState,
    _shutdown: crate::Shutdown,
) -> Result<(Sender, crate::ServiceHandle), crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
