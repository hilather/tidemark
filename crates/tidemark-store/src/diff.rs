//! Owned by: W3-A
//! Bounded snapshot differences and deterministic growth explanations.

use crate::{reader::Snapshot, StoreError};
use std::path::Path;
use tidemark_core::path::RawPath;
/// Snapshot counter selected for comparison.
#[derive(Debug, Clone, Copy)]
pub enum DiffMetric {
    /// Allocated disk bytes with distinct-inode semantics.
    Disk,
    /// Bytes freed by removing the subtree, subject to snapshot semantics.
    Reclaimable,
    /// Non-directory entry count.
    Files,
}
/// Presence and direction of a directory change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffClass {
    /// Present on both sides with a positive delta.
    Grown,
    /// Present on both sides with a negative delta.
    Shrunk,
    /// Present only in the new snapshot.
    New,
    /// Present only in the old snapshot.
    Removed,
    /// Present on both sides with unchanged selected metric.
    Same,
}
/// One directory comparison, ordered by absolute delta then raw path bytes.
pub struct DiffRow {
    /// Absolute byte-preserving directory path.
    pub path: RawPath,
    /// Depth relative to the requested target.
    pub depth_rel: u32,
    /// Selected metric in the new snapshot, absent if removed.
    pub new: Option<u64>,
    /// Selected metric in the old snapshot, absent if new.
    pub old: Option<u64>,
    /// Signed new-minus-old change, treating absence as zero.
    pub delta: i64,
    /// Presence and direction of change.
    pub class: DiffClass,
}
/// Compare a target subtree in two immutable snapshots of the same root.
///
/// Missing targets on one side produce new or removed rows. A target absent
/// on both sides is an error. Results and intermediate rows are capped at
/// 1,000,000; exceeding the cap returns an error instead of truncating inputs.
/// Deltas are checked against the signed 64-bit output contract.
///
/// The current reader interface requires bounded subtree queries and a sorted
/// merge; the ATTACH implementation awaits SCR-1. Neither snapshot is written.
pub fn diff(
    new: &Snapshot,
    old: &Path,
    target: &RawPath,
    metric: DiffMetric,
    max_depth_rel: u32,
    limit: usize,
) -> Result<Vec<DiffRow>, StoreError> {
    check_limit(limit)?;
    let old = Snapshot::open(old)?;
    check_roots(new, &old)?;
    let n = new.dir_by_path(target)?;
    let o = old.dir_by_path(target)?;
    if n.is_none() && o.is_none() {
        return Err(StoreError::Invalid(
            "target absent from both snapshots".into(),
        ));
    }
    let mut n = subtree(new, n.as_ref(), metric, max_depth_rel)?
        .into_iter()
        .peekable();
    let mut o = subtree(&old, o.as_ref(), metric, max_depth_rel)?
        .into_iter()
        .peekable();
    let mut rows = Vec::new();
    loop {
        let order = match (n.peek(), o.peek()) {
            (Some(n), Some(o)) => n.path.cmp(&o.path),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };
        if rows.len() == CAP {
            return Err(StoreError::Invalid("diff exceeds row cap".into()));
        }
        match order {
            std::cmp::Ordering::Less => {
                if let Some(n) = n.next() {
                    rows.push(make_row(&n.path, n.depth_rel, Some(n.value), None)?);
                }
            }
            std::cmp::Ordering::Greater => {
                if let Some(o) = o.next() {
                    rows.push(make_row(&o.path, o.depth_rel, None, Some(o.value))?);
                }
            }
            std::cmp::Ordering::Equal => {
                if let (Some(n), Some(o)) = (n.next(), o.next()) {
                    rows.push(make_row(
                        &n.path,
                        n.depth_rel,
                        Some(n.value),
                        Some(o.value),
                    )?);
                }
            }
        }
    }
    rows.sort_by(|a, b| {
        b.delta
            .unsigned_abs()
            .cmp(&a.delta.unsigned_abs())
            .then_with(|| a.path.cmp(&b.path))
    });
    rows.truncate(limit);
    Ok(rows)
}

