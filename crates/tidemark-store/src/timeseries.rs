//! Owned by: W1-E
//! WAL-backed samples and idempotent last-value downsampling.
use crate::{
    catalog::root_slug,
    error::{sql_limit, sql_u64},
    StoreError,
};
use rusqlite::{params, Connection, OpenFlags};
use std::os::unix::ffi::OsStrExt;
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};
use tidemark_core::{config::SampleRetention, live::*, path::RawPath, snapshot::DirRow};
pub(crate) const SCHEMA: &str = include_str!("../migrations/timeseries-v1.sql");
/// Long-lived single-writer time-series database.
///
/// Query results are capped at 1,000,000 rows; narrow ranges exceeding that bound.
/// Writable handles retain a shared writer lease until their SQLite connection closes.
pub struct TimeSeries {
    pub(crate) conn: Connection,
    _lease: Option<crate::lease::WriterLease>,
}
/// Counts changed by an idempotent compaction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CompactStats {
    /// Coarser gauge rows inserted.
    pub inserted: u64,
    /// Expired rows deleted across all tables.
    pub deleted: u64,
}
pub(crate) fn open_writer_connection(
    path: &Path,
) -> Result<(Connection, crate::lease::WriterLease), StoreError> {
    let parent = crate::state_dir::file_parent(path)?;
    // Inspect before taking shared ownership: migration requires absence of all writers.
    let initialize = match open_connection(path, false, None) {
        Ok(_) => false,
        Err(StoreError::Invalid(ref text)) if text == "uninitialized timeseries database" => true,
        Err(StoreError::Io(ref error)) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(StoreError::Sql(ref error))
            if error
                .sqlite_error()
                .is_some_and(|e| e.extended_code & 255 == 14)
                && !path.exists() =>
        {
            true
        }
        Err(error) => return Err(error),
    };
    if initialize {
        let lease =
            crate::lease::RecoveryLease::acquire(&parent, std::time::Duration::from_secs(2))?;
        let conn = open_connection(path, true, Some(&lease))?;
        drop(conn);
        drop(lease);
    }
    let lease = crate::lease::WriterLease::acquire(&parent)?;
    let conn = open_connection(path, true, None)?;
    Ok((conn, lease))
}
pub(crate) fn open_connection(
    path: &Path,
    rw: bool,
    recovery: Option<&crate::lease::RecoveryLease>,
) -> Result<Connection, StoreError> {
    let parent = crate::state_dir::file_parent(path)?;
    let name = path
        .file_name()
        .ok_or_else(|| StoreError::Invalid("database name missing".into()))?;
    let directory = crate::state_dir::StateDir::open(&parent, rw)?;
    let check = || -> Result<(), StoreError> {
        directory.check_leaf(name)?;
        for suffix in [
            "-wal",
            "-shm",
            "-journal",
            ".bak-v0",
            ".bak-v0-wal",
            ".bak-v0-shm",
            ".bak-v0-journal",
        ] {
            let mut companion = name.to_os_string();
            companion.push(suffix);
            directory.check_leaf(&companion)?;
        }
        Ok(())
    };
    check()?;
    let existing = directory.check_leaf(name)?;
    if existing {
        preflight_header(path)?;
    }
    if rw && !existing {
        recovery
            .ok_or_else(|| {
                StoreError::Invalid("database creation requires exclusive writer ownership".into())
            })?
            .verify(&parent)?;
        directory.open_leaf(name, true, true)?;
    }
    check()?;
    let flags = if rw {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let conn = Connection::open_with_flags(path, flags)?;
    // SQLite scratch must not create files outside Tidemark state.
    conn.execute_batch("PRAGMA temp_store=MEMORY;")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if !(0..=1).contains(&version) {
        return Err(StoreError::Invalid(format!(
            "unsupported timeseries version {version}"
        )));
    }
    if version == 0 {
        if !rw {
            return Err(StoreError::Invalid(
                "uninitialized timeseries database".into(),
            ));
        }
        let lease = recovery.ok_or_else(|| {
            StoreError::Invalid("initialization requires exclusive writer ownership".into())
        })?;
        lease.verify(&parent)?;
        if existing {
            backup_v0(&conn, path, lease)?;
        }
        migration_v1(&conn)?;
    }
    if rw {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    }
    Ok(conn)
}
// Database backup creation requires absence of all live writer handles.
fn backup_v0(
    conn: &Connection,
    path: &Path,
    lease: &crate::lease::RecoveryLease,
) -> Result<(), StoreError> {
    lease.verify(&crate::state_dir::file_parent(path)?)?;
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".bak-v0");
    let backup = std::path::PathBuf::from(backup);
    if backup.try_exists()? {
        return Err(StoreError::Invalid(
            "migration backup already exists".into(),
        ));
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&backup)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    let target = crate::catalog::SqlPath(backup.as_os_str().as_bytes());
    conn.execute("VACUUM INTO ?", [target])?;
    std::fs::File::open(&backup)?.sync_all()?;
    if let Some(parent) = backup.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
// Inspect only our own state file before SQLite can mutate its sidecars.
fn preflight_header(path: &Path) -> Result<(), StoreError> {
    let parent = crate::state_dir::file_parent(path)?;
    let name = path
        .file_name()
        .ok_or_else(|| StoreError::Invalid("database name missing".into()))?;
    let directory = crate::state_dir::StateDir::open(&parent, false)?;
    let mut file = directory.open_leaf(name, false, false)?;
    let mut header = [0; 16];
    let mut len = 0;
    while len < header.len() {
        match file.read(&mut header[len..]) {
            Ok(0) => break,
            Ok(n) => len += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    if len != 0 && (len != header.len() || &header != b"SQLite format 3\0") {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_NOTADB),
            Some("invalid timeseries database header".into()),
        )
        .into());
    }
    Ok(())
}
fn migration_v1(conn: &Connection) -> Result<(), StoreError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.pragma_update(None, "user_version", 1)?;
    tx.commit()?;
    Ok(())
}
fn resolution(from: i64, to: i64) -> i64 {
    let span = to.saturating_sub(from);
    if span <= 172800 {
        0
    } else if span <= 3024000 {
        1
    } else {
        2
    }
}
impl TimeSeries {
    /// Open a sample database; writable opens apply numbered migrations.
    pub fn open(path: &Path, rw: bool) -> Result<Self, StoreError> {
        let (conn, lease) = if rw {
            let (conn, lease) = open_writer_connection(path)?;
            (conn, Some(lease))
        } else {
            (open_connection(path, false, None)?, None)
        };
        Ok(Self {
            conn,
            _lease: lease,
        })
    }
    /// Append fs_samples rows atomically with checked integer values.
    pub fn insert_fs(&self, rows: &[FsSample]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt=tx.prepare_cached("INSERT INTO fs_samples (ts,res,mount,size,used,avail,inodes,inodes_used) VALUES (?,?,?,?,?,?,?,?)")?;
            for r in rows {
                stmt.execute(params![
                    r.ts,
                    0,
                    &r.mount,
                    sql_u64(r.size)?,
                    sql_u64(r.used)?,
                    sql_u64(r.avail)?,
                    sql_u64(r.inodes)?,
                    sql_u64(r.inodes_used)?
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Append lvm_samples rows atomically with checked integer values.
    pub fn insert_lvm(&self, rows: &[LvmSample]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt=tx.prepare_cached("INSERT INTO lvm_samples (ts,res,vg,lv,lv_size,vg_free,data_pct,meta_pct) VALUES (?,?,?,?,?,?,?,?)")?;
            for r in rows {
                if r.data_pct.is_some_and(|v| !v.is_finite())
                    || r.meta_pct.is_some_and(|v| !v.is_finite())
                {
                    return Err(StoreError::Invalid("nonfinite LVM gauge".into()));
                }
                stmt.execute(params![
                    r.ts,
                    0,
                    &r.vg,
                    &r.lv,
                    sql_u64(r.lv_size)?,
                    sql_u64(r.vg_free)?,
                    r.data_pct,
                    r.meta_pct
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Append hot_minutes rows atomically with checked integer values.
    pub fn insert_hot_minutes(&self, rows: &[HotMinute]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt=tx.prepare_cached("INSERT INTO hot_minutes (mount,scope,key,ts,events,files,bytes_delta,lossy) VALUES (?,?,?,?,?,?,?,?)")?;
            for r in rows {
                stmt.execute(params![
                    &r.mount,
                    &r.scope,
                    &r.key,
                    r.ts,
                    sql_u64(r.events)?,
                    sql_u64(r.files)?,
                    r.bytes_delta,
                    r.lossy
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Append phantom_samples rows atomically with checked integer values.
    pub fn insert_phantom(&self, rows: &[PhantomSample]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt=tx.prepare_cached("INSERT INTO phantom_samples (ts,mount,pid,comm,unit,path,dev,ino,bytes) VALUES (?,?,?,?,?,?,?,?,?)")?;
            for r in rows {
                stmt.execute(params![
                    r.ts,
                    &r.mount,
                    r.pid,
                    &r.comm,
                    &r.unit,
                    &r.path,
                    sql_u64(r.dev)?,
                    sql_u64(r.ino)?,
                    sql_u64(r.bytes)?
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Append writer_samples rows atomically with checked integer values.
    pub fn insert_writers(&self, rows: &[WriterSample]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt=tx.prepare_cached("INSERT INTO writer_samples (ts,mount,pid,comm,unit,path,dev,ino,size,disk) VALUES (?,?,?,?,?,?,?,?,?,?)")?;
            for r in rows {
                stmt.execute(params![
                    r.ts,
                    &r.mount,
                    r.pid,
                    &r.comm,
                    &r.unit,
                    &r.path,
                    sql_u64(r.dev)?,
                    sql_u64(r.ino)?,
                    sql_u64(r.size)?,
                    sql_u64(r.disk)?
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Record at most 20,000 supplied series rows, largest disk usage first.
    ///
    /// The caller selects rows through its configured series_depth.
    pub fn insert_dir_series(
        &self,
        root: &RawPath,
        scan_id: &str,
        ts: i64,
        rows: &[DirRow],
    ) -> Result<(), StoreError> {
        // Keep bounded references even when a caller submits a huge scan.
        let mut selected = std::collections::BinaryHeap::new();
        for (i, row) in rows.iter().enumerate() {
            selected.push(std::cmp::Reverse((
                row.t_disk,
                std::cmp::Reverse(&row.path),
                std::cmp::Reverse(i),
            )));
            if selected.len() > 20_000 {
                selected.pop();
            }
        }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached("INSERT INTO dir_series VALUES (?,?,?,?,?,?,?,?)")?;
            let slug = root_slug(root);
            for std::cmp::Reverse((_, _, std::cmp::Reverse(i))) in selected.into_sorted_vec() {
                let r = &rows[i];
                stmt.execute(params![
                    slug,
                    r.path_hash,
                    ts,
                    scan_id,
                    r.path.as_bytes(),
                    sql_u64(r.t_disk)?,
                    sql_u64(r.t_reclaimable)?,
                    sql_u64(r.t_files)?
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Append an alert state transition.
    pub fn insert_alert(&self, a: &AlertEvent) -> Result<(), StoreError> {
        if a.value.is_some_and(|v| !v.is_finite()) {
            return Err(StoreError::Invalid("nonfinite alert gauge".into()));
        }
        self.conn.execute(
            "INSERT INTO alerts (ts,rule,subject,state,value,message) VALUES (?,?,?,?,?,?)",
            params![
                a.ts,
                a.rule,
                a.subject,
                crate::writer::enum_text(&a.state)?,
                a.value,
                a.message
            ],
        )?;
        Ok(())
    }
    /// Query fs_samples in timestamp order at a resolution selected by range length.
    pub fn query_fs(&self, mount: &str, from: i64, to: i64) -> Result<Vec<FsSample>, StoreError> {
        let available: Option<i64> = self.conn.query_row(
            "SELECT MIN(res) FROM fs_samples WHERE mount=? AND ts BETWEEN ? AND ?",
            params![mount, from, to],
            |r| r.get(0),
        )?;
        let desired = resolution(from, to).max(available.unwrap_or(0));
        // Prefer the requested tier when present, retaining newer finer samples.
        let sql = FS_QUERY;
        let mut stmt = self.conn.prepare(sql)?;
        let result = stmt
            .query_map(params![mount, from, to, desired, desired, from, to], |r| {
                Ok(FsSample {
                    ts: r.get(0)?,
                    mount: r.get(1)?,
                    size: crate::error::read_u64(r, 2)?,
                    used: crate::error::read_u64(r, 3)?,
                    avail: crate::error::read_u64(r, 4)?,
                    inodes: crate::error::read_u64(r, 5)?,
                    inodes_used: crate::error::read_u64(r, 6)?,
                })
            })?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Query lvm_samples in timestamp order at a resolution selected by range length.
    pub fn query_lvm(
        &self,
        vg: &str,
        lv: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<LvmSample>, StoreError> {
        let available: Option<i64> = self.conn.query_row(
            "SELECT MIN(res) FROM lvm_samples WHERE vg=? AND lv=? AND ts BETWEEN ? AND ?",
            params![vg, lv, from, to],
            |r| r.get(0),
        )?;
        let desired = resolution(from, to).max(available.unwrap_or(0));
        // Prefer the requested tier when present, retaining newer finer samples.
        let sql = LVM_QUERY;
        let mut stmt = self.conn.prepare(sql)?;
        let result = stmt
            .query_map(params![vg, lv, from, to, desired, desired, from, to], |r| {
                Ok(LvmSample {
                    ts: r.get(0)?,
                    vg: r.get(1)?,
                    lv: r.get(2)?,
                    lv_size: crate::error::read_u64(r, 3)?,
                    vg_free: crate::error::read_u64(r, 4)?,
                    data_pct: r.get(5)?,
                    meta_pct: r.get(6)?,
                })
            })?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Query retained hot-minute records, greatest event count first.
    pub fn query_hot(
        &self,
        mount: &str,
        scope: &str,
        from: i64,
        to: i64,
        limit: usize,
    ) -> Result<Vec<HotMinute>, StoreError> {
        let mut stmt=self.conn.prepare("SELECT mount,scope,key,ts,events,files,bytes_delta,lossy FROM hot_minutes WHERE mount=? AND scope=? AND ts BETWEEN ? AND ? ORDER BY events DESC,key,ts LIMIT ?")?;
        let result = stmt
            .query_map(params![mount, scope, from, to, sql_limit(limit)?], |r| {
                Ok(HotMinute {
                    mount: r.get(0)?,
                    scope: r.get(1)?,
                    key: r.get(2)?,
                    ts: r.get(3)?,
                    events: crate::error::read_u64(r, 4)?,
                    files: crate::error::read_u64(r, 5)?,
                    bytes_delta: r.get(6)?,
                    lossy: crate::reader::decode_bool(r, 7)?,
                })
            })?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Query one exact byte path's disk usage across snapshots.
    pub fn query_dir_series(
        &self,
        root: &RawPath,
        path: &RawPath,
        from: i64,
        to: i64,
    ) -> Result<Vec<(i64, u64)>, StoreError> {
        let mut stmt=self.conn.prepare("SELECT ts,t_disk FROM dir_series WHERE root_slug=? AND path_hash=? AND path=? AND ts BETWEEN ? AND ? ORDER BY ts")?;
        let result = stmt
            .query_map(
                params![root_slug(root), path.hash64(), path.as_bytes(), from, to],
                |r| Ok((r.get(0)?, crate::error::read_u64(r, 1)?)),
            )?
            .take(crate::error::MAX_QUERY_ROWS + 1)
            .collect::<rusqlite::Result<Vec<_>>>()?;
        crate::error::check_result_bound(&result)?;
        Ok(result)
    }
    /// Downsample expired gauges to each bucket's last value and remove old rows.
    pub fn compact(&self, now: i64, policy: &SampleRetention) -> Result<CompactStats, StoreError> {
        let cutoff = |d: std::time::Duration| -> Result<i64, StoreError> {
            now.checked_sub(sql_u64(d.as_secs())?)
                .ok_or_else(|| StoreError::Integer("retention cutoff".into()))
        };
        let raw_cut = cutoff(policy.raw)?;
        let raw = raw_cut
            .checked_sub(raw_cut.rem_euclid(300))
            .ok_or_else(|| StoreError::Integer("raw bucket cutoff".into()))?;
        let five_cut = cutoff(policy.five_min)?;
        let five = five_cut
            .checked_sub(five_cut.rem_euclid(3600))
            .ok_or_else(|| StoreError::Integer("hour bucket cutoff".into()))?;
        let hour = cutoff(policy.hourly)?;
        let hot = cutoff(policy.hot_minutes)?;
        let tx = self.conn.unchecked_transaction()?;
        let mut stats = CompactStats::default();
        stats.inserted+=u64::try_from(tx.execute("INSERT OR IGNORE INTO fs_samples (mount,res,ts,size,used,avail,inodes,inodes_used) SELECT a.mount,1,a.ts-((a.ts%300+300)%300),a.size,a.used,a.avail,a.inodes,a.inodes_used FROM fs_samples a WHERE a.res=0 AND a.ts<? AND NOT EXISTS (SELECT 1 FROM fs_samples b WHERE b.mount=a.mount AND b.res=0 AND b.ts<? AND b.ts>a.ts AND b.ts<a.ts-((a.ts%300+300)%300)+300 AND b.ts-((b.ts%300+300)%300)=a.ts-((a.ts%300+300)%300))",params![raw,raw])?).map_err(|_|StoreError::Integer("row count".into()))?;
        stats.inserted+=u64::try_from(tx.execute("INSERT OR IGNORE INTO fs_samples (mount,res,ts,size,used,avail,inodes,inodes_used) SELECT a.mount,2,a.ts-((a.ts%3600+3600)%3600),a.size,a.used,a.avail,a.inodes,a.inodes_used FROM fs_samples a WHERE a.res=1 AND a.ts<? AND NOT EXISTS (SELECT 1 FROM fs_samples b WHERE b.mount=a.mount AND b.res=1 AND b.ts<? AND b.ts>a.ts AND b.ts<a.ts-((a.ts%3600+3600)%3600)+3600 AND b.ts-((b.ts%3600+3600)%3600)=a.ts-((a.ts%3600+3600)%3600))",params![five,five])?).map_err(|_|StoreError::Integer("row count".into()))?;
        stats.deleted += u64::try_from(tx.execute(
            "DELETE FROM fs_samples WHERE (res=0 AND ts<?) OR (res=1 AND ts<?) OR (res=2 AND ts<?)",
            params![raw, five, hour],
        )?)
        .map_err(|_| StoreError::Integer("row count".into()))?;
        stats.inserted+=u64::try_from(tx.execute("INSERT OR IGNORE INTO lvm_samples (vg,lv,res,ts,lv_size,vg_free,data_pct,meta_pct) SELECT a.vg,a.lv,1,a.ts-((a.ts%300+300)%300),a.lv_size,a.vg_free,a.data_pct,a.meta_pct FROM lvm_samples a WHERE a.res=0 AND a.ts<? AND NOT EXISTS (SELECT 1 FROM lvm_samples b WHERE b.vg=a.vg AND b.lv=a.lv AND b.res=0 AND b.ts<? AND b.ts>a.ts AND b.ts<a.ts-((a.ts%300+300)%300)+300 AND b.ts-((b.ts%300+300)%300)=a.ts-((a.ts%300+300)%300))",params![raw,raw])?).map_err(|_|StoreError::Integer("row count".into()))?;
        stats.inserted+=u64::try_from(tx.execute("INSERT OR IGNORE INTO lvm_samples (vg,lv,res,ts,lv_size,vg_free,data_pct,meta_pct) SELECT a.vg,a.lv,2,a.ts-((a.ts%3600+3600)%3600),a.lv_size,a.vg_free,a.data_pct,a.meta_pct FROM lvm_samples a WHERE a.res=1 AND a.ts<? AND NOT EXISTS (SELECT 1 FROM lvm_samples b WHERE b.vg=a.vg AND b.lv=a.lv AND b.res=1 AND b.ts<? AND b.ts>a.ts AND b.ts<a.ts-((a.ts%3600+3600)%3600)+3600 AND b.ts-((b.ts%3600+3600)%3600)=a.ts-((a.ts%3600+3600)%3600))",params![five,five])?).map_err(|_|StoreError::Integer("row count".into()))?;
        stats.deleted+=u64::try_from(tx.execute("DELETE FROM lvm_samples WHERE (res=0 AND ts<?) OR (res=1 AND ts<?) OR (res=2 AND ts<?)",params![raw,five,hour])?).map_err(|_|StoreError::Integer("row count".into()))?;
        for (table, cut) in [
            ("hot_minutes", hot),
            ("phantom_samples", hot),
            ("writer_samples", hot),
            ("dir_series", hour),
            ("alerts", hour),
        ] {
            stats.deleted +=
                u64::try_from(tx.execute(&format!("DELETE FROM {table} WHERE ts<?"), [cut])?)
                    .map_err(|_| StoreError::Integer("row count".into()))?;
        }
        tx.commit()?;
        Ok(stats)
    }
}

const FS_QUERY: &str = "SELECT ts,mount,size,used,avail,inodes,inodes_used FROM fs_samples a WHERE mount=? AND ts BETWEEN ? AND ? AND (a.res<=? OR (a.res=1 AND NOT EXISTS (SELECT 1 FROM fs_samples f WHERE f.mount=a.mount AND f.res=0 AND f.ts>=a.ts AND f.ts<a.ts+300)) OR (a.res=2 AND NOT EXISTS (SELECT 1 FROM fs_samples f WHERE f.mount=a.mount AND f.res=0 AND f.ts>=a.ts AND f.ts<a.ts+3600) AND NOT EXISTS (SELECT 1 FROM fs_samples f WHERE f.mount=a.mount AND f.res=1 AND f.ts>=a.ts AND f.ts<a.ts+3600))) AND NOT EXISTS (SELECT 1 FROM fs_samples b WHERE b.mount=a.mount AND b.res=1 AND a.res<1 AND 1<=?5 AND b.ts=(a.ts-((a.ts%300+300)%300)) AND b.ts BETWEEN ?6 AND ?7) AND NOT EXISTS (SELECT 1 FROM fs_samples b WHERE b.mount=a.mount AND b.res=2 AND a.res<2 AND 2<=?5 AND b.ts=(a.ts-((a.ts%3600+3600)%3600)) AND b.ts BETWEEN ?6 AND ?7) ORDER BY ts";

const LVM_QUERY: &str = "SELECT ts,vg,lv,lv_size,vg_free,data_pct,meta_pct FROM lvm_samples a WHERE vg=? AND lv=? AND ts BETWEEN ? AND ? AND (a.res<=? OR (a.res=1 AND NOT EXISTS (SELECT 1 FROM lvm_samples f WHERE f.vg=a.vg AND f.lv=a.lv AND f.res=0 AND f.ts>=a.ts AND f.ts<a.ts+300)) OR (a.res=2 AND NOT EXISTS (SELECT 1 FROM lvm_samples f WHERE f.vg=a.vg AND f.lv=a.lv AND f.res=0 AND f.ts>=a.ts AND f.ts<a.ts+3600) AND NOT EXISTS (SELECT 1 FROM lvm_samples f WHERE f.vg=a.vg AND f.lv=a.lv AND f.res=1 AND f.ts>=a.ts AND f.ts<a.ts+3600))) AND NOT EXISTS (SELECT 1 FROM lvm_samples b WHERE b.vg=a.vg AND b.lv=a.lv AND b.res=1 AND a.res<1 AND 1<=?6 AND b.ts=(a.ts-((a.ts%300+300)%300)) AND b.ts BETWEEN ?7 AND ?8) AND NOT EXISTS (SELECT 1 FROM lvm_samples b WHERE b.vg=a.vg AND b.lv=a.lv AND b.res=2 AND a.res<2 AND 2<=?6 AND b.ts=(a.ts-((a.ts%3600+3600)%3600)) AND b.ts BETWEEN ?7 AND ?8) ORDER BY ts";

#[cfg(test)]
mod query_plan_tests {
    use super::*;

    #[test]
    fn tier_checks_seek_timestamp_primary_keys() {
        let tmp = ::test_support::private_tempdir().unwrap();
        let store = TimeSeries::open(&tmp.path().join("timeseries.db"), true).unwrap();
        for sql in [FS_QUERY, LVM_QUERY] {
            let mut statement = store
                .conn
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            let count = statement.parameter_count();
            let params = vec![rusqlite::types::Value::Integer(0); count];
            let details = statement
                .query_map(rusqlite::params_from_iter(params), |r| {
                    r.get::<_, String>(3)
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            let searches: Vec<_> = details
                .iter()
                .filter(|s| s.contains("SEARCH f ") || s.contains("SEARCH b "))
                .collect();
            assert_eq!(searches.len(), 5, "{details:?}");
            for detail in searches {
                assert!(
                    detail.contains("PRIMARY KEY")
                        && detail.contains("res=?")
                        && detail.contains("ts"),
                    "{detail}"
                );
            }
        }
    }
}
