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
    snapshot::{AnnotationRow, DirRow, FileRow, LinkInstanceRow, LinkSetRow},
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

/// One page of v1 link instances in `(ino, rowid)` order, served by the `li_ino` index
/// (columns 0-2 are the row; 3 and 4 are the keyset position).
pub(crate) const V1_INSTANCE_PAGE: &str = "SELECT ino,dir_id,name,ino,rowid FROM link_instances \
     WHERE (ino,rowid)>(?1,?2) ORDER BY ino,rowid LIMIT ?3";

/// A private staging directory this export created (and only this one).
///
/// The directory is created exclusively (`mkdir` fails on any existing entry, which is
/// then never touched); its device/inode are recorded so cleanup removes only the
/// directory it created, never a different entry that later appears under the name.
struct Staging {
    path: PathBuf,
    dev: u64,
    ino: u64,
}
impl Staging {
    /// Create a fresh 0700 directory with an unpredictable name under `parent`.
    fn create(parent: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        for attempt in 0..64u32 {
            let path = parent.join(format!(".tidemark-export-{:016x}", random_token(attempt)));
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
            return match std::fs::symlink_metadata(&path) {
                Ok(m) => Ok(Self {
                    path,
                    dev: m.dev(),
                    ino: m.ino(),
                }),
                Err(e) => {
                    // Just created by this call and still empty.
                    let _ = std::fs::remove_dir(&path);
                    Err(e)
                }
            };
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create a unique staging directory",
        ))
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        // Remove only the directory this export created.
        if let Ok(m) = std::fs::symlink_metadata(&self.path) {
            if m.is_dir() && m.dev() == self.dev && m.ino() == self.ino {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }
}
fn random_token(salt: u32) -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.write_u32(salt);
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    h.finish()
}

fn interrupted() -> StoreError {
    std::io::Error::new(std::io::ErrorKind::Interrupted, "export interrupted").into()
}
fn check(cancel: &dyn Fn() -> bool) -> Result<(), StoreError> {
    if cancel() {
        Err(interrupted())
    } else {
        Ok(())
    }
}
/// SQLite extended result codes for a failed write, fsync or truncate.
const IOERR_WRITE: i32 = 778;
const IOERR_FSYNC: i32 = 1034;
const IOERR_TRUNCATE: i32 = 1546;

/// Disk-full from the destination database is a space failure.
///
/// SQLite reports a failed `pwrite`/`fsync`/`ftruncate` as `SQLITE_IOERR_*` without the OS
/// error, and this crate cannot call `sqlite3_system_errno` (`unsafe` is confined to
/// `tidemark-sys`). For those three codes, `probe` re-tries a write in the staging directory
/// and returns its errno; only `ENOSPC`/`EDQUOT` make it a space failure, so unrelated
/// I/O errors keep their classification.
fn classify_with(error: StoreError, probe: impl FnOnce() -> Option<i32>) -> StoreError {
    let StoreError::Sql(rusqlite::Error::SqliteFailure(e, _)) = &error else {
        return error;
    };
    if e.code == rusqlite::ErrorCode::DiskFull {
        return StoreError::Space("destination disk full".into());
    }
    if matches!(e.extended_code, IOERR_WRITE | IOERR_FSYNC | IOERR_TRUNCATE) {
        if let Some(n) = probe() {
            if n == rustix::io::Errno::NOSPC.raw_os_error()
                || n == rustix::io::Errno::DQUOT.raw_os_error()
            {
                return StoreError::Space("destination disk full or quota exceeded".into());
            }
        }
    }
    error
}
/// Write 1 MiB and sync it in `dir`; the errno of the first failure, if any.
fn probe_write_errno(dir: &Path) -> Option<i32> {
    use std::io::Write;
    let path = dir.join("space-probe");
    let result = std::fs::File::create(&path).and_then(|mut f| {
        f.write_all(&vec![0u8; 1 << 20])?;
        f.sync_all()
    });
    let _ = std::fs::remove_file(&path);
    result.err().and_then(|e| e.raw_os_error())
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
    export_sqlite_cancellable(snapshot, out, &|| false)
}

