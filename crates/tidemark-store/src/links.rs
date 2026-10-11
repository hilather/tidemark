//! Owned by: W3-B
//! Compiling cross-task interfaces; runtime behavior belongs to W3-B.

use crate::{reader::Snapshot, StoreError};
use tidemark_core::path::RawPath;
use tidemark_core::snapshot::LinkSetHit;

/// Cross-task contract reserved for the owning task.
pub enum RegionSpec {
    #[doc = "Contract variant."]
    Globs(Vec<String>),
    #[doc = "Contract variant."]
    Depth(u32),
}
/// Cross-task contract reserved for the owning task.
pub struct Regions {
    placeholder: RawPath,
}
impl Regions {
    /// Cross-task contract reserved for the owning task.
    pub fn build(
        snap: &Snapshot,
        under: &RawPath,
        spec: &RegionSpec,
    ) -> Result<Regions, StoreError> {
        let _ = snap;
        let _ = under;
        let _ = spec;
        Err(StoreError::NotYetImplemented)
    }
    /// Cross-task contract reserved for the owning task.
    pub fn region_of(&self, dir_id: u32) -> Option<RegionId> {
        let _ = dir_id;
        None
    }
    /// Cross-task contract reserved for the owning task.
    pub fn name(&self, r: RegionId) -> &RawPath {
        let _ = r;
        &self.placeholder
    }
}
/// Cross-task contract reserved for the owning task.
pub struct CrossingSet {
    #[doc = "Contract field."]
    pub ino: u64,
    #[doc = "Contract field."]
    pub disk: u64,
    #[doc = "Contract field."]
    pub nlink: u32,
    #[doc = "Contract field."]
    pub found: u32,
    #[doc = "Contract field."]
    pub regions: Vec<Option<RegionId>>,
    #[doc = "Contract field."]
    pub sample_dirs: Vec<u32>,
}
/// Cross-task contract reserved for the owning task.
pub fn crossing_sets(
    snap: &Snapshot,
    regions: &Regions,
    limit: usize,
) -> Result<(Vec<CrossingSet>, CrossingTotals), StoreError> {
    let _ = snap;
    let _ = regions;
    let _ = limit;
    Err(StoreError::NotYetImplemented)
}
/// Cross-task contract reserved for the owning task.
pub struct LinkReport {
    #[doc = "Contract field."]
    pub totals: LinkTotals,
    #[doc = "Contract field."]
    pub crossing: Vec<CrossingSet>,
    #[doc = "Contract field."]
    pub region_pairs: Vec<RegionPair>,
    #[doc = "Contract field."]
    pub incomplete_top_dirs: Vec<(RawPath, u64 /*pinned bytes*/, u64 /*sets*/)>,
    #[doc = "Contract field."]
    pub largest_sets: Vec<LinkSetHit>,
    #[doc = "Contract field."]
    pub most_shared_dirs: Vec<(RawPath, u64 /*shared*/, u64 /*t_disk*/)>,
}
/// Cross-task contract reserved for the owning task.
pub fn link_report(
    snap: &Snapshot,
    under: &RawPath,
    spec: Option<&RegionSpec>,
    top: usize,
) -> Result<LinkReport, StoreError> {
    let _ = snap;
    let _ = under;
    let _ = spec;
    let _ = top;
    Err(StoreError::NotYetImplemented)
}

/// Cross-task contract reserved for the owning task.
pub struct RegionId;
/// Cross-task contract reserved for the owning task.
pub struct CrossingTotals;
/// Cross-task contract reserved for the owning task.
pub struct LinkTotals;
/// Cross-task contract reserved for the owning task.
pub struct RegionPair;
