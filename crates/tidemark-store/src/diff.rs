//! Owned by: W3-A
//! Bounded snapshot differences and deterministic growth explanations.
//! Walks both trees by name on the public `Snapshot` API; pure-v1 pairs keep the ATTACH join.

use crate::{reader::Snapshot, StoreError};
use std::{cmp::Ordering, collections::BinaryHeap, path::Path};
use tidemark_core::{
    path::RawPath,
    snapshot::{DirRow, SortKey},
};
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
/// on both sides is an error. Both trees are walked together by name bytes
/// through the public [`Snapshot`] API; only a bounded top-`limit` heap is
/// kept. Output limits remain bounded at 1,000,000. Neither snapshot is written.
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
    check_target(&n, &o)?;
    ranked(
        new,
        &old,
        n.as_ref(),
        o.as_ref(),
        metric,
        max_depth_rel,
        limit,
        None,
    )
    .map(|(rows, _)| rows)
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
/// mirrors growth. Steps and returned contributors are capped at 1,000,000;
/// direct children are merged by name without a child-count cap. `own_delta` always compares own charged bytes, including when
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
        // Keep the dominant candidate even when no spread rows were requested.
        let (mut contributors, spread_children) = ranked(
            new,
            &old,
            n.as_ref(),
            o.as_ref(),
            metric,
            1,
            top.max(1),
            Some(current_delta),
        )?;
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

fn check_target(
    n: &Option<tidemark_core::snapshot::DirRow>,
    o: &Option<tidemark_core::snapshot::DirRow>,
) -> Result<(), StoreError> {
    if n.is_none() && o.is_none() {
        return Err(StoreError::Invalid(
            "target absent from both snapshots".into(),
        ));
    }
    Ok(())
}

/// Rank rows for `diff` (`direction = None`, whole subtree) or `explain`
/// (`Some(delta)`, direct children). Pure-v1 pairs keep the ATTACH join, which is
/// about 6x faster than the walk there (see the handoff); everything else walks.
#[allow(clippy::too_many_arguments)]
fn ranked(
    new: &Snapshot,
    old: &Snapshot,
    n: Option<&DirRow>,
    o: Option<&DirRow>,
    metric: DiffMetric,
    depth: u32,
    limit: usize,
    direction: Option<i64>,
) -> Result<(Vec<DiffRow>, u32), StoreError> {
    if new.v1_conn().is_ok() && old.v1_conn().is_ok() {
        return with_old(new, old, || {
            joined(new, n, o, metric, depth, limit, direction)
        });
    }
    let mut top = TopN::new(limit, direction.map_or(Order::AbsDelta, Order::Directed));
    walk(new, old, n, o, metric, depth, direction.is_none(), &mut top)?;
    let count = top.count;
    Ok((top.finish()?, count))
}

// ---- v1/v1 fast path: the previous ATTACH + join implementation, unchanged. ----
fn with_old<T>(
    new: &Snapshot,
    old: &Snapshot,
    query: impl FnOnce() -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file:");
    // Absolute paths avoid SQLite URI authority/relative-path ambiguity.
    let file = std::fs::canonicalize(old.backing_file())?;
    for &byte in file.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/.-_".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push('%');
            uri.push_str(&crate::writer::hex(&[byte]));
        }
    }
    uri.push_str("?immutable=1&mode=ro");
    let mmap: i64 = new
        .v1_conn()
        .and_then(|_| old.v1_conn())
        .and_then(|_| new.v1_conn())?
        .pragma_query_value(Some("main"), "mmap_size", |r| r.get(0))?;
    new.v1_conn()?.execute("ATTACH DATABASE ?1 AS o", [uri])?;
    // All statements/cursors are dropped by the closure before detachment,
    // including on query/conversion errors. Drop also covers unwinding.
    struct Attachment<'a> {
        conn: &'a rusqlite::Connection,
        mmap: i64,
        attached: bool,
        restore: bool,
    }
    impl Drop for Attachment<'_> {
        fn drop(&mut self) {
            if self.attached {
                if let Err(error) = self.conn.execute("DETACH DATABASE o", []) {
                    tracing::warn!(%error, "could not detach old snapshot");
                }
            }
            if self.restore {
                if let Err(error) = self
                    .conn
                    .pragma_update(Some("main"), "mmap_size", self.mmap)
                {
                    tracing::warn!(%error, "could not restore snapshot mapping limit");
                }
            }
        }
    }
    let mut attachment = Attachment {
        conn: new.v1_conn()?,
        mmap,
        attached: true,
        restore: true,
    };
    // Random hash-index probes otherwise churn SQLite's small page cache.
    // Bound read-only mappings at 1 GiB per database (2 GiB address space total),
    // falling back to ordinary page reads above that range or without mmap.
    // These connection-local PRAGMAs never modify either snapshot. SQLite does
    // not support bind parameters for PRAGMA settings; use its typed API with
    // a fixed constant and the previously read integer, never caller input.
    const MMAP_BYTES: i64 = 1 << 30;
    new.v1_conn()?
        .pragma_update(Some("main"), "mmap_size", MMAP_BYTES)?;
    new.v1_conn()?
        .pragma_update(Some("o"), "mmap_size", MMAP_BYTES)?;
    let result = query();
    let detached = new.v1_conn()?.execute("DETACH DATABASE o", []);
    attachment.attached = detached.is_err();
    let restored = new
        .v1_conn()?
        .pragma_update(Some("main"), "mmap_size", mmap);
    attachment.restore = restored.is_err();
    let result = result?;
    detached?;
    restored?;
    Ok(result)
}

