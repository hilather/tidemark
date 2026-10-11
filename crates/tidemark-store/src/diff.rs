//! Owned by: W3-A
//! Compiling cross-task interfaces; runtime behavior belongs to W3-A.

use crate::{reader::Snapshot, StoreError};
use std::path::Path;
use tidemark_core::path::RawPath;
/// Cross-task contract reserved for the owning task.
pub enum DiffMetric {
    #[doc = "Contract variant."]
    Disk,
    #[doc = "Contract variant."]
    Reclaimable,
    #[doc = "Contract variant."]
    Files,
}
/// Cross-task contract reserved for the owning task.
pub enum DiffClass {
    #[doc = "Contract variant."]
    Grown,
    #[doc = "Contract variant."]
    Shrunk,
    #[doc = "Contract variant."]
    New,
    #[doc = "Contract variant."]
    Removed,
    #[doc = "Contract variant."]
    Same,
}
/// Cross-task contract reserved for the owning task.
pub struct DiffRow {
    #[doc = "Contract field."]
    pub path: RawPath,
    #[doc = "Contract field."]
    pub depth_rel: u32,
    #[doc = "Contract field."]
    pub new: Option<u64>,
    #[doc = "Contract field."]
    pub old: Option<u64>,
    #[doc = "Contract field."]
    pub delta: i64,
    #[doc = "Contract field."]
    pub class: DiffClass,
}
/// Cross-task contract reserved for the owning task.
pub fn diff(
    new: &Snapshot,
    old: &Path,
    target: &RawPath,
    metric: DiffMetric,
    max_depth_rel: u32,
    limit: usize,
) -> Result<Vec<DiffRow>, StoreError> {
    let _ = new;
    let _ = old;
    let _ = target;
    let _ = metric;
    let _ = max_depth_rel;
    let _ = limit;
    Err(StoreError::NotYetImplemented)
}

/// Cross-task contract reserved for the owning task.
pub struct ExplainStep {
    #[doc = "Contract field."]
    pub path: RawPath,
    #[doc = "Contract field."]
    pub delta: i64,
    #[doc = "Contract field."]
    pub share_of_parent: f64,
}
/// Cross-task contract reserved for the owning task.
pub struct Explanation {
    #[doc = "Contract field."]
    pub target_delta: i64,
    #[doc = "Contract field."]
    pub steps: Vec<ExplainStep>,
    #[doc = "Contract field."]
    pub own_delta: i64,
    #[doc = "Contract field."]
    pub spread: Vec<DiffRow>,
    #[doc = "Contract field."]
    pub spread_children: u32,
}
/// Cross-task contract reserved for the owning task.
pub fn explain(
    new: &Snapshot,
    old: &Path,
    target: &RawPath,
    metric: DiffMetric,
    ratio: f64,
    min_bytes: u64,
    top: usize,
) -> Result<Explanation, StoreError> {
    let _ = new;
    let _ = old;
    let _ = target;
    let _ = metric;
    let _ = ratio;
    let _ = min_bytes;
    let _ = top;
    Err(StoreError::NotYetImplemented)
}
