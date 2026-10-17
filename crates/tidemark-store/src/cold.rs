//! Owned by: WSNAP-cold-store
//! Bounded cold headers and streaming, crash-safe snapshot compression.
use crate::{error::sql_u64, reader::Snapshot, state_dir::StateDir, StoreError};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tidemark_core::{
    path::RawPath,
    snapshot::{DirRow, SnapshotMeta},
};
type Result<T> = std::result::Result<T, StoreError>;
const MAGIC: &[u8; 8] = b"TMCOLD02";
const MAX_HEADER: usize = 1024 * 1024;
/// Cold container metadata; reading this never inflates the SQLite payload.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColdHeader {
    /// SQLite user_version.
    pub user_version: u32,
    /// All original meta table values, without reinterpretation.
    pub meta: BTreeMap<String, String>,
    /// Root counters used by catalog reconstruction.
    pub root: DirRow,
    /// Expected payload length, stored in the binary prefix.
    #[serde(skip)]
    pub uncompressed_len: u64,
}
impl ColdHeader {
    /// Decode and validate the original snapshot metadata.
    pub fn metadata(&self) -> Result<SnapshotMeta> {
        let meta = decode_meta(&self.meta)?;
        for value in [
            self.root.ino,
            self.root.dir_disk,
            self.root.dir_apparent,
            self.root.own_files,
            self.root.own_symlinks,
            self.root.own_charged,
            self.root.own_apparent,
            self.root.t_dirs,
            self.root.t_files,
            self.root.t_inodes,
            self.root.t_disk,
            self.root.t_apparent,
            self.root.t_charged,
            self.root.t_reclaimable,
            self.root.t_errors,
            self.root.age0,
            self.root.age1,
            self.root.age2,
            self.root.age3,
            self.root.age4,
            self.root.age5,
        ] {
            sql_u64(value)?;
        }
        if !matches!(self.user_version, 1 | 2)
            || meta.schema_version != self.user_version
            || meta.status != tidemark_core::scan::SnapshotStatus::Complete
            || self.root.depth != 0
            || self.root.id != 0
            || self.root.parent_id.is_some()
            || self.root.path != meta.root
            || self.uncompressed_len == 0
            || self.uncompressed_len > i64::MAX as u64
        {
            return Err(StoreError::Invalid(
                "invalid cold header identity or length".into(),
            ));
        }
        Ok(meta)
    }
}
fn parent(path: &Path) -> Result<(StateDir, &OsStr)> {
    let dir = StateDir::snapshot_dir(&crate::state_dir::file_parent(path)?, false)?;
    let name = path
        .file_name()
        .ok_or_else(|| StoreError::Invalid("missing cold filename".into()))?;
    Ok((dir, name))
}
fn open(path: &Path) -> Result<File> {
    let (dir, name) = parent(path)?;
    let file = dir.open_leaf(name, false, false)?;
    if crate::space_guard::mount_id(&file)? != crate::space_guard::mount_id(dir.fd())? {
        return Err(StoreError::Invalid("cold leaf crosses a mount".into()));
    }
    Ok(file)
}
fn read_header(file: &mut File) -> Result<ColdHeader> {
    let mut prefix = [0; 20];
    file.read_exact(&mut prefix)?;
    if &prefix[..8] != MAGIC {
        return Err(StoreError::Invalid("bad cold magic".into()));
    }
    let mut length = [0; 8];
    length.copy_from_slice(&prefix[8..16]);
    let mut json_len = [0; 4];
    json_len.copy_from_slice(&prefix[16..]);
    let n = u32::from_le_bytes(json_len) as usize;
    if n == 0 || n > MAX_HEADER {
        return Err(StoreError::Invalid("cold header exceeds bound".into()));
    }
    let mut json = vec![0; n];
    file.read_exact(&mut json)?;
    let mut checksum = [0; 8];
    file.read_exact(&mut checksum)?;
    if u64::from_le_bytes(checksum) != xxhash_rust::xxh3::xxh3_64(&json) {
        return Err(StoreError::Invalid("cold header checksum mismatch".into()));
    }
    let mut header: ColdHeader = serde_json::from_slice(&json)?;
    header.uncompressed_len = u64::from_le_bytes(length);
    header.metadata()?;
    Ok(header)
}
/// Read a bounded, validated cold header without decompressing the frame.
pub fn read_cold_header(path: &Path) -> Result<ColdHeader> {
    read_header(&mut open(path)?)
}
// Read all catalog-visible values with the same bounded metadata policy as compression.
fn snapshot_header(snapshot: &Snapshot, length: u64) -> Result<ColdHeader> {
    let mut meta = BTreeMap::new();
    let mut stmt = snapshot
        .conn()
        .prepare("SELECT key,value FROM meta ORDER BY key")?;
    let mut rows = stmt.query([])?;
    let mut size = 0usize;
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        size = size.saturating_add(key.len()).saturating_add(value.len());
        if size > MAX_HEADER {
            return Err(StoreError::Invalid("cold metadata exceeds bound".into()));
        }
        meta.insert(key, value);
    }
    Ok(ColdHeader {
        user_version: snapshot.meta().schema_version,
        meta,
        root: snapshot.root()?,
        uncompressed_len: length,
    })
}
fn checked_snapshot(path: &Path, scan: &str) -> Result<Snapshot> {
    let snapshot = Snapshot::open(path)?;
    let check: String = snapshot
        .conn()
        .pragma_query_value(None, "quick_check", |r| r.get(0))?;
    if check != "ok" || snapshot.meta().scan_id.to_string() != scan {
        return Err(StoreError::Invalid(
            "invalid snapshot integrity or identity".into(),
        ));
    }
    snapshot.root()?;
    Ok(snapshot)
}
/// Check hot SQLite integrity, supported version, filename identity and root presence.
pub(crate) fn valid_hot(path: &Path) -> Result<Snapshot> {
    let scan = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| StoreError::Invalid("invalid hot filename".into()))?;
    checked_snapshot(path, scan)
}
fn header_matches_snapshot(header: &ColdHeader, snapshot: &Snapshot) -> Result<()> {
    let actual = snapshot_header(snapshot, header.uncompressed_len)?;
    if header.user_version != actual.user_version
        || header.meta != actual.meta
        || serde_json::to_value(&header.root)? != serde_json::to_value(&actual.root)?
    {
        return Err(StoreError::Invalid(
            "cold header disagrees with SQLite snapshot".into(),
        ));
    }
    Ok(())
}
/// Fully validate a cold member using a guarded, bounded-memory inflate leaf.
pub(crate) fn valid_cold(path: &Path) -> Result<()> {
    let header = read_cold_header(path)?;
    let scan = header.metadata()?.scan_id.to_string();
    if path.file_name() != Some(OsStr::new(&format!("{scan}.db.zst"))) {
        return Err(StoreError::Invalid(
            "cold filename identity mismatch".into(),
        ));
    }
    let snapshot = checked_snapshot(path, &scan)?;
    header_matches_snapshot(&header, &snapshot)
}
/// Stream a complete snapshot to an exclusive temporary, sync, rename without replacement,
/// then sync its directory. Catalog update and source unlink belong to the caller.
pub fn compress_snapshot(path: &Path, level: i32, cancelled: &dyn Fn() -> bool) -> Result<PathBuf> {
    if !(1..=19).contains(&level) {
        return Err(StoreError::Invalid("cold level outside 1..=19".into()));
    }
    if path.extension() != Some(OsStr::new("db")) {
        return Err(StoreError::Invalid(
            "compression source must be a .db snapshot".into(),
        ));
    }
    let snapshot = Snapshot::open(path)?;
    if snapshot.meta().status != tidemark_core::scan::SnapshotStatus::Complete {
        return Err(StoreError::Invalid(
            "only complete snapshots can become cold".into(),
        ));
    }
    let mut source = open(path)?;
    let header = snapshot_header(&snapshot, source.metadata()?.len())?;
    header.metadata()?;
    let json = serde_json::to_vec(&header)?;
    if json.len() > MAX_HEADER {
        return Err(StoreError::Invalid("cold header exceeds bound".into()));
    }
    let destination = path.with_extension("db.zst");
    let temporary = path.with_extension("db.zst.tmp");
    let (dir, name) = parent(&temporary)?;
    if cancelled() {
        return Err(StoreError::Cancelled);
    }
    let mut output = dir.create_leaf(name)?;
    output.write_all(MAGIC)?;
    output.write_all(&header.uncompressed_len.to_le_bytes())?;
    output.write_all(&(json.len() as u32).to_le_bytes())?;
    output.write_all(&json)?;
    output.write_all(&xxhash_rust::xxh3::xxh3_64(&json).to_le_bytes())?;
    let mut encoder = zstd::stream::write::Encoder::new(output, level)?;
    encoder.include_checksum(true)?;
    let mut buffer = [0; 128 * 1024];
    loop {
        if cancelled() {
            return Err(StoreError::Cancelled);
        }
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        encoder.write_all(&buffer[..n])?;
    }
    encoder.finish()?.sync_all()?;
    if cancelled() {
        return Err(StoreError::Cancelled);
    }
    let dest_name = destination
        .file_name()
        .ok_or_else(|| StoreError::Invalid("missing destination".into()))?;
    rustix::fs::renameat_with(
        dir.fd(),
        name,
        dir.fd(),
        dest_name,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)?;
    rustix::fs::fsync(dir.fd()).map_err(std::io::Error::from)?;
    Ok(destination)
}
fn inflate(file: &mut File, output: &mut impl Write, length: u64) -> Result<()> {
    // Require the single standard zstd frame's content-checksum flag; decoding
    // alone would also accept frames that omit the checksum entirely.
    let mut prefix = [0; 5];
    file.read_exact(&mut prefix)?;
    if prefix[..4] != [0x28, 0xb5, 0x2f, 0xfd] || prefix[4] & 4 == 0 {
        return Err(StoreError::Invalid(
            "cold frame requires zstd checksum".into(),
        ));
    }
    let input = std::io::Cursor::new(prefix).chain(file);
    let mut decoder = zstd::stream::read::Decoder::new(input)?.single_frame();
    decoder.window_log_max(27)?;
    let n = std::io::copy(&mut decoder.by_ref().take(length.saturating_add(1)), output)?;
    if n != length {
        return Err(StoreError::Invalid("cold payload length mismatch".into()));
    }
    let mut reader = decoder.finish();
    let mut trailing = [0];
    if reader.read(&mut trailing)? != 0 {
        return Err(StoreError::Invalid("trailing cold payload".into()));
    }
    Ok(())
}
/// Validate the complete frame without materialising a SQLite file.
/// This checks framing only; destructive transitions require `matches_hot`.
pub fn validate_cold(path: &Path) -> Result<()> {
    let mut source = open(path)?;
    let header = read_header(&mut source)?;
    inflate(&mut source, &mut std::io::sink(), header.uncompressed_len)
}
/// Prove that a cold payload reproduces the immutable hot source, with bounded buffers.
pub(crate) fn matches_hot(cold: &Path, hot: &Path) -> Result<()> {
    struct Compare(File);
    impl Write for Compare {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let mut buffer = [0; 128 * 1024];
            for chunk in bytes.chunks(buffer.len()) {
                self.0.read_exact(&mut buffer[..chunk.len()])?;
                if &buffer[..chunk.len()] != chunk {
                    return Err(std::io::Error::other(
                        "cold payload differs from hot snapshot",
                    ));
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut source = open(cold)?;
    let header = read_header(&mut source)?;
    header_matches_snapshot(&header, &valid_hot(hot)?)?;
    let mut compare = Compare(open(hot)?);
    inflate(&mut source, &mut compare, header.uncompressed_len)?;
    if compare.0.read(&mut [0])? != 0 {
        return Err(StoreError::Invalid(
            "hot snapshot exceeds cold payload".into(),
        ));
    }
    Ok(())
}
/// Inflate into an exclusively created private leaf, checking the exact payload length.
/// Removes the destination on any decoding failure.
pub fn inflate_to(path: &Path, dest: &Path) -> Result<()> {
    let mut source = open(path)?;
    let header = read_header(&mut source)?;
    let (dir, name) = parent(dest)?;
    let mut output = dir.create_leaf(name)?;
    let result = inflate(&mut source, &mut output, header.uncompressed_len);
    if result.is_err() {
        dir.unlink_leaf(name)?;
    }
    result
}
/// Lifetime guard for a private inflated snapshot and its cleanup lock.
pub(crate) struct Inflated {
    pub(crate) path: PathBuf,
    _lock: File,
    dir: StateDir,
}
impl Drop for Inflated {
    fn drop(&mut self) {
        if let Some(name) = self.path.file_name() {
            let _ = self.dir.unlink_leaf(name);
        }
    }
}
pub(crate) fn prepare(path: &Path, guard: impl FnOnce(u64) -> Result<()>) -> Result<Inflated> {
    let mut source = open(path)?;
    let header = read_header(&mut source)?;
    guard(header.uncompressed_len)?;
    let (root, _) = parent(path)?;
    let dir = root.subdir(OsStr::new(".inflate"), true)?;
    if rustix::fs::fstat(dir.fd())
        .map_err(std::io::Error::from)?
        .st_mode
        & 0o077
        != 0
    {
        return Err(StoreError::Invalid(
            "inflate directory must be private (0700)".into(),
        ));
    }
    // Coordinate creation with the sweeper until the new leaf's flock is held.
    rustix::fs::flock(dir.fd(), rustix::fs::FlockOperation::LockShared)
        .map_err(std::io::Error::from)?;
    let base = crate::state_dir::file_parent(path)?.join(".inflate");
    let scan = header.metadata()?.scan_id;
    for n in 0..10_000u32 {
        let name = format!("{scan}.{}.{n}.db", std::process::id());
        let output = match dir.create_leaf(OsStr::new(&name)) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        };
        let mut leaf = Inflated {
            path: base.join(name),
            _lock: output,
            dir,
        };
        rustix::fs::flock(&leaf._lock, rustix::fs::FlockOperation::LockExclusive)
            .map_err(std::io::Error::from)?;
        rustix::fs::flock(leaf.dir.fd(), rustix::fs::FlockOperation::Unlock)
            .map_err(std::io::Error::from)?;
        inflate(&mut source, &mut leaf._lock, header.uncompressed_len)?;
        return Ok(leaf);
    }
    Err(StoreError::Invalid("inflate namespace exhausted".into()))
}
fn decode_meta(values: &BTreeMap<String, String>) -> Result<SnapshotMeta> {
    let get = |key: &str| -> Result<String> {
        values
            .get(key)
            .cloned()
            .ok_or_else(|| StoreError::Invalid(format!("missing cold metadata {key}")))
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
