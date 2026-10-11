//! Owned by: W1-E
//! Scan catalog shared with the long-lived sample database.
use crate::{error::sql_u64, reader::Snapshot, StoreError};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::os::unix::fs::DirBuilderExt;
use std::{
    fs,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
use tidemark_core::{path::RawPath, scan::ScanMode};
/// Encode root bytes injectively for the snapshot directory name.
pub fn root_slug(root: &RawPath) -> String {
    let mut out = String::new();
    for &b in root.as_bytes() {
        match b {
            b'/' => out.push('_'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' => out.push(char::from(b)),
            _ => {
                out.push('%');
                out.push_str(&crate::writer::hex(&[b]));
            }
        }
    }
    out
}
/// One scan's catalog record, including incomplete and failed scans.
pub struct ScanEntry {
    #[doc = "Scan catalog field (05 §4)."]
    pub scan_id: String,
    #[doc = "Scan catalog field (05 §4)."]
    pub root: RawPath,
    #[doc = "Scan catalog field (05 §4)."]
    pub mode: ScanMode,
    #[doc = "Scan catalog field (05 §4)."]
    pub started_at: i64,
    #[doc = "Scan catalog field (05 §4)."]
    pub finished_at: Option<i64>,
    #[doc = "Scan catalog field (05 §4)."]
    pub status: String,
    #[doc = "Scan catalog field (05 §4)."]
    pub degraded: bool,
    #[doc = "Scan catalog field (05 §4)."]
    pub file: Option<PathBuf>,
    #[doc = "Scan catalog field (05 §4)."]
    pub t_disk: Option<u64>,
    #[doc = "Scan catalog field (05 §4)."]
    pub t_inodes: Option<u64>,
    #[doc = "Scan catalog field (05 §4)."]
    pub t_dirs: Option<u64>,
    #[doc = "Scan catalog field (05 §4)."]
    pub errors: Option<u64>,
    #[doc = "Scan catalog field (05 §4)."]
    pub duration_ms: Option<u64>,
}

/// Catalog backed by the scans table in timeseries.db.
/// Lists are capped at 1,000,000 rows; select a root when exceeding that bound.
pub struct Catalog {
    conn: Connection,
    state_dir: PathBuf,
    recovery_files: Vec<PathBuf>,
}
fn row(r: &Row<'_>) -> rusqlite::Result<ScanEntry> {
    let path = r.get::<_, Vec<u8>>(1)?;
    let root = RawPath::new(path).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e))
    })?;
    let mode: String = r.get(3)?;
    let mode = match mode.as_str() {
        "walk" => ScanMode::Walk,
        "bulk" => ScanMode::Bulk,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let degraded = match r.get::<_, i64>(7)? {
        0 => false,
        1 => true,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(ScanEntry {
        scan_id: r.get(0)?,
        root,
        mode,
        started_at: r.get(4)?,
        finished_at: r.get(5)?,
        status: r.get(6)?,
        degraded,
        file: match r.get_ref(8)? {
            rusqlite::types::ValueRef::Null => None,
            rusqlite::types::ValueRef::Text(bytes) => {
                Some(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
            }
            _ => {
                return Err(rusqlite::Error::InvalidColumnType(
                    8,
                    "file".into(),
                    r.get_ref(8)?.data_type(),
                ))
            }
        },
        t_disk: crate::error::read_optional_u64(r, 9)?,
        t_inodes: crate::error::read_optional_u64(r, 10)?,
        t_dirs: crate::error::read_optional_u64(r, 11)?,
        errors: crate::error::read_optional_u64(r, 12)?,
        duration_ms: crate::error::read_optional_u64(r, 13)?,
    })
}
pub(crate) struct SqlPath<'a>(pub(crate) &'a [u8]);
impl rusqlite::types::ToSql for SqlPath<'_> {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Borrowed(
            rusqlite::types::ValueRef::Text(self.0),
        ))
    }
}
impl Catalog {
    /// Open the source-of-truth catalog, optionally applying sample migrations.
    pub fn open(state_dir: &Path, rw: bool) -> Result<Self, StoreError> {
        if rw {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(state_dir)?;
        }
        Ok(Self {
            conn: crate::timeseries::open_connection(&state_dir.join("timeseries.db"), rw)?,
            state_dir: state_dir.to_owned(),
            recovery_files: Vec::new(),
        })
    }
    /// Paths preserved by explicit recovery; historical samples remain in these files.
    pub fn recovery_files(&self) -> &[PathBuf] {
        &self.recovery_files
    }
    /// List records newest first, optionally restricted to exact root bytes.
    pub fn list(&self, root: Option<&RawPath>) -> Result<Vec<ScanEntry>, StoreError> {
        let root = root.map(RawPath::as_bytes);
        let mut stmt = self.conn.prepare(
            "SELECT * FROM scans WHERE (? IS NULL OR root=?) ORDER BY started_at DESC,scan_id DESC",
        )?;
        let result = stmt
            .query_map(params![root, root], row)?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Select the newest complete scan for one root.
    pub fn latest(&self, root: &RawPath) -> Result<Option<ScanEntry>, StoreError> {
        Ok(self.conn.query_row("SELECT * FROM scans WHERE root=? AND status='complete' ORDER BY started_at DESC,scan_id DESC LIMIT 1",[root.as_bytes()],row).optional()?)
    }
    /// Select the newest complete scan starting at or before the supplied time.
    pub fn nearest_before(&self, root: &RawPath, ts: i64) -> Result<Option<ScanEntry>, StoreError> {
        Ok(self.conn.query_row("SELECT * FROM scans WHERE root=? AND status='complete' AND started_at<=? ORDER BY started_at DESC,scan_id DESC LIMIT 1",params![root.as_bytes(),ts],row).optional()?)
    }
    /// Fetch an explicitly selected scan, including partial scans.
    pub fn get(&self, scan_id: &str) -> Result<Option<ScanEntry>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT * FROM scans WHERE scan_id=?", [scan_id], row)
            .optional()?)
    }
    /// Record a new scan; duplicate ids are rejected.
    pub fn record_start(&self, e: &ScanEntry) -> Result<(), StoreError> {
        self.record(e, false, env!("CARGO_PKG_VERSION"))
    }
    /// Upsert the final state after a snapshot has been published.
    pub fn record_finish(&self, e: &ScanEntry) -> Result<(), StoreError> {
        self.record(e, true, env!("CARGO_PKG_VERSION"))
    }
    fn record(&self, e: &ScanEntry, replace: bool, tool_version: &str) -> Result<(), StoreError> {
        if !["running", "complete", "partial", "failed"].contains(&e.status.as_str()) {
            return Err(StoreError::Invalid("invalid scan status".into()));
        }
        let file = e.file.as_ref().map(|p| SqlPath(p.as_os_str().as_bytes()));
        let tail = if replace {
            " ON CONFLICT(scan_id) DO UPDATE SET root=excluded.root,root_slug=excluded.root_slug,mode=excluded.mode,started_at=excluded.started_at,finished_at=excluded.finished_at,status=excluded.status,degraded=excluded.degraded,file=excluded.file,t_disk=excluded.t_disk,t_inodes=excluded.t_inodes,t_dirs=excluded.t_dirs,errors=excluded.errors,duration_ms=excluded.duration_ms,tool_version=excluded.tool_version"
        } else {
            ""
        };
        self.conn.execute(
            &format!("INSERT INTO scans VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?){tail}"),
            params![
                e.scan_id,
                e.root.as_bytes(),
                root_slug(&e.root),
                crate::writer::enum_text(&e.mode)?,
                e.started_at,
                e.finished_at,
                e.status,
                e.degraded,
                file,
                e.t_disk.map(sql_u64).transpose()?,
                e.t_inodes.map(sql_u64).transpose()?,
                e.t_dirs.map(sql_u64).transpose()?,
                e.errors.map(sql_u64).transpose()?,
                e.duration_ms.map(sql_u64).transpose()?,
                tool_version
            ],
        )?;
        Ok(())
    }
    /// Reconstruct catalog records atomically from valid published snapshot files.
    ///
    /// Corrupt/unsupported files are logged and skipped; temporary files are ignored.
    /// Existing catalog rows are replaced, including records with missing files.
    pub fn rebuild_from_files(&self) -> Result<usize, StoreError> {
        let snapshots = self.state_dir.join("snapshots");
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM scans", [])?;
        if !snapshots.try_exists()? {
            tx.commit()?;
            return Ok(0);
        }
        let mut count = 0usize;
        for root in fs::read_dir(snapshots)? {
            let root = root?;
            if !root.file_type()?.is_dir() {
                continue;
            }
            for file in fs::read_dir(root.path())? {
                let file = file?;
                if !file.file_type()?.is_file() || !file.file_name().as_bytes().ends_with(b".db") {
                    continue;
                }
                let path = file.path();
                let loaded = Snapshot::open(&path).and_then(|s| Ok((s.root()?, s.meta().clone())));
                let (d, m) = match loaded {
                    Ok(rows) => rows,
                    Err(error) => {
                        tracing::warn!(path=%path.display(),%error,"skipping unreadable snapshot");
                        continue;
                    }
                };
                let duration = m
                    .finished_at
                    .checked_sub(m.started_at)
                    .and_then(|v| u64::try_from(v).ok())
                    .and_then(|v| v.checked_mul(1000));
                self.record(
                    &ScanEntry {
                        scan_id: m.scan_id.to_string(),
                        root: m.root,
                        mode: m.mode,
                        started_at: m.started_at,
                        finished_at: Some(m.finished_at),
                        status: crate::writer::enum_text(&m.status)?,
                        degraded: m.degraded,
                        file: Some(path),
                        t_disk: Some(d.t_disk),
                        t_inodes: Some(d.t_inodes),
                        t_dirs: Some(d.t_dirs),
                        errors: Some(m.errors),
                        duration_ms: duration,
                    },
                    false,
                    &m.tool_version,
                )?;
                count = count
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Integer("catalog count".into()))?;
            }
        }
        tx.commit()?;
        Ok(count)
    }
}

