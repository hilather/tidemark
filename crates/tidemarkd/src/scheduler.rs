//! Owned by: W6-A2
//! Schedule parsing, scan control and first-boot coordination interfaces.

/// Parsed local-time schedule. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub struct Schedule;
/// Parsed local-time window, including midnight wrapping. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub struct Window;
/// Shared single-scan control. Shape proposed by W6-prep; the owning task may refine it inside its own file.
#[derive(Clone, Default)]
pub struct ScanControl;
/// Parse the normative schedule/window syntax. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn parse_schedule(_text: &str) -> Result<Schedule, crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
/// Parse the normative schedule/window syntax. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn parse_window(_text: &str) -> Result<Window, crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
/// Compute next due time with injected Unix time and local-time/DST policy.
/// Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn next_run(
    _schedule: &Schedule,
    _window: Option<&Window>,
    _last_complete: Option<i64>,
    _now: i64,
) -> Result<Option<i64>, crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
impl ScanControl {
    /// Control scan.start with frozen wire payloads. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn start(
        &self,
        _params: &tidemark_core::api::ScanStartParams,
    ) -> Result<tidemark_core::api::ScanStartResult, crate::DaemonError> {
        Err(crate::DaemonError::NotYetImplemented)
    }
    /// Control scan.cancel with frozen wire payloads. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn cancel(
        &self,
        _params: &tidemark_core::api::ScanCancelParams,
    ) -> Result<tidemark_core::api::ScanCancelResult, crate::DaemonError> {
        Err(crate::DaemonError::NotYetImplemented)
    }
    /// Control scan.status with frozen wire payloads. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn status(
        &self,
        _params: &tidemark_core::api::ScanStatusParams,
    ) -> Result<Vec<tidemark_core::live::ScanStatus>, crate::DaemonError> {
        Err(crate::DaemonError::NotYetImplemented)
    }
}
/// Start scheduler and first-boot coordination. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn start(
    _config: std::sync::Arc<tidemark_core::config::Config>,
    _state: crate::state::LiveState,
    _writer: crate::services::tswriter::Sender,
    _shutdown: crate::Shutdown,
) -> Result<(ScanControl, crate::ServiceHandle), crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
