//! Owned by: W3-C
//! Iterative ncdu charged-view export with bounded query and stack storage.

use crate::{reader::Snapshot, StoreError};
use std::io::Write;
use tidemark_core::path::RawPath;

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
// Directory depth is bounded independently of input tree width.
const MAX_OPEN_ARRAYS: usize = 65536;
// Reader sorting scratch lives in memory. LIMIT bounds SQLite's top-N sorter
// even without a (dir_id, name, ino) index; keyset pages impose no total cap.
const FILE_PAGE_ROWS: u32 = 256;

/// Stream ncdu v1.2 arrays in preorder without recursion or a resident tree.
///
/// Depth-limited descendants are represented by an aggregate file so the disk
/// total remains additive. Missing bulk-mode names remain in the aggregate.
/// Notable files are streamed one row at a time from a per-directory SQL cursor.
pub fn export_ncdu(
    snap: &Snapshot,
    under: &RawPath,
    out: &mut dyn Write,
    opts: &NcduOptions,
) -> Result<ExportStats, StoreError> {
    let root = snap
        .dir_by_path(under)?
        .ok_or_else(|| StoreError::Invalid("export root absent from snapshot".into()))?;
    let mut files = snap.conn().prepare(
        "SELECT name, apparent, disk, ino, id FROM files
         WHERE dir_id = ?1 AND nlink = 1 AND name IS NOT NULL
           AND (?2 IS NULL OR (name, ino, id) > (?2, ?3, ?4))
         ORDER BY name, ino, id LIMIT ?5",
    )?;
    let metadata = serde_json::json!({"progname":"tidemark", "progver":snap.meta().tool_version,
        "timestamp":snap.meta().started_at, "note":"Sizes are the charged (additive) view."});
    write!(out, "[1,2,{metadata},")?;
    let mut stack: Vec<u32> = Vec::new();
    let mut stats = ExportStats::default();
    let mut directories = snap
        .conn()
        .prepare("SELECT * FROM dirs WHERE id BETWEEN ?1 AND ?2 ORDER BY id")?;
    let mut directory_rows = directories.query(rusqlite::params![root.id, root.subtree_end])?;
    let mut next_id = root.id;
    while let Some(row) = directory_rows.next()? {
        let id: u32 = row.get(0)?;
        if id < next_id {
            continue;
        }
        let dir = crate::reader::dir_row(row)?;
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
        let mut after: Option<(Vec<u8>, i64, u32)> = None;
        loop {
            let mut rows = files.query(rusqlite::params![
                id,
                after.as_ref().map(|key| &key.0),
                after.as_ref().map(|key| key.1),
                after.as_ref().map(|key| key.2),
                FILE_PAGE_ROWS,
            ])?;
            let mut last: Option<(Vec<u8>, i64, u32)> = None;
            let mut emitted = 0;
            while let Some(file) = rows.next()? {
                let name = file.get_ref(0)?.as_blob().map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Blob,
                        Box::new(error),
                    )
                })?;
                let file_apparent = crate::error::read_u64(file, 1)?;
                let file_disk = crate::error::read_u64(file, 2)?;
                let ino = crate::error::read_u64(file, 3)?;
                disk = disk
                    .checked_sub(file_disk)
                    .ok_or_else(|| StoreError::Invalid("notable disk exceeds own charge".into()))?;
                apparent = apparent.checked_sub(file_apparent).ok_or_else(|| {
                    StoreError::Invalid("notable apparent exceeds own apparent".into())
                })?;
                count = count
                    .checked_sub(1)
                    .ok_or_else(|| StoreError::Invalid("notable count exceeds own count".into()))?;
                out.write_all(b",")?;
                info(out, name, file_apparent, file_disk, ino, None, opts)?;
                stats.files += 1;
                add_disk(&mut stats, file_disk)?;
                emitted += 1;
                // Retain only the final row's key, never the page or subtree.
                let key = last.get_or_insert_with(|| (Vec::new(), 0, 0));
                key.0.clear();
                key.0.extend_from_slice(name);
                key.1 = file.get(3)?;
                key.2 = file.get(4)?;
            }
            if emitted < FILE_PAGE_ROWS {
                break;
            }
            after = last;
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
        if last >= root.subtree_end {
            break;
        }
        next_id = last + 1; // last < root.subtree_end <= u32::MAX
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
