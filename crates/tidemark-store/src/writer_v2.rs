//! Owned by: WSNAP-writer2
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
/// Snapshot v2 layout and compression settings. Chunk buffers never exceed 8192 rows.
#[derive(Clone, Debug)]
pub struct WriterOptions {
    /// Maximum directory depth copied into plain SQL rows.
    pub plain_depth: u32,
    /// Path lookup hash bits: 32 or 64.
    pub hash_width: u32,
    /// Directory rows per chunk (1..=8192).
    pub chunk_dirs: usize,
    /// Notable file rows per chunk (1..=8192).
    pub chunk_files: usize,
    /// Link instance rows per chunk (1..=8192).
    pub chunk_li: usize,
    /// Zstd compression level (1..=19).
    pub zstd_level: i32,
}
impl Default for WriterOptions {
    fn default() -> Self {
        Self {
            plain_depth: 2,
            hash_width: 64,
            chunk_dirs: 4096,
            chunk_files: 8192,
            chunk_li: 8192,
            zstd_level: 3,
        }
    }
}
impl WriterOptions {
    fn validate(&self) -> Result<(), StoreError> {
        crate::chunk::HashWidth::new(self.hash_width)?;
        if [self.chunk_dirs, self.chunk_files, self.chunk_li]
            .iter()
            .any(|n| !(1..=crate::chunk::MAX_ROWS).contains(n))
            || !(1..=19).contains(&self.zstd_level)
        {
            return Err(StoreError::Invalid("invalid writer options".into()));
        }
        Ok(())
    }
}
/// An unpublished snapshot; dropping it removes its temporary file.
pub struct SnapshotWriterV2 {
    conn: Option<Connection>,
    tmp: PathBuf,
    stage: Option<PathBuf>,
    options: WriterOptions,
    final_path: PathBuf,
    meta: SnapshotMeta,
    last_dir: Option<u32>,
    rows_since_check: u64,
    check_interval: u64,
    check: Option<Box<dyn FnMut() -> SpaceVerdict + Send>>,
    failed: bool,
}
impl SnapshotWriterV2 {
    /// Create a private temporary file, refusing an existing scan id.
    pub fn create(dir: &Path, meta: &SnapshotMeta) -> Result<Self, StoreError> {
        Self::create_with_options(dir, meta, WriterOptions::default())
    }
    /// Create a writer with validated chunk and lookup options.
    pub fn create_with_options(
        dir: &Path,
        meta: &SnapshotMeta,
        options: WriterOptions,
    ) -> Result<Self, StoreError> {
        options.validate()?;
        if meta.schema_version != 2 {
            return Err(StoreError::Invalid("snapshot schema must be 2".into()));
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
            stage: None,
            options,
            final_path,
            meta: meta.clone(),
            last_dir: None,
            rows_since_check: 0,
            check_interval: 100_000,
            check: None,
            failed: false,
        };
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        drop(file);
        directory.check_leaf(
            writer
                .tmp
                .file_name()
                .ok_or_else(|| StoreError::Invalid("snapshot name missing".into()))?,
        )?;
        let stage = dir.join(format!("{}.db.stage.tmp", meta.scan_id));
        directory.check_leaf(
            stage
                .file_name()
                .ok_or_else(|| StoreError::Invalid("stage name missing".into()))?,
        )?;
        let stage_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::NONBLOCK
                    | rustix::fs::OFlags::CLOEXEC)
                    .bits() as i32,
            )
            .open(&stage)?;
        writer.stage = Some(stage);
        stage_file.set_permissions(fs::Permissions::from_mode(0o600))?;
        drop(stage_file);
        directory.check_leaf(
            writer
                .stage
                .as_ref()
                .and_then(|p| p.file_name())
                .ok_or_else(|| StoreError::Invalid("stage name missing".into()))?,
        )?;
        let conn = Connection::open(
            writer
                .stage
                .as_ref()
                .ok_or_else(|| StoreError::Invalid("stage missing".into()))?,
        )?;
        conn.execute_batch("PRAGMA page_size=32768; PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA locking_mode=EXCLUSIVE; PRAGMA temp_store=FILE; PRAGMA cache_size=-8192; PRAGMA user_version=1;")?;
        conn.execute_batch(
            SCHEMA
                .split("-- Created AFTER bulk load")
                .next()
                .ok_or_else(|| StoreError::Invalid("missing schema".into()))?,
        )?;
        writer.conn = Some(conn);
        Ok(writer)
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
                    debug_assert!(
                        self.last_dir.is_none_or(|id| r.id > id),
                        "directory ids must increase"
                    );
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
                            rusqlite::types::Null,
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
        if final_meta.scan_id != self.meta.scan_id
            || final_meta.root != self.meta.root
            || final_meta.schema_version != 2
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
        let output = Connection::open(&self.tmp)?;
        output.execute_batch("PRAGMA page_size=4096; PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA cache_size=-8192; PRAGMA temp_store=FILE; PRAGMA user_version=2;")?;
        self.build_output(&output, &check_cancel)?;
        for index in [
            "CREATE INDEX shallow_parent ON shallow_dirs(parent_id)",
            "CREATE INDEX ls_lca ON link_sets(lca_dir_id)",
        ] {
            output.execute_batch(index)?;
            check_cancel()?;
        }
        check_cancel()?;
        write_meta(&output, final_meta)?;
        for (key, value) in [
            ("plain_depth", self.options.plain_depth.to_string()),
            ("hash_width", self.options.hash_width.to_string()),
            ("chunk_dirs", self.options.chunk_dirs.to_string()),
            ("chunk_files", self.options.chunk_files.to_string()),
            ("chunk_li", self.options.chunk_li.to_string()),
            ("zstd_level", self.options.zstd_level.to_string()),
        ] {
            output.execute("INSERT INTO meta VALUES (?,?)", params![key, value])?;
        }
        output.close().map_err(|(_, e)| StoreError::Sql(e))?;
        self.conn.take();
        if let Some(stage) = &self.stage {
            fs::remove_file(stage)?;
        }
        self.stage = None;
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

    fn build_output(
        &self,
        output: &Connection,
        cancelled: &dyn Fn() -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        use crate::chunk::*;
        let stage = self.connection()?;
        output.execute_batch(V2_SCHEMA)?;
        stage.execute_batch(
            "CREATE TABLE spans(dir_id INTEGER PRIMARY KEY,lo INTEGER NOT NULL,n INTEGER NOT NULL)",
        )?;
        for index in [
            "CREATE INDEX stage_files ON files(dir_id,name)",
            "CREATE INDEX stage_li ON link_instances(ino,dir_id,name)",
        ] {
            stage.execute_batch(index)?;
            cancelled()?;
        }
        // files.id is the zero-based clustered row number in v2; caller ids are discarded.
        let mut stmt = stage.prepare("SELECT * FROM files ORDER BY dir_id,name")?;
        let mut rows = stmt.query([])?;
        let mut total = 0u32;
        let mut span: Option<(u32, u32, u32)> = None;
        loop {
            cancelled()?;
            let mut buf = Vec::with_capacity(self.options.chunk_files);
            for _ in 0..self.options.chunk_files {
                let Some(row) = rows.next()? else { break };
                let r = crate::reader::file_row(row)?;
                if span.is_some_and(|s| s.0 != r.dir_id) {
                    if let Some((id, lo, n)) = span.take() {
                        stage.execute("INSERT INTO spans VALUES (?,?,?)", params![id, lo, n])?;
                    }
                }
                let current = span.get_or_insert((r.dir_id, total, 0));
                current.2 = current
                    .2
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Invalid("too many files".into()))?;
                total = total
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Invalid("too many files".into()))?;
                buf.push(ChunkFile {
                    dir_id: r.dir_id,
                    name: r.name.map(|n| n.as_bytes().to_vec()),
                    ino: r.ino,
                    disk: r.disk,
                    apparent: r.apparent,
                    mtime: r.mtime,
                    nlink: r.nlink,
                });
            }
            if buf.is_empty() {
                break;
            }
            output.execute(
                "INSERT INTO file_chunks VALUES (?,?,?,?,?)",
                params![
                    total - buf.len() as u32,
                    total - 1,
                    buf[0].dir_id,
                    buf[buf.len() - 1].dir_id,
                    encode_files_with_level(&buf, self.options.zstd_level)?
                ],
            )?;
        }
        if let Some((id, lo, n)) = span {
            stage.execute("INSERT INTO spans VALUES (?,?,?)", params![id, lo, n])?;
        }
        drop(rows);
        drop(stmt);
        let mut stmt = stage.prepare("SELECT * FROM dirs ORDER BY id")?;
        let mut rows = stmt.query([])?;
        let mut next_lo = 0u32;
        loop {
            cancelled()?;
            let mut buf = Vec::with_capacity(self.options.chunk_dirs);
            for _ in 0..self.options.chunk_dirs {
                let Some(row) = rows.next()? else { break };
                let r = crate::reader::dir_row(row)?;
                let mut spans = stage.prepare_cached("SELECT lo,n FROM spans WHERE dir_id=?")?;
                let mut found = spans.query([r.id])?;
                let (file_lo, file_count) = if let Some(row) = found.next()? {
                    (row.get::<_, u32>(0)?, row.get::<_, u32>(1)?)
                } else {
                    (next_lo, 0)
                };
                next_lo = file_lo
                    .checked_add(file_count)
                    .ok_or_else(|| StoreError::Invalid("file span overflow".into()))?;
                output.execute(
                    "INSERT INTO dir_lookup VALUES (?,?)",
                    params![
                        HashWidth::new(self.options.hash_width)?.key(r.path.hash64() as u64),
                        r.id
                    ],
                )?;
                buf.push(ChunkDir {
                    id: r.id,
                    parent_id: r.parent_id,
                    subtree_end: r.subtree_end,
                    depth: r.depth,
                    name: r.name,
                    ino: r.ino,
                    mtime: r.mtime,
                    dir_disk: r.dir_disk,
                    dir_apparent: r.dir_apparent,
                    projid: r.projid,
                    entries: r.entries,
                    errors: r.errors,
                    own_files: r.own_files,
                    own_symlinks: r.own_symlinks,
                    own_charged: r.own_charged,
                    own_apparent: r.own_apparent,
                    t_dirs: r.t_dirs,
                    t_files: r.t_files,
                    t_inodes: r.t_inodes,
                    t_disk: r.t_disk,
                    t_apparent: r.t_apparent,
                    t_charged: r.t_charged,
                    t_reclaimable: r.t_reclaimable,
                    t_errors: r.t_errors,
                    age0: r.age0,
                    age1: r.age1,
                    age2: r.age2,
                    age3: r.age3,
                    age4: r.age4,
                    age5: r.age5,
                    newest_mtime: r.newest_mtime,
                    max_entries: r.max_entries,
                    file_lo,
                    file_count,
                });
            }
            if buf.is_empty() {
                break;
            }
            output.execute(
                "INSERT INTO dir_chunks VALUES (?,?,?)",
                params![
                    buf[0].id,
                    buf[buf.len() - 1].id,
                    encode_dirs_with_level(&buf, self.options.zstd_level)?
                ],
            )?;
        }
        let mut stmt = stage.prepare("SELECT * FROM link_instances ORDER BY ino,dir_id,name")?;
        let mut rows = stmt.query([])?;
        let mut total = 0u64;
        loop {
            cancelled()?;
            let mut buf = Vec::with_capacity(self.options.chunk_li);
            for _ in 0..self.options.chunk_li {
                let Some(row) = rows.next()? else { break };
                let r = crate::reader::instance_row(row)?;
                buf.push(ChunkLinkInstance {
                    ino: r.ino,
                    dir_id: r.dir_id,
                    name: r.name.map(|n| n.as_bytes().to_vec()),
                });
            }
            if buf.is_empty() {
                break;
            }
            let first = total;
            total = total
                .checked_add(buf.len() as u64)
                .ok_or_else(|| StoreError::Invalid("too many link instances".into()))?;
            output.execute(
                "INSERT INTO li_chunks VALUES (?,?,?,?,?)",
                params![
                    sql_u64(first)?,
                    sql_u64(total - 1)?,
                    sql_u64(buf[0].ino)?,
                    sql_u64(buf[buf.len() - 1].ino)?,
                    encode_link_instances_with_level(&buf, self.options.zstd_level)?
                ],
            )?;
        }
        for table in ["link_sets", "skipped", "errors", "annotations"] {
            copy_rows(
                stage,
                output,
                &format!("SELECT * FROM {table}"),
                table,
                cancelled,
            )?;
        }
        copy_rows(stage,output,&format!("SELECT id,parent_id,subtree_end,depth,name,ino,mtime,dir_disk,dir_apparent,projid,entries,errors,own_files,own_symlinks,own_charged,own_apparent,t_dirs,t_files,t_inodes,t_disk,t_apparent,t_charged,t_reclaimable,t_errors,age0,age1,age2,age3,age4,age5,newest_mtime,max_entries FROM dirs WHERE depth <= {} ORDER BY id",self.options.plain_depth),"shallow_dirs",cancelled)?;
        Ok(())
    }
    /// Discard the unpublished snapshot.
    pub fn abort(self) {}
}
impl Drop for SnapshotWriterV2 {
    fn drop(&mut self) {
        self.conn.take();
        let _ = fs::remove_file(&self.tmp);
        if let Some(stage) = &self.stage {
            let _ = fs::remove_file(stage);
        }
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

// Copy fixed-width SQL rows without collecting a table. Both SQLite page caches are capped
// at 8 MiB; sort/index scratch uses disk, and only one codec chunk is buffered in Rust.
fn copy_rows(
    stage: &Connection,
    output: &Connection,
    query: &str,
    table: &str,
    cancelled: &dyn Fn() -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    let mut stmt = stage.prepare(query)?;
    let count = stmt.column_count();
    let sql = format!(
        "INSERT INTO {table} VALUES ({})",
        vec!["?"; count].join(",")
    );
    let mut insert = output.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut values = Vec::with_capacity(count);
    while let Some(row) = rows.next()? {
        cancelled()?;
        values.clear();
        for i in 0..count {
            values.push(row.get::<_, rusqlite::types::Value>(i)?);
        }
        insert.execute(rusqlite::params_from_iter(values.iter()))?;
    }
    Ok(())
}

const V2_SCHEMA: &str = r#"CREATE TABLE meta (key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE shallow_dirs (
  id             INTEGER PRIMARY KEY,   -- preorder index; root = 0
  parent_id      INTEGER,               -- NULL for root
  subtree_end    INTEGER NOT NULL,      -- last preorder id in this subtree (inclusive)
  depth          INTEGER NOT NULL,      -- root = 0
  name           BLOB    NOT NULL,      -- last component; root row: full root path
  ino            INTEGER NOT NULL,
  mtime          INTEGER NOT NULL,
  dir_disk       INTEGER NOT NULL,      -- this directory inode's own allocated bytes
  dir_apparent   INTEGER NOT NULL,      -- this directory inode's st_size
  projid         INTEGER,               -- bulk mode only
  entries        INTEGER NOT NULL,      -- direct entries
  errors         INTEGER NOT NULL,      -- errors in this directory itself
  own_files      INTEGER NOT NULL,      -- direct non-directory entries (link instances)
  own_symlinks   INTEGER NOT NULL,
  own_charged    INTEGER NOT NULL,      -- dir_disk + single-found inodes here + link sets whose LCA is here (03 §4)
  own_apparent   INTEGER NOT NULL,      -- same composition using apparent sizes (dir_apparent + …)
  t_dirs         INTEGER NOT NULL,
  t_files        INTEGER NOT NULL,
  t_inodes       INTEGER NOT NULL,
  t_disk         INTEGER NOT NULL,
  t_apparent     INTEGER NOT NULL,
  t_charged      INTEGER NOT NULL,
  t_reclaimable  INTEGER NOT NULL,
  t_errors       INTEGER NOT NULL,
  age0 INTEGER NOT NULL, age1 INTEGER NOT NULL, age2 INTEGER NOT NULL,
  age3 INTEGER NOT NULL, age4 INTEGER NOT NULL, age5 INTEGER NOT NULL,
  newest_mtime   INTEGER NOT NULL,
  max_entries    INTEGER NOT NULL
);
CREATE TABLE link_sets (
  ino         INTEGER PRIMARY KEY,
  nlink       INTEGER NOT NULL,
  found       INTEGER NOT NULL,
  complete    INTEGER NOT NULL,         -- found >= nlink
  disk        INTEGER NOT NULL,
  apparent    INTEGER NOT NULL,
  mtime       INTEGER NOT NULL,
  lca_dir_id  INTEGER NOT NULL
);
CREATE TABLE skipped     (dir_id INTEGER NOT NULL, name BLOB NOT NULL, reason TEXT NOT NULL);   -- mount|other_fs|excluded
CREATE TABLE errors      (dir_id INTEGER, path BLOB NOT NULL, op TEXT NOT NULL, errno INTEGER NOT NULL);
CREATE TABLE annotations (dir_id INTEGER NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL);


CREATE TABLE dir_chunks(first_id INTEGER PRIMARY KEY,last_id INTEGER NOT NULL,data BLOB NOT NULL);
CREATE TABLE file_chunks(first_row INTEGER PRIMARY KEY,last_row INTEGER NOT NULL,first_dir INTEGER NOT NULL,last_dir INTEGER NOT NULL,data BLOB NOT NULL);
CREATE TABLE li_chunks(first_row INTEGER PRIMARY KEY,last_row INTEGER NOT NULL,first_ino INTEGER NOT NULL,last_ino INTEGER NOT NULL,data BLOB NOT NULL);
CREATE TABLE dir_lookup(hash INTEGER NOT NULL,id INTEGER NOT NULL,PRIMARY KEY(hash,id)) WITHOUT ROWID;
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_codec_limits() {
        let opts = WriterOptions::default();
        assert_eq!(
            (
                opts.plain_depth,
                opts.hash_width,
                opts.chunk_dirs,
                opts.chunk_files,
                opts.chunk_li,
                opts.zstd_level
            ),
            (2, 64, 4096, 8192, 8192, 3)
        );
        assert!(opts.validate().is_ok());
        for n in [0, crate::chunk::MAX_ROWS + 1, usize::MAX] {
            for column in 0..3 {
                let mut opts = WriterOptions::default();
                match column {
                    0 => opts.chunk_dirs = n,
                    1 => opts.chunk_files = n,
                    _ => opts.chunk_li = n,
                };
                assert!(opts.validate().is_err());
            }
        }
    }
    #[test]
    fn final_meta_validation_removes_staging_and_output() {
        let tmp = test_support::private_tempdir().unwrap();
        let mut meta = crate::test_meta();
        meta.schema_version = 2;
        let writer = SnapshotWriterV2::create(tmp.path(), &meta).unwrap();
        meta.gate_stats.mean_rate = f64::NAN;
        assert!(writer.finish(&meta).is_err());
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
    }
}