/// One dominant child selected during iterative descent.
pub struct ExplainStep {
    /// Absolute byte-preserving directory path.
    pub path: RawPath,
    /// Signed new-minus-old change, treating absence as zero.
    pub delta: i64,
    /// Signed child delta divided by signed parent delta.
    pub share_of_parent: f64,
}
/// Growth or shrink path and contributors at the stopping directory.
pub struct Explanation {
    /// Selected metric change at the original target.
    pub target_delta: i64,
    /// Dominant children, ordered from the target downwards.
    pub steps: Vec<ExplainStep>,
    /// Own charged byte change at the stopping directory.
    pub own_delta: i64,
    /// Top child contributors at the stopping directory.
    pub spread: Vec<DiffRow>,
    /// Number of children changing in the target direction.
    pub spread_children: u32,
}
/// Follow the largest same-direction child while it meets ratio and minimum.
///
/// Ratio must be finite and in `(0, 1]`. Ties use raw path bytes; shrinkage
/// mirrors growth. Steps, direct children and returned contributors are capped
/// at 1,000,000. `own_delta` always compares own charged bytes, including when
/// the selected subtree metric is reclaimable bytes or file entries.
pub fn explain(
    new: &Snapshot,
    old: &Path,
    target: &RawPath,
    metric: DiffMetric,
    ratio: f64,
    min_bytes: u64,
    top: usize,
) -> Result<Explanation, StoreError> {
    check_limit(top)?;
    if !ratio.is_finite() || ratio <= 0.0 || ratio > 1.0 {
        return Err(StoreError::Invalid("explain ratio must be in (0,1]".into()));
    }
    let old = Snapshot::open(old)?;
    check_roots(new, &old)?;
    let mut path = target.clone();
    let mut n = new.dir_by_path(&path)?;
    let mut o = old.dir_by_path(&path)?;
    if n.is_none() && o.is_none() {
        return Err(StoreError::Invalid(
            "target absent from both snapshots".into(),
        ));
    }
    let target_delta = delta(
        n.as_ref().map(|r| value(r, metric)),
        o.as_ref().map(|r| value(r, metric)),
    )?;
    let mut current_delta = target_delta;
    let mut steps = Vec::new();
    loop {
        let mut children = std::collections::BTreeMap::new();
        if let Some(r) = &n {
            for c in new.children(r.id, tidemark_core::snapshot::SortKey::Name, CAP + 1)? {
                children.insert(c.path.clone(), (Some(value(&c, metric)), None));
            }
        }
        if let Some(r) = &o {
            for c in old.children(r.id, tidemark_core::snapshot::SortKey::Name, CAP + 1)? {
                if children.len() == CAP && !children.contains_key(&c.path) {
                    return Err(StoreError::Invalid("explain exceeds child cap".into()));
                }
                children.entry(c.path.clone()).or_insert((None, None)).1 = Some(value(&c, metric));
            }
        }
        let mut contributors = Vec::with_capacity(children.len());
        for (p, (nv, ov)) in children {
            contributors.push(make_row(&p, 1, nv, ov)?);
        }
        contributors.sort_by(|a, b| {
            let signed = if current_delta < 0 {
                a.delta.cmp(&b.delta)
            } else {
                b.delta.cmp(&a.delta)
            };
            signed.then_with(|| a.path.cmp(&b.path))
        });
        let dominant = contributors.first().filter(|c| {
            current_delta != 0
                && c.delta.signum() == current_delta.signum()
                && c.delta.unsigned_abs() >= min_bytes
                && c.delta.unsigned_abs() as f64 >= ratio * current_delta.unsigned_abs() as f64
        });
        if let Some(c) = dominant {
            if steps.len() == CAP {
                return Err(StoreError::Invalid("explain exceeds step cap".into()));
            }
            steps.push(ExplainStep {
                path: c.path.clone(),
                delta: c.delta,
                share_of_parent: c.delta as f64 / current_delta as f64,
            });
            path = c.path.clone();
            current_delta = c.delta;
            n = new.dir_by_path(&path)?;
            o = old.dir_by_path(&path)?;
        } else {
            let spread_children = u32::try_from(
                contributors
                    .iter()
                    .filter(|c| c.delta.signum() == current_delta.signum() && c.delta != 0)
                    .count(),
            )
            .map_err(|_| StoreError::Integer("child count".into()))?;
            contributors.truncate(top);
            return Ok(Explanation {
                target_delta,
                steps,
                own_delta: delta(
                    n.as_ref().map(|r| r.own_charged),
                    o.as_ref().map(|r| r.own_charged),
                )?,
                spread: contributors,
                spread_children,
            });
        }
    }
}

// Matches the existing reader result cap. No traversal recursion is used.
const CAP: usize = 1_000_000;
fn check_limit(limit: usize) -> Result<(), StoreError> {
    if limit > CAP {
        return Err(StoreError::Invalid("limit exceeds 1,000,000".into()));
    }
    Ok(())
}
fn check_roots(new: &Snapshot, old: &Snapshot) -> Result<(), StoreError> {
    if new.meta().root != old.meta().root {
        return Err(StoreError::Invalid("snapshot roots differ".into()));
    }
    Ok(())
}
fn value(row: &tidemark_core::snapshot::DirRow, metric: DiffMetric) -> u64 {
    match metric {
        DiffMetric::Disk => row.t_disk,
        DiffMetric::Reclaimable => row.t_reclaimable,
        DiffMetric::Files => row.t_files,
    }
}
fn delta(new: Option<u64>, old: Option<u64>) -> Result<i64, StoreError> {
    i64::try_from(i128::from(new.unwrap_or(0)) - i128::from(old.unwrap_or(0)))
        .map_err(|_| StoreError::Integer("diff delta".into()))
}
fn make_row(
    path: &RawPath,
    depth_rel: u32,
    new: Option<u64>,
    old: Option<u64>,
) -> Result<DiffRow, StoreError> {
    let delta = delta(new, old)?;
    let class = match (new, old) {
        (Some(_), None) => DiffClass::New,
        (None, Some(_)) => DiffClass::Removed,
        _ if delta > 0 => DiffClass::Grown,
        _ if delta < 0 => DiffClass::Shrunk,
        _ => DiffClass::Same,
    };
    Ok(DiffRow {
        path: path.clone(),
        depth_rel,
        new,
        old,
        delta,
        class,
    })
}

struct SideRow {
    path: RawPath,
    depth_rel: u32,
    value: u64,
}
fn subtree(
    snapshot: &Snapshot,
    root: Option<&tidemark_core::snapshot::DirRow>,
    metric: DiffMetric,
    depth: u32,
) -> Result<Vec<SideRow>, StoreError> {
    let Some(root) = root else {
        return Ok(Vec::new());
    };
    let mut rows = snapshot
        .subtree_dirs(
            root.id,
            depth,
            tidemark_core::snapshot::SortKey::Name,
            CAP + 1,
        )?
        .into_iter()
        .map(|row| {
            let value = value(&row, metric);
            let depth_rel = row
                .depth
                .checked_sub(root.depth)
                .ok_or_else(|| StoreError::Invalid("subtree depth precedes target".into()))?;
            Ok(SideRow {
                path: row.path,
                depth_rel,
                value,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    rows.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(rows)
}
