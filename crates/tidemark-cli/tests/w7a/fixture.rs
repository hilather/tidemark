//! Owned by: W7-A
//! Query and renderer coverage from deterministic hand-built snapshot rows.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use tidemark_core::{
    config::LinkInstances,
    path::RawPath,
    scan::{ScanMode, SnapshotStatus},
    snapshot::{DirRow, SnapshotMeta},
    time::ScanId,
};
pub fn row(id: u32, parent: Option<u32>, path: &[u8], disk: u64) -> DirRow {
    let path = RawPath::new(path.to_vec()).unwrap();
    DirRow {
        id,
        parent_id: parent,
        subtree_end: if id == 0 { 3 } else { id },
        depth: u32::from(parent.is_some()),
        name: path
            .file_name()
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_else(|| path.as_bytes().to_vec()),
        path_hash: path.hash64(),
        path,
        ino: u64::from(id) + 1,
        mtime: 100,
        dir_disk: 0,
        dir_apparent: 0,
        projid: None,
        entries: 1,
        errors: 0,
        own_files: 1,
        own_symlinks: 0,
        own_charged: disk,
        own_apparent: disk,
        t_dirs: 1,
        t_files: 1,
        t_inodes: 1,
        t_disk: disk,
        t_apparent: disk,
        t_charged: disk,
        t_reclaimable: disk / 2,
        t_errors: 0,
        age0: disk,
        age1: 0,
        age2: 0,
        age3: 0,
        age4: 0,
        age5: 0,
        newest_mtime: 100,
        max_entries: 1,
    }
}
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
