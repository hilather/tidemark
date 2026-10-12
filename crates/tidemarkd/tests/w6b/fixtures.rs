//! Owned by: W6-B
//! Immutable snapshot fixtures with hostile raw paths.
#![allow(clippy::unwrap_used)]
use std::collections::BTreeMap;
use tidemark_core::{
    config::LinkInstances,
    path::{RawName, RawPath},
    scan::*,
    snapshot::*,
};
fn path(bytes: &[u8]) -> RawPath {
    RawPath::new(bytes.to_vec()).unwrap()
}
pub fn meta() -> SnapshotMeta {
    SnapshotMeta {
        schema_version: 1,
        tool_version: "test".into(),
        scan_id: serde_json::from_str("\"20260101T000000Z-00000000\"").unwrap(),
        root: path(b"/fixture/\xff\"\\\n"),
        root_display: "display".into(),
        mode: ScanMode::Walk,
        mode_reason: String::new(),
        status: SnapshotStatus::Complete,
        degraded: false,
        exclude: vec![],
        link_instances: LinkInstances::All,
        started_at: 1000000,
        finished_at: 1000010,
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
        phase_ms: BTreeMap::new(),
        gate_stats: Default::default(),
    }
}
pub fn dir(id: u32) -> DirRow {
    let p = if id == 0 {
        path(b"/fixture/\xff\"\\\n")
    } else {
        path(b"/fixture/\xff\"\\\n").join(&RawName::new(format!("{id:08}").into_bytes()).unwrap())
    };
    DirRow {
        id,
        parent_id: if id == 0 { None } else { Some(0) },
        subtree_end: if id == 0 { 99999 } else { id },
        depth: if id == 0 { 0 } else { 1 },
        name: if id == 0 {
            p.as_bytes().to_vec()
        } else {
            format!("{id:08}").into_bytes()
        },
        path: p.clone(),
        path_hash: p.hash64(),
        ino: u64::from(id) + 1,
        mtime: 999900,
        dir_disk: 0,
        dir_apparent: 0,
        projid: None,
        entries: 7,
        errors: 0,
        own_files: 0,
        own_symlinks: 0,
        own_charged: 0,
        own_apparent: 0,
        t_dirs: 1,
        t_files: 0,
        t_inodes: 0,
        t_disk: u64::from(id) + 4096,
        t_apparent: 0,
        t_charged: 0,
        t_reclaimable: 0,
        t_errors: 0,
        age0: 0,
        age1: 0,
        age2: 0,
        age3: 0,
        age4: 0,
        age5: 0,
        newest_mtime: 999900,
        max_entries: 0,
    }
}
