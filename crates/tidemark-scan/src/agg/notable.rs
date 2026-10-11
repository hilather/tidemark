//! Owned by: W2-A
//! Deterministic final-id notable selection.
use super::{count, AggConfig, AggError, FileRow};
pub(super) fn select(
    mut files: Vec<FileRow>,
    ids: &[u32],
    cfg: &AggConfig,
) -> Result<Vec<FileRow>, AggError> {
    for f in &mut files {
        f.dir_id = *ids
            .get(f.dir_id as usize)
            .filter(|&&n| n != u32::MAX)
            .ok_or(AggError::Tree)?;
    }
    files.sort_by(|a, b| {
        b.disk
            .cmp(&a.disk)
            .then(a.dir_id.cmp(&b.dir_id))
            .then(a.ino.cmp(&b.ino))
    });
    let keep = files
        .iter()
        .take_while(|f| f.disk >= cfg.big_file_threshold)
        .count()
        .max(cfg.top_files)
        .min(cfg.max_notable);
    files.truncate(keep);
    files.shrink_to_fit();
    for (id, f) in files.iter_mut().enumerate() {
        f.id = count(id)?;
    }
    Ok(files)
}

/// Before final ids exist, retain all directories tied at the global size cutoff.
/// At the cutoff, each directory contributes at most K=min(top_files,max_notable) below the
/// threshold, or max_notable above it. Thus ties cost at most D*max_notable
/// rows, independent of inode count; non-tied winners cost O(max_notable).
/// Overall at most (D+1)*max_notable rows, plus one transient inserted row.
#[derive(Default)]
pub(super) struct Candidates {
    buckets: std::collections::BTreeMap<u64, Bucket>,
}
#[derive(Default)]
struct Bucket {
    dirs: std::collections::BTreeMap<u32, smallvec::SmallVec<[FileRow; 1]>>,
    count: usize,
}
impl Candidates {
    pub(super) fn insert(&mut self, f: FileRow, cfg: &AggConfig) {
        let limit = if f.disk >= cfg.big_file_threshold {
            cfg.max_notable
        } else {
            cfg.top_files.min(cfg.max_notable)
        };
        if limit == 0 {
            return;
        }
        let bucket = self.buckets.entry(f.disk).or_default();
        let dir = bucket.dirs.entry(f.dir_id).or_default();
        let previous = dir.len();
        match dir.binary_search_by_key(&f.ino, |row| row.ino) {
            Ok(index) => dir[index] = f,
            Err(index) if index < limit => {
                // Pop before insertion so top_files=1 never spills its inline row.
                if dir.len() == limit {
                    dir.pop();
                }
                dir.insert(index, f);
            }
            Err(_) => return,
        }
        bucket.count += dir.len() - previous;
        let mut preceding = 0usize;
        let mut cutoff = None;
        for (&disk, dirs) in self.buckets.iter().rev() {
            let limit = if disk >= cfg.big_file_threshold {
                cfg.max_notable
            } else {
                cfg.top_files.min(cfg.max_notable)
            };
            if preceding >= limit {
                cutoff = Some(disk);
                break;
            }
            preceding = preceding.saturating_add(dirs.count);
        }
        if let Some(cutoff) = cutoff {
            self.buckets = self.buckets.split_off(&cutoff.saturating_add(1));
        }
    }
    pub(super) fn select(self, ids: &[u32], cfg: &AggConfig) -> Result<Vec<FileRow>, AggError> {
        // Final ids now allow an exact cap, including the former cutoff ties.
        // Drain lazily: preparation scratch never scales with retained tie rows.
        let mut winners = std::collections::BTreeMap::new();
        for f in self
            .buckets
            .into_values()
            .flat_map(|b| b.dirs.into_values())
            .flatten()
        {
            let id = *ids
                .get(f.dir_id as usize)
                .filter(|&&id| id != u32::MAX)
                .ok_or(AggError::Tree)?;
            winners.insert((std::cmp::Reverse(f.disk), id, f.ino), f);
            if winners.len() > cfg.max_notable {
                winners.pop_last();
            }
        }
        select(winners.into_values().collect(), ids, cfg)
    }
    #[cfg(test)]
    pub(super) fn assert_single_bulk_storage_bound(&self) {
        assert_eq!(self.buckets.len(), 1);
        let bucket = self.buckets.values().next().unwrap();
        assert_eq!(bucket.dirs.len(), 1);
        let dir = bucket.dirs.values().next().unwrap();
        assert_eq!(dir.len(), 1);
        assert!(!dir.spilled());
        // Rust 1.98 BTreeMap leaf capacity is eleven. Charge each of the two
        // leaves 512 extra bytes for headers/edge slack, plus inline containers.
        // This is a conservative structural estimate, excluding allocator metadata.
        let bytes = std::mem::size_of::<Self>()
            + 2 * 512
            + 11 * (8 + std::mem::size_of::<Bucket>())
            + 11 * (4 + std::mem::size_of::<smallvec::SmallVec<[FileRow; 1]>>());
        assert!(bytes <= 8 * 1024);
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.buckets.values().map(|d| d.count).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    fn cfg(top: usize, cap: usize, threshold: u64) -> AggConfig {
        AggConfig {
            root: tidemark_core::path::RawPath::new(b"/r".to_vec()).unwrap(),
            scan_start: 0,
            top_files: top,
            max_notable: cap,
            big_file_threshold: threshold,
            link_instances: false,
            max_stored_errors: 0,
        }
    }
    fn file(ino: u64, dir: u32, disk: u64) -> FileRow {
        FileRow {
            id: 0,
            dir_id: dir,
            name: None,
            ino,
            disk,
            apparent: disk,
            mtime: 0,
            nlink: 1,
        }
    }
    proptest! {
        #[test]
        fn bounded_matches_unbounded(rows in prop::collection::vec((0u32..8, 0u64..20), 0..600), top in 0usize..20, cap in 1usize..25, threshold in 0u64..25, rotation in 0usize..8) {
            let cfg = cfg(top, cap, threshold);
            let ids: Vec<u32> = (0..8).map(|i| ((i + rotation) % 8) as u32).collect();
            let files: Vec<_> = rows.iter().enumerate().map(|(i, &(d, disk))| file(i as u64, d, disk)).collect();
            let expected = select(files.clone(), &ids, &cfg).unwrap();
            for input in [files.clone(), files.into_iter().rev().collect()] {
                let mut pool = Candidates::default();
                for f in input {
                    pool.insert(f, &cfg);
                    prop_assert!(pool.len() <= 9 * cap);
                }
                prop_assert_eq!(pool.select(&ids, &cfg).unwrap(), expected.clone());
            }
        }
    }
    #[test]
    #[ignore = "scale"]
    fn half_million_equal_size_walk_directories() {
        let cfg = cfg(1, 1, u64::MAX);
        let mut pool = Candidates::default();
        for dir in 0..500_000 {
            for ino in (0..4).rev() {
                pool.insert(file(u64::from(dir) * 4 + ino, dir, 10), &cfg);
            }
        }
        assert_eq!(pool.len(), 500_000);
        let ids: Vec<u32> = (0..500_000).rev().collect();
        let selected = pool.select(&ids, &cfg).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].ino, 499_999 * 4);
    }
    #[test]
    fn cutoff_ties_keep_each_directorys_smallest_inodes() {
        let cfg = cfg(1, 1, u64::MAX);
        let mut pool = Candidates::default();
        for ino in (0..1000).rev() {
            pool.insert(file(ino, (ino % 8) as u32, 10), &cfg);
        }
        assert_eq!(pool.len(), 8);
        assert_eq!(
            pool.select(&[7, 6, 5, 4, 3, 2, 1, 0], &cfg).unwrap()[0].ino,
            7
        );
    }
}
