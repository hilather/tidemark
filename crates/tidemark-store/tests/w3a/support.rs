//! Owned by: W3-A
//! Query and renderer coverage from deterministic hand-built snapshot rows.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use tidemark_core::{
    config::LinkInstances,
    path::RawPath,
    scan::{ScanMode, SnapshotStatus},
    snapshot::{DirRow, SnapshotMeta},
    time::ScanId,
};
#[path = "row.rs"]
mod fixture;
pub use fixture::row;
pub fn meta(root: RawPath, now: i64) -> SnapshotMeta {
    SnapshotMeta {
        schema_version: 1,
        tool_version: "test".into(),
        scan_id: ScanId::new(now).unwrap(),
        root_display: root.display_lossy(),
        root,
        mode: ScanMode::Walk,
        mode_reason: "fixture".into(),
        status: SnapshotStatus::Complete,
        degraded: false,
        exclude: vec![],
        link_instances: LinkInstances::All,
        started_at: now,
        finished_at: now + 1,
        host: "test".into(),
        kernel: "test".into(),
        os_release: "test".into(),
        fs_uuid: String::new(),
        fs_dev: 0,
        fs_blocksize: 4096,
        threads: 1,
        throttle_profile: "max".into(),
        dirs: 4,
        files: 3,
        inodes: 3,
        errors: 0,
        races: 0,
        skipped: 0,
        noatime_fallbacks: 0,
        phase_ms: Default::default(),
        gate_stats: Default::default(),
    }
}

pub fn snapshot(dir: &std::path::Path, rows: &[DirRow], now: i64) -> std::path::PathBuf {
    let mut metadata = meta(rows[0].path.clone(), now);
    metadata.dirs = rows.len() as u64;
    let mut writer = tidemark_store::writer::SnapshotWriter::create(dir, &metadata).unwrap();
    writer.put_dirs(rows).unwrap();
    writer.finish(&metadata).unwrap()
}