// The only interpolated SQL fragments are closed internal choices. Every
// path/range/depth/limit/direction is bound. Both branches match by hash AND
// full byte path; different preorder ids between snapshots are irrelevant.
#[allow(clippy::too_many_arguments)]
fn joined(
    new: &Snapshot,
    n: Option<&tidemark_core::snapshot::DirRow>,
    o: Option<&tidemark_core::snapshot::DirRow>,
    metric: DiffMetric,
    depth: u32,
    limit: usize,
    direction: Option<i64>,
) -> Result<(Vec<DiffRow>, u32), StoreError> {
    let (sql, aggregate) = joined_sql(metric, direction.is_some());
    // COUNT streams the union with constant aggregate state. In particular, no
    // window or materialized CTE retains all children under temp_store=MEMORY.
    // Target totals already come from dir_by_path. A child SUM cannot replace
    // them: own files and hard-link LCA charges also belong to the target.
    let count = if let Some(sql) = aggregate {
        let mut statement = new.v1_conn()?.prepare(&sql)?;
        bind_joined(&mut statement, n, o, depth, limit, direction)?;
        let count = statement
            .raw_query()
            .next()?
            .ok_or_else(|| StoreError::Invalid("missing contributor count".into()))?
            .get::<_, u32>(0)?;
        count
    } else {
        0
    };
    let mut statement = new.v1_conn()?.prepare(&sql)?;
    bind_joined(&mut statement, n, o, depth, limit, direction)?;
    let mut cursor = statement.raw_query();
    let mut rows = Vec::new();
    while let Some(row) = cursor.next()? {
        let path = RawPath::new(row.get::<_, Vec<u8>>(0)?)?;
        let depth_rel = row.get::<_, u32>(1)?;
        let nv = crate::error::read_optional_u64(row, 2)?;
        let ov = crate::error::read_optional_u64(row, 3)?;
        rows.push(make_row(&path, depth_rel, nv, ov)?);
    }
    Ok((rows, count))
}

fn joined_sql(metric: DiffMetric, children: bool) -> (String, Option<String>) {
    let column = match metric {
        DiffMetric::Disk => "t_disk",
        DiffMetric::Reclaimable => "t_reclaimable",
        DiffMetric::Files => "t_files",
    };
    let (n_filter, o_filter, order) = if children {
        (
            "n.parent_id = :n_lo",
            "o2.parent_id = :o_lo",
            "delta * :sign DESC, path",
        )
    } else {
        (
            "n.id BETWEEN :n_lo AND :n_hi AND n.depth <= :n_depth",
            "o2.id BETWEEN :o_lo AND :o_hi AND o2.depth <= :o_depth",
            "ABS(delta) DESC, path",
        )
    };
    let union = format!(
        "SELECT n.path, n.depth - :n_base AS depth_rel,
                n.{column} AS new_value, o2.{column} AS old_value,
                n.{column} - COALESCE(o2.{column}, 0) AS delta
         FROM main.dirs n LEFT JOIN o.dirs o2
           ON o2.path_hash = n.path_hash AND o2.path = n.path
         WHERE {n_filter}
         UNION ALL
         SELECT o2.path, o2.depth - :o_base, NULL, o2.{column}, -o2.{column}
         FROM o.dirs o2 WHERE {o_filter}
           AND NOT EXISTS (SELECT 1 FROM main.dirs n
                           WHERE n.path_hash = o2.path_hash AND n.path = o2.path)
         "
    );
    let aggregate =
        children.then(|| format!("SELECT COUNT(*) FROM ({union}) WHERE delta * :count_sign > 0"));
    (format!("SELECT path, depth_rel, new_value, old_value FROM ({union}) ORDER BY {order} LIMIT :limit"), aggregate)
}

