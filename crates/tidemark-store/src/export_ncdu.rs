//! Owned by: W3-C
//! Iterative ncdu charged-view export with bounded query and stack storage.

use crate::{reader::Snapshot, StoreError};
use std::io::Write;
use tidemark_core::{path::RawPath, snapshot::FileFilter};

/// Name encoding and relative directory depth for an ncdu export.
pub struct NcduOptions {
    /// Replace invalid UTF-8 sequences with the Unicode replacement character.
    pub utf8_lossy: bool,
    /// Include directories through this depth relative to the exported root.
    pub max_depth: Option<u32>,
}
/// Counters for successfully emitted ncdu entries.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExportStats {
    /// Number of directory arrays emitted.
    pub directories: u64,
    /// Number of notable single-link file entries emitted.
    pub files: u64,
    /// Number of aggregate file entries emitted.
    pub synthetic_files: u64,
    /// Total additive disk bytes emitted.
    pub disk_bytes: u64,
}
// Public Snapshot queries materialize results. Fail explicitly rather than
// allocating a million FileHits until SCR-001 supplies a cursor.
const MAX_NOTABLE_QUERY: usize = 4096;
// Directory depth is bounded independently of input tree width.
const MAX_OPEN_ARRAYS: usize = 65536;

/// Stream ncdu v1.2 arrays in preorder without recursion or a resident tree.
///
/// Depth-limited descendants are represented by an aggregate file so the disk
/// total remains additive. Missing bulk-mode names remain in the aggregate.
/// The current reader contract requires a bounded subtree notable-file query;
/// exports exceeding 4096 notable files in a queried subtree fail explicitly.
pub fn export_ncdu(
    snap: &Snapshot,
    under: &RawPath,
    out: &mut dyn Write,
    opts: &NcduOptions,
) -> Result<ExportStats, StoreError> {
    let root = snap
        .dir_by_path(under)?
        .ok_or_else(|| StoreError::Invalid("export root absent from snapshot".into()))?;
    let mut hits = snap.subtree_files(root.id, &FileFilter::default(), MAX_NOTABLE_QUERY + 1)?;
    if hits.len() > MAX_NOTABLE_QUERY {
        return Err(StoreError::Invalid(
            "ncdu export notable subtree exceeds 4096-row cap (SCR-W3-C-001)".into(),
        ));
    }
    hits.retain(|h| h.row.nlink == 1 && h.row.name.is_some());
    hits.sort_by(|a, b| {
        a.row
            .dir_id
            .cmp(&b.row.dir_id)
            .then_with(|| a.row.name.cmp(&b.row.name))
            .then_with(|| a.row.ino.cmp(&b.row.ino))
    });
    let mut hits = hits.into_iter().peekable();
    let metadata = serde_json::json!({"progname":"tidemark", "progver":snap.meta().tool_version,
        "timestamp":snap.meta().started_at, "note":"Sizes are the charged (additive) view."});
    write!(out, "[1,2,{metadata},")?;
    let mut stack: Vec<u32> = Vec::new();
    let mut stats = ExportStats::default();
    let mut next = Some(root.id);
    while let Some(id) = next {
        let dir = snap.dir(id)?;
        while stack.last().is_some_and(|end| *end < id) {
            out.write_all(b"]")?;
            stack.pop();
        }
        if !stack.is_empty() {
            out.write_all(b",")?;
        }
        if stack.len() >= MAX_OPEN_ARRAYS {
            return Err(StoreError::Invalid(
                "export exceeds 65536 open arrays".into(),
            ));
        }
        out.write_all(b"[")?;
        let name = if id == root.id {
            under.as_bytes()
        } else {
            &dir.name
        };
        info(
            out,
            name,
            dir.dir_apparent,
            dir.dir_disk,
            dir.ino,
            (id == root.id).then_some(snap.meta().fs_dev),
            opts,
        )?;
        stats.directories += 1;
        add_disk(&mut stats, dir.dir_disk)?;
        let mut disk = dir
            .own_charged
            .checked_sub(dir.dir_disk)
            .ok_or_else(|| StoreError::Invalid("own_charged below dir_disk".into()))?;
        let mut apparent = dir
            .own_apparent
            .checked_sub(dir.dir_apparent)
            .ok_or_else(|| StoreError::Invalid("own_apparent below dir_apparent".into()))?;
        let mut count = dir
            .own_files
            .checked_add(dir.own_symlinks)
            .ok_or_else(|| StoreError::Integer("aggregate file count".into()))?;
        while hits.peek().is_some_and(|h| h.row.dir_id < id) {
            hits.next();
        }
        while hits.peek().is_some_and(|h| h.row.dir_id == id) {
            let Some(hit) = hits.next() else {
                break;
            };
            let f = hit.row;
            if let Some(name) = f.name {
                disk = disk
                    .checked_sub(f.disk)
                    .ok_or_else(|| StoreError::Invalid("notable disk exceeds own charge".into()))?;
                apparent = apparent.checked_sub(f.apparent).ok_or_else(|| {
                    StoreError::Invalid("notable apparent exceeds own apparent".into())
                })?;
                count = count
                    .checked_sub(1)
                    .ok_or_else(|| StoreError::Invalid("notable count exceeds own count".into()))?;
                out.write_all(b",")?;
                info(out, name.as_bytes(), f.apparent, f.disk, f.ino, None, opts)?;
                stats.files += 1;
                add_disk(&mut stats, f.disk)?;
            }
        }
        out.write_all(b",")?;
        info(
            out,
            format!("[other files: {count}]").as_bytes(),
            apparent,
            disk,
            0,
            None,
            opts,
        )?;
        stats.synthetic_files += 1;
        add_disk(&mut stats, disk)?;
        let cut = opts
            .max_depth
            .is_some_and(|depth| dir.depth.saturating_sub(root.depth) >= depth);
        if cut && dir.subtree_end > id {
            let descendants = dir
                .t_charged
                .checked_sub(dir.own_charged)
                .ok_or_else(|| StoreError::Invalid("subtree charge below own charge".into()))?;
            out.write_all(b",")?;
            info(
                out,
                b"[depth-limited descendants]",
                dir.t_apparent
                    .checked_sub(dir.own_apparent)
                    .ok_or_else(|| {
                        StoreError::Invalid("subtree apparent below own apparent".into())
                    })?,
                descendants,
                0,
                None,
                opts,
            )?;
            stats.synthetic_files += 1;
            add_disk(&mut stats, descendants)?;
        }
        stack.push(dir.subtree_end);
        let last = if cut { dir.subtree_end } else { id };
        next = if last < root.subtree_end {
            last.checked_add(1)
        } else {
            None
        };
    }
    if stats.disk_bytes != root.t_charged {
        return Err(StoreError::Invalid(
            "export disk total disagrees with root t_charged".into(),
        ));
    }
    for _ in stack {
        out.write_all(b"]")?;
    }
    out.write_all(b"]\n")?;
    Ok(stats)
}
fn add_disk(stats: &mut ExportStats, disk: u64) -> Result<(), StoreError> {
    stats.disk_bytes = stats
        .disk_bytes
        .checked_add(disk)
        .ok_or_else(|| StoreError::Integer("export disk total".into()))?;
    Ok(())
}
fn info(
    out: &mut dyn Write,
    name: &[u8],
    apparent: u64,
    disk: u64,
    ino: u64,
    dev: Option<u64>,
    opts: &NcduOptions,
) -> Result<(), StoreError> {
    out.write_all(b"{\"name\":")?;
    if opts.utf8_lossy {
        json_bytes(out, String::from_utf8_lossy(name).as_bytes())?;
    } else {
        json_bytes(out, name)?;
    }
    write!(out, ",\"asize\":{apparent},\"dsize\":{disk},\"ino\":{ino}")?;
    if let Some(dev) = dev {
        write!(out, ",\"dev\":{dev}")?;
    }
    out.write_all(b"}")?;
    Ok(())
}
fn json_bytes(out: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    out.write_all(b"\"")?;
    let mut start = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        if byte < 0x20 || byte == b'"' || byte == b'\\' {
            out.write_all(&bytes[start..i])?;
            match byte {
                b'"' => out.write_all(b"\\\"")?,
                b'\\' => out.write_all(b"\\\\")?,
                _ => write!(out, "\\u{byte:04x}")?,
            }
            start = i + 1;
        }
    }
    out.write_all(&bytes[start..])?;
    out.write_all(b"\"")
}
