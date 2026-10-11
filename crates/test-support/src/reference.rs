//! Owned by: W0-C
//! Brute-force subtree sets, independent of scanner conversion and aggregation.
use crate::random::ModelTree;
use std::collections::{BTreeMap, BTreeSet};
use tidemark_core::{scan::DirId, time::age_bucket};
/// All normative per-directory metrics (03 §3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metrics {
    /// Distinct inode allocated bytes plus directory allocated bytes.
    pub t_disk: u64,
    /// Distinct inode apparent bytes plus directory apparent bytes.
    pub t_apparent: u64,
    /// Allocated bytes charged within the subtree.
    pub t_charged: u64,
    /// Complete inode bytes charged within the subtree.
    pub t_reclaimable: u64,
    /// Visible bytes retained by outside links.
    pub shared: u64,
    /// Non-directory entry instances.
    pub t_files: u64,
    /// Distinct non-directory inodes.
    pub t_inodes: u64,
    /// Directories including this directory.
    pub t_dirs: u64,
    /// Charged bytes by mtime age, including directory blocks.
    pub age: [u64; 6],
    /// Maximum visible mtime, including directory inodes.
    pub newest_mtime: i64,
    /// Maximum direct entry count in subtree, including skipped entries.
    pub max_entries: u64,
    /// Total recorded errors in subtree.
    pub t_errors: u64,
}
/// Compute every metric from fresh subtree and inode sets for each directory.
/// An inode's charge is in a subtree exactly when *all* discovered instances
/// are in that subtree. This deliberately avoids the aggregator's LCA/rollup code.
pub fn compute(tree: &ModelTree) -> BTreeMap<DirId, Metrics> {
    tree.validate();
    let mut result = BTreeMap::new();
    for root in 0..tree.dirs.len() {
        let mut subtree = BTreeSet::new();
        for id in 0..tree.dirs.len() {
            let mut cursor = Some(id);
            while let Some(d) = cursor {
                if d == root {
                    subtree.insert(id);
                    break;
                }
                cursor = tree.dirs[d].parent;
            }
        }
        let mut m = Metrics {
            newest_mtime: i64::MIN,
            ..Metrics::default()
        };
        for &id in &subtree {
            let d = &tree.dirs[id];
            m.t_disk += d.disk;
            m.t_apparent += d.apparent;
            m.t_charged += d.disk;
            m.t_reclaimable += d.disk;
            m.age[age_bucket(tree.now, d.mtime)] += d.disk;
            m.newest_mtime = m.newest_mtime.max(d.mtime);
            m.t_dirs += 1;
            m.t_errors += u64::from(d.errors_total);
            let width = tree.dirs.iter().filter(|d| d.parent == Some(id)).count()
                + tree.entries.iter().filter(|e| e.dir == id).count()
                + d.skipped.len();
            m.max_entries = m.max_entries.max(width as u64);
        }
        let visible: BTreeSet<_> = tree
            .entries
            .iter()
            .filter(|e| subtree.contains(&e.dir))
            .map(|e| e.ino)
            .collect();
        m.t_files = tree
            .entries
            .iter()
            .filter(|e| subtree.contains(&e.dir))
            .count() as u64;
        m.t_inodes = visible.len() as u64;
        for ino in visible {
            let i = &tree.inodes[&ino];
            m.t_disk += i.disk;
            m.t_apparent += i.apparent;
            m.newest_mtime = m.newest_mtime.max(i.mtime);
            let holders: Vec<_> = tree.entries.iter().filter(|e| e.ino == ino).collect();
            if holders.iter().all(|e| subtree.contains(&e.dir)) {
                m.t_charged += i.disk;
                m.age[age_bucket(tree.now, i.mtime)] += i.disk;
                if holders.len() >= i.nlink as usize {
                    m.t_reclaimable += i.disk;
                }
            }
        }
        m.shared = m.t_disk - m.t_reclaimable;
        result.insert(DirId(root as u32), m);
    }
    result
}
