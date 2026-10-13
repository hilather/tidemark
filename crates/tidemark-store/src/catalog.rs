//! Owned by: W1-E
//! Scan catalog shared with the long-lived sample database.
use crate::{error::sql_u64, reader::Snapshot, StoreError};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::{
    fs,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf},
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
        crate::state_dir::StateDir::open(state_dir, rw)?;
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
                if !file.file_name().as_bytes().ends_with(b".db") {
                    continue;
                }
                root_dir.check_leaf(&file.file_name())?;
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

/// Recover with a retained scan-lock descriptor, verifying its identity and exclusion.
/// All local catalog/sample connections must be closed before calling this function.
/// The legacy recovery entry point remains for existing callers; production callers
/// should use this checked entry point.
pub fn recover_catalog_with_lock(
    state_dir: &Path,
    scan_lock: &fs::File,
) -> Result<Catalog, StoreError> {
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
    recover_catalog_owned(state_dir)
}

/// Preserve the shared database and sidecars, then rebuild from published snapshots.
///
/// The caller must stop writers and close all catalog/sample connections first.
/// This compatibility entry point acquires the scan lock itself; callers already
/// holding it must use `recover_catalog_with_lock` instead.
/// Historical samples remain in `Catalog::recovery_files()` and are unavailable in
/// the new database. Ordinary `Catalog::open` never performs this recovery.
pub fn recover_catalog(state_dir: &Path) -> Result<Catalog, StoreError> {
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
    let lock = verified.open_leaf(std::ffi::OsStr::new("scan.lock"), true, true)?;
    recover_catalog_with_lock(state_dir, &lock)
}
fn recover_catalog_owned(state_dir: &Path) -> Result<Catalog, StoreError> {
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

/// Repair abandoned scans and remove snapshot temporaries, best effort.
///
/// The caller MUST hold the exclusive host scan lock and must call this before
/// recording its own running scan. `now` is the repair timestamp in UTC seconds.
/// Errors are logged independently so file failures do not prevent row repair.
pub fn cleanup_abandoned_scans(state_dir: &Path, now: i64) {
    if let Err(error) = cleanup_temporaries(state_dir) {
        tracing::warn!(%error, "snapshot temporary cleanup failed");
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
                if !filename.to_bytes().ends_with(b".db.tmp") {
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
