//! Owned by: W1-E
//! Immutable snapshot queries over preorder subtree ranges.
use crate::{
    error::{sql_limit, sql_u64},
    StoreError,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row};
use std::{os::unix::ffi::OsStrExt, path::Path};
use tidemark_core::{
    path::{RawName, RawPath},
    snapshot::*,
};
type Result<T> = std::result::Result<T, StoreError>;
/// A lock-free reader of a published immutable snapshot.
///
/// All queries use snapshot time and raw byte paths. Results are capped at
/// 1,000,000 rows; narrow queries exceeding that bound.
///
/// ```no_run
/// # fn main() -> Result<(), tidemark_store::StoreError> {
/// use tidemark_core::{path::RawPath, snapshot::{SortKey, FileFilter, LinkFilter}};
/// use tidemark_store::reader::Snapshot;
/// let snapshot = Snapshot::open(std::path::Path::new("scan.db"))?;
/// let root = snapshot.root()?;
/// let meta = snapshot.meta();
/// let row = snapshot.dir(root.id)?;
/// let found = snapshot.dir_by_path(&meta.root)?;
/// let children = snapshot.children(root.id, SortKey::Disk, 20)?;
/// let directories = snapshot.subtree_dirs(root.id, 2, SortKey::Name, 100)?;
/// let files = snapshot.subtree_files(root.id, &FileFilter::default(), 20)?;
/// let wide = snapshot.wide_dirs(root.id, 100_000, 20)?;
/// let sets = snapshot.link_sets_under(root.id, &LinkFilter::default(), 20)?;
/// if let Some(set) = sets.first() {
///     let instances = snapshot.link_instances(set.row.ino)?;
/// }
/// let skipped = snapshot.skipped_under(root.id)?;
/// let errors = snapshot.errors_under(root.id, 20)?;
/// let annotations = snapshot.annotations(root.id)?;
/// # Ok(()) }
/// ```
pub struct Snapshot {
    conn: Connection,
    meta: SnapshotMeta,
    file: std::path::PathBuf,
}