#[allow(clippy::too_many_arguments)]
fn bind_joined(
    statement: &mut rusqlite::Statement<'_>,
    n: Option<&tidemark_core::snapshot::DirRow>,
    o: Option<&tidemark_core::snapshot::DirRow>,
    depth: u32,
    limit: usize,
    direction: Option<i64>,
) -> Result<(), StoreError> {
    // Bind only parameters present in the chosen query variant.
    let mut bind = |name: &str, value: i64| -> Result<(), StoreError> {
        if let Some(index) = statement.parameter_index(name)? {
            statement.raw_bind_parameter(index, value)?;
        }
        Ok(())
    };
    for (prefix, root) in [("n", n), ("o", o)] {
        bind(
            &format!(":{prefix}_lo"),
            root.map_or(-1, |r| i64::from(r.id)),
        )?;
        bind(
            &format!(":{prefix}_hi"),
            root.map_or(-1, |r| i64::from(r.subtree_end)),
        )?;
        let base = root.map_or(0, |r| i64::from(r.depth));
        bind(&format!(":{prefix}_base"), base)?;
        bind(&format!(":{prefix}_depth"), base + i64::from(depth))?;
    }
    bind(":limit", crate::error::sql_limit(limit)?)?;
    bind(
        ":sign",
        if direction.is_some_and(|d| d < 0) {
            -1
        } else {
            1
        },
    )?;
    bind(":count_sign", direction.map_or(0, i64::signum))?;
    Ok(())
}

/// Sort order of ranked rows; ties always break by ascending raw path bytes.
#[derive(Clone, Copy)]
enum Order {
    /// Largest absolute delta first.
    AbsDelta,
    /// Largest `delta * sign(direction)` first; also counts rows with that sign.
    Directed(i64),
}

struct Cand {
    key: i128,
    path: RawPath,
    depth_rel: u32,
    new: Option<u64>,
    old: Option<u64>,
}
// "Greater" means worse, so the max-heap root is the first row to evict.
impl Ord for Cand {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .key
            .cmp(&self.key)
            .then_with(|| self.path.as_bytes().cmp(other.path.as_bytes()))
    }
}
impl PartialOrd for Cand {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Cand {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Cand {}

/// Bounded top-N selection: memory is O(limit).
struct TopN {
    limit: usize,
    order: Order,
    heap: BinaryHeap<Cand>,
    /// Rows changing in the direction of `Order::Directed`.
    count: u32,
}
impl TopN {
    fn new(limit: usize, order: Order) -> Self {
        Self {
            limit,
            order,
            heap: BinaryHeap::new(),
            count: 0,
        }
    }
    fn offer(&mut self, path: &RawPath, depth_rel: u32, new: Option<u64>, old: Option<u64>) {
        let delta = i128::from(new.unwrap_or(0)) - i128::from(old.unwrap_or(0));
        let key = match self.order {
            Order::AbsDelta => delta.abs(),
            Order::Directed(direction) => {
                let sign = if direction < 0 { -1 } else { 1 };
                if direction != 0 && delta * i128::from(direction.signum()) > 0 {
                    self.count = self.count.saturating_add(1);
                }
                delta * sign
            }
        };
        if self.limit == 0 {
            return;
        }
        if self.heap.len() >= self.limit {
            if let Some(worst) = self.heap.peek() {
                if key < worst.key || (key == worst.key && path.as_bytes() >= worst.path.as_bytes())
                {
                    return;
                }
            }
            self.heap.pop();
        }
        self.heap.push(Cand {
            key,
            path: path.clone(),
            depth_rel,
            new,
            old,
        });
    }
    fn finish(self) -> Result<Vec<DiffRow>, StoreError> {
        self.heap
            .into_sorted_vec()
            .into_iter()
            .map(|c| make_row(&c.path, c.depth_rel, c.new, c.old))
            .collect()
    }
}

const PAGE_START: usize = 256;
const PAGE_MAX: usize = 16_384;

/// Name-ordered children of one directory, read page by page.
struct Kids<'a> {
    snap: &'a Snapshot,
    parent: Option<u32>,
    buf: std::vec::IntoIter<DirRow>,
    head: Option<DirRow>,
    last: Option<DirRow>,
    page: usize,
    done: bool,
}
impl<'a> Kids<'a> {
    /// Leaf directories (`subtree_end == id`) need no child query.
    fn new(snap: &'a Snapshot, parent: Option<&DirRow>) -> Self {
        let has_kids = parent.is_some_and(|r| r.subtree_end > r.id);
        let parent = parent.map(|r| r.id);
        Self {
            snap,
            parent,
            buf: Vec::new().into_iter(),
            head: None,
            last: None,
            page: PAGE_START,
            done: !has_kids,
        }
    }
    fn peek(&mut self) -> Result<Option<&DirRow>, StoreError> {
        if self.head.is_none() {
            if self.buf.len() == 0 && !self.done {
                let id = self.parent.unwrap_or(0);
                let rows = self.snap.children_page(
                    id,
                    SortKey::Name,
                    false,
                    self.last.as_ref(),
                    false,
                    self.page,
                    &[],
                )?;
                if rows.len() < self.page {
                    self.done = true;
                }
                self.last = rows.last().cloned();
                self.page = (self.page * 2).min(PAGE_MAX);
                self.buf = rows.into_iter();
            }
            self.head = self.buf.next();
        }
        Ok(self.head.as_ref())
    }
    fn take(&mut self) -> Option<DirRow> {
        self.head.take()
    }
}

struct Frame<'a> {
    n: Kids<'a>,
    o: Kids<'a>,
    depth_rel: u32,
}