/// [`export_sqlite`] that polls `cancel` between batches; once it returns `true` the export
/// stops with an `Io(Interrupted)` error and removes everything it created.
pub fn export_sqlite_cancellable(
    snapshot: &Snapshot,
    out: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<SqliteExportStats, StoreError> {
    export_inner(snapshot, out, cancel).map_err(|e| match e {
        StoreError::Cancelled => interrupted(),
        other => other,
    })
}

fn export_inner(
    snapshot: &Snapshot,
    out: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<SqliteExportStats, StoreError> {
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

    check(cancel)?;
    let staging = Staging::create(&parent)?;
    let mut meta = snapshot.meta().clone();
    meta.schema_version = 1;
    let mut writer = SnapshotWriter::create(&staging.path, &meta)?;
    let mut stats = SqliteExportStats::default();
    let built = (|| {
        copy_all(snapshot, &mut writer, &mut stats, cancel)?;
        check(cancel)?;
        writer.finish_with(&meta, cancel)
    })()
    .map_err(|e| match e {
        StoreError::Cancelled => interrupted(),
        other => classify_with(other, || probe_write_errno(&staging.path)),
    })?;
    // Last chance to cancel: after the rename below the export is published and succeeds.
    check(cancel)?;
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
    cancel: &dyn Fn() -> bool,
) -> Result<(), StoreError> {
    let last_dir = snap.root()?.subtree_end;
    // dirs, in id order (validated strictly increasing before the writer sees them).
    let mut batch: Vec<DirRow> = Vec::with_capacity(BATCH);
    let mut last_id: Option<u32> = None;
    let mut flush_dirs = |batch: &mut Vec<DirRow>, stats: &mut SqliteExportStats| {
        check(cancel)?;
        stats.dirs += batch.len() as u64;
        let result = writer.put_dirs(batch);
        batch.clear();
        result
    };
    let mut push_dir = |batch: &mut Vec<DirRow>, row: DirRow| {
        if last_id.is_some_and(|id| row.id <= id) {
            return Err(StoreError::Invalid(
                "directory ids are not strictly increasing".into(),
            ));
        }
        last_id = Some(row.id);
        batch.push(row);
        Ok(())
    };
    if snap.is_v2() {
        for id in 0..=last_dir {
            push_dir(&mut batch, snap.dir(id)?)?;
            if batch.len() >= BATCH {
                flush_dirs(&mut batch, stats)?;
            }
        }
    } else {
        let mut stmt = snap.conn().prepare("SELECT * FROM dirs ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            push_dir(&mut batch, dir_row(r)?)?;
            if batch.len() >= BATCH {
                flush_dirs(&mut batch, stats)?;
            }
        }
    }
    flush_dirs(&mut batch, stats)?;

    // files (the buffer never exceeds BATCH rows, even inside one wide directory).
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
                if files.len() >= BATCH {
                    check(cancel)?;
                    stats.files += files.len() as u64;
                    writer.put_files(&files)?;
                    files.clear();
                }
                Ok(())
            })?;
            check(cancel)?;
        }
    } else {
        let mut stmt = snap.conn().prepare("SELECT * FROM files ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            files.push(file_row(r)?);
            if files.len() >= BATCH {
                check(cancel)?;
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
        stats.link_sets += sets.len() as u64;
        writer.put_link_sets(&sets)?;
        check(cancel)?;
    }
    // Every stored instance, streamed uncapped (v1 in `(ino, rowid)` order, v2 in `(ino, dir_id, name)` order), including
    // instances of inodes without a `link_sets` row (unresolved partial scans hold them).
    let mut batch: Vec<LinkInstanceRow> = Vec::with_capacity(BATCH);
    if snap.is_v2() {
        snap.v2_for_each_link_instance(&mut |row| {
            batch.push(row);
            if batch.len() >= BATCH {
                check(cancel)?;
                stats.link_instances += batch.len() as u64;
                writer.put_link_instances(&batch)?;
                batch.clear();
            }
            Ok(())
        })?;
    } else {
        // Keyset pages on (ino, rowid): `li_ino` is (ino, rowid), so no sorter is needed.
        let (mut ino, mut rowid) = (i64::MIN, i64::MIN);
        loop {
            let mut stmt = snap.conn().prepare_cached(V1_INSTANCE_PAGE)?;
            let mut rows = stmt.query(params![ino, rowid, BATCH as i64])?;
            let mut n = 0usize;
            while let Some(r) = rows.next()? {
                ino = r.get(3)?;
                rowid = r.get(4)?;
                batch.push(crate::reader::instance_row(r)?);
                n += 1;
            }
            drop(rows);
            if n == 0 {
                break;
            }
            check(cancel)?;
            stats.link_instances += batch.len() as u64;
            writer.put_link_instances(&batch)?;
            batch.clear();
        }
    }
    stats.link_instances += batch.len() as u64;
    writer.put_link_instances(&batch)?;

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
                check(cancel)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_failure(extended: i32) -> StoreError {
        StoreError::Sql(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(extended),
            None,
        ))
    }

    #[test]
    fn s3_quota_and_enospc_io_errors_are_space() {
        for ext in [IOERR_WRITE, IOERR_FSYNC, IOERR_TRUNCATE] {
            for errno in [rustix::io::Errno::DQUOT, rustix::io::Errno::NOSPC] {
                let out = classify_with(sqlite_failure(ext), || Some(errno.raw_os_error()));
                assert!(
                    matches!(out, StoreError::Space(_)),
                    "{ext} {errno:?}: {out:?}"
                );
            }
        }
        let full = classify_with(sqlite_failure(13), || None);
        assert!(
            matches!(full, StoreError::Space(_)),
            "SQLITE_FULL: {full:?}"
        );
    }

    #[test]
    fn s3_unrelated_io_errors_are_not_space() {
        let eio = rustix::io::Errno::IO.raw_os_error();
        for probe in [Some(eio), None] {
            let out = classify_with(sqlite_failure(IOERR_WRITE), || probe);
            assert!(matches!(out, StoreError::Sql(_)), "{out:?}");
        }
        // Other I/O codes (read) never consult the probe.
        let out = classify_with(sqlite_failure(266), || {
            Some(rustix::io::Errno::DQUOT.raw_os_error())
        });
        assert!(matches!(out, StoreError::Sql(_)), "{out:?}");
    }

    #[test]
    fn s3_probe_reports_no_errno_on_a_healthy_directory() {
        let dir = test_support::private_tempdir().unwrap();
        assert_eq!(probe_write_errno(dir.path()), None);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// S2: the v1 instance page is served by `li_ino` without a sorter.
    #[test]
    fn s2_v1_instance_page_needs_no_temp_btree() {
        let dir = test_support::private_tempdir().unwrap();
        let db = dir.path().join("v1.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE link_instances (ino INTEGER NOT NULL, dir_id INTEGER NOT NULL, name BLOB);
             CREATE INDEX li_ino ON link_instances(ino);
             CREATE INDEX li_dir ON link_instances(dir_id);",
        )
        .unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {V1_INSTANCE_PAGE}"))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map(params![0i64, 0i64, 10i64], |r| r.get::<_, String>(3))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(!plan.is_empty());
        for line in &plan {
            assert!(!line.contains("TEMP B-TREE"), "sorter in plan: {plan:?}");
        }
        assert!(plan.iter().any(|l| l.contains("li_ino")), "{plan:?}");
    }
}