fn decode_path(bytes: Vec<u8>) -> rusqlite::Result<RawPath> {
    RawPath::new(bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
    })
}
fn decode_name(bytes: Vec<u8>) -> rusqlite::Result<RawName> {
    RawName::new(bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
    })
}
fn decode_enum<T: serde::de::DeserializeOwned>(s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
pub(crate) fn decode_bool(r: &Row<'_>, i: usize) -> rusqlite::Result<bool> {
    match r.get::<_, i64>(i)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(rusqlite::Error::IntegralValueOutOfRange(i, r.get(i)?)),
    }
}
pub(crate) fn dir_row(r: &Row<'_>) -> rusqlite::Result<DirRow> {
    Ok(DirRow {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        subtree_end: r.get(2)?,
        depth: r.get(3)?,
        name: r.get(4)?,
        path: decode_path(r.get(5)?)?,
        path_hash: r.get(6)?,
        ino: crate::error::read_u64(r, 7)?,
        mtime: r.get(8)?,
        dir_disk: crate::error::read_u64(r, 9)?,
        dir_apparent: crate::error::read_u64(r, 10)?,
        projid: r.get(11)?,
        entries: r.get(12)?,
        errors: r.get(13)?,
        own_files: crate::error::read_u64(r, 14)?,
        own_symlinks: crate::error::read_u64(r, 15)?,
        own_charged: crate::error::read_u64(r, 16)?,
        own_apparent: crate::error::read_u64(r, 17)?,
        t_dirs: crate::error::read_u64(r, 18)?,
        t_files: crate::error::read_u64(r, 19)?,
        t_inodes: crate::error::read_u64(r, 20)?,
        t_disk: crate::error::read_u64(r, 21)?,
        t_apparent: crate::error::read_u64(r, 22)?,
        t_charged: crate::error::read_u64(r, 23)?,
        t_reclaimable: crate::error::read_u64(r, 24)?,
        t_errors: crate::error::read_u64(r, 25)?,
        age0: crate::error::read_u64(r, 26)?,
        age1: crate::error::read_u64(r, 27)?,
        age2: crate::error::read_u64(r, 28)?,
        age3: crate::error::read_u64(r, 29)?,
        age4: crate::error::read_u64(r, 30)?,
        age5: crate::error::read_u64(r, 31)?,
        newest_mtime: r.get(32)?,
        max_entries: r.get(33)?,
    })
}
pub(crate) fn file_row(r: &Row<'_>) -> rusqlite::Result<FileRow> {
    Ok(FileRow {
        id: r.get(0)?,
        dir_id: r.get(1)?,
        name: r
            .get::<_, Option<Vec<u8>>>(2)?
            .map(decode_name)
            .transpose()?,
        ino: crate::error::read_u64(r, 3)?,
        disk: crate::error::read_u64(r, 4)?,
        apparent: crate::error::read_u64(r, 5)?,
        mtime: r.get(6)?,
        nlink: r.get(7)?,
    })
}
pub(crate) fn link_row(r: &Row<'_>) -> rusqlite::Result<LinkSetRow> {
    Ok(LinkSetRow {
        ino: crate::error::read_u64(r, 0)?,
        nlink: r.get(1)?,
        found: r.get(2)?,
        complete: decode_bool(r, 3)?,
        disk: crate::error::read_u64(r, 4)?,
        apparent: crate::error::read_u64(r, 5)?,
        mtime: r.get(6)?,
        lca_dir_id: r.get(7)?,
    })
}
pub(crate) fn instance_row(r: &Row<'_>) -> rusqlite::Result<LinkInstanceRow> {
    Ok(LinkInstanceRow {
        ino: crate::error::read_u64(r, 0)?,
        dir_id: r.get(1)?,
        name: r
            .get::<_, Option<Vec<u8>>>(2)?
            .map(decode_name)
            .transpose()?,
    })
}
pub(crate) fn skip_row(r: &Row<'_>) -> rusqlite::Result<SkipRow> {
    Ok(SkipRow {
        dir_id: r.get(0)?,
        name: decode_name(r.get(1)?)?,
        reason: decode_enum(r.get(2)?)?,
    })
}
pub(crate) fn error_row(r: &Row<'_>) -> rusqlite::Result<ErrorRow> {
    Ok(ErrorRow {
        dir_id: r.get(0)?,
        path: decode_path(r.get(1)?)?,
        op: decode_enum(r.get(2)?)?,
        errno: r.get(3)?,
    })
}
pub(crate) fn immutable_connection(path: &Path) -> Result<Connection> {
    if path.as_os_str().as_bytes().contains(&0) {
        return Err(StoreError::Invalid("snapshot path contains NUL".into()));
    }
    let mut uri = String::from("file:");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/.-_".contains(&b) {
            uri.push(char::from(b));
        } else {
            uri.push('%');
            uri.push_str(&crate::writer::hex(&[b]));
        }
    }
    uri.push_str("?immutable=1&mode=ro");
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    // Keep query sorting scratch in memory rather than writing outside state_dir.
    conn.execute_batch("PRAGMA temp_store=MEMORY;")?;
    Ok(conn)
}
fn read_meta(conn: &Connection) -> Result<SnapshotMeta> {
    let get = |key: &str| -> Result<String> {
        Ok(conn.query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0))?)
    };
    let parse = |key: &str| -> Result<serde_json::Value> { Ok(serde_json::from_str(&get(key)?)?) };
    let root = get("root")?;
    if root.len() % 2 != 0 {
        return Err(StoreError::Invalid("odd root hex".into()));
    }
    let bytes = (0..root.len())
        .step_by(2)
        .map(|i| {
            root.get(i..i + 2)
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .ok_or_else(|| StoreError::Invalid("bad root hex".into()))
        })
        .collect::<Result<Vec<_>>>()?;
    let root = RawPath::new(bytes)?;
    let meta = SnapshotMeta {
        schema_version: serde_json::from_value(parse("schema_version")?)?,
        tool_version: get("tool_version")?,
        scan_id: serde_json::from_value(serde_json::Value::String(get("scan_id")?))?,
        root,
        root_display: get("root_display")?,
        mode: serde_json::from_value(serde_json::Value::String(get("mode")?))?,
        mode_reason: get("mode_reason")?,
        status: serde_json::from_value(serde_json::Value::String(get("status")?))?,
        degraded: match get("degraded")?.as_str() {
            "0" => false,
            "1" => true,
            _ => return Err(StoreError::Invalid("bad bool".into())),
        },
        exclude: serde_json::from_value(parse("exclude")?)?,
        link_instances: serde_json::from_value(serde_json::Value::String(get("link_instances")?))?,
        started_at: serde_json::from_value(parse("started_at")?)?,
        finished_at: serde_json::from_value(parse("finished_at")?)?,
        host: get("host")?,
        kernel: get("kernel")?,
        os_release: get("os_release")?,
        fs_uuid: get("fs_uuid")?,
        fs_dev: serde_json::from_value(parse("fs_dev")?)?,
        fs_blocksize: serde_json::from_value(parse("fs_blocksize")?)?,
        threads: serde_json::from_value(parse("threads")?)?,
        throttle_profile: get("throttle_profile")?,
        dirs: serde_json::from_value(parse("dirs")?)?,
        files: serde_json::from_value(parse("files")?)?,
        inodes: serde_json::from_value(parse("inodes")?)?,
        errors: serde_json::from_value(parse("errors")?)?,
        races: serde_json::from_value(parse("races")?)?,
        skipped: serde_json::from_value(parse("skipped")?)?,
        noatime_fallbacks: serde_json::from_value(parse("noatime_fallbacks")?)?,
        phase_ms: serde_json::from_value(parse("phase_ms")?)?,
        gate_stats: serde_json::from_value(parse("gate_stats")?)?,
    };
    // Validate all unsigned metadata integers against SQLite's signed domain.
    sql_u64(meta.fs_dev)?;
    sql_u64(meta.fs_blocksize)?;
    sql_u64(meta.dirs)?;
    sql_u64(meta.files)?;
    sql_u64(meta.inodes)?;
    sql_u64(meta.errors)?;
    sql_u64(meta.races)?;
    sql_u64(meta.skipped)?;
    sql_u64(meta.noatime_fallbacks)?;
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
    Ok(meta)
}
fn order(sort: SortKey) -> &'static str {
    match sort {
        SortKey::Disk => "t_disk DESC, name",
        SortKey::Apparent => "t_apparent DESC, name",
        SortKey::Reclaimable => "t_reclaimable DESC, name",
        SortKey::Charged => "t_charged DESC, name",
        SortKey::Files => "t_files DESC, name",
        SortKey::Inodes => "t_inodes DESC, name",
        SortKey::Newest => "newest_mtime DESC, name",
        SortKey::Name => "name",
    }
}
impl Snapshot {
    /// The read-only, immutable SQLite connection of this snapshot. For streamed queries inside
    /// this crate. Callers must not write, must use bound parameters, and must detach anything
    /// they attach.
    #[allow(dead_code)] // Sibling query owners adopt this seam in their follow-up tasks.
    pub(crate) fn conn(&self) -> &rusqlite::Connection {
        &self.conn
    }
    /// The snapshot file this reader was opened from (to ATTACH it read-only and immutable from
    /// another snapshot's connection).
    #[allow(dead_code)] // Sibling query owners adopt this seam in their follow-up tasks.
    pub(crate) fn file(&self) -> &std::path::Path {
        &self.file
    }

