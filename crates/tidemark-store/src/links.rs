//! Owned by: W3-B
//! Bounded hard-link topology queries and laminar preorder regions.

use crate::{reader::Snapshot, StoreError};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use tidemark_core::{
    path::RawPath,
    snapshot::{LinkFilter, LinkSetHit},
};

const CAP: usize = 1_000_000;
fn require_instances(snap: &Snapshot) -> Result<(), StoreError> {
    if snap.meta().link_instances != tidemark_core::config::LinkInstances::All {
        return Err(StoreError::Invalid(
            "link topology requires a snapshot with scan.link_instances = all".into(),
        ));
    }
    Ok(())
}
/// Region selection below the report root.
#[derive(Clone, Debug)]
pub enum RegionSpec {
    /// Match complete raw directory paths with literal separators.
    Globs(Vec<String>),
    /// Select directories at this relative depth.
    Depth(u32),
}
/// Stable index into the regions sorted by preorder start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegionId(usize);
struct Interval {
    start: u32,
    end: u32,
    parent: Option<usize>,
    path: RawPath,
}
/// Laminar intervals, capped at one million regions.
///
/// ```no_run
/// # fn main() -> Result<(), tidemark_store::StoreError> {
/// use tidemark_store::{reader::Snapshot, links::{Regions, RegionSpec}};
/// let snap = Snapshot::open(std::path::Path::new("scan.db"))?;
/// let regions = Regions::build(&snap, &snap.meta().root, &RegionSpec::Depth(1))?;
/// if let Some(id) = regions.region_of(1) { let path = regions.name(id); }
/// # Ok(()) }
/// ```
pub struct Regions {
    intervals: Vec<Interval>,
    under: RawPath,
}
impl Regions {
    /// Build preorder intervals with precomputed nearest containing parents.
    pub fn build(
        snap: &Snapshot,
        under: &RawPath,
        spec: &RegionSpec,
    ) -> Result<Regions, StoreError> {
        let root = snap
            .dir_by_path(under)?
            .ok_or_else(|| StoreError::Invalid("report root absent".into()))?;
        let filter = match spec {
            RegionSpec::Globs(g) => LinkFilter {
                boundary: g.clone(),
                ..Default::default()
            },
            RegionSpec::Depth(d) => LinkFilter {
                boundary_depth: Some(*d),
                ..Default::default()
            },
        };
        crate::error::check_result_bound(&filter.boundary)?;
        let mut builder = globset::GlobSetBuilder::new();
        for g in &filter.boundary {
            builder.add(
                globset::GlobBuilder::new(g)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| StoreError::Invalid(e.to_string()))?,
            );
        }
        let globs = builder
            .build()
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let mut stmt = snap.v1_conn()?.prepare(
            "SELECT id,subtree_end,path,depth FROM dirs WHERE id BETWEEN ?1 AND ?2 ORDER BY id",
        )?;
        let mut rows = stmt.query(rusqlite::params![root.id, root.subtree_end])?;
        let mut selected = Vec::new();
        while let Some(row) = rows.next()? {
            let depth: u32 = row.get(3)?;
            let bytes: Vec<u8> = row.get(2)?;
            let matches = match spec {
                RegionSpec::Depth(d) => u64::from(depth) == u64::from(root.depth) + u64::from(*d),
                RegionSpec::Globs(_) => {
                    globs.is_match_candidate(&globset::Candidate::from_bytes(&bytes))
                }
            };
            if matches {
                if selected.len() == CAP {
                    return Err(StoreError::Invalid("region cap reached".into()));
                }
                selected.push((row.get(0)?, row.get(1)?, RawPath::new(bytes)?));
            }
        }
        Ok(Self {
            intervals: intervals(selected.into_iter()),
            under: under.clone(),
        })
    }
    /// Find the deepest region containing a preorder directory id.
    pub fn region_of(&self, dir_id: u32) -> Option<RegionId> {
        let mut i = self
            .intervals
            .partition_point(|r| r.start <= dir_id)
            .checked_sub(1)?;
        loop {
            let r = &self.intervals[i];
            if dir_id <= r.end {
                return Some(RegionId(i));
            }
            i = r.parent?;
        }
    }
    /// Borrow a region's byte-preserving path; ids must originate from this object.
    pub fn name(&self, r: RegionId) -> &RawPath {
        &self.intervals[r.0].path
    }
}
fn intervals(rows: impl Iterator<Item = (u32, u32, RawPath)>) -> Vec<Interval> {
    let mut result: Vec<Interval> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for (start, end, path) in rows {
        while stack.last().is_some_and(|&i| result[i].end < start) {
            stack.pop();
        }
        result.push(Interval {
            start,
            end,
            path,
            parent: stack.last().copied(),
        });
        stack.push(result.len() - 1);
    }
    result
}
/// A set with entries in distinct regions, including the unmatched region.
#[derive(Debug)]
pub struct CrossingSet {
    /// Inode number.
    pub ino: u64,
    /// Allocated bytes, counted once for this set.
    pub disk: u64,
    /// Kernel link count.
    pub nlink: u32,
    /// Discovered instances, including unstored ones.
    pub found: u32,
    /// Distinct regions in deterministic order; None means unmatched.
    pub regions: Vec<Option<RegionId>>,
    /// One representative directory per region, aligned with regions.
    pub sample_dirs: Vec<u32>,
}
/// Aggregate counts of crossing sets; independent of the result limit.
#[derive(Debug, Default)]
pub struct CrossingTotals {
    /// Number of crossing sets.
    pub sets: u64,
    /// Allocated bytes in crossing sets.
    pub disk: u64,
    /// Crossing sets with undiscovered external links.
    pub incomplete_sets: u64,
    /// Bytes in incomplete crossing sets.
    pub incomplete_disk: u64,
}
/// Find crossings from inode-ordered streamed rows, retaining only bounded results.
///
/// ```no_run
/// # fn main() -> Result<(), tidemark_store::StoreError> {
/// use tidemark_store::{reader::Snapshot, links::*};
/// let snap = Snapshot::open(std::path::Path::new("scan.db"))?;
/// let r = Regions::build(&snap, &snap.meta().root, &RegionSpec::Depth(1))?;
/// let (sets, totals) = crossing_sets(&snap, &r, 20)?;
/// assert!(totals.sets >= sets.len() as u64);
/// # Ok(()) }
/// ```
pub fn crossing_sets(
    snap: &Snapshot,
    regions: &Regions,
    limit: usize,
) -> Result<(Vec<CrossingSet>, CrossingTotals), StoreError> {
    if limit > CAP {
        return Err(StoreError::Invalid("limit exceeds 1,000,000".into()));
    }
    require_instances(snap)?;
    let root = snap
        .dir_by_path(&regions.under)?
        .ok_or_else(|| StoreError::Invalid("report root absent".into()))?;
    let mut totals = CrossingTotals::default();
    let mut out = BinaryHeap::new();
    let mut sample_count = 0usize;
    let mut samples = BTreeMap::new();
    stream_sets(snap, root.id, root.subtree_end, |hit, dirs| {
        samples.clear();
        for &dir_id in dirs {
            samples
                .entry(regions.region_of(dir_id))
                .and_modify(|id: &mut u32| *id = (*id).min(dir_id))
                .or_insert(dir_id);
        }
        if samples.len() < 2 {
            return Ok(());
        }
        totals.sets += 1;
        totals.disk = add(totals.disk, hit.row.disk)?;
        if hit.row.found < hit.row.nlink {
            totals.incomplete_sets += 1;
            totals.incomplete_disk = add(totals.incomplete_disk, hit.row.disk)?;
        }
        retain_crossing(&mut out, &mut sample_count, hit, &samples, limit)?;
        Ok(())
    })?;
    let out = out
        .into_sorted_vec()
        .into_iter()
        .map(|entry| entry.value)
        .collect();
    Ok((out, totals))
}
fn order(
    disk: u64,
    path: &RawPath,
    ino: u64,
    other_disk: u64,
    other_path: &RawPath,
    other_ino: u64,
) -> std::cmp::Ordering {
    #[cfg(test)]
    COMPARISONS.with(|count| count.set(count.get() + 1));
    other_disk
        .cmp(&disk)
        .then_with(|| path.cmp(other_path))
        .then_with(|| ino.cmp(&other_ino))
}
// The maximum heap entry is the worst retained result. Payloads do not
// participate in ordering; all three selectors share the frozen ranking.
struct Ranked<T> {
    disk: u64,
    path: RawPath,
    ino: u64,
    value: T,
}
impl<T> Ranked<T> {
    fn new(hit: &LinkSetHit, value: T) -> Self {
        Self {
            disk: hit.row.disk,
            path: hit.lca_path.clone(),
            ino: hit.row.ino,
            value,
        }
    }
}
impl<T> PartialEq for Ranked<T> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl<T> Eq for Ranked<T> {}
impl<T> PartialOrd for Ranked<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Ranked<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        order(
            self.disk,
            &self.path,
            self.ino,
            other.disk,
            &other.path,
            other.ino,
        )
    }
}
fn admits<T>(heap: &BinaryHeap<Ranked<T>>, hit: &LinkSetHit, limit: usize) -> bool {
    limit > 0
        && (heap.len() < limit
            || heap.peek().is_some_and(|worst| {
                order(
                    hit.row.disk,
                    &hit.lca_path,
                    hit.row.ino,
                    worst.disk,
                    &worst.path,
                    worst.ino,
                )
                .is_lt()
            }))
}
fn retain_crossing(
    heap: &mut BinaryHeap<Ranked<CrossingSet>>,
    sample_count: &mut usize,
    hit: &LinkSetHit,
    samples: &BTreeMap<Option<RegionId>, u32>,
    limit: usize,
) -> Result<(), StoreError> {
    if !admits(heap, hit, limit) {
        return Ok(());
    }
    let displaced = if heap.len() == limit {
        heap.peek().map_or(0, |entry| entry.value.regions.len())
    } else {
        0
    };
    let prospective = *sample_count - displaced + samples.len();
    if prospective > CAP {
        return Err(StoreError::Invalid("crossing sample cap reached".into()));
    }
    // Release obsolete samples before allocating the replacement's vectors.
    if heap.len() == limit {
        heap.pop();
    }
    *sample_count = prospective;
    let (regions, sample_dirs) = samples.iter().map(|(&r, &d)| (r, d)).unzip();
    heap.push(Ranked::new(
        hit,
        CrossingSet {
            ino: hit.row.ino,
            disk: hit.row.disk,
            nlink: hit.row.nlink,
            found: hit.row.found,
            regions,
            sample_dirs,
        },
    ));
    Ok(())
}
#[cfg(test)]
thread_local! {
    static COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
// One reusable flat directory-id buffer; no names or global instance collection.
fn stream_sets(
    snap: &Snapshot,
    start: u32,
    end: u32,
    mut visit: impl FnMut(&LinkSetHit, &[u32]) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    // Two ordered cursors avoid both a global sort and an index lookup per inode.
    // The primary-key scan remains outermost for byte-path tie ordering at the limit.
    let mut sets_stmt = snap.v1_conn()?.prepare(
        "SELECT l.*,d.path FROM link_sets l NOT INDEXED CROSS JOIN dirs d ON d.id=l.lca_dir_id \
         WHERE l.lca_dir_id BETWEEN ?1 AND ?2 ORDER BY l.ino",
    )?;
    let mut instances_stmt = snap
        .v1_conn()?
        .prepare("SELECT ino,dir_id FROM link_instances ORDER BY ino")?;
    let mut sets = sets_stmt.query(rusqlite::params![start, end])?;
    let mut instances = instances_stmt.query([])?;
    let mut instance = instances.next()?;
    let mut dirs = Vec::new();
    while let Some(row) = sets.next()? {
        let mut hit = LinkSetHit {
            row: crate::reader::link_row(row)?,
            lca_path: RawPath::new(row.get::<_, Vec<u8>>(8)?)?,
            instance_count: 0,
        };
        dirs.clear();
        while let Some(i) = instance {
            let ino = crate::error::read_u64(i, 0)?;
            if ino > hit.row.ino {
                break;
            }
            if ino == hit.row.ino {
                if dirs.len() == CAP {
                    return Err(StoreError::Invalid("per-set instance cap reached".into()));
                }
                dirs.push(i.get::<_, u32>(1)?);
            }
            instance = instances.next()?;
        }
        hit.instance_count = dirs.len() as u64;
        visit(&hit, &dirs)?;
    }
    Ok(())
}

/// Link-set totals for the charged subtree.
#[derive(Debug, Default)]
pub struct LinkTotals {
    /// Link sets.
    pub sets: u64,
    /// Discovered instances.
    pub instances: u64,
    /// Allocated bytes counted once per set.
    pub disk: u64,
    /// Incomplete sets.
    pub incomplete_sets: u64,
    /// Bytes pinned by links outside the scan.
    pub incomplete_disk: u64,
    /// Aggregate crossing statistics when boundaries were requested.
    pub crossing: CrossingTotals,
}
/// Pair of region paths; None identifies unmatched instances inside the scan.
#[derive(Debug)]
pub struct RegionPair {
    /// First region.
    pub a: Option<RawPath>,
    /// Second region.
    pub b: Option<RawPath>,
    /// Sets represented by this pair.
    pub sets: u64,
    /// Allocated bytes represented by this pair.
    pub disk: u64,
}
/// Five sections of the hard-link topology report.
pub struct LinkReport {
    /// Global subtree totals.
    pub totals: LinkTotals,
    /// Largest crossing sets.
    pub crossing: Vec<CrossingSet>,
    /// Largest region pairs.
    pub region_pairs: Vec<RegionPair>,
    /// Subtrees with pinned bytes from incomplete sets (once per set per subtree).
    pub incomplete_top_dirs: Vec<(RawPath, u64, u64)>,
    /// Largest link sets with their LCA paths.
    pub largest_sets: Vec<LinkSetHit>,
    /// Most shared directories, with shared and total disk bytes.
    pub most_shared_dirs: Vec<(RawPath, u64, u64)>,
}
fn add(a: u64, b: u64) -> Result<u64, StoreError> {
    a.checked_add(b)
        .ok_or_else(|| StoreError::Integer("link report sum".into()))
}
/// Build a report from two inode-ordered cursors with a reusable per-set directory buffer.
/// Pair aggregation is capped at one million distinct pairs and fails explicitly above it.
pub fn link_report(
    snap: &Snapshot,
    under: &RawPath,
    spec: Option<&RegionSpec>,
    top: usize,
) -> Result<LinkReport, StoreError> {
    if top > CAP {
        return Err(StoreError::Invalid("top exceeds 1,000,000".into()));
    }
    let root = snap
        .dir_by_path(under)?
        .ok_or_else(|| StoreError::Invalid("report root absent".into()))?;
    require_instances(snap)?;
    let mut largest_sets = BinaryHeap::new();
    let regions = spec.map(|s| Regions::build(snap, under, s)).transpose()?;
    let mut totals = LinkTotals::default();
    let mut pinned: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
    let mut pairs: BTreeMap<(Option<RegionId>, Option<RegionId>), (u64, u64)> = BTreeMap::new();
    let mut crossing = BinaryHeap::new();
    let mut sample_count = 0usize;
    let mut samples = BTreeMap::new();
    let mut visited = BTreeSet::new();
    stream_sets(snap, root.id, root.subtree_end, |hit, dirs| {
        if admits(&largest_sets, hit, top) {
            if largest_sets.len() == top {
                largest_sets.pop();
            }
            largest_sets.push(Ranked::new(hit, hit.clone()));
        }
        totals.sets += 1;
        totals.instances = add(totals.instances, u64::from(hit.row.found))?;
        totals.disk = add(totals.disk, hit.row.disk)?;
        let incomplete = hit.row.found < hit.row.nlink;
        if incomplete {
            totals.incomplete_sets += 1;
            totals.incomplete_disk = add(totals.incomplete_disk, hit.row.disk)?;
        }
        visited.clear();
        samples.clear();
        for &dir_id in dirs {
            if incomplete {
                let mut cursor = Some(dir_id);
                while let Some(id) = cursor {
                    if id < root.id || id > root.subtree_end || !visited.insert(id) {
                        break;
                    }
                    if visited.len() > CAP {
                        return Err(StoreError::Invalid("per-set ancestor cap".into()));
                    }
                    let entry = pinned.entry(id).or_default();
                    entry.0 = add(entry.0, hit.row.disk)?;
                    entry.1 += 1;
                    if pinned.len() > CAP {
                        return Err(StoreError::Invalid("pinned directory cap".into()));
                    }
                    cursor = snap.dir(id)?.parent_id;
                }
            }
            if let Some(r) = &regions {
                samples
                    .entry(r.region_of(dir_id))
                    .and_modify(|id: &mut u32| *id = (*id).min(dir_id))
                    .or_insert(dir_id);
            }
        }
        if samples.len() > 1 {
            totals.crossing.sets += 1;
            totals.crossing.disk = add(totals.crossing.disk, hit.row.disk)?;
            if incomplete {
                totals.crossing.incomplete_sets += 1;
                totals.crossing.incomplete_disk =
                    add(totals.crossing.incomplete_disk, hit.row.disk)?;
            }
            let keys: Vec<_> = samples.keys().copied().collect();
            for (j, &a) in keys.iter().enumerate() {
                for &b in &keys[j + 1..] {
                    let e = pairs.entry((a, b)).or_default();
                    e.0 += 1;
                    e.1 = add(e.1, hit.row.disk)?;
                    if pairs.len() > CAP {
                        return Err(StoreError::Invalid("region pair cap".into()));
                    }
                }
            }
            retain_crossing(&mut crossing, &mut sample_count, hit, &samples, top)?;
        }
        Ok(())
    })?;
    let crossing = crossing
        .into_sorted_vec()
        .into_iter()
        .map(|entry| entry.value)
        .collect();
    let largest_sets = largest_sets
        .into_sorted_vec()
        .into_iter()
        .map(|entry| entry.value)
        .collect();
    let mut incomplete_top_dirs = pinned
        .into_iter()
        .map(|(id, (bytes, sets))| Ok((snap.dir(id)?.path, bytes, sets)))
        .collect::<Result<Vec<_>, StoreError>>()?;
    incomplete_top_dirs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    incomplete_top_dirs.truncate(top);
    let mut region_pairs = Vec::new();
    if let Some(r) = &regions {
        for ((a, b), (sets, disk)) in pairs {
            region_pairs.push(RegionPair {
                a: a.map(|id| r.name(id).clone()),
                b: b.map(|id| r.name(id).clone()),
                sets,
                disk,
            });
        }
    }
    region_pairs.sort_by(|a, b| {
        b.disk
            .cmp(&a.disk)
            .then_with(|| a.a.cmp(&b.a))
            .then_with(|| a.b.cmp(&b.b))
    });
    region_pairs.truncate(top);
    let mut stmt = snap.v1_conn()?.prepare("SELECT path,t_disk-t_reclaimable,t_disk FROM dirs WHERE id BETWEEN ?1 AND ?2 AND t_disk>t_reclaimable ORDER BY t_disk-t_reclaimable DESC,path LIMIT ?3")?;
    let mut rows = stmt.query(rusqlite::params![
        root.id,
        root.subtree_end,
        crate::error::sql_limit(top)?
    ])?;
    let mut most_shared_dirs = Vec::new();
    while let Some(row) = rows.next()? {
        most_shared_dirs.push((
            RawPath::new(row.get::<_, Vec<u8>>(0)?)?,
            crate::error::read_u64(row, 1)?,
            crate::error::read_u64(row, 2)?,
        ));
    }
    Ok(LinkReport {
        totals,
        crossing,
        region_pairs,
        incomplete_top_dirs,
        largest_sets,
        most_shared_dirs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    fn hit(ino: u64, disk: u64) -> LinkSetHit {
        LinkSetHit {
            row: tidemark_core::snapshot::LinkSetRow {
                ino,
                disk,
                apparent: disk,
                nlink: 2,
                found: 2,
                complete: true,
                lca_dir_id: 0,
                mtime: 0,
            },
            lca_path: RawPath::new(b"/r".to_vec()).unwrap(),
            instance_count: 2,
        }
    }

    #[test]
    fn crossing_replacement_at_sample_cap() {
        // Shared by crossing_sets and link_report: 1,000 retained sets,
        // each spanning 1,000 regions, followed by a better replacement.
        let samples = (0..1000)
            .map(|id| (Some(RegionId(id)), id as u32))
            .collect();
        let mut heap = BinaryHeap::new();
        let mut count = 0;
        for ino in 0..1001 {
            retain_crossing(&mut heap, &mut count, &hit(ino, ino), &samples, 1000).unwrap();
            assert_eq!(count, heap.len() * 1000);
        }
        assert_eq!(count, CAP);
        let result = heap.into_sorted_vec();
        assert_eq!(result.first().unwrap().value.ino, 1000);
        assert_eq!(result.last().unwrap().value.ino, 1);
        assert!(result
            .iter()
            .all(|entry| entry.value.sample_dirs.len() == 1000));

        // A genuinely oversized retained result must still fail.
        let mut heap = BinaryHeap::new();
        let mut count = 0;
        for ino in 0..1000 {
            retain_crossing(&mut heap, &mut count, &hit(ino, ino), &samples, 1001).unwrap();
        }
        assert!(retain_crossing(&mut heap, &mut count, &hit(1000, 1000), &samples, 1001).is_err());
        assert_eq!(count, CAP);
        assert_eq!(heap.len(), 1000);
    }

    #[test]
    fn largest_supported_limit_has_logarithmic_selection_work() {
        // Exercise the production heap/comparator at the maximum supported k.
        // Increasing sizes force replacement after filling the entire heap.
        let mut heap = BinaryHeap::new();
        COMPARISONS.with(|count| count.set(0));
        for ino in 0..2 * CAP as u64 {
            let candidate = hit(ino, ino);
            if admits(&heap, &candidate, CAP) {
                if heap.len() == CAP {
                    heap.pop();
                }
                heap.push(Ranked::new(&candidate, ()));
            }
        }
        let selection = COMPARISONS.with(|count| count.get());
        let log_k = (usize::BITS - CAP.leading_zeros()) as usize;
        assert!(selection < 4 * 2 * CAP * log_k, "{selection} comparisons");
        let result = heap.into_sorted_vec();
        let total = COMPARISONS.with(|count| count.get());
        assert!(
            total < 6 * 2 * CAP * log_k,
            "{total} comparisons including final sort"
        );
        assert_eq!(result.len(), CAP);
        for (i, entry) in result.iter().enumerate() {
            assert_eq!(entry.ino, (2 * CAP - 1 - i) as u64);
        }
        eprintln!(
            "maximum top={CAP}: selection={selection}, including final sort={total} comparisons"
        );
    }

    proptest! {
        #[test]
        fn intervals_match_bruteforce(selected in prop::collection::vec(any::<bool>(), 1..200), point in 0u32..220) {
            // Heap-tree preorder: each directory interval is nested or disjoint.
            let mut rows = Vec::new();
            fn tree(rows:&mut Vec<(u32,u32,RawPath)>,level:u32,next:&mut u32) {
                let id=*next; *next+=1;
                let pos=rows.len();
                rows.push((id,id,RawPath::new(format!("/r/{id}").into_bytes()).unwrap()));
                if level>0 { tree(rows,level-1,next); tree(rows,level-1,next); }
                rows[pos].1=*next-1;
            }
            tree(&mut rows,6,&mut 0);
            let chosen:Vec<_>=rows.into_iter().enumerate().filter(|(i,_)| selected[*i%selected.len()]).map(|(_,r)|r).collect();
            let r=Regions { intervals:intervals(chosen.into_iter()),under:RawPath::new(b"/r".to_vec()).unwrap() };
            let expected=r.intervals.iter().enumerate().filter(|(_,r)|r.start<=point && point<=r.end).max_by_key(|(_,r)|r.start).map(|(i,_)|RegionId(i));
            prop_assert_eq!(r.region_of(point),expected);
        }
    }
}
