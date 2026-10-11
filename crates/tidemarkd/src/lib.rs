//! Owned by: W0-B
//! tidemarkd crate skeleton.

#![forbid(unsafe_code)]

/// api module skeleton.
pub mod api;
/// firstboot module skeleton.
pub mod firstboot;
/// metrics module skeleton.
pub mod metrics;
/// scheduler module skeleton.
pub mod scheduler;
/// Daemon service modules.
pub mod services {
    /// alerts service skeleton.
    pub mod alerts;
    /// lvm service skeleton.
    pub mod lvm;
    /// pg service skeleton.
    pub mod pg;
    /// procscan service skeleton.
    pub mod procscan;
    /// sampler service skeleton.
    pub mod sampler;
    /// tswriter service skeleton.
    pub mod tswriter;
    /// watcher service skeleton.
    pub mod watcher;
}
/// state module skeleton.
pub mod state;
