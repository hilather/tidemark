//! Owned by: W0-A
//! tidemark-store crate skeleton.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// catalog module skeleton.
pub mod catalog;
pub mod chunk;
/// diff module skeleton.
pub mod diff;
/// export_ncdu module skeleton.
pub mod export_ncdu;
/// links module skeleton.
pub mod links;
/// reader module skeleton.
pub mod reader;
/// retention module skeleton.
pub mod retention;
/// space_guard module skeleton.
pub mod space_guard;
/// timeseries module skeleton.
pub mod timeseries;
/// writer module skeleton.
pub mod writer;

/// Verified state-directory trust boundary.
pub mod state_dir;

/// State-wide writer and recovery leases.
pub mod lease;

mod error;

pub use error::StoreError;

#[cfg(test)]
fn test_meta() -> tidemark_core::snapshot::SnapshotMeta {
    tidemark_core::snapshot::SnapshotMeta {
        schema_version: 0,
        tool_version: String::new(),
        scan_id: serde_json::from_str("\"20260101T000000Z-00000000\"").unwrap(),
        root: tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap(),
        root_display: String::new(),
        mode: tidemark_core::scan::ScanMode::Walk,
        mode_reason: String::new(),
        status: tidemark_core::scan::SnapshotStatus::Partial,
        degraded: false,
        exclude: Vec::new(),
        link_instances: tidemark_core::config::LinkInstances::All,
        started_at: 0,
        finished_at: 0,
        host: String::new(),
        kernel: String::new(),
        os_release: String::new(),
        fs_uuid: String::new(),
        fs_dev: 0,
        fs_blocksize: 0,
        threads: 0,
        throttle_profile: String::new(),
        dirs: 0,
        files: 0,
        inodes: 0,
        errors: 0,
        races: 0,
        skipped: 0,
        noatime_fallbacks: 0,
        phase_ms: std::collections::BTreeMap::new(),
        gate_stats: tidemark_core::scan::GateStats::default(),
    }
}

/// Streaming ncdu export.
pub use export_ncdu::{export_ncdu, ExportStats, NcduOptions};
/// Snapshot reader used by query interfaces.
pub use reader::Snapshot;