    /// Open an immutable snapshot without acquiring SQLite locks.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), tidemark_store::StoreError> {
    /// let scan = tidemark_store::reader::Snapshot::open(std::path::Path::new("scan.db"))?;
    /// let root = scan.root()?;
    /// assert_eq!(root.id, 0);
    /// # Ok(()) }
    /// ```
    pub fn open(path: &Path) -> Result<Self> {
        let parent = path.parent().ok_or_else(|| StoreError::Invalid("snapshot parent missing".into()))?;
        let name = path.file_name().ok_or_else(|| StoreError::Invalid("snapshot name missing".into()))?;
        let directory = crate::state_dir::StateDir::snapshot_dir(parent, false)?;
        let _leaf = directory.open_leaf(name, false, false)?;
        let conn = immutable_connection(path)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != 1 {
            return Err(StoreError::Invalid(format!(
                "unsupported snapshot version {version}"
            )));
        }
        let meta = read_meta(&conn)?;
        if meta.schema_version != 1 {
            return Err(StoreError::Invalid(
                "metadata version disagrees with database".into(),
            ));
        }
        Ok(Self {
            conn,
            meta,
            file: path.to_path_buf(),
        })
    }
    /// Return scan metadata without querying the database.
    pub fn meta(&self) -> &SnapshotMeta {
        &self.meta
    }
    /// Return preorder directory zero.
    pub fn root(&self) -> Result<DirRow> {
        self.dir(0)
    }
    /// Fetch a directory by its preorder identifier.
    pub fn dir(&self, id: u32) -> Result<DirRow> {
        Ok(self
            .conn
            .query_row("SELECT * FROM dirs WHERE id=?", [id], dir_row)?)
    }
    /// Look up exact raw path bytes; the hash is only an accelerator.
    pub fn dir_by_path(&self, path: &RawPath) -> Result<Option<DirRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM dirs WHERE path_hash=? AND path=?",
                params![path.hash64(), path.as_bytes()],
                dir_row,
            )
            .optional()?)
    }
    /// Fetch direct children ordered by the requested metric and raw name.
    pub fn children(&self, id: u32, sort: SortKey, limit: usize) -> Result<Vec<DirRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT * FROM dirs WHERE parent_id=? ORDER BY {}, id LIMIT ?",
            order(sort)
        ))?;
        let rows = stmt.query_map(params![id, sql_limit(limit)?], dir_row)?;
        let result = rows
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Count direct children matching a raw-byte substring; empty matches all.
    pub fn children_count(&self, id: u32, filter: &[u8]) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT count(*) FROM dirs WHERE parent_id=?1 AND (length(?2)=0 OR instr(name,?2)>0)",
            params![id, filter],
            |r| crate::error::read_u64(r, 0),
        )?)
    }

    /// Fetch a bounded page after an exclusive sort-key/raw-name cursor.
    /// Numeric ties always use ascending raw names; name sorting uses `descending`.
    /// `reverse` returns the preceding rows in reverse order, or the last page
    /// when no cursor is supplied. Reuse parent, sort, direction and filter.
    /// Existing indexes narrow by parent, but no index covers the sort order.
    #[allow(clippy::too_many_arguments)]
    pub fn children_page(
        &self,
        id: u32,
        sort: SortKey,
        descending: bool,
        cursor: Option<&DirRow>,
        reverse: bool,
        limit: usize,
        filter: &[u8],
    ) -> Result<Vec<DirRow>> {
        if limit > crate::error::MAX_QUERY_ROWS {
            return Err(StoreError::Invalid("child page exceeds query cap".into()));
        }
        let column = match sort {
            SortKey::Disk => "t_disk",
            SortKey::Apparent => "t_apparent",
            SortKey::Reclaimable => "t_reclaimable",
            SortKey::Charged => "t_charged",
            SortKey::Files => "t_files",
            SortKey::Inodes => "t_inodes",
            SortKey::Newest => "newest_mtime",
            SortKey::Name => "name",
        };
        let key = match cursor {
            Some(r) => match sort {
                SortKey::Disk => rusqlite::types::Value::Integer(sql_u64(r.t_disk)?),
                SortKey::Apparent => rusqlite::types::Value::Integer(sql_u64(r.t_apparent)?),
                SortKey::Reclaimable => rusqlite::types::Value::Integer(sql_u64(r.t_reclaimable)?),
                SortKey::Charged => rusqlite::types::Value::Integer(sql_u64(r.t_charged)?),
                SortKey::Files => rusqlite::types::Value::Integer(sql_u64(r.t_files)?),
                SortKey::Inodes => rusqlite::types::Value::Integer(sql_u64(r.t_inodes)?),
                SortKey::Newest => rusqlite::types::Value::Integer(r.newest_mtime),
                SortKey::Name => rusqlite::types::Value::Blob(r.name.clone()),
            },
            None => rusqlite::types::Value::Null,
        };
        let primary_desc = descending ^ reverse;
        let name_desc = if sort == SortKey::Name {
            primary_desc
        } else {
            reverse
        };
        let cmp = if primary_desc { "<" } else { ">" };
        let name_cmp = if name_desc { "<" } else { ">" };
        let direction = if primary_desc { "DESC" } else { "ASC" };
        let name_direction = if name_desc { "DESC" } else { "ASC" };
        // Omit the cursor predicate entirely for the first page.
        let after = if cursor.is_some() {
            format!(" AND ({column}{cmp}?3 OR ({column}=?3 AND name{name_cmp}?4))")
        } else {
            String::new()
        };
        let mut stmt = self.conn.prepare(&format!(
            "WITH page AS (SELECT id FROM dirs WHERE parent_id=?1 AND (length(?2)=0 OR instr(name,?2)>0){after} ORDER BY {column} {direction},name {name_direction} LIMIT ?5) SELECT dirs.* FROM page JOIN dirs ON dirs.id=page.id ORDER BY {column} {direction},name {name_direction}"
        ))?;
        let result = stmt
            .query_map(
                params![
                    id,
                    filter,
                    key,
                    cursor.map(|r| r.name.as_slice()).unwrap_or_default(),
                    sql_limit(limit)?
                ],
                dir_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(result)
    }

    /// Fetch a bounded link page after an exclusive `(rowid, instance)` cursor.
    /// Order is directory ID, nullable raw name (NULL first), then rowid to
    /// distinguish repeated unresolved instances. Reverse reads preceding rows
    /// in reverse order; without a cursor it starts at the end. Cursors must
    /// come from this snapshot and inode. No existing index covers this order.
    pub fn link_instances_page(
        &self,
        ino: u64,
        cursor: Option<&(i64, LinkInstanceRow)>,
        reverse: bool,
        limit: usize,
    ) -> Result<Vec<(i64, LinkInstanceRow)>> {
        if limit > crate::error::MAX_QUERY_ROWS {
            return Err(StoreError::Invalid("link page exceeds query cap".into()));
        }
        let cmp = if reverse { "<" } else { ">" };
        let direction = if reverse { "DESC" } else { "ASC" };
        let after = if cursor.is_some() {
            format!(" AND (dir_id,name IS NOT NULL,COALESCE(name,x''),rowid){cmp}(?2,?3,?4,?5)")
        } else {
            String::new()
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT ino,dir_id,name,rowid FROM link_instances WHERE ino=?1{after} ORDER BY dir_id {direction},name {direction},rowid {direction} LIMIT ?6"
        ))?;
        let result = stmt
            .query_map(
                params![
                    sql_u64(ino)?,
                    cursor.map(|r| r.1.dir_id).unwrap_or(0),
                    cursor.is_some_and(|r| r.1.name.is_some()),
                    cursor
                        .and_then(|r| r.1.name.as_ref())
                        .map(|n| n.as_bytes())
                        .unwrap_or_default(),
                    cursor.map(|r| r.0).unwrap_or(0),
                    sql_limit(limit)?
                ],
                |r| Ok((r.get(3)?, instance_row(r)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(result)
    }

    /// Return independent largest used/reclaimable candidates through a root-relative
    /// depth, the exact eligible row count, and the immutable source path.
    /// Each family is bounded by `limit` (the existing query ceiling applies); ties use raw path bytes.
    pub fn metrics_candidates(
        &self,
        depth: u32,
        limit: usize,
    ) -> Result<(Vec<DirRow>, Vec<DirRow>, u64, std::path::PathBuf)> {
        if limit > crate::error::MAX_QUERY_ROWS {
            return Err(StoreError::Invalid(
                "metrics candidate limit exceeds 1,000,000-row query cap".into(),
            ));
        }
        sql_limit(limit)?;
        let count: i64 =
            self.conn
                .query_row("SELECT count(*) FROM dirs WHERE depth<=?", [depth], |r| {
                    r.get(0)
                })?;
        let query = |column: &str| -> Result<Vec<DirRow>> {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT * FROM dirs WHERE depth<=? ORDER BY {column} DESC, path, id LIMIT ?"
            ))?;
            let result = stmt
                .query_map(params![depth, sql_limit(limit)?], dir_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(result)
        };
        Ok((
            query("t_disk")?,
            query("t_reclaimable")?,
            count as u64,
            self.file.clone(),
        ))
    }
    /// Fetch the inclusive subtree through a relative depth, using an id range.
    pub fn subtree_dirs(
        &self,
        id: u32,
        max_depth_rel: u32,
        sort: SortKey,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        let d = self.dir(id)?;
        let depth = u64::from(d.depth) + u64::from(max_depth_rel);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT * FROM dirs WHERE id BETWEEN ? AND ? AND depth<=? ORDER BY {}, id LIMIT ?",
            order(sort)
        ))?;
        let result = stmt
            .query_map(
                params![id, d.subtree_end, sql_u64(depth)?, sql_limit(limit)?],
                dir_row,
            )?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Fetch notable files by allocated size, preserving absent bulk paths.
    pub fn subtree_files(
        &self,
        id: u32,
        filter: &FileFilter,
        limit: usize,
    ) -> Result<Vec<FileHit>> {
        let d = self.dir(id)?;
        let cutoff = |age: Option<std::time::Duration>| -> Result<Option<i64>> {
            age.map(|age| {
                let secs = sql_u64(age.as_secs())?;
                self.meta
                    .started_at
                    .checked_sub(secs)
                    .ok_or_else(|| StoreError::Integer("age cutoff".into()))
            })
            .transpose()
        };
        let old = cutoff(filter.older_than)?;
        let new = cutoff(filter.newer_than)?;
        let min = filter.min_size.map(sql_u64).transpose()?;
        let mut stmt=self.conn.prepare("SELECT f.*,d.path FROM files f JOIN dirs d ON f.dir_id=d.id WHERE d.id BETWEEN ? AND ? AND (? IS NULL OR f.disk>=?) AND (? IS NULL OR f.mtime<=?) AND (? IS NULL OR f.mtime>=?) ORDER BY f.disk DESC,d.path,f.name,f.id LIMIT ?")?;
        let result = stmt
            .query_map(
                params![
                    id,
                    d.subtree_end,
                    min,
                    min,
                    old,
                    old,
                    new,
                    new,
                    sql_limit(limit)?
                ],
                |r| {
                    let row = file_row(r)?;
                    let dir = decode_path(r.get(8)?)?;
                    let path = row.name.as_ref().map(|n| dir.join(n));
                    Ok(FileHit { row, path })
                },
            )?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Fetch subtree directories with at least the direct-entry threshold.
    pub fn wide_dirs(&self, id: u32, min_entries: u32, limit: usize) -> Result<Vec<DirRow>> {
        let d = self.dir(id)?;
        let mut stmt=self.conn.prepare("SELECT * FROM dirs WHERE id BETWEEN ? AND ? AND entries>=? ORDER BY entries DESC,name,id LIMIT ?")?;
        let result = stmt
            .query_map(
                params![id, d.subtree_end, min_entries, sql_limit(limit)?],
                dir_row,
            )?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Fetch link sets charged inside this subtree and count stored instances.
    ///
    /// Depth boundaries select sets crossing two or more regions (including outside).
    /// Glob regions are matching directory subtrees; the deepest match owns an instance.
    /// Sets must cross regions (unmatched instances share an outside region).
    pub fn link_sets_under(
        &self,
        id: u32,
        filter: &LinkFilter,
        limit: usize,
    ) -> Result<Vec<LinkSetHit>> {
        if !filter.boundary.is_empty() && filter.boundary_depth.is_some() {
            return Err(StoreError::Invalid(
                "boundary globs and depth are mutually exclusive".into(),
            ));
        }
        let d = self.dir(id)?;
        if !filter.boundary.is_empty() {
            crate::error::check_result_bound(&filter.boundary)?;
            let mut builder = globset::GlobSetBuilder::new();
            for pattern in &filter.boundary {
                builder.add(
                    globset::GlobBuilder::new(pattern)
                        .literal_separator(true)
                        .build()
                        .map_err(|e| StoreError::Invalid(format!("invalid boundary glob: {e}")))?,
                );
            }
            let globs = builder
                .build()
                .map_err(|e| StoreError::Invalid(e.to_string()))?;
            let mut stmt = self.conn.prepare(
                "SELECT id,subtree_end,path FROM dirs WHERE id BETWEEN ? AND ? ORDER BY id",
            )?;
            let mut rows = stmt.query(params![id, d.subtree_end])?;
            let mut regions = Vec::new();
            let mut stack: Vec<(u32, u32)> = Vec::new();
            while let Some(row) = rows.next()? {
                let dir_id: u32 = row.get(0)?;
                let end: u32 = row.get(1)?;
                let bytes: Vec<u8> = row.get(2)?;
                while stack.last().is_some_and(|&(_, end)| end < dir_id) {
                    stack.pop();
                }
                if globs.is_match_candidate(&globset::Candidate::from_bytes(&bytes)) {
                    stack.push((dir_id, end));
                }
                regions.push((dir_id, stack.last().map(|&(id, _)| id)));
                crate::error::check_result_bound(&regions)?;
            }
            // Stream candidates in the normal order; apply the result limit after crossing.
            let mut candidates = self.conn.prepare("SELECT l.*,d.path,(SELECT COUNT(*) FROM link_instances i WHERE i.ino=l.ino) FROM link_sets l JOIN dirs d ON d.id=l.lca_dir_id WHERE d.id BETWEEN ?1 AND ?2 AND (?3=0 OR l.complete=0) ORDER BY l.disk DESC,d.path,l.ino")?;
            let mut rows = candidates.query(params![id, d.subtree_end, filter.incomplete])?;
            let mut result = Vec::new();
            while result.len() < limit {
                let Some(row) = rows.next()? else {
                    break;
                };
                let hit = LinkSetHit {
                    row: link_row(row)?,
                    lca_path: decode_path(row.get(8)?)?,
                    instance_count: crate::error::read_u64(row, 9)?,
                };
                let mut instances = self
                    .conn
                    .prepare("SELECT dir_id FROM link_instances WHERE ino=?")?;
                let mut instance_rows = instances.query([sql_u64(hit.row.ino)?])?;
                let mut first = None;
                let mut crossing = false;
                while let Some(instance) = instance_rows.next()? {
                    let instance_id: u32 = instance.get(0)?;
                    let region = regions
                        .binary_search_by_key(&instance_id, |&(id, _)| id)
                        .ok()
                        .and_then(|i| regions[i].1);
                    match first {
                        None => first = Some(region),
                        Some(previous) if previous != region => {
                            crossing = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if crossing {
                    result.push(hit);
                    crate::error::check_result_bound(&result)?;
                }
            }
            return Ok(result);
        }
        let depth = filter
            .boundary_depth
            .map(|rel| u64::from(d.depth) + u64::from(rel))
            .map(sql_u64)
            .transpose()?;
        let mut stmt=self.conn.prepare("SELECT l.*,d.path,(SELECT COUNT(*) FROM link_instances i WHERE i.ino=l.ino) FROM link_sets l JOIN dirs d ON d.id=l.lca_dir_id WHERE d.id BETWEEN ?1 AND ?2 AND (?3=0 OR l.complete=0) AND (?5 IS NULL OR (SELECT COUNT(DISTINCT COALESCE((SELECT region.id FROM dirs region WHERE region.depth=?5 AND region.id BETWEEN ?1 AND ?2 AND i.dir_id BETWEEN region.id AND region.subtree_end ORDER BY region.id DESC LIMIT 1),-1)) FROM link_instances i WHERE i.ino=l.ino)>1) ORDER BY l.disk DESC,d.path,l.ino LIMIT ?4")?;
        let result = stmt
            .query_map(
                params![
                    id,
                    d.subtree_end,
                    filter.incomplete,
                    sql_limit(limit)?,
                    depth
                ],
                |r| {
                    Ok(LinkSetHit {
                        row: link_row(r)?,
                        lca_path: decode_path(r.get(8)?)?,
                        instance_count: crate::error::read_u64(r, 9)?,
                    })
                },
            )?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Fetch stored hard-link instances in deterministic byte-name order.
    pub fn link_instances(&self, ino: u64) -> Result<Vec<LinkInstanceRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM link_instances WHERE ino=? ORDER BY dir_id,name")?;
        let result = stmt
            .query_map([sql_u64(ino)?], instance_row)?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Fetch skipped entries in the inclusive subtree.
    pub fn skipped_under(&self, id: u32) -> Result<Vec<SkipRow>> {
        let d = self.dir(id)?;
        let mut stmt = self.conn.prepare(
            "SELECT * FROM skipped WHERE dir_id BETWEEN ? AND ? ORDER BY dir_id,name,reason",
        )?;
        let result = stmt
            .query_map(params![id, d.subtree_end], skip_row)?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Fetch errors in the subtree, including unknown-parent errors at the root.
    pub fn errors_under(&self, id: u32, limit: usize) -> Result<Vec<ErrorRow>> {
        let d = self.dir(id)?;
        let mut stmt=self.conn.prepare("SELECT * FROM errors WHERE dir_id BETWEEN ? AND ? OR (?=0 AND dir_id IS NULL) ORDER BY path,op,errno LIMIT ?")?;
        let result = stmt
            .query_map(params![id, d.subtree_end, id, sql_limit(limit)?], error_row)?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Fetch annotation key/value pairs for one directory in lexical order.
    pub fn annotations(&self, id: u32) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key,value FROM annotations WHERE dir_id=? ORDER BY key,value")?;
        let result = stmt
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    fn fixture(name: &str) -> (tempfile::TempDir, super::Snapshot) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        let mut meta = crate::test_meta();
        meta.schema_version = 1;
        let writer = crate::writer::SnapshotWriter::create(dir.path(), &meta).unwrap();
        let published = writer.finish(&meta).unwrap();
        std::fs::rename(published, &file).unwrap();
        (dir, super::Snapshot::open(&file).unwrap())
    }
    #[test]
    fn connection_streams_bound_query_rows() {
        let (_dir, snapshot) = fixture("stream.db");
        let mut statement = snapshot
            .conn()
            .prepare("SELECT key FROM meta WHERE key >= ? ORDER BY key")
            .unwrap();
        let mut rows = statement.query(["schema_version"]).unwrap();
        let mut count = 0;
        while let Some(row) = rows.next().unwrap() {
            let key: String = row.get(0).unwrap();
            assert!(key.as_str() >= "schema_version");
            count += 1;
        }
        assert!(count > 1);
        assert!(snapshot.conn().execute("DELETE FROM meta", []).is_err());
    }
}
