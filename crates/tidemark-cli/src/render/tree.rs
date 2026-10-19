//! Owned by: W2-C
//! Deterministic directory trees and notable-file tables with byte-safe paths.
use std::io::Write;
use tidemark_core::snapshot::{DirRow, FileHit};
/// Directory metrics and optional comparisons from older/live trees.
#[derive(serde::Serialize)]
pub struct Directory {
    /// Stored snapshot directory metrics.
    #[serde(flatten)]
    pub row: DirRow,
    /// Signed disk-byte change versus the older complete snapshot.
    ///
    /// Null when no older snapshot was selected, or when inflation was skipped.
    pub delta: Option<i128>,
    /// Why `delta` is null even though an older snapshot was selected.
    ///
    /// Present only for a skipped inflation. `free_space_floor` means inflating
    /// that snapshot would breach the free-space floor (`retention.min_free_pct`
    /// and the mandatory 2 GiB floor). A `max_state_size` refusal is not a skip:
    /// the command fails and this field is absent. Omitted when `delta` is known
    /// and when no older snapshot was selected, so a missing predecessor stays
    /// `delta: null` with no `delta_skipped` field. `tidemark.top.v1` emits this
    /// on each row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_skipped: Option<&'static str>,
    /// Live disk bytes from a gentle walk.
    pub live_disk: Option<u64>,
}
/// Use ASCII when LANG does not explicitly select UTF-8.
pub fn ascii_locale() -> bool {
    ascii_for_lang(std::env::var("LANG").ok().as_deref())
}
/// Locale decision with injected environment text.
pub fn ascii_for_lang(lang: Option<&str>) -> bool {
    !lang.is_some_and(|s| {
        let s = s.to_ascii_lowercase();
        s.contains("utf-8") || s.contains("utf8")
    })
}
/// Format a signed byte difference without overflow, including shrinkage.
pub fn signed_bytes(n: i128) -> String {
    if n == 0 {
        return "0".into();
    }
    let magnitude = u64::try_from(n.unsigned_abs()).unwrap_or(u64::MAX);
    format!(
        "{}{}",
        if n < 0 { "-" } else { "+" },
        super::size::bytes(magnitude)
    )
}
/// Describe the snapshot backing a text report, with an injected observation time.
pub fn snapshot_header(
    meta: &tidemark_core::snapshot::SnapshotMeta,
    now: i64,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let age = now.saturating_sub(meta.started_at).max(0) as u64;
    writeln!(
        out,
        "Snapshot {} · {} · {}s ago · {}{}",
        meta.scan_id,
        super::enum_label(meta.mode),
        age,
        super::enum_label(meta.status),
        if meta.degraded { " (degraded)" } else { "" }
    )?;
    Ok(())
}
/// Render rows as an iterative tree, preserving the caller's sibling ordering.
///
/// `blank_delta` keeps the Δ24h column when an older snapshot exists but could
/// not be inflated. Those cells are `-`.
pub fn directories(
    rows: &[Directory],
    wide: bool,
    ascii: bool,
    json: bool,
    blank_delta: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return super::json("tidemark.top.v1", rows, out);
    }
    let delta = blank_delta || rows.iter().any(|r| r.delta.is_some());
    let live = rows.iter().any(|r| r.live_disk.is_some());
    let mut header = vec![
        "PATH".into(),
        "SIZE".into(),
        "RECLAIM".into(),
        "SHARED".into(),
        "FILES".into(),
    ];
    if wide {
        header.extend(["NEWEST".into(), "INODES".into(), "APPARENT".into()]);
    }
    if delta {
        header.push(if ascii { "DELTA24h" } else { "Δ24h" }.into());
    }
    if live {
        header.extend(["LIVE".into(), "DELTA LIVE".into()]);
    }
    let mut table = vec![header];
    let ids: std::collections::BTreeSet<_> = rows.iter().map(|r| r.row.id).collect();
    let mut children = std::collections::BTreeMap::<Option<u32>, Vec<usize>>::new();
    for (i, r) in rows.iter().enumerate() {
        children
            .entry(r.row.parent_id.filter(|p| ids.contains(p)))
            .or_default()
            .push(i);
    }
    let roots = children.get(&None).cloned().unwrap_or_default();
    let mut stack: Vec<_> = roots
        .iter()
        .enumerate()
        .rev()
        .map(|(j, &i)| (i, String::new(), j + 1 == roots.len(), true))
        .collect();
    while let Some((i, prefix, last, root)) = stack.pop() {
        let r = &rows[i];
        let label = format!(
            "{}{}{}",
            prefix,
            if root {
                ""
            } else {
                super::table::branch(last, ascii)
            },
            r.row.path.display_lossy()
        );
        let shared = r.row.t_disk.saturating_sub(r.row.t_reclaimable);
        let mut cells = vec![
            label,
            super::size::bytes(r.row.t_disk),
            super::size::bytes(r.row.t_reclaimable),
            format!(
                "{}{}",
                if shared > 0 { "H " } else { "" },
                super::size::bytes(shared)
            ),
            r.row.t_files.to_string(),
        ];
        if wide {
            cells.extend([
                crate::render::size::timestamp(r.row.newest_mtime),
                r.row.t_inodes.to_string(),
                super::size::bytes(r.row.t_apparent),
            ]);
        }
        if delta {
            cells.push(match r.delta {
                Some(n) => signed_bytes(n),
                None if blank_delta => "-".into(),
                None => "unknown".into(),
            });
        }
        if live {
            cells.extend([
                r.live_disk
                    .map(super::size::bytes)
                    .unwrap_or_else(|| "unknown".into()),
                r.live_disk
                    .map(|n| signed_bytes(i128::from(n) - i128::from(r.row.t_disk)))
                    .unwrap_or_else(|| "unknown".into()),
            ]);
        }
        table.push(cells);
        if let Some(child) = children.get(&Some(r.row.id)) {
            let next = if root {
                prefix
            } else {
                format!(
                    "{prefix}{}",
                    if last {
                        "   "
                    } else if ascii {
                        "|  "
                    } else {
                        "│  "
                    }
                )
            };
            for (j, &index) in child.iter().enumerate().rev() {
                stack.push((index, next.clone(), j + 1 == child.len(), false));
            }
        }
    }
    write!(out, "{}", super::table::aligned(&table))?;
    if rows.iter().any(|r| r.row.t_disk > r.row.t_reclaimable) {
        writeln!(
            out,
            "H = hard links shared outside this directory (see tidemark links)"
        )?;
    }
    Ok(())
}
/// Render notable files, representing unresolved bulk names honestly.
pub fn files(rows: &[FileHit], json: bool, out: &mut impl Write) -> anyhow::Result<()> {
    if json {
        return super::json("tidemark.files.v1", rows, out);
    }
    let mut table = vec![vec![
        "PATH".into(),
        "SIZE".into(),
        "MTIME".into(),
        "NLINK".into(),
    ]];
    for hit in rows {
        table.push(vec![
            format!(
                "{}{}",
                hit.path
                    .as_ref()
                    .map(|p| p.display_lossy())
                    .unwrap_or_else(|| format!("[unresolved inode {}]", hit.row.ino)),
                if hit.row.nlink > 1 { " (shared)" } else { "" }
            ),
            super::size::bytes(hit.row.disk),
            crate::render::size::timestamp(hit.row.mtime),
            hit.row.nlink.to_string(),
        ]);
    }
    write!(out, "{}", super::table::aligned(&table))?;
    Ok(())
}
