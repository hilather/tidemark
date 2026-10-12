//! Owned by: W6-A1
//! Shared daemon publication and read interfaces.

use std::{sync::Arc, time::Duration};
use tidemark_core::{api::*, live::*, path::RawPath};
use tidemark_live::alerts::{AlertDiagnostic, AlertSource, ScanAge};

/// Shared state handle. Shape proposed by W6-prep; the owning task may refine it inside its own file.
#[derive(Clone, Default)]
pub struct LiveState;
/// Consistent read view; inert until W6-A1 implements collection. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub struct LiveStateView;
/// Service publication; all collections must respect configured collector caps. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub enum Publication {
    /// Publish fs measurements.
    Fs(FsSample),
    /// Publish lvm measurements.
    Lvm(LvmResult),
    /// Publish watcher measurements.
    Watcher((String, Arc<tidemark_live::watch::Watcher>)),
    /// Publish procscan measurements.
    Procscan(Arc<parking_lot::Mutex<tidemark_live::procscan::ProcScanner>>),
    /// Publish alerts measurements.
    Alerts((Vec<AlertSummary>, Vec<AlertDiagnostic>)),
    /// Publish scan measurements.
    Scan(ScanStatus),
    /// Publish dirgrowth measurements.
    DirGrowth((String, u32, Vec<(RawPath, i64)>)),
    /// Publish cached metrics from a completed scan.
    SnapshotMetrics(crate::metrics::SnapshotMetrics),
    /// Publish the current installed config hash and daemon startup time for firstboot.
    Deployment {
        /// SHA-256 of the actual loaded config bytes.
        config_sha256: String,
        /// Injected Unix startup time.
        started_at: i64,
    },
    /// Publish snapshot space-guard refusal.
    StateSpaceRefused(bool),
    /// Publish degraded measurements.
    Degraded(String),
}
impl LiveState {
    /// Create unavailable state; configuration supplies eventual storage caps.
    pub fn new(_config: &tidemark_core::config::Config, _started_at: i64) -> Self {
        Self
    }
    /// Read a consistent snapshot of available state.
    pub fn view(&self) -> LiveStateView {
        LiveStateView
    }
    /// Publish a result and its component health atomically.
    pub fn publish(
        &self,
        _result: Publication,
        _component: ComponentState,
    ) -> Result<(), crate::DaemonError> {
        Err(crate::DaemonError::NotYetImplemented)
    }
    /// Publish health even when no measurement is available.
    pub fn publish_component(&self, _component: ComponentState) -> Result<(), crate::DaemonError> {
        Err(crate::DaemonError::NotYetImplemented)
    }
}
impl LiveStateView {
    /// Build firstboot inputs once components have reported, preserving current config identity.
    /// Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn firstboot_inputs(&self) -> Option<crate::firstboot::FirstbootInputs> {
        None
    }

    /// Clear watcher dirtiness beneath a successfully completed root only.
    /// Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn clear_watch_dirty(&self, _root: &RawPath) -> Result<(), crate::DaemonError> {
        Err(crate::DaemonError::NotYetImplemented)
    }
    /// Snapshot space guard input for the internal state_space alert.
    /// Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn state_space_refused(&self) -> Option<bool> {
        None
    }

    /// Read recent alert history; API owner bounds requests before storage access.
    /// Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn alerts_history(
        &self,
        _params: &AlertsHistoryParams,
    ) -> Result<Vec<AlertEvent>, crate::DaemonError> {
        Err(crate::DaemonError::NotYetImplemented)
    }
    /// Read latest snapshot-derived metrics; unavailable until a completed scan is cached.
    /// Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn snapshot_metrics(&self) -> Option<crate::metrics::SnapshotMetrics> {
        None
    }

    /// Read status; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn status(&self, _now: i64) -> Option<StatusResult> {
        None
    }
    /// Read now; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn now(&self, _params: &NowParams, _now: i64) -> Option<NowReport> {
        None
    }
    /// Read hot; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn hot(&self, _params: &HotParams) -> Option<Vec<HotEntry>> {
        None
    }
    /// Read lvm report; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn lvm_report(&self, _params: &LvmParams) -> Option<LvmResult> {
        None
    }
    /// Read phantom; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn phantom(&self, _params: &PhantomParams) -> Option<Vec<PhantomSummary>> {
        None
    }
    /// Read writers; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn writers(&self, _params: &WritersParams) -> Option<Vec<OpenWriter>> {
        None
    }
    /// Read active alerts; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn active_alerts(&self) -> Option<Vec<AlertSummary>> {
        None
    }
    /// Read alert diagnostics; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn alert_diagnostics(&self) -> Option<Vec<AlertDiagnostic>> {
        None
    }
    /// Read scans; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn scans(&self) -> Option<Vec<ScanStatus>> {
        None
    }
    /// Read fs samples; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn fs_samples(&self) -> Option<Vec<FsSample>> {
        None
    }
    /// Read watch stats; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn watch_stats(&self) -> Option<Vec<(String, tidemark_live::watch::WatchStats)>> {
        None
    }
    /// Read degraded; None means unavailable. Shape proposed by W6-prep; the owning task may refine it inside its own file.
    pub fn degraded(&self) -> Option<Vec<String>> {
        None
    }
}
impl AlertSource for LiveStateView {
    fn fs_sample(&self, _mount: &str) -> Option<FsSample> {
        None
    }
    fn fs_rate(&self, _mount: &str, _window: Duration) -> Option<f64> {
        None
    }
    fn hot_dirs(&self, _mount: &str, _window: Duration) -> Option<Vec<HotEntry>> {
        None
    }
    fn watch_overflows(&self, _mount: &str, _window: Duration) -> Option<u64> {
        None
    }
    fn phantom_bytes(&self, _mount: &str) -> Option<u64> {
        None
    }
    fn lvm(&self) -> Option<Vec<LvmNow>> {
        None
    }
    fn dir_growth(&self, _root: &str, _depth: u32) -> Option<Vec<(RawPath, i64)>> {
        None
    }
    fn scan_age(&self, _root: &str) -> Option<ScanAge> {
        None
    }
}
