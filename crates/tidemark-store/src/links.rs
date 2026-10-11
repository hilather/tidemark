//! Owned by: W3-B
//! Bounded hard-link topology queries and laminar preorder regions.

use crate::{reader::Snapshot, StoreError};
use std::collections::{BTreeMap, BTreeSet};
use tidemark_core::{
    path::RawPath,
    snapshot::{LinkFilter, LinkSetHit, SortKey},
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
    filter: LinkFilter,
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
        let mut rows = snap.subtree_dirs(root.id, u32::MAX, SortKey::Name, CAP + 1)?;
        rows.sort_unstable_by_key(|r| r.id);
        let selected = rows
            .into_iter()
            .filter(|r| match spec {
                RegionSpec::Depth(d) => u64::from(r.depth) == u64::from(root.depth) + u64::from(*d),
                RegionSpec::Globs(_) => {
                    globs.is_match_candidate(&globset::Candidate::from_bytes(r.path.as_bytes()))
                }
            })
            .map(|r| (r.id, r.subtree_end, r.path));
        Ok(Self {
            intervals: intervals(selected),
            under: under.clone(),
            filter,
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
/// Find crossings, using the reader's byte-boundary selection contract.
/// Result and intermediate row vectors are bounded by the reader's million-row cap.
/// The pending SCR-001 requests a streaming reader hook to remove these vectors.
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
    let hits = snap.link_sets_under(root.id, &regions.filter, CAP + 1)?;
    let mut totals = CrossingTotals::default();
    let mut out = Vec::new();
    let mut sample_count = 0usize;
    for hit in hits {
        let mut samples = BTreeMap::new();
        for instance in snap.link_instances(hit.row.ino)? {
            samples
                .entry(regions.region_of(instance.dir_id))
                .or_insert(instance.dir_id);
        }
        if samples.len() < 2 {
            continue;
        }
        totals.sets += 1;
        totals.disk = add(totals.disk, hit.row.disk)?;
        if hit.row.found < hit.row.nlink {
            totals.incomplete_sets += 1;
            totals.incomplete_disk = add(totals.incomplete_disk, hit.row.disk)?;
        }
        if out.len() < limit {
            sample_count += samples.len();
            if sample_count > CAP {
                return Err(StoreError::Invalid("crossing sample cap reached".into()));
            }
            let (rs, ds) = samples.into_iter().unzip();
            out.push(CrossingSet {
                ino: hit.row.ino,
                disk: hit.row.disk,
                nlink: hit.row.nlink,
                found: hit.row.found,
                regions: rs,
                sample_dirs: ds,
            });
        }
    }
    Ok((out, totals))
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
/// Build a report; all link instances are fetched one set at a time, never globally.
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
    let hits = snap.link_sets_under(root.id, &LinkFilter::default(), CAP + 1)?;
    let regions = spec.map(|s| Regions::build(snap, under, s)).transpose()?;
    let mut totals = LinkTotals::default();
    let mut pinned: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
    let mut pairs: BTreeMap<(Option<RegionId>, Option<RegionId>), (u64, u64)> = BTreeMap::new();
    let mut crossing = Vec::new();
    let mut sample_count = 0usize;
    for hit in &hits {
        totals.sets += 1;
        totals.instances = add(totals.instances, u64::from(hit.row.found))?;
        totals.disk = add(totals.disk, hit.row.disk)?;
        let incomplete = hit.row.found < hit.row.nlink;
        if incomplete {
            totals.incomplete_sets += 1;
            totals.incomplete_disk = add(totals.incomplete_disk, hit.row.disk)?;
        }
        let mut visited = BTreeSet::new();
        let mut samples = BTreeMap::new();
        for i in snap.link_instances(hit.row.ino)? {
            if incomplete {
                let mut cursor = Some(i.dir_id);
                while let Some(id) = cursor {
                    if id < root.id || id > root.subtree_end || !visited.insert(id) {
                        break;
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
                samples.entry(r.region_of(i.dir_id)).or_insert(i.dir_id);
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
            if crossing.len() < top {
                sample_count += samples.len();
                if sample_count > CAP {
                    return Err(StoreError::Invalid("crossing sample cap reached".into()));
                }
                let (rs, ds) = samples.into_iter().unzip();
                crossing.push(CrossingSet {
                    ino: hit.row.ino,
                    disk: hit.row.disk,
                    nlink: hit.row.nlink,
                    found: hit.row.found,
                    regions: rs,
                    sample_dirs: ds,
                });
            }
        }
    }
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
    let mut most_shared_dirs: Vec<_> = snap
        .subtree_dirs(root.id, u32::MAX, SortKey::Name, CAP + 1)?
        .into_iter()
        .map(|d| (d.path, d.t_disk.saturating_sub(d.t_reclaimable), d.t_disk))
        .filter(|d| d.1 > 0)
        .collect();
    most_shared_dirs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    most_shared_dirs.truncate(top);
    let largest_sets = hits.into_iter().take(top).collect();
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
            let r=Regions { intervals:intervals(chosen.into_iter()),under:RawPath::new(b"/r".to_vec()).unwrap(),filter:LinkFilter::default() };
            let expected=r.intervals.iter().enumerate().filter(|(_,r)|r.start<=point && point<=r.end).max_by_key(|(_,r)|r.start).map(|(i,_)|RegionId(i));
            prop_assert_eq!(r.region_of(point),expected);
        }
    }
}
