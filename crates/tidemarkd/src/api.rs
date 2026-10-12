//! Owned by: W6-A3
//! Unix socket server and request dispatch interfaces.

/// Reload callback supplied by W6-A1; validation must keep old config on failure.
/// Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub type Reload = std::sync::Arc<
    dyn Fn() -> Result<tidemark_core::api::ConfigReloadResult, crate::DaemonError> + Send + Sync,
>;
/// Handler dependencies. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub struct Context {
    /// Read-only measurement publication handle.
    pub state: crate::state::LiveState,
    /// Single-scan scheduler control.
    pub scans: crate::scheduler::ScanControl,
    /// Lifecycle reload callback.
    pub reload: Reload,
}
/// Dispatch after framing; owning task enforces peer authorization for every method.
/// Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn dispatch(
    _request: &tidemark_core::api::Request,
    _peer_uid: u32,
    _context: &Context,
    _now: i64,
) -> Result<tidemark_core::api::Response, crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
/// Start bounded socket server. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn start(
    _config: std::sync::Arc<tidemark_core::config::Config>,
    _context: Context,
    _shutdown: crate::Shutdown,
) -> Result<crate::ServiceHandle, crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