/// Walk both subtrees together and offer every directory within
/// `max_depth_rel` to `top`. Iterative: state is one frame per level.
#[allow(clippy::too_many_arguments)]
fn walk(
    new: &Snapshot,
    old: &Snapshot,
    n: Option<&DirRow>,
    o: Option<&DirRow>,
    metric: DiffMetric,
    max_depth_rel: u32,
    include_root: bool,
    top: &mut TopN,
) -> Result<(), StoreError> {
    if include_root {
        if let Some(path) = n.or(o).map(|r| &r.path) {
            top.offer(
                path,
                0,
                n.map(|r| value(r, metric)),
                o.map(|r| value(r, metric)),
            );
        }
    }
    if max_depth_rel == 0 || (n.is_none() && o.is_none()) {
        return Ok(());
    }
    let mut stack = vec![Frame {
        n: Kids::new(new, n),
        o: Kids::new(old, o),
        depth_rel: 0,
    }];
    while let Some(frame) = stack.last_mut() {
        let ord = match (frame.n.peek()?, frame.o.peek()?) {
            (None, None) => {
                stack.pop();
                continue;
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.name.cmp(&b.name),
        };
        let cn = if ord != Ordering::Greater {
            frame.n.take()
        } else {
            None
        };
        let co = if ord != Ordering::Less {
            frame.o.take()
        } else {
            None
        };
        let depth_rel = frame.depth_rel + 1;
        let Some(path) = cn.as_ref().or(co.as_ref()).map(|r| r.path.clone()) else {
            continue;
        };
        top.offer(
            &path,
            depth_rel,
            cn.as_ref().map(|r| value(r, metric)),
            co.as_ref().map(|r| value(r, metric)),
        );
        if depth_rel < max_depth_rel {
            stack.push(Frame {
                n: Kids::new(new, cn.as_ref()),
                o: Kids::new(old, co.as_ref()),
                depth_rel,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> RawPath {
        RawPath::new(s.as_bytes().to_vec()).unwrap()
    }

    #[test]
    fn top_n_keeps_best_by_key_then_path_bytes() {
        let mut top = TopN::new(3, Order::AbsDelta);
        for (path, new, old) in [
            ("/r/z", 5, 0),
            ("/r/a-c", 5, 0),
            ("/r/a/x", 5, 0),
            ("/r/m", 0, 9),
            ("/r/q", 1, 1),
            ("/r/b", 6, 0),
        ] {
            top.offer(&p(path), 1, Some(new), Some(old));
        }
        let paths: Vec<_> = top
            .finish()
            .unwrap()
            .iter()
            .map(|r| r.path.display_lossy())
            .collect();
        assert_eq!(paths, ["/r/m", "/r/b", "/r/a-c"]);
    }

    #[test]
    fn directed_order_counts_only_the_target_direction() {
        let mut top = TopN::new(2, Order::Directed(-4));
        top.offer(&p("/r/a"), 1, Some(1), Some(5)); // -4
        top.offer(&p("/r/b"), 1, Some(9), Some(5)); // +4
        top.offer(&p("/r/c"), 1, Some(5), Some(5));
        top.offer(&p("/r/d"), 1, None, Some(2)); // -2
        assert_eq!(top.count, 2);
        let rows = top.finish().unwrap();
        assert_eq!(rows[0].delta, -4);
        assert_eq!(rows[1].delta, -2);
    }

    #[test]
    fn zero_limit_keeps_nothing() {
        let mut top = TopN::new(0, Order::AbsDelta);
        top.offer(&p("/r/a"), 0, Some(1), None);
        assert!(top.finish().unwrap().is_empty());
    }
}
