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
/// on both sides is an error. Only the requested output is collected; SQL joins
/// stream the selected rows without a subtree-size cap. Output limits remain
/// bounded at 1,000,000. Neither snapshot is written.
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
    with_old(new, &old, || {
        joined(
            new,
            n.as_ref(),
            o.as_ref(),
            metric,
            max_depth_rel,
            limit,
            None,
        )
        .map(|(rows, _)| rows)
    })
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
/// direct children are joined in SQL without a child-count cap. `own_delta` always compares own charged bytes, including when
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
    with_old(new, &old, || loop {
        // Keep the dominant candidate even when no spread rows were requested.
        let (mut contributors, spread_children) = joined(
            new,
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
    })
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

#[cfg(test)]
#[path = "../tests/w3a/row.rs"]
mod test_row;

#[cfg(test)]
mod test_support {
    pub use super::test_row::row;
    use crate::writer::SnapshotWriter;

    pub fn snapshot(
        dir: &std::path::Path,
        rows: &[tidemark_core::snapshot::DirRow],
        now: i64,
    ) -> std::path::PathBuf {
        let mut meta = crate::test_meta();
        meta.schema_version = 1;
        meta.root = rows[0].path.clone();
        meta.scan_id = tidemark_core::time::ScanId::new(now).unwrap();
        meta.dirs = rows.len() as u64;
        let mut writer = SnapshotWriter::create(dir, &meta).unwrap();
        writer.put_dirs(rows).unwrap();
        writer.finish(&meta).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "scale"]
    fn wide_explain_uses_streaming_count_and_bounded_top_n() {
        const CHILDREN: u32 = 200_000;
        let dir = ::test_support::private_tempdir().unwrap();
        let mut rows = vec![test_support::row(0, None, b"/fixture", 0)];
        rows[0].subtree_end = CHILDREN;
        // Long paths amplify the old window query's per-child scratch usage.
        let prefix = "x".repeat(220);
        for id in 1..=CHILDREN {
            rows.push(test_support::row(
                id,
                Some(0),
                format!("/fixture/{prefix}{id:08}").as_bytes(),
                0,
            ));
        }
        let a = test_support::snapshot(dir.path(), &rows, 1);
        rows[0].t_disk = u64::from(CHILDREN);
        for child in &mut rows[1..] {
            child.t_disk = 1;
        }
        let b = test_support::snapshot(dir.path(), &rows, 2);
        drop(rows);
        let new = Snapshot::open(&b).unwrap();
        let old = Snapshot::open(&a).unwrap();
        let target = RawPath::new(b"/fixture".to_vec()).unwrap();
        let n = new.dir_by_path(&target).unwrap();
        let o = old.dir_by_path(&target).unwrap();
        with_old(&new, &old, || {
            // Inspect the exact production SQL, including subtree diff and all
            // metrics. EXPLAIN QUERY PLAN's "TEMP B-TREE" also labels bounded
            // top-N sorts, so inspect the VDBE instructions instead.
            for metric in [DiffMetric::Disk, DiffMetric::Reclaimable, DiffMetric::Files] {
                for children in [false, true] {
                    let (limited, aggregate) = joined_sql(metric, children);
                    for (sql, limited) in std::iter::once((limited, true))
                        .chain(aggregate.into_iter().map(|sql| (sql, false)))
                    {
                        let mut stmt = new.conn().prepare(&format!("EXPLAIN {sql}"))?;
                        bind_joined(&mut stmt, n.as_ref(), o.as_ref(), 1, 5, Some(1))?;
                        let ops: Vec<String> = stmt
                            .raw_query()
                            .mapped(|r| r.get(1))
                            .collect::<Result<_, _>>()?;
                        assert!(
                            !ops.iter().any(|op| op == "SorterOpen"),
                            "unbounded sort: {ops:?}"
                        );
                        assert!(
                            !ops.iter().any(|op| op == "OpenAutoindex"),
                            "unbounded automatic index: {ops:?}"
                        );
                        if limited {
                            // Once LIMIT slots are filled, IfNotZero falls
                            // through to Last/IdxLE/Delete before IdxInsert:
                            // only the best LIMIT rows stay in the sorter.
                            let mut sorts = 0;
                            for (i, op) in ops.iter().enumerate() {
                                if op == "OpenEphemeral" {
                                    sorts += 1;
                                }
                                if op == "IdxInsert" {
                                    let guard =
                                        ops[..i].iter().rposition(|op| op == "IfNotZero").unwrap();
                                    assert_eq!(
                                        &ops[guard + 1..guard + 4],
                                        &["Last", "IdxLE", "Delete"]
                                    );
                                    assert!(!ops[guard + 4..i].iter().any(|op| op == "IdxInsert"));
                                }
                            }
                            assert!(sorts > 0);
                            assert_eq!(sorts, ops.iter().filter(|op| *op == "IdxInsert").count());
                        } else {
                            assert!(
                                !ops.iter().any(|op| op == "OpenEphemeral"),
                                "materialized aggregate: {ops:?}"
                            );
                            assert!(ops.iter().any(|op| op == "AggStep"));
                        }
                    }
                }
            }
            Ok(())
        })
        .unwrap();
        for (new, old, sign) in [(&new, &a, 1), (&old, &b, -1)] {
            let result = explain(new, old, &target, DiffMetric::Disk, 0.5, 1, 5).unwrap();
            assert_eq!(result.target_delta, i64::from(CHILDREN) * sign);
            assert!(result.steps.is_empty());
            assert_eq!(result.spread_children, CHILDREN);
            assert_eq!(result.spread.len(), 5);
            for (i, child) in result.spread.iter().enumerate() {
                assert_eq!(child.delta, sign);
                assert_eq!(
                    child.path.as_bytes(),
                    format!("/fixture/{prefix}{:08}", i + 1).as_bytes()
                );
            }
        }
    }

    #[test]
    fn query_restores_mapping_limit_and_detaches_on_error_and_unwind() {
        let dir = ::test_support::private_tempdir().unwrap();
        let mut meta = crate::test_meta();
        meta.schema_version = 1;
        let a = crate::writer::SnapshotWriter::create(dir.path(), &meta)
            .unwrap()
            .finish(&meta)
            .unwrap();
        meta.scan_id = tidemark_core::time::ScanId::new(20).unwrap();
        let b = crate::writer::SnapshotWriter::create(dir.path(), &meta)
            .unwrap()
            .finish(&meta)
            .unwrap();
        let old = Snapshot::open(&a).unwrap();
        let new = Snapshot::open(&b).unwrap();
        let mapping = || -> i64 {
            new.conn()
                .pragma_query_value(Some("main"), "mmap_size", |r| r.get(0))
                .unwrap()
        };
        let original = mapping();
        let error = with_old(&new, &old, || -> Result<(), StoreError> {
            assert!(new.conn().execute("DELETE FROM o.dirs", []).is_err());
            assert!(new.conn().execute("DELETE FROM main.dirs", []).is_err());
            Err(StoreError::Invalid("injected query failure".into()))
        });
        assert!(matches!(error, Err(StoreError::Invalid(_))));
        assert_eq!(mapping(), original);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = with_old(&new, &old, || -> Result<(), StoreError> {
                panic!("injected query unwind");
            });
        }));
        assert!(panic.is_err());
        assert_eq!(mapping(), original);
        // Both previous attachments must be gone before this succeeds.
        assert_eq!(with_old(&new, &old, || Ok(42)).unwrap(), 42);
        assert_eq!(mapping(), original);
    }
}
