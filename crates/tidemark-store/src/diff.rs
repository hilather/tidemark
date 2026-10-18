//! Owned by: W3-A
//! Bounded snapshot differences and deterministic growth explanations.
//! Walks both trees by name on the public `Snapshot` API; pure-v1 pairs keep the ATTACH join.

use crate::{
    reader::{ChildCursor, ChildEntry, Snapshot},
    StoreError,
};
use std::{cmp::Ordering, collections::BinaryHeap, path::Path};
use tidemark_core::{path::RawPath, snapshot::DirRow};
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
    /// Offer one directory; a `RawPath` is built only if the row enters the heap.
    fn offer(
        &mut self,
        path: &[u8],
        depth_rel: u32,
        new: Option<u64>,
        old: Option<u64>,
    ) -> Result<(), StoreError> {
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
            return Ok(());
        }
        if self.heap.len() >= self.limit {
            if let Some(worst) = self.heap.peek() {
                if key < worst.key || (key == worst.key && path >= worst.path.as_bytes()) {
                    return Ok(());
                }
            }
            self.heap.pop();
        }
        self.heap.push(Cand {
            key,
            path: RawPath::new(path)?,
            depth_rel,
            new,
            old,
        });
        Ok(())
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

/// Name-ordered children of one directory on one side, read page by page.
/// Holds compact entries and a cursor only: no paths, no `DirRow`s.
struct Kids<'a> {
    snap: &'a Snapshot,
    parent: u32,
    buf: Vec<ChildEntry>,
    pos: usize,
    cursor: Option<ChildCursor>,
    more: bool,
    page: usize,
}
impl<'a> Kids<'a> {
    /// Leaf directories (`subtree_end == id`) and absent sides need no child query.
    fn new(snap: &'a Snapshot, parent: Option<(u32, u32)>) -> Self {
        let (id, end) = parent.unwrap_or((0, 0));
        Self {
            snap,
            parent: id,
            buf: Vec::new(),
            pos: 0,
            cursor: None,
            more: parent.is_some() && end > id,
            page: PAGE_START,
        }
    }
    fn head(&mut self) -> Result<Option<&ChildEntry>, StoreError> {
        while self.pos == self.buf.len() && self.more {
            self.buf.clear();
            self.pos = 0;
            self.cursor = self.snap.children_by_name(
                self.parent,
                self.cursor.as_ref(),
                self.page,
                &mut self.buf,
            )?;
            self.more = self.cursor.is_some();
            #[cfg(test)]
            FETCHED.with(|c| c.set(c.get() + self.buf.len() as u64));
            self.page = (self.page * 2).min(PAGE_MAX);
        }
        Ok(self.buf.get(self.pos))
    }
    /// Metric value, preorder range of the head entry, then advance past it.
    fn take(&mut self, metric: DiffMetric) -> Option<(u64, u32, u32)> {
        let e = self.buf.get(self.pos)?;
        let out = (child_value(e, metric), e.id, e.subtree_end);
        self.pos += 1;
        Some(out)
    }
}
#[cfg(test)]
thread_local! {
    /// Child entries fetched through `children_by_name` (sibling visits).
    static FETCHED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn child_value(e: &ChildEntry, metric: DiffMetric) -> u64 {
    match metric {
        DiffMetric::Disk => e.t_disk,
        DiffMetric::Reclaimable => e.t_reclaimable,
        DiffMetric::Files => e.t_files,
    }
}

struct Frame<'a> {
    n: Kids<'a>,
    o: Kids<'a>,
    depth_rel: u32,
    /// Length of the shared path buffer at this frame's directory.
    path_len: usize,
}

