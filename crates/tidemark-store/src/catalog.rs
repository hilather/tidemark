//! Owned by: W1-E
//! Scan catalog shared with the long-lived sample database.
use crate::{error::sql_u64, reader::Snapshot, StoreError};
use rusqlite::{params, OptionalExtension, Row};
use std::{
    fs,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf},
};
use tidemark_core::{config::RetentionConfig, path::RawPath, scan::ScanMode};
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
    /// Snapshot path resolved against the reading catalog's state directory.
    /// Stored as relative bytes; existing legacy absolute paths remain readable.
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
/// Writable handles retain a shared writer lease until their SQLite connection closes.
pub struct Catalog {
    conn: crate::timeseries::StoreConnection,
    _lease: Option<crate::lease::WriterLease>,
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
        crate::state_dir::StateDir::open(state_dir, rw)?;
        let (conn, lease) = if rw {
            let (conn, lease) =
                crate::timeseries::open_writer_connection(&state_dir.join("timeseries.db"))?;
            (conn, Some(lease))
        } else {
            (
                crate::timeseries::open_connection(&state_dir.join("timeseries.db"), false, None)?,
                None,
            )
        };
        Ok(Self {
            conn,
            state_dir: state_dir.to_owned(),
            recovery_files: Vec::new(),
            _lease: lease,
        })
    }
    /// Initialize or migrate storage while exclusive writer ownership is retained.
    /// The temporary connection closes before this function returns.
    pub fn initialize_with_lease(
        state: &Path,
        lease: &crate::lease::RecoveryLease,
    ) -> Result<(), StoreError> {
        lease.verify(state)?;
        let conn =
            crate::timeseries::open_connection(&state.join("timeseries.db"), true, Some(lease))?;
        drop(conn);
        Ok(())
    }
    /// Open normally after releasing recovery ownership, retaining artifact paths.
    pub fn open_after_recovery(state: &Path, files: Vec<PathBuf>) -> Result<Self, StoreError> {
        let mut catalog = Self::open(state, true)?;
        catalog.recovery_files = files;
        Ok(catalog)
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
            .query_map(params![root, root], |r| self.read_row(r))?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Select the newest complete scan for one root.
    pub fn latest(&self, root: &RawPath) -> Result<Option<ScanEntry>, StoreError> {
        Ok(self.conn.query_row("SELECT * FROM scans WHERE root=? AND status='complete' ORDER BY started_at DESC,scan_id DESC LIMIT 1",[root.as_bytes()],|r| self.read_row(r)).optional()?)
    }
    /// Select the newest complete scan starting at or before the supplied time.
    pub fn nearest_before(&self, root: &RawPath, ts: i64) -> Result<Option<ScanEntry>, StoreError> {
        Ok(self.conn.query_row("SELECT * FROM scans WHERE root=? AND status='complete' AND started_at<=? ORDER BY started_at DESC,scan_id DESC LIMIT 1",params![root.as_bytes(),ts],|r| self.read_row(r)).optional()?)
    }
    /// Fetch an explicitly selected scan, including partial scans.
    pub fn get(&self, scan_id: &str) -> Result<Option<ScanEntry>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT * FROM scans WHERE scan_id=?", [scan_id], |r| {
                self.read_row(r)
            })
            .optional()?)
    }
    fn relative_file(&self, file: &Path) -> Result<PathBuf, StoreError> {
        // Components removes interior dots without resolving symlinks or parents.
        let clean =
            |p: &Path| -> PathBuf { p.components().filter(|c| *c != Component::CurDir).collect() };
        let file = clean(file);
        let state = clean(&self.state_dir);
        let relative = file.strip_prefix(state).map_err(|_| {
            StoreError::Invalid("snapshot file is outside catalog state directory".into())
        })?;
        if relative.components().any(|c| c == Component::ParentDir) {
            return Err(StoreError::Invalid(
                "snapshot file contains parent traversal".into(),
            ));
        }
        Ok(relative.to_owned())
    }
    fn read_row(&self, r: &Row<'_>) -> rusqlite::Result<ScanEntry> {
        let mut entry = row(r)?;
        entry.file = entry.file.map(|file| {
            if !file.is_absolute() {
                return self.state_dir.join(file);
            }
            if file.exists() {
                return file;
            }
            let mut components = file.components();
            while let Some(component) = components.next_back() {
                if component == Component::Normal(std::ffi::OsStr::new("snapshots")) {
                    let tail = file.strip_prefix(components.as_path());
                    if let Ok(tail) = tail {
                        let resolved = self.state_dir.join(tail);
                        if resolved.exists() {
                            return resolved;
                        }
                    }
                    break;
                }
            }
            file
        });
        Ok(entry)
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
        let relative = e
            .file
            .as_deref()
            .map(|p| self.relative_file(p))
            .transpose()?;
        let file = relative.as_ref().map(|p| SqlPath(p.as_os_str().as_bytes()));
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
    /// Point an existing complete scan at its published cold file. Source unlink is separate.
    pub fn point_to_cold(&self, scan_id: &str, path: &Path) -> Result<(), StoreError> {
        let header = crate::cold::read_cold_header(path)?;
        if header.metadata()?.scan_id.to_string() != scan_id {
            return Err(StoreError::Invalid("cold scan identity mismatch".into()));
        }
        let relative = path
            .strip_prefix(&self.state_dir)
            .map_err(|_| StoreError::Invalid("cold file outside state directory".into()))?;
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(StoreError::Invalid("unsafe cold catalog path".into()));
        }
        let hot = path.with_extension("");
        let directory =
            crate::state_dir::StateDir::snapshot_dir(&crate::state_dir::file_parent(path)?, false)?;
        if directory.check_leaf(
            hot.file_name()
                .ok_or_else(|| StoreError::Invalid("missing hot name".into()))?,
        )? {
            crate::cold::matches_hot(path, &hot)?;
        }
        // FULL synchronizes the WAL commit before callers may unlink the hot source.
        self.conn.pragma_update(None, "synchronous", "FULL")?;
        self.conn.execute(
            "UPDATE scans SET file=? WHERE scan_id=? AND status='complete'",
            params![SqlPath(relative.as_os_str().as_bytes()), scan_id],
        )?;
        Ok(())
    }
    /// Reconstruct catalog records atomically from valid published snapshot files.
    ///
    /// Corrupt/unsupported files are logged and skipped; temporary files are ignored.
    /// Existing catalog rows are replaced, including records with missing files.
    pub fn rebuild_from_files(&self) -> Result<usize, StoreError> {
        self.rebuild(false)
    }
    /// Refresh published snapshots after scan retention, preserving failed and
    /// running lifecycle records that have no snapshot file.
    pub fn rebuild_after_scan(&self) -> Result<usize, StoreError> {
        self.rebuild(true)
    }
    fn rebuild(&self, preserve_lifecycle: bool) -> Result<usize, StoreError> {
        let state = crate::state_dir::StateDir::open(&self.state_dir, false)?;
        let snapshots = self.state_dir.join("snapshots");
        let snapshot_dir = match state.subdir(std::ffi::OsStr::new("snapshots"), false) {
            Ok(dir) => Some(dir),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if let Some(directory) = snapshot_dir.as_ref() {
            verify_snapshot_tree(directory, &snapshots)?;
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            if preserve_lifecycle {
                "DELETE FROM scans WHERE file IS NOT NULL OR status NOT IN ('failed','running')"
            } else {
                "DELETE FROM scans"
            },
            [],
        )?;
        if snapshot_dir.is_none() {
            tx.commit()?;
            return Ok(0);
        }
        let mut count = 0usize;
        for root in fs::read_dir(snapshots)? {
            let root = root?;
            if !root.file_type()?.is_dir() {
                if root.file_type()?.is_symlink() {
                    return Err(StoreError::Invalid(format!(
                        "state snapshot directory {}: symbolic link refused",
                        tidemark_core::path::display_lossy_bytes(
                            root.path().as_os_str().as_bytes()
                        )
                    )));
                }
                continue;
            }
            let root_dir = snapshot_dir
                .as_ref()
                .ok_or_else(|| StoreError::Invalid("snapshot directory missing".into()))?
                .subdir(&root.file_name(), false)?;
            for file in fs::read_dir(root.path())? {
                let file = file?;
                if !file.file_name().as_bytes().ends_with(b".db")
                    && !file.file_name().as_bytes().ends_with(b".db.zst")
                {
                    continue;
                }
                root_dir.check_leaf(&file.file_name())?;
                let path = file.path();
                let cold = path.as_os_str().as_bytes().ends_with(b".db.zst");
                if cold
                    && root_dir.check_leaf(
                        path.with_extension("")
                            .file_name()
                            .ok_or_else(|| StoreError::Invalid("missing hot name".into()))?,
                    )?
                    && crate::cold::valid_hot(&path.with_extension("")).is_ok()
                {
                    continue;
                }
                let loaded = if cold {
                    crate::cold::read_cold_header(&path).and_then(|h| {
                        let m = h.metadata()?;
                        if path.file_name()
                            != Some(std::ffi::OsStr::new(&format!("{}.db.zst", m.scan_id)))
                        {
                            return Err(StoreError::Invalid(
                                "cold filename identity mismatch".into(),
                            ));
                        }
                        Ok((h.root, m))
                    })
                } else {
                    // Legacy hot-only rebuild accepts historical filenames. A pair must
                    // satisfy the stricter predicate before hot can suppress cold.
                    let paired = root_dir.check_leaf(
                        path.with_extension("db.zst")
                            .file_name()
                            .ok_or_else(|| StoreError::Invalid("missing cold name".into()))?,
                    )?;
                    let snapshot = if paired {
                        crate::cold::valid_hot(&path)
                    } else {
                        Snapshot::open(&path)
                    };
                    snapshot.and_then(|s| Ok((s.root()?, s.meta().clone())))
                };
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
                    preserve_lifecycle,
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

// Validate the whole used namespace before a transaction or recovery mutates state.
// Width is streamed; only the two-level snapshot hierarchy is traversed.
fn verify_snapshot_tree(
    directory: &crate::state_dir::StateDir,
    path: &Path,
) -> Result<(), StoreError> {
    for root in fs::read_dir(path)? {
        let root = root?;
        if root.file_type()?.is_symlink() {
            return Err(StoreError::Invalid(format!(
                "state snapshot directory {}: symbolic link refused",
                tidemark_core::path::display_lossy_bytes(root.path().as_os_str().as_bytes())
            )));
        }
        if !root.file_type()?.is_dir() {
            continue;
        }
        let child = directory.subdir(&root.file_name(), false)?;
        for leaf in fs::read_dir(root.path())? {
            let leaf = leaf?;
            if leaf.file_name().as_bytes().ends_with(b".db")
                || leaf.file_name().as_bytes().ends_with(b".db.zst")
            {
                child.check_leaf(&leaf.file_name())?;
            }
        }
    }
    Ok(())
}

/// Recover with a retained scan-lock descriptor, verifying its identity and exclusion.
/// All local catalog/sample connections must be closed before calling this function.
/// This compatibility entry point acquires writer ownership with a two-second bound.
/// New callers acquire writer ownership before scan ownership and use
/// `recover_catalog_with_leases` to retain that acquisition order.
pub fn recover_catalog_with_lock(
    state_dir: &Path,
    scan_lock: &fs::File,
) -> Result<Catalog, StoreError> {
    let lease = crate::lease::RecoveryLease::acquire(state_dir, std::time::Duration::from_secs(2))?;
    let files = recover_catalog_with_leases(state_dir, scan_lock, &lease)?;
    drop(lease);
    let mut catalog = Catalog::open(state_dir, true)?;
    catalog.recovery_files = files;
    Ok(catalog)
}

/// Recover under exclusive writer ownership and retained scan ownership.
/// Close all local writers first; acquire writer ownership before scan ownership.
pub fn recover_catalog_with_leases(
    state_dir: &Path,
    scan_lock: &fs::File,
    lease: &crate::lease::RecoveryLease,
) -> Result<Vec<PathBuf>, StoreError> {
    lease.verify(state_dir)?;
    use std::os::unix::fs::MetadataExt;
    let verified = crate::state_dir::StateDir::open(state_dir, false)?;
    verified.check_leaf(std::ffi::OsStr::new("scan.lock"))?;
    let held = scan_lock.metadata()?;
    let named = fs::symlink_metadata(state_dir.join("scan.lock"))?;
    if !named.is_file() || (held.dev(), held.ino()) != (named.dev(), named.ino()) {
        return Err(StoreError::Invalid(
            "recovery requires the state directory scan lock".into(),
        ));
    }
    rustix::fs::flock(
        scan_lock,
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
    )
    .map_err(std::io::Error::from)?;
    recover_catalog_owned(state_dir, lease)
}

/// Preserve the shared database and sidecars, then rebuild from published snapshots.
///
/// The caller must stop writers and close all catalog/sample connections first.
/// This compatibility entry point acquires the scan lock itself; callers already
/// holding it must use `recover_catalog_with_lock` instead.
/// Historical samples remain in `Catalog::recovery_files()` and are unavailable in
/// the new database. Ordinary `Catalog::open` never performs this recovery.
pub fn recover_catalog(state_dir: &Path) -> Result<Catalog, StoreError> {
    let lease = crate::lease::RecoveryLease::acquire(state_dir, std::time::Duration::from_secs(2))?;
    let verified = crate::state_dir::StateDir::open(state_dir, false)?;
    for name in [
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-shm",
        "timeseries.db-journal",
        "timeseries.db.bak-v0",
        "timeseries.db.bak-v0-wal",
        "timeseries.db.bak-v0-shm",
        "timeseries.db.bak-v0-journal",
    ] {
        verified.check_leaf(std::ffi::OsStr::new(name))?;
    }
    match verified.subdir(std::ffi::OsStr::new("snapshots"), false) {
        Ok(directory) => verify_snapshot_tree(&directory, &state_dir.join("snapshots"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let lock = verified.open_leaf(std::ffi::OsStr::new("scan.lock"), true, true)?;
    let files = recover_catalog_with_leases(state_dir, &lock, &lease)?;
    drop(lock);
    drop(lease);
    let mut catalog = Catalog::open(state_dir, true)?;
    catalog.recovery_files = files;
    Ok(catalog)
}
fn recover_catalog_owned(
    state_dir: &Path,
    lease: &crate::lease::RecoveryLease,
) -> Result<Vec<PathBuf>, StoreError> {
    lease.verify(state_dir)?;
    let verified = crate::state_dir::StateDir::open(state_dir, false)?;
    for name in [
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-shm",
        "timeseries.db-journal",
        "timeseries.db.bak-v0",
        "timeseries.db.bak-v0-wal",
        "timeseries.db.bak-v0-shm",
        "timeseries.db.bak-v0-journal",
    ] {
        verified.check_leaf(std::ffi::OsStr::new(name))?;
    }
    match verified.subdir(std::ffi::OsStr::new("snapshots"), false) {
        Ok(directory) => verify_snapshot_tree(&directory, &state_dir.join("snapshots"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // Recovery ownership excludes writers. Do not allocate an artifact directory
    // for first startup when there is nothing to preserve.
    let names = [
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-shm",
        "timeseries.db-journal",
        "timeseries.db.bak-v0",
        "timeseries.db.bak-v0-wal",
        "timeseries.db.bak-v0-shm",
        "timeseries.db.bak-v0-journal",
    ];
    let mut has_artifact = false;
    for name in names {
        match fs::symlink_metadata(state_dir.join(name)) {
            Ok(_) => has_artifact = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if !has_artifact {
        let catalog = Catalog {
            conn: crate::timeseries::open_connection(
                &state_dir.join("timeseries.db"),
                true,
                Some(lease),
            )?,
            state_dir: state_dir.to_owned(),
            recovery_files: Vec::new(),
            _lease: None,
        };
        catalog.rebuild_from_files()?;
        return Ok(Vec::new());
    }
    // A private, exclusively created directory supplies a collision-free namespace.
    let mut recovery = None;
    for sequence in 0..10_000u32 {
        let path = state_dir.join(format!("timeseries-recovery-{sequence}"));
        match verified.create_subdir(
            path.file_name()
                .ok_or_else(|| StoreError::Invalid("recovery name missing".into()))?,
        ) {
            Ok(_) => {
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
    for name in [
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-shm",
        "timeseries.db-journal",
        "timeseries.db.bak-v0",
        "timeseries.db.bak-v0-wal",
        "timeseries.db.bak-v0-shm",
        "timeseries.db.bak-v0-journal",
    ] {
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
    let catalog = Catalog {
        conn: crate::timeseries::open_connection(
            &state_dir.join("timeseries.db"),
            true,
            Some(lease),
        )?,
        state_dir: state_dir.to_owned(),
        recovery_files: Vec::new(),
        _lease: None,
    };
    catalog.rebuild_from_files()?;
    drop(catalog);
    Ok(preserved)
}

/// Repair abandoned scans and remove snapshot temporaries, best effort.
///
/// The caller MUST hold the exclusive host scan lock and must call this before
/// recording its own running scan. `now` is the repair timestamp in UTC seconds.
/// Errors are logged independently so file failures do not prevent row repair.
pub fn cleanup_abandoned_scans(state_dir: &Path, now: i64) {
    cleanup_abandoned_scans_with_policy(state_dir, now, &RetentionConfig::default())
}
/// Repair abandoned scans, inflating cold pairs under `policy`.
///
/// The caller MUST hold the exclusive host scan lock and must call this before
/// recording its own running scan. `now` is the repair timestamp in UTC seconds.
/// Errors are logged independently so file failures do not prevent row repair.
pub fn cleanup_abandoned_scans_with_policy(state_dir: &Path, now: i64, policy: &RetentionConfig) {
    if let Err(error) = cleanup_temporaries(state_dir) {
        tracing::warn!(%error, "snapshot temporary cleanup failed");
    }
    if let Err(error) = cleanup_cold(state_dir, policy) {
        tracing::warn!(%error, "cold snapshot cleanup failed");
    }
    if let Err(error) = repair_running(state_dir, now) {
        tracing::warn!(%error, "abandoned scan catalog repair failed");
    }
}

fn repair_running(state_dir: &Path, now: i64) -> Result<(), StoreError> {
    let catalog = Catalog::open(state_dir, true)?;
    // One row per transaction bounds memory and logs only committed repairs.
    // A crash during cleanup leaves the remaining rows for the next lock holder.
    let mut cursor = String::new();
    loop {
        let tx = catalog.conn.unchecked_transaction()?;
        let entry = tx
            .query_row(
                "SELECT scan_id,root FROM scans WHERE scan_id>=? AND status='running' ORDER BY scan_id LIMIT 1",
                [&cursor],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        let Some((scan_id, root)) = entry else {
            break;
        };
        let root = RawPath::new(root)?;
        tx.execute("UPDATE scans SET status='failed',finished_at=?,file=NULL WHERE scan_id=? AND status='running'", params![now, scan_id])?;
        tx.commit()?;
        cursor.clone_from(&scan_id);
        tracing::warn!(root=%root.display_lossy(), %scan_id, "repaired abandoned running scan");
    }
    Ok(())
}

fn cleanup_temporaries(state_dir: &Path) -> std::io::Result<()> {
    use rustix::fs::{self as sys, Dir, FileType, Mode, OFlags};
    fn directory(
        fd: impl std::os::fd::AsFd,
        name: impl rustix::path::Arg + Copy,
    ) -> std::io::Result<rustix::fd::OwnedFd> {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        match sys::openat(&fd, name, flags | OFlags::NOATIME, Mode::empty()) {
            Err(rustix::io::Errno::PERM) => Ok(sys::openat(fd, name, flags, Mode::empty())?),
            result => Ok(result?),
        }
    }
    // A proc-fd path ending in /. is intentional: the final component is a
    // directory, while the already retained ancestor descriptor binds the state.
    let verified = crate::state_dir::StateDir::open(state_dir, false)?;
    let state = rustix::io::dup(verified.fd())?;
    let mount = crate::space_guard::mount_id(&state)?;
    let snapshot_dir = match verified.subdir(std::ffi::OsStr::new("snapshots"), false) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        result => result?,
    };
    let snapshots = rustix::io::dup(snapshot_dir.fd())?;
    if crate::space_guard::mount_id(&snapshots)? != mount {
        return Ok(());
    }
    let mut roots = Dir::new(snapshots)?;
    while let Some(entry) = roots.next() {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let result = (|| -> std::io::Result<()> {
            let bound = sys::openat(
                roots.fd()?,
                name,
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            if FileType::from_raw_mode(sys::fstat(&bound)?.st_mode) != FileType::Directory
                || crate::space_guard::mount_id(&bound)? != mount
            {
                return Ok(());
            }
            let root_dir =
                snapshot_dir.subdir(std::ffi::OsStr::from_bytes(name.to_bytes()), false)?;
            let mut files = Dir::new(directory(&bound, ".")?)?;
            while let Some(file) = files.next() {
                let file = file?;
                let filename = file.file_name();
                if !filename.to_bytes().ends_with(b".db.tmp")
                    && !filename.to_bytes().ends_with(b".db.zst.tmp")
                    && !filename.to_bytes().ends_with(b".db.stage.tmp")
                {
                    continue;
                }
                let removed = (|| -> std::io::Result<()> {
                    let file = sys::openat(
                        files.fd()?,
                        filename,
                        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )?;
                    let stat = sys::fstat(&file)?;
                    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
                        || crate::space_guard::mount_id(&file)? != mount
                    {
                        return Ok(());
                    }
                    root_dir.unlink_leaf(std::ffi::OsStr::from_bytes(filename.to_bytes()))?;
                    tracing::warn!(root=%name.to_string_lossy(), scan_id=%String::from_utf8_lossy(&filename.to_bytes()[..filename.to_bytes().len()-7]), "removed abandoned snapshot temporary");
                    Ok(())
                })();
                if let Err(error) = removed {
                    tracing::warn!(root=%name.to_string_lossy(), file=%filename.to_string_lossy(), %error, "snapshot temporary cleanup failed");
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            tracing::warn!(root=%name.to_string_lossy(), %error, "snapshot directory cleanup failed");
        }
    }
    Ok(())
}

// Resource and access failures do not prove corruption. Keep both members when
// validation cannot finish (notably a space-guard refusal or descriptor exhaustion).
// An export output collision is not a catalog-pair result either.
fn pair_validity(result: Result<(), StoreError>) -> Option<bool> {
    match result {
        Ok(()) => Some(true),
        Err(StoreError::Io(e)) => match e.kind() {
            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::InvalidData => Some(false),
            _ => None,
        },
        Err(StoreError::Sql(rusqlite::Error::SqliteFailure(e, _))) => matches!(
            e.code,
            rusqlite::ErrorCode::DatabaseCorrupt
                | rusqlite::ErrorCode::NotADatabase
                | rusqlite::ErrorCode::Unknown
        )
        .then_some(false),
        Err(
            StoreError::Space { .. }
            | StoreError::Cancelled
            | StoreError::NotYetImplemented
            | StoreError::OutputExists(_),
        ) => None,
        Err(_) => Some(false),
    }
}

// Readers retain a flock on each inflated leaf. An unlocked inode is stale regardless
// of PID reuse; cleanup does not guess process liveness or consult the wall clock.
fn cleanup_cold(state: &Path, policy: &RetentionConfig) -> Result<(), StoreError> {
    use std::ffi::OsStr;
    let verified = crate::state_dir::StateDir::open(state, false)?;
    let snapshots = match verified.subdir(OsStr::new("snapshots"), false) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mount = crate::space_guard::mount_id(snapshots.fd())?;
    for root in fs::read_dir(state.join("snapshots"))? {
        let root = root?;
        if !root.file_type()?.is_dir() {
            continue;
        }
        let dir = snapshots.subdir(&root.file_name(), false)?;
        if crate::space_guard::mount_id(dir.fd())? != mount {
            continue;
        }
        for entry in fs::read_dir(root.path())? {
            let entry = entry?;
            if entry.file_name() == ".inflate" {
                let inflate = dir.subdir(OsStr::new(".inflate"), false)?;
                if crate::space_guard::mount_id(inflate.fd())? != mount {
                    continue;
                }
                match rustix::fs::flock(
                    inflate.fd(),
                    rustix::fs::FlockOperation::NonBlockingLockExclusive,
                ) {
                    Ok(()) => {}
                    Err(rustix::io::Errno::WOULDBLOCK) => continue,
                    Err(e) => return Err(std::io::Error::from(e).into()),
                }
                for leaf in fs::read_dir(entry.path())? {
                    let leaf = leaf?;
                    if !leaf.file_name().as_bytes().ends_with(b".db") {
                        continue;
                    }
                    let file = inflate.open_leaf(&leaf.file_name(), false, false)?;
                    if crate::space_guard::mount_id(&file)? != mount {
                        continue;
                    }
                    match rustix::fs::flock(
                        &file,
                        rustix::fs::FlockOperation::NonBlockingLockExclusive,
                    ) {
                        Ok(()) => inflate.unlink_leaf(&leaf.file_name())?,
                        Err(rustix::io::Errno::WOULDBLOCK) => {}
                        Err(e) => return Err(std::io::Error::from(e).into()),
                    }
                }
            } else if entry.file_name().as_bytes().ends_with(b".db.zst") {
                let path = entry.path();
                let name = entry.file_name();
                let Some(scan) = name.to_str().and_then(|n| n.strip_suffix(".db.zst")) else {
                    continue;
                };
                let hot = format!("{scan}.db");
                if dir.check_leaf(OsStr::new(&hot))? {
                    let hot_file = dir.open_leaf(OsStr::new(&hot), false, false)?;
                    if crate::space_guard::mount_id(&hot_file)? != mount {
                        continue;
                    }

                    let hot_path = path.with_extension("");
                    if crate::cold::matches_hot(&path, &hot_path).is_ok() {
                        Catalog::open(state, true)?.point_to_cold(scan, &path)?;
                        dir.unlink_leaf(OsStr::new(&hot))?;
                        rustix::fs::fsync(dir.fd()).map_err(std::io::Error::from)?;
                        continue;
                    }
                    let hot_valid = pair_validity(crate::cold::valid_hot(&hot_path).map(|_| ()));
                    let cold_valid = pair_validity(crate::cold::valid_cold(&path, policy));
                    let (keep, bad) = match (hot_valid, cold_valid) {
                        (Some(true), Some(false)) => (&hot_path, &path),
                        (Some(false), Some(true)) => (&path, &hot_path),
                        _ => {
                            tracing::warn!(hot=%hot_path.display(), cold=%path.display(),
                                "inconclusive snapshot pair; preserving both and catalog");
                            continue;
                        }
                    };
                    // Commit the verified survivor before quarantining its sibling. Never replace
                    // an earlier quarantine: a collision leaves both published members intact.
                    let catalog = Catalog::open(state, true)?;
                    catalog.conn.pragma_update(None, "synchronous", "FULL")?;
                    catalog.conn.execute(
                        "UPDATE scans SET file=? WHERE scan_id=? AND status='complete'",
                        params![
                            SqlPath(
                                keep.strip_prefix(state)
                                    .map_err(|_| StoreError::Invalid(
                                        "snapshot outside state".into()
                                    ))?
                                    .as_os_str()
                                    .as_bytes()
                            ),
                            scan
                        ],
                    )?;
                    let bad_name = bad
                        .file_name()
                        .ok_or_else(|| StoreError::Invalid("missing quarantine source".into()))?;
                    let mut quarantine = bad_name.to_os_string();
                    quarantine.push(".bad");
                    let rename = rustix::fs::renameat_with(
                        dir.fd(),
                        bad_name,
                        dir.fd(),
                        &quarantine,
                        rustix::fs::RenameFlags::NOREPLACE,
                    );
                    if rename == Err(rustix::io::Errno::EXIST) {
                        tracing::warn!(hot=%hot_path.display(), cold=%path.display(), source=%bad.display(), destination=%bad.with_file_name(&quarantine).display(),
                            "quarantine already exists; preserving snapshot pair");
                        continue;
                    }
                    rename.map_err(std::io::Error::from)?;
                    rustix::fs::fsync(dir.fd()).map_err(std::io::Error::from)?;
                    tracing::warn!(hot=%hot_path.display(), cold=%path.display(),
                        "quarantined invalid snapshot pair member");
                } else if crate::cold::valid_cold(&path, policy).is_ok() {
                    Catalog::open(state, true)?.point_to_cold(scan, &path)?;
                }
            }
        }
    }
    Ok(())
}