/// Preserve the shared database and sidecars, then rebuild from published snapshots.
///
/// The caller must stop writers and close all catalog/sample connections first.
/// Historical samples remain in `Catalog::recovery_files()` and are unavailable in
/// the new database. Ordinary `Catalog::open` never performs this recovery.
pub fn recover_catalog(state_dir: &Path) -> Result<Catalog, StoreError> {
    // A private, exclusively created directory supplies a collision-free namespace.
    let mut recovery = None;
    for sequence in 0..10_000u32 {
        let path = state_dir.join(format!("timeseries-recovery-{sequence}"));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                recovery = Some(path);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let recovery =
        recovery.ok_or_else(|| StoreError::Invalid("recovery namespace exhausted".into()))?;
    let mut preserved = Vec::new();
    for name in ["timeseries.db", "timeseries.db-wal", "timeseries.db-shm"] {
        let source = state_dir.join(name);
        let destination = recovery.join(name);
        match fs::rename(&source, &destination) {
            Ok(()) => {
                tracing::warn!(path=%destination.display(), "preserved shared database recovery artifact");
                preserved.push(destination);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    fs::File::open(&recovery)?.sync_all()?;
    fs::File::open(state_dir)?.sync_all()?;
    let mut catalog = Catalog::open(state_dir, true)?;
    catalog.recovery_files = preserved;
    catalog.rebuild_from_files()?;
    Ok(catalog)
}
