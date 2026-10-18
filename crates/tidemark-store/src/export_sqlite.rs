//! Owned by: SNAP-export-sqlite
//! Materialise any snapshot (v1, v2 or inflated cold) as a v1-schema SQLite database.

use crate::{
    reader::{dir_row, file_row, link_row, plain_row_error, Snapshot},
    writer::SnapshotWriter,
    StoreError,
};
use rusqlite::params;
use std::path::{Path, PathBuf};
use tidemark_core::{
    path::RawName,
    snapshot::{AnnotationRow, DirRow, FileRow, LinkSetRow},
};

/// Rows copied by [`export_sqlite`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SqliteExportStats {
    /// `dirs` rows.
    pub dirs: u64,
    /// `files` rows.
    pub files: u64,
    /// `link_sets` rows.
    pub link_sets: u64,
    /// `link_instances` rows.
    pub link_instances: u64,
    /// `skipped` rows.
    pub skipped: u64,
    /// `errors` rows.
    pub errors: u64,
    /// `annotations` rows.
    pub annotations: u64,
}

/// Rows buffered between writer calls; memory is bounded by this, not by tree size.
const BATCH: usize = 10_000;

/// Removes the private staging directory on every exit path.
struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        // The directory was created by this export and holds only its own files.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Write `snapshot` to `out` as a v1-schema database (with `dirs.path`/`path_hash` and the v1 indexes).
///
/// `out` must not exist; it is refused before any row is read and is never replaced. The
/// database is built in a private 0700 staging directory beside `out` (so the final step
/// stays on one filesystem), synced, then published with `RENAME_NOREPLACE`; the file is
/// mode 0600. On any error the staging directory and everything in it is removed.
///
/// Memory is bounded: rows stream through batches of 10,000. v1 sources are read with
/// keyset-free ordered cursors; v2 sources are read directory by directory (`dir`,
/// `v2_dir_files`) and link sets page by `ino` with per-set `link_instances`. v2 stores
/// notable files in name order without ids, so exported file ids are assigned in
/// directory order; v1 file ids are preserved.
pub fn export_sqlite(snapshot: &Snapshot, out: &Path) -> Result<SqliteExportStats, StoreError> {
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("output path already exists: {}", out.display()),
        )
        .into());
    }
    let name = out
        .file_name()
        .ok_or_else(|| StoreError::Invalid("output path has no file name".into()))?;
    let parent = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent = std::fs::canonicalize(parent)?;
    let final_path = parent.join(name);

    let staging = Staging(parent.join(format!(
        ".tidemark-export-{}-{}",
        std::process::id(),
        snapshot.meta().scan_id
    )));
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&staging.0)?;
    }
    let mut meta = snapshot.meta().clone();
    meta.schema_version = 1;
    let mut writer = SnapshotWriter::create(&staging.0, &meta)?;
    let mut stats = SqliteExportStats::default();
    copy_all(snapshot, &mut writer, &mut stats)?;
    let built = writer.finish(&meta)?;
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &built,
        rustix::fs::CWD,
        &final_path,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)?;
    std::fs::File::open(&parent)?.sync_all()?;
    drop(staging);
    Ok(stats)
}

