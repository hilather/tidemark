//! Owned by: W6-B
//! Metrics rendering and exporter lifecycle interfaces.

/// Latest-snapshot metric cache; refresh after each completed scan.
/// Shape proposed by W6-prep; the owning task may refine it inside its own file.
#[derive(Default)]
pub struct SnapshotMetrics;
impl SnapshotMetrics {
    /// Refresh the cache from the latest complete snapshot after a scan.
    /// Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn refresh(
        &self,
        _snapshot: &tidemark_store::reader::Snapshot,
        _depth: u32,
    ) -> Result<(), crate::DaemonError> {
        Err(crate::DaemonError::NotYetImplemented)
    }
}
/// Render normative metrics from available measurements and cached snapshots.
/// Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn render(
    _view: &crate::state::LiveStateView,
    _snapshots: &SnapshotMetrics,
    _config: &tidemark_core::config::MetricsConfig,
    _now: i64,
) -> Result<String, crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
/// Start textfile and optional loopback exporter. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn start(
    _config: std::sync::Arc<tidemark_core::config::Config>,
    _state: crate::state::LiveState,
    _shutdown: crate::Shutdown,
) -> Result<crate::ServiceHandle, crate::DaemonError> {
    Err(crate::DaemonError::NotYetImplemented)
}
