//! Owned by: W1-E
//! Bulk snapshot construction with checked values and immutable publication.

use crate::{error::sql_u64, space_guard::SpaceVerdict, StoreError};
use rusqlite::{params, Connection};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tidemark_core::snapshot::*;

pub(crate) const SCHEMA: &str = include_str!("../migrations/snapshot-v1.sql");
/// An unpublished snapshot; dropping it removes its temporary file.
pub struct SnapshotWriter {
    conn: Option<Connection>,
    tmp: PathBuf,
    final_path: PathBuf,
    meta: SnapshotMeta,
    last_dir: Option<u32>,
    rows_since_check: u64,
    check_interval: u64,
    check: Option<Box<dyn FnMut() -> SpaceVerdict + Send>>,
    failed: bool,
    hooks: WriterHooks,
}
type ClassifyFn = Box<dyn Fn(StoreError) -> StoreError + Send>;
type FaultFn = Box<dyn Fn(&str) -> Option<StoreError> + Send>;
/// Callbacks that run while the writer and its temporary file still exist.
#[derive(Default)]
pub(crate) struct WriterHooks {
    /// Reclassifies a failure of `create`/`finish_with` before cleanup frees any space.
    pub(crate) classify: Option<ClassifyFn>,
    /// Test fault injection: `Some(error)` fails the named stage (`create`, `finish`).
    pub(crate) fault: Option<FaultFn>,
}
impl WriterHooks {
    fn fail(&self, stage: &str) -> Result<(), StoreError> {
        match self.fault.as_ref().and_then(|f| f(stage)) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    fn map(&self, e: StoreError) -> StoreError {
        match &self.classify {
            Some(c) => c(e),
            None => e,
        }
    }
}
impl SnapshotWriter {
    /// Create a private temporary file, refusing an existing scan id.
    pub fn create(dir: &Path, meta: &SnapshotMeta) -> Result<Self, StoreError> {
        Self::create_with(dir, meta, WriterHooks::default())
    }
    /// [`create`](Self::create) with hooks; a failure after the temporary file exists is
    /// passed through `hooks.classify` before the file is removed.
    pub(crate) fn create_with(
        dir: &Path,
        meta: &SnapshotMeta,
        hooks: WriterHooks,
    ) -> Result<Self, StoreError> {
        if meta.schema_version != 1 {
            return Err(StoreError::Invalid("snapshot schema must be 1".into()));
        }
        let directory = crate::state_dir::StateDir::snapshot_dir(dir, true)?;
        let final_path = dir.join(format!("{}.db", meta.scan_id));
        if directory.check_leaf(
            final_path
                .file_name()
                .ok_or_else(|| StoreError::Invalid("snapshot name missing".into()))?,
        )? {
            return Err(
                std::io::Error::new(std::io::ErrorKind::AlreadyExists, "scan id exists").into(),
            );
        }
        let tmp = dir.join(format!("{}.db.tmp", meta.scan_id));
        let name = tmp
            .file_name()
            .ok_or_else(|| StoreError::Invalid("snapshot name missing".into()))?;
        directory.check_leaf(name)?;
        // A preexisting temporary belongs to another/abandoned publication.
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::NONBLOCK
                    | rustix::fs::OFlags::CLOEXEC)
                    .bits() as i32,
            )
            .open(&tmp)?;
        let mut writer = Self {
            conn: None,
            tmp,
            final_path,
            meta: meta.clone(),
            last_dir: None,
            rows_since_check: 0,
            check_interval: 100_000,
            check: None,
            failed: false,
            hooks,
        };
        let init = (|| {
            writer.hooks.fail("create")?;
            writer.init_db(file, &directory)
        })();
        init.map_err(|e| writer.hooks.map(e))?;
        Ok(writer)
    }
    fn init_db(
        &mut self,
        file: File,
        directory: &crate::state_dir::StateDir,
    ) -> Result<(), StoreError> {
        let writer = self;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        drop(file);
        directory.check_leaf(
            writer
                .tmp
                .file_name()
                .ok_or_else(|| StoreError::Invalid("snapshot name missing".into()))?,
        )?;
        let conn = Connection::open(&writer.tmp)?;
        conn.execute_batch("PRAGMA page_size=32768; PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA locking_mode=EXCLUSIVE; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-262144; PRAGMA user_version=1;")?;
        conn.execute_batch(
            SCHEMA
                .split("-- Created AFTER bulk load")
                .next()
                .ok_or_else(|| StoreError::Invalid("missing schema".into()))?,
        )?;
        writer.conn = Some(conn);
        Ok(())
    }
    fn connection(&self) -> Result<&Connection, StoreError> {
        if self.failed {
            return Err(StoreError::Invalid(
                "writer was aborted by an earlier error".into(),
            ));
        }
        self.conn
            .as_ref()
            .ok_or_else(|| StoreError::Invalid("writer closed".into()))
    }
    fn before_row(&mut self) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::Invalid("writer failed".into()));
        }
        self.rows_since_check += 1;
        if self.rows_since_check >= self.check_interval {
            self.rows_since_check = 0;
            if let Some(check) = self.check.as_mut() {
                if let SpaceVerdict::Refuse { reason } = check() {
                    self.failed = true;
                    return Err(StoreError::Space(reason));
                }
            }
        }
        Ok(())
    }
    /// Insert dirs rows, committing in batches of at most 50,000.
    pub fn put_dirs(&mut self, rows: &[DirRow]) -> Result<(), StoreError> {
        let result = (|| {
            for chunk in rows.chunks(50_000) {
                self.connection()?.execute_batch("BEGIN")?;
                for r in chunk {
                    self.before_row()?;
                    if self.last_dir.is_some_and(|id| r.id <= id) {
                        return Err(StoreError::Invalid("directory ids must increase".into()));
                    }
                    self.last_dir = Some(r.id);
                    self.connection()?.prepare_cached("INSERT INTO dirs VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")?.execute(params![r.id,r.parent_id,r.subtree_end,r.depth,&r.name,r.path.as_bytes(),r.path_hash,sql_u64(r.ino)?,r.mtime,sql_u64(r.dir_disk)?,sql_u64(r.dir_apparent)?,r.projid,r.entries,r.errors,sql_u64(r.own_files)?,sql_u64(r.own_symlinks)?,sql_u64(r.own_charged)?,sql_u64(r.own_apparent)?,sql_u64(r.t_dirs)?,sql_u64(r.t_files)?,sql_u64(r.t_inodes)?,sql_u64(r.t_disk)?,sql_u64(r.t_apparent)?,sql_u64(r.t_charged)?,sql_u64(r.t_reclaimable)?,sql_u64(r.t_errors)?,sql_u64(r.age0)?,sql_u64(r.age1)?,sql_u64(r.age2)?,sql_u64(r.age3)?,sql_u64(r.age4)?,sql_u64(r.age5)?,r.newest_mtime,r.max_entries])?;
                }
                self.connection()?.execute_batch("COMMIT")?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            if let Some(conn) = &self.conn {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }
    /// Insert files rows, committing in batches of at most 50,000.
    pub fn put_files(&mut self, rows: &[FileRow]) -> Result<(), StoreError> {
        let result = (|| {
            for chunk in rows.chunks(50_000) {
                self.connection()?.execute_batch("BEGIN")?;
                for r in chunk {
                    self.before_row()?;
                    self.connection()?
                        .prepare_cached("INSERT INTO files VALUES (?,?,?,?,?,?,?,?)")?
                        .execute(params![
                            r.id,
                            r.dir_id,
                            r.name.as_ref().map(|n| n.as_bytes()),
                            sql_u64(r.ino)?,
                            sql_u64(r.disk)?,
                            sql_u64(r.apparent)?,
                            r.mtime,
                            r.nlink
                        ])?;
                }
                self.connection()?.execute_batch("COMMIT")?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            if let Some(conn) = &self.conn {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }
    /// Insert link_sets rows, committing in batches of at most 50,000.
    pub fn put_link_sets(&mut self, rows: &[LinkSetRow]) -> Result<(), StoreError> {
        let result = (|| {
            for chunk in rows.chunks(50_000) {
                self.connection()?.execute_batch("BEGIN")?;
                for r in chunk {
                    self.before_row()?;
                    self.connection()?
                        .prepare_cached("INSERT INTO link_sets VALUES (?,?,?,?,?,?,?,?)")?
                        .execute(params![
                            sql_u64(r.ino)?,
                            r.nlink,
                            r.found,
                            r.complete,
                            sql_u64(r.disk)?,
                            sql_u64(r.apparent)?,
                            r.mtime,
                            r.lca_dir_id
                        ])?;
                }
                self.connection()?.execute_batch("COMMIT")?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            if let Some(conn) = &self.conn {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }
    /// Insert link_instances rows, committing in batches of at most 50,000.
    pub fn put_link_instances(&mut self, rows: &[LinkInstanceRow]) -> Result<(), StoreError> {
        let result = (|| {
            for chunk in rows.chunks(50_000) {
                self.connection()?.execute_batch("BEGIN")?;
                for r in chunk {
                    self.before_row()?;
                    self.connection()?
                        .prepare_cached("INSERT INTO link_instances VALUES (?,?,?)")?
                        .execute(params![
                            sql_u64(r.ino)?,
                            r.dir_id,
                            r.name.as_ref().map(|n| n.as_bytes())
                        ])?;
                }
                self.connection()?.execute_batch("COMMIT")?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            if let Some(conn) = &self.conn {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }
    /// Insert skipped rows, committing in batches of at most 50,000.
    pub fn put_skipped(&mut self, rows: &[SkipRow]) -> Result<(), StoreError> {
        let result = (|| {
            for chunk in rows.chunks(50_000) {
                self.connection()?.execute_batch("BEGIN")?;
                for r in chunk {
                    self.before_row()?;
                    self.connection()?
                        .prepare_cached("INSERT INTO skipped VALUES (?,?,?)")?
                        .execute(params![r.dir_id, r.name.as_bytes(), enum_text(&r.reason)?])?;
                }
                self.connection()?.execute_batch("COMMIT")?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            if let Some(conn) = &self.conn {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }
    /// Insert errors rows, committing in batches of at most 50,000.
    pub fn put_errors(&mut self, rows: &[ErrorRow]) -> Result<(), StoreError> {
        let result = (|| {
            for chunk in rows.chunks(50_000) {
                self.connection()?.execute_batch("BEGIN")?;
                for r in chunk {
                    self.before_row()?;
                    self.connection()?
                        .prepare_cached("INSERT INTO errors VALUES (?,?,?,?)")?
                        .execute(params![
                            r.dir_id,
                            r.path.as_bytes(),
                            enum_text(&r.op)?,
                            r.errno
                        ])?;
                }
                self.connection()?.execute_batch("COMMIT")?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            if let Some(conn) = &self.conn {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }
    /// Insert annotations rows, committing in batches of at most 50,000.
    pub fn put_annotations(&mut self, rows: &[AnnotationRow]) -> Result<(), StoreError> {
        let result = (|| {
            for chunk in rows.chunks(50_000) {
                self.connection()?.execute_batch("BEGIN")?;
                for r in chunk {
                    self.before_row()?;
                    self.connection()?
                        .prepare_cached("INSERT INTO annotations VALUES (?,?,?)")?
                        .execute(params![r.dir_id, &r.key, &r.value])?;
                }
                self.connection()?.execute_batch("COMMIT")?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
            if let Some(conn) = &self.conn {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }
    /// Configure periodic space checks; zero means check before every row.
    pub fn space_check_every(&mut self, rows: u64, check: Box<dyn FnMut() -> SpaceVerdict + Send>) {
        self.check_interval = rows.max(1);
        self.rows_since_check = 0;
        self.check = Some(check);
    }
    /// Build indexes and metadata, sync, and publish without replacing any file.
    pub fn finish(self, final_meta: &SnapshotMeta) -> Result<PathBuf, StoreError> {
        self.finish_with(final_meta, &|| false)
    }
    /// Like `finish`, but calls `cancelled` before building indexes, after each index,
    /// before the final metadata write, and immediately before the rename.
    /// Cancellation removes the temporary file and returns `StoreError::Cancelled`.
    /// After rename the snapshot is complete and `cancelled` is not consulted again.
    pub fn finish_with(
        mut self,
        final_meta: &SnapshotMeta,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PathBuf, StoreError> {
        // Classify while `self` (and its temporary file) still exists.
        let result = self.finish_steps(final_meta, cancelled);
        result.map_err(|e| self.hooks.map(e))
    }
    fn finish_steps(
        &mut self,
        final_meta: &SnapshotMeta,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PathBuf, StoreError> {
        self.hooks.fail("finish")?;
        if final_meta.scan_id != self.meta.scan_id
            || final_meta.root != self.meta.root
            || final_meta.schema_version != 1
        {
            return Err(StoreError::Invalid(
                "final snapshot identity differs".into(),
            ));
        }
        let check_cancel = || {
            if cancelled() {
                Err(StoreError::Cancelled)
            } else {
                Ok(())
            }
        };
        check_cancel()?;
        let indexes = SCHEMA
            .split("-- Created AFTER bulk load (see §2):")
            .nth(1)
            .ok_or_else(|| StoreError::Invalid("missing indexes".into()))?;
        for index in indexes.split(';').filter(|sql| !sql.trim().is_empty()) {
            self.connection()?.execute_batch(index)?;
            check_cancel()?;
        }
        self.connection()?.execute_batch("ANALYZE; BEGIN;")?;
        check_cancel()?;
        write_meta(self.connection()?, final_meta)?;
        self.connection()?.execute_batch("COMMIT")?;
        let conn = self
            .conn
            .take()
            .ok_or_else(|| StoreError::Invalid("writer closed".into()))?;
        conn.close().map_err(|(_, e)| StoreError::Sql(e))?;
        File::open(&self.tmp)?.sync_all()?;
        check_cancel()?;
        // RENAME_NOREPLACE preserves a concurrent publisher with the same scan id.
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            &self.tmp,
            rustix::fs::CWD,
            &self.final_path,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        File::open(
            self.final_path
                .parent()
                .ok_or_else(|| StoreError::Invalid("missing parent".into()))?,
        )?
        .sync_all()?;
        Ok(self.final_path.clone())
    }
    /// Discard the unpublished snapshot.
    pub fn abort(self) {}
}
impl Drop for SnapshotWriter {
    fn drop(&mut self) {
        self.conn.take();
        let _ = fs::remove_file(&self.tmp);
    }
}
pub(crate) fn enum_text<T: serde::Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| StoreError::Invalid("expected string enum".into()))
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 15)]));
    }
    out
}
fn write_meta(conn: &Connection, meta: &SnapshotMeta) -> Result<(), StoreError> {
    if !meta.gate_stats.mean_rate.is_finite() || !meta.gate_stats.min_rate.is_finite() {
        return Err(StoreError::Invalid("nonfinite gate statistic".into()));
    }
    for &n in meta.phase_ms.values() {
        sql_u64(n)?;
    }
    for n in [
        meta.gate_stats.throttled_ms,
        meta.gate_stats.decreases,
        meta.gate_stats.increases,
    ] {
        sql_u64(n)?;
    }
    let mut stmt = conn.prepare("INSERT INTO meta VALUES (?,?)")?;
    stmt.execute(params!["schema_version", meta.schema_version.to_string()])?;
    stmt.execute(params!["tool_version", meta.tool_version.clone()])?;
    stmt.execute(params!["scan_id", meta.scan_id.to_string()])?;
    stmt.execute(params!["root", hex(meta.root.as_bytes())])?;
    stmt.execute(params!["root_display", meta.root_display.clone()])?;
    stmt.execute(params!["mode", enum_text(&meta.mode)?])?;
    stmt.execute(params!["mode_reason", meta.mode_reason.clone()])?;
    stmt.execute(params!["status", enum_text(&meta.status)?])?;
    stmt.execute(params![
        "degraded",
        if meta.degraded { "1" } else { "0" }.to_owned()
    ])?;
    stmt.execute(params!["exclude", serde_json::to_string(&meta.exclude)?])?;
    stmt.execute(params!["link_instances", enum_text(&meta.link_instances)?])?;
    stmt.execute(params!["started_at", meta.started_at.to_string()])?;
    stmt.execute(params!["finished_at", meta.finished_at.to_string()])?;
    stmt.execute(params!["host", meta.host.clone()])?;
    stmt.execute(params!["kernel", meta.kernel.clone()])?;
    stmt.execute(params!["os_release", meta.os_release.clone()])?;
    stmt.execute(params!["fs_uuid", meta.fs_uuid.clone()])?;
    stmt.execute(params!["fs_dev", sql_u64(meta.fs_dev)?.to_string()])?;
    stmt.execute(params![
        "fs_blocksize",
        sql_u64(meta.fs_blocksize)?.to_string()
    ])?;
    stmt.execute(params!["threads", meta.threads.to_string()])?;
    stmt.execute(params!["throttle_profile", meta.throttle_profile.clone()])?;
    stmt.execute(params!["dirs", sql_u64(meta.dirs)?.to_string()])?;
    stmt.execute(params!["files", sql_u64(meta.files)?.to_string()])?;
    stmt.execute(params!["inodes", sql_u64(meta.inodes)?.to_string()])?;
    stmt.execute(params!["errors", sql_u64(meta.errors)?.to_string()])?;
    stmt.execute(params!["races", sql_u64(meta.races)?.to_string()])?;
    stmt.execute(params!["skipped", sql_u64(meta.skipped)?.to_string()])?;
    stmt.execute(params![
        "noatime_fallbacks",
        sql_u64(meta.noatime_fallbacks)?.to_string()
    ])?;
    stmt.execute(params!["phase_ms", serde_json::to_string(&meta.phase_ms)?])?;
    stmt.execute(params![
        "gate_stats",
        serde_json::to_string(&meta.gate_stats)?
    ])?;
    Ok(())
}