fn copy_all(
    snap: &Snapshot,
    writer: &mut SnapshotWriter,
    stats: &mut SqliteExportStats,
) -> Result<(), StoreError> {
    let last_dir = snap.root()?.subtree_end;
    // dirs, in id order.
    let mut batch: Vec<DirRow> = Vec::with_capacity(BATCH);
    let mut flush_dirs = |batch: &mut Vec<DirRow>, stats: &mut SqliteExportStats| {
        stats.dirs += batch.len() as u64;
        let result = writer.put_dirs(batch);
        batch.clear();
        result
    };
    if snap.is_v2() {
        for id in 0..=last_dir {
            batch.push(snap.dir(id)?);
            if batch.len() >= BATCH {
                flush_dirs(&mut batch, stats)?;
            }
        }
    } else {
        let mut stmt = snap.conn().prepare("SELECT * FROM dirs ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            batch.push(dir_row(r)?);
            if batch.len() >= BATCH {
                flush_dirs(&mut batch, stats)?;
            }
        }
    }
    flush_dirs(&mut batch, stats)?;

    // files.
    let mut files: Vec<FileRow> = Vec::with_capacity(BATCH);
    if snap.is_v2() {
        let mut next_id = 0u32;
        for dir in 0..=last_dir {
            snap.v2_dir_files(dir, &mut |f| {
                let name = f.name.clone().map(RawName::new).transpose()?;
                files.push(FileRow {
                    id: next_id,
                    dir_id: f.dir_id,
                    name,
                    ino: f.ino,
                    disk: f.disk,
                    apparent: f.apparent,
                    mtime: f.mtime,
                    nlink: f.nlink,
                });
                next_id = next_id
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Integer("file id".into()))?;
                Ok(())
            })?;
            if files.len() >= BATCH {
                stats.files += files.len() as u64;
                writer.put_files(&files)?;
                files.clear();
            }
        }
    } else {
        let mut stmt = snap.conn().prepare("SELECT * FROM files ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            files.push(file_row(r)?);
            if files.len() >= BATCH {
                stats.files += files.len() as u64;
                writer.put_files(&files)?;
                files.clear();
            }
        }
    }
    stats.files += files.len() as u64;
    writer.put_files(&files)?;

    // link sets (plain table in both layouts), paged by inode; instances per set.
    let plain = |e: rusqlite::Error| {
        if snap.is_v2() {
            plain_row_error(e)
        } else {
            StoreError::Sql(e)
        }
    };
    let mut after: i64 = i64::MIN;
    loop {
        let mut stmt = snap
            .conn()
            .prepare("SELECT * FROM link_sets WHERE ino>?1 ORDER BY ino LIMIT ?2")?;
        let sets: Vec<LinkSetRow> = stmt
            .query_map(params![after, BATCH as i64], link_row)?
            .collect::<rusqlite::Result<_>>()
            .map_err(plain)?;
        let Some(last) = sets.last() else { break };
        after = i64::try_from(last.ino).map_err(|_| StoreError::Integer("ino".into()))?;
        let mut instances = Vec::new();
        for set in &sets {
            instances.extend(snap.link_instances(set.ino)?);
            if instances.len() >= BATCH {
                stats.link_instances += instances.len() as u64;
                writer.put_link_instances(&instances)?;
                instances.clear();
            }
        }
        stats.link_instances += instances.len() as u64;
        writer.put_link_instances(&instances)?;
        stats.link_sets += sets.len() as u64;
        writer.put_link_sets(&sets)?;
    }
    // Instances of inodes without a link_sets row cannot be reached through the sets;
    // v1 sources can hold them (unresolved partial scans), so copy leftovers explicitly.
    if !snap.is_v2() {
        let mut stmt = snap.conn().prepare(
            "SELECT ino,dir_id,name FROM link_instances WHERE ino NOT IN (SELECT ino FROM link_sets) ORDER BY ino,dir_id,name",
        )?;
        let mut rows = stmt.query([])?;
        let mut batch = Vec::new();
        while let Some(r) = rows.next()? {
            batch.push(crate::reader::instance_row(r)?);
            if batch.len() >= BATCH {
                stats.link_instances += batch.len() as u64;
                writer.put_link_instances(&batch)?;
                batch.clear();
            }
        }
        stats.link_instances += batch.len() as u64;
        writer.put_link_instances(&batch)?;
    }

    // skipped / errors / annotations: plain tables, keyset on rowid.
    macro_rules! copy_plain {
        ($sql:literal, $decode:expr, $put:ident, $count:ident) => {{
            let mut after = 0i64;
            loop {
                let mut stmt = snap.conn().prepare($sql)?;
                let mut rows = stmt.query(params![after, BATCH as i64]).map_err(plain)?;
                let mut batch = Vec::new();
                while let Some(r) = rows.next().map_err(plain)? {
                    after = r.get(0).map_err(plain)?;
                    batch.push($decode(r).map_err(plain)?);
                }
                if batch.is_empty() {
                    break;
                }
                stats.$count += batch.len() as u64;
                writer.$put(&batch)?;
            }
        }};
    }
    copy_plain!(
        "SELECT rowid,dir_id,name,reason FROM skipped WHERE rowid>?1 ORDER BY rowid LIMIT ?2",
        |r: &rusqlite::Row<'_>| skip_row_at(r),
        put_skipped,
        skipped
    );
    copy_plain!(
        "SELECT rowid,dir_id,path,op,errno FROM errors WHERE rowid>?1 ORDER BY rowid LIMIT ?2",
        |r: &rusqlite::Row<'_>| error_row_at(r),
        put_errors,
        errors
    );
    copy_plain!(
        "SELECT rowid,dir_id,key,value FROM annotations WHERE rowid>?1 ORDER BY rowid LIMIT ?2",
        |r: &rusqlite::Row<'_>| -> rusqlite::Result<AnnotationRow> {
            Ok(AnnotationRow {
                dir_id: r.get(1)?,
                key: r.get(2)?,
                value: r.get(3)?,
            })
        },
        put_annotations,
        annotations
    );
    Ok(())
}

// The shared decoders read from column 0; shift the rowid column away with a view of the tail.
fn skip_row_at(r: &rusqlite::Row<'_>) -> rusqlite::Result<tidemark_core::snapshot::SkipRow> {
    Ok(tidemark_core::snapshot::SkipRow {
        dir_id: r.get(1)?,
        name: RawName::new(r.get::<_, Vec<u8>>(2)?).map_err(conv)?,
        reason: serde_json::from_value(serde_json::Value::String(r.get::<_, String>(3)?))
            .map_err(conv)?,
    })
}
fn error_row_at(r: &rusqlite::Row<'_>) -> rusqlite::Result<tidemark_core::snapshot::ErrorRow> {
    Ok(tidemark_core::snapshot::ErrorRow {
        dir_id: r.get(1)?,
        path: tidemark_core::path::RawPath::new(r.get::<_, Vec<u8>>(2)?).map_err(conv)?,
        op: serde_json::from_value(serde_json::Value::String(r.get::<_, String>(3)?))
            .map_err(conv)?,
        errno: r.get(4)?,
    })
}
fn conv<E: std::error::Error + Send + Sync + 'static>(e: E) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
}