/// Walk both subtrees together and offer every directory within
/// `max_depth_rel` to `top`. Iterative; memory is one frame (two pages) per
/// level plus one shared path buffer, and no path is copied except into `top`.
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
    let Some(root) = n.or(o) else {
        return Ok(());
    };
    let mut path: Vec<u8> = root.path.as_bytes().to_vec();
    if include_root {
        top.offer(
            &path,
            0,
            n.map(|r| value(r, metric)),
            o.map(|r| value(r, metric)),
        )?;
    }
    if max_depth_rel == 0 {
        return Ok(());
    }
    let mut stack = vec![Frame {
        n: Kids::new(new, n.map(|r| (r.id, r.subtree_end))),
        o: Kids::new(old, o.map(|r| (r.id, r.subtree_end))),
        depth_rel: 0,
        path_len: path.len(),
    }];
    while let Some(frame) = stack.last_mut() {
        let ord = match (frame.n.head()?, frame.o.head()?) {
            (None, None) => {
                stack.pop();
                continue;
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.name.cmp(&b.name),
        };
        // Name of the child being visited goes onto the shared path buffer.
        path.truncate(frame.path_len);
        if path.len() > 1 {
            path.push(b'/');
        }
        let name_source = if ord == Ordering::Greater {
            &frame.o
        } else {
            &frame.n
        };
        if let Some(e) = name_source.buf.get(name_source.pos) {
            path.extend_from_slice(&e.name);
        }
        let cn = if ord != Ordering::Greater {
            frame.n.take(metric)
        } else {
            None
        };
        let co = if ord != Ordering::Less {
            frame.o.take(metric)
        } else {
            None
        };
        let depth_rel = frame.depth_rel + 1;
        top.offer(&path, depth_rel, cn.map(|c| c.0), co.map(|c| c.0))?;
        if depth_rel < max_depth_rel {
            stack.push(Frame {
                n: Kids::new(new, cn.map(|c| (c.1, c.2))),
                o: Kids::new(old, co.map(|c| (c.1, c.2))),
                depth_rel,
                path_len: path.len(),
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
            top.offer(p(path).as_bytes(), 1, Some(new), Some(old))
                .unwrap();
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
        top.offer(p("/r/a").as_bytes(), 1, Some(1), Some(5))
            .unwrap(); // -4
        top.offer(p("/r/b").as_bytes(), 1, Some(9), Some(5))
            .unwrap(); // +4
        top.offer(p("/r/c").as_bytes(), 1, Some(5), Some(5))
            .unwrap();
        top.offer(p("/r/d").as_bytes(), 1, None, Some(2)).unwrap(); // -2
        assert_eq!(top.count, 2);
        let rows = top.finish().unwrap();
        assert_eq!(rows[0].delta, -4);
        assert_eq!(rows[1].delta, -2);
    }

    #[test]
    fn zero_limit_keeps_nothing() {
        let mut top = TopN::new(0, Order::AbsDelta);
        top.offer(p("/r/a").as_bytes(), 0, Some(1), None).unwrap();
        assert!(top.finish().unwrap().is_empty());
    }

    fn dir(id: u32, parent: Option<u32>, depth: u32, end: u32, name: &[u8], disk: u64) -> DirRow {
        let mut path = b"/r".to_vec();
        if parent.is_some() {
            path.push(b'/');
            path.extend_from_slice(name);
        }
        let path = RawPath::new(path).unwrap();
        DirRow {
            id,
            parent_id: parent,
            subtree_end: end,
            depth,
            name: if parent.is_some() {
                name.to_vec()
            } else {
                path.as_bytes().to_vec()
            },
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

    fn wide_snapshot(
        dir_path: &Path,
        v2: bool,
        width: u32,
        skip: u32,
        now: i64,
    ) -> std::path::PathBuf {
        use tidemark_core::{
            config::LinkInstances,
            scan::{ScanMode, SnapshotStatus},
            snapshot::SnapshotMeta,
            time::ScanId,
        };
        let root = RawPath::new(b"/r".to_vec()).unwrap();
        let meta = SnapshotMeta {
            schema_version: if v2 { 2 } else { 1 },
            tool_version: "test".into(),
            scan_id: ScanId::new(now).unwrap(),
            root_display: root.display_lossy(),
            root,
            mode: ScanMode::Walk,
            mode_reason: "fixture".into(),
            status: SnapshotStatus::Complete,
            degraded: false,
            exclude: vec![],
            link_instances: LinkInstances::All,
            started_at: now,
            finished_at: now + 1,
            host: "test".into(),
            kernel: "test".into(),
            os_release: "test".into(),
            fs_uuid: String::new(),
            fs_dev: 0,
            fs_blocksize: 4096,
            threads: 1,
            throttle_profile: "max".into(),
            dirs: 0,
            files: 0,
            inodes: 0,
            errors: 0,
            races: 0,
            skipped: 0,
            noatime_fallbacks: 0,
            phase_ms: Default::default(),
            gate_stats: Default::default(),
        };
        let mut rows = vec![dir(0, None, 0, width, b"/r", 5)];
        let mut id = 1;
        for i in 0..width {
            if skip != 0 && i % skip == 0 {
                continue;
            }
            let name = format!("k{i:06}").into_bytes();
            rows.push(dir(
                id,
                Some(0),
                1,
                id,
                &name,
                u64::from(i % 17) + u64::from(now as u32 % 3),
            ));
            id += 1;
        }
        rows[0].subtree_end = id - 1;
        let mut meta = meta;
        meta.dirs = rows.len() as u64;
        if v2 {
            let mut w = crate::writer_v2::SnapshotWriterV2::create_with_options(
                dir_path,
                &meta,
                crate::writer_v2::WriterOptions {
                    plain_depth: std::env::var("TM_PLAIN").map_or(0, |v| v.parse().unwrap()),
                    chunk_dirs: 512,
                    ..Default::default()
                },
            )
            .unwrap();
            for c in rows.chunks(1000) {
                w.put_dirs(c).unwrap();
            }
            w.finish(&meta).unwrap()
        } else {
            let mut w = crate::writer::SnapshotWriter::create(dir_path, &meta).unwrap();
            for c in rows.chunks(1000) {
                w.put_dirs(c).unwrap();
            }
            w.finish(&meta).unwrap()
        }
    }

    /// Each side's children are fetched exactly once: O(W) sibling visits, not
    /// O(W * pages) as when every page restarted from the first child.
    #[test]
    fn wide_directory_costs_one_visit_per_child_per_side() {
        const W: u32 = 120_000;
        let tmp = test_support::private_tempdir().unwrap();
        for old_v2 in [true, false] {
            let o = wide_snapshot(tmp.path(), old_v2, W, 7, 10 + i64::from(old_v2));
            let n = wide_snapshot(tmp.path(), true, W, 11, 20 + i64::from(old_v2));
            let ns = Snapshot::open(&n).unwrap();
            let root = RawPath::new(b"/r".to_vec()).unwrap();
            FETCHED.with(|c| c.set(0));
            let rows = diff(&ns, &o, &root, DiffMetric::Disk, 2, 10).unwrap();
            assert_eq!(rows.len(), 10);
            let (new_kids, old_kids) =
                (u64::from(W - W.div_ceil(11)), u64::from(W - W.div_ceil(7)));
            let fetched = FETCHED.with(|c| c.get());
            assert_eq!(fetched, new_kids + old_kids, "old_v2={old_v2}");
        }
    }

    /// `cargo test --release -p tidemark-store --lib -- --ignored --nocapture profile_wide`
    /// Splits the wide-directory walk into paging (`children_by_name`) and the rest.
    #[test]
    #[ignore = "scale"]
    fn profile_wide_split() {
        const W: u32 = 150_000;
        let tmp = test_support::private_tempdir().unwrap();
        let o = wide_snapshot(tmp.path(), true, W, 0, 10);
        let n = wide_snapshot(tmp.path(), true, W, 11, 20);
        let (ns, os) = (Snapshot::open(&n).unwrap(), Snapshot::open(&o).unwrap());
        let root = RawPath::new(b"/r".to_vec()).unwrap();
        for round in 0..3 {
            let t = std::time::Instant::now();
            let mut out = Vec::new();
            let mut cursor = None;
            let mut pages = 0;
            loop {
                out.clear();
                cursor = ns
                    .children_by_name(0, cursor.as_ref(), PAGE_MAX, &mut out)
                    .unwrap();
                pages += 1;
                if cursor.is_none() {
                    break;
                }
            }
            let paging = t.elapsed();
            let t = std::time::Instant::now();
            let rows = diff(&ns, &o, &root, DiffMetric::Disk, 2, 50).unwrap();
            eprintln!(
                "round {round}: one-side paging {paging:?} ({pages} pages); full diff {:?} ({} rows); open old {:?}",
                t.elapsed(),
                rows.len(),
                { let t = std::time::Instant::now(); drop(Snapshot::open(&o).unwrap()); t.elapsed() },
            );
        }
        drop(os);
    }
}
