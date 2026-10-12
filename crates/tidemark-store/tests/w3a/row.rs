//! Owned by: W3-A-fix
//! Shared deterministic directory-row fixture for integration and memory-plan tests.
use tidemark_core::{path::RawPath, snapshot::DirRow};
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
