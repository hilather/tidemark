//! Owned by: W6-A2
//! alerts service lifecycle interface.

/// Start the service with shared config, publication, persistence and shutdown handles.
/// Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn start(
    _config: std::sync::Arc<tidemark_core::config::Config>,
    _state: crate::state::LiveState,
    _writer: super::tswriter::Sender,
    _shutdown: crate::Shutdown,
) -> Result<crate::ServiceHandle, crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
