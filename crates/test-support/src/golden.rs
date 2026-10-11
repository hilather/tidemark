//! Owned by: W0-C
//! Published worked-example model and exact golden expectations.
use crate::random::{ModelDir, ModelEntry, ModelInode, ModelTree};
use std::collections::BTreeMap;
use tidemark_core::{
    path::{RawName, RawPath},
    scan::FileKind,
};
/// Normative row without filesystem-dependent directory contributions.
#[derive(Clone, Copy, Debug)]
pub struct GoldenRow {
    /// Absolute model path.
    pub path: &'static str,
    /// t_disk, t_charged, t_reclaimable, shared, t_files, t_inodes, t_dirs.
    pub values: [u64; 7],
}
/// Exactly the table in 03 §6, in model directory discovery order.
pub const LINK_SEMANTICS_BASIC: [GoldenRow; 6] = [
    GoldenRow {
        path: "/r",
        values: [1073152, 1073152, 1064960, 8192, 7, 5, 6],
    },
    GoldenRow {
        path: "/r/a",
        values: [1052672, 1048576, 1048576, 4096, 2, 2, 1],
    },
    GoldenRow {
        path: "/r/b",
        values: [12288, 8192, 0, 12288, 2, 2, 1],
    },
    GoldenRow {
        path: "/r/c",
        values: [12288, 12288, 12288, 0, 3, 2, 3],
    },
    GoldenRow {
        path: "/r/c/d",
        values: [12288, 0, 0, 12288, 1, 1, 1],
    },
    GoldenRow {
        path: "/r/c/e",
        values: [12288, 0, 0, 12288, 1, 1, 1],
    },
];
/// Model of 03 §6. Apparent sizes equal allocated sizes except symlink (4 bytes).
/// Injected mtime zero for all objects; directory blocks and apparent sizes zero.
pub fn link_semantics_basic() -> ModelTree {
    let raw = |s: &str| {
        RawName::new(s.as_bytes().to_vec()).unwrap_or_else(|e| panic!("golden name: {e}"))
    };
    let mut dirs = vec![];
    for (id, (parent, name)) in [
        (None, None),
        (Some(0), Some("a")),
        (Some(0), Some("b")),
        (Some(0), Some("c")),
        (Some(3), Some("d")),
        (Some(3), Some("e")),
    ]
    .into_iter()
    .enumerate()
    {
        dirs.push(ModelDir {
            parent,
            name: name.map(raw),
            ino: 10 + id as u64,
            disk: 0,
            apparent: 0,
            mtime: 0,
            errors: vec![],
            skipped: vec![],
        });
    }
    let mut inodes = BTreeMap::new();
    for (ino, nlink, disk) in [
        (100, 2, 4096),
        (101, 1, 1048576),
        (102, 3, 8192),
        (103, 2, 12288),
        (104, 1, 0),
    ] {
        inodes.insert(
            ino,
            ModelInode {
                nlink,
                kind: if ino == 104 {
                    FileKind::Symlink
                } else {
                    FileKind::Regular
                },
                disk,
                apparent: if ino == 104 { 4 } else { disk },
                mtime: 0,
            },
        );
    }
    let entries = [
        (1, "x", 100),
        (1, "big", 101),
        (2, "y", 100),
        (2, "z", 102),
        (3, "s", 104),
        (4, "w", 103),
        (5, "v", 103),
    ]
    .into_iter()
    .map(|(dir, name, ino)| ModelEntry {
        dir,
        name: raw(name),
        ino,
    })
    .collect();
    ModelTree {
        root: RawPath::new(b"/r".to_vec()).unwrap_or_else(|e| panic!("golden root: {e}")),
        now: 0,
        dirs,
        inodes,
        entries,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::compute;
    use tidemark_core::scan::DirId;
    #[test]
    fn reference_reproduces_03_section_6_exactly() {
        let tree = link_semantics_basic();
        let rows = compute(&tree);
        let paths = tree.paths();
        for (id, g) in LINK_SEMANTICS_BASIC.iter().enumerate() {
            assert_eq!(paths[id].display_lossy(), g.path);
            let m = &rows[&DirId(id as u32)];
            assert_eq!(
                [
                    m.t_disk,
                    m.t_charged,
                    m.t_reclaimable,
                    m.shared,
                    m.t_files,
                    m.t_inodes,
                    m.t_dirs
                ],
                g.values
            );
            assert_eq!(m.t_apparent, m.t_disk + u64::from([0, 3].contains(&id)) * 4);
            assert_eq!(m.age, [m.t_charged, 0, 0, 0, 0, 0]);
            assert_eq!(m.newest_mtime, 0);
            assert_eq!(m.t_errors, 0);
            assert_eq!(
                m.max_entries,
                if id == 0 || id == 3 {
                    3
                } else if id < 3 {
                    2
                } else {
                    1
                }
            );
        }
        let records = tree.to_dir_records();
        assert_eq!(records[1].own.disk, 1048576);
        assert_eq!(records[1].links[0].ino, 100);
        assert_eq!(records[0].entries, 3);
    }
}
