//! Owned by: W0-B1
//! Snapshot contracts and helpers.

use crate::{
    config::LinkInstances,
    path::{RawName, RawPath},
    scan::{ErrOp, GateStats, ScanMode, SkipReason, SnapshotStatus},
    time::ScanId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// SnapshotMeta storage contract (05 §1).
/// Consumers must use checked SQL integer conversions and checked count/ID
/// arithmetic. Bulk directory slots must fit 31 bits before setting the
/// self-reference flag; narrower internal depths must be checked, never truncated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SnapshotMetaWire", into = "SnapshotMetaWire")]
pub struct SnapshotMeta {
    /// schema_version column; sizes are bytes and timestamps Unix seconds.
    pub schema_version: u32,
    /// tool_version column; sizes are bytes and timestamps Unix seconds.
    pub tool_version: String,
    /// scan_id column; sizes are bytes and timestamps Unix seconds.
    pub scan_id: ScanId,
    /// Root bytes; report JSON uses mutually exclusive `root`/`root_b64` fields.
    pub root: RawPath,
    /// root_display column; sizes are bytes and timestamps Unix seconds.
    pub root_display: String,
    /// mode column; sizes are bytes and timestamps Unix seconds.
    pub mode: ScanMode,
    /// mode_reason column; sizes are bytes and timestamps Unix seconds.
    pub mode_reason: String,
    /// status column; sizes are bytes and timestamps Unix seconds.
    pub status: SnapshotStatus,
    /// degraded column; sizes are bytes and timestamps Unix seconds.
    pub degraded: bool,
    /// exclude column; sizes are bytes and timestamps Unix seconds.
    pub exclude: Vec<String>,
    /// link_instances column; sizes are bytes and timestamps Unix seconds.
    pub link_instances: LinkInstances,
    /// started_at column; sizes are bytes and timestamps Unix seconds.
    pub started_at: i64,
    /// finished_at column; sizes are bytes and timestamps Unix seconds.
    pub finished_at: i64,
    /// host column; sizes are bytes and timestamps Unix seconds.
    pub host: String,
    /// kernel column; sizes are bytes and timestamps Unix seconds.
    pub kernel: String,
    /// os_release column; sizes are bytes and timestamps Unix seconds.
    pub os_release: String,
    /// fs_uuid column; sizes are bytes and timestamps Unix seconds.
    pub fs_uuid: String,
    /// fs_dev column; sizes are bytes and timestamps Unix seconds.
    pub fs_dev: u64,
    /// fs_blocksize column; sizes are bytes and timestamps Unix seconds.
    pub fs_blocksize: u64,
    /// threads column; sizes are bytes and timestamps Unix seconds.
    pub threads: u32,
    /// throttle_profile column; sizes are bytes and timestamps Unix seconds.
    pub throttle_profile: String,
    /// dirs column; sizes are bytes and timestamps Unix seconds.
    pub dirs: u64,
    /// files column; sizes are bytes and timestamps Unix seconds.
    pub files: u64,
    /// inodes column; sizes are bytes and timestamps Unix seconds.
    pub inodes: u64,
    /// Total errors recorded in the scan.
    pub errors: u64,
    /// races column; sizes are bytes and timestamps Unix seconds.
    pub races: u64,
    /// skipped column; sizes are bytes and timestamps Unix seconds.
    pub skipped: u64,
    /// noatime_fallbacks column; sizes are bytes and timestamps Unix seconds.
    pub noatime_fallbacks: u64,
    /// Named phase durations in milliseconds.
    pub phase_ms: BTreeMap<String, u64>,
    /// Throttle controller statistics (W1-D).
    pub gate_stats: GateStats,
}
/// DirRow storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirRow {
    /// Row identifier; directory ids are preorder indices (root = 0).
    pub id: u32,
    /// Parent directory id; None for the root.
    pub parent_id: Option<u32>,
    /// Last preorder directory id in this subtree, inclusive.
    pub subtree_end: u32,
    /// Directory depth relative to the scan root.
    pub depth: u32,
    /// Raw name bytes; the root directory row stores the full root path.
    pub name: Vec<u8>,
    /// Full absolute normalized raw path.
    #[serde(flatten, with = "crate::path::report_path")]
    pub path: RawPath,
    /// xxh3_64 of raw path bytes reinterpreted as i64.
    pub path_hash: i64,
    /// Filesystem inode number.
    pub ino: u64,
    /// Inode modification time, Unix seconds.
    pub mtime: i64,
    /// Directory inode allocated bytes (st_blocks × 512).
    pub dir_disk: u64,
    /// Directory inode apparent bytes (st_size).
    pub dir_apparent: u64,
    /// XFS project id; None in walk mode.
    pub projid: Option<u32>,
    /// All direct entries, including subdirectories and skipped entries.
    pub entries: u32,
    /// Errors recorded directly in this directory.
    pub errors: u32,
    /// Direct non-directory entries, counting link instances.
    pub own_files: u64,
    /// Direct symlink entries (a subset of own_files).
    pub own_symlinks: u64,
    /// Directory inode bytes plus single-found inodes and link sets charged here.
    pub own_charged: u64,
    /// Apparent bytes of the same inodes as own_charged.
    pub own_apparent: u64,
    /// Subtree directory count, including this directory.
    pub t_dirs: u64,
    /// Subtree non-directory entry count, counting link instances.
    pub t_files: u64,
    /// Distinct non-directory inodes visible in the subtree.
    pub t_inodes: u64,
    /// Allocated bytes of distinct visible inodes plus directory inode bytes.
    pub t_disk: u64,
    /// Apparent bytes using the same union semantics as t_disk.
    pub t_apparent: u64,
    /// Subtree allocated bytes charged at each inode’s directory or link-set LCA.
    pub t_charged: u64,
    /// Charged bytes whose complete link sets are inside the subtree.
    pub t_reclaimable: u64,
    /// Total subtree error count.
    pub t_errors: u64,
    /// Charged bytes with mtime age less than 1 day.
    pub age0: u64,
    /// Charged bytes with mtime age 1–7 days.
    pub age1: u64,
    /// Charged bytes with mtime age 7–30 days.
    pub age2: u64,
    /// Charged bytes with mtime age 30–90 days.
    pub age3: u64,
    /// Charged bytes with mtime age 90–365 days.
    pub age4: u64,
    /// Charged bytes with mtime age at least 365 days.
    pub age5: u64,
    /// Newest mtime of every visible inode in the subtree, Unix seconds.
    pub newest_mtime: i64,
    /// Largest direct-entry count of any subtree directory.
    pub max_entries: u32,
}
/// FileRow storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRow {
    /// Notable-file row identifier, zero-based and assigned with checked conversion.
    pub id: u32,
    /// Containing directory id.
    pub dir_id: u32,
    /// Raw entry name; nullable when bulk name resolution lost a race.
    pub name: Option<RawName>,
    /// Filesystem inode number.
    pub ino: u64,
    /// Inode allocated bytes.
    pub disk: u64,
    /// Inode apparent bytes.
    pub apparent: u64,
    /// Inode modification time, Unix seconds.
    pub mtime: i64,
    /// Kernel-reported inode link count.
    pub nlink: u32,
}
/// LinkSetRow storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkSetRow {
    /// Filesystem inode number.
    pub ino: u64,
    /// Kernel-reported inode link count.
    pub nlink: u32,
    /// Link instances discovered inside the scan root.
    pub found: u32,
    /// Whether found >= nlink, tolerating link removal races.
    pub complete: bool,
    /// Inode allocated bytes.
    pub disk: u64,
    /// Inode apparent bytes.
    pub apparent: u64,
    /// Inode modification time, Unix seconds.
    pub mtime: i64,
    /// Lowest common ancestor directory of discovered link instances.
    pub lca_dir_id: u32,
}
/// LinkInstanceRow storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkInstanceRow {
    /// Filesystem inode number.
    pub ino: u64,
    /// Containing directory id.
    pub dir_id: u32,
    /// Raw entry name; None for bulk snapshots, resolved live on demand.
    pub name: Option<RawName>,
}
/// SkipRow storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkipRow {
    /// Containing directory id.
    pub dir_id: u32,
    /// Raw name of the skipped directory entry.
    pub name: RawName,
    /// Why traversal skipped this entry.
    pub reason: SkipReason,
}
/// ErrorRow storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorRow {
    /// Containing directory id.
    pub dir_id: Option<u32>,
    /// Full absolute normalized raw path.
    #[serde(flatten, with = "crate::path::report_path")]
    pub path: RawPath,
    /// Metadata operation that failed.
    pub op: ErrOp,
    /// Operating-system errno value.
    pub errno: i32,
}
/// AnnotationRow storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationRow {
    /// Containing directory id.
    pub dir_id: u32,
    /// Annotation key.
    pub key: String,
    /// Annotation value.
    pub value: String,
}
/// Directory ordering supported by snapshot readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    /// Disk variant.
    Disk,
    /// Apparent variant.
    Apparent,
    /// Reclaimable variant.
    Reclaimable,
    /// Charged variant.
    Charged,
    /// Files variant.
    Files,
    /// Inodes variant.
    Inodes,
    /// Newest variant.
    Newest,
    /// Name variant.
    Name,
}
/// FileFilter storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FileFilter {
    /// Optional minimum allocated bytes.
    pub min_size: Option<u64>,
    /// Optional minimum age relative to snapshot start.
    pub older_than: Option<std::time::Duration>,
    /// Optional maximum age relative to snapshot start.
    pub newer_than: Option<std::time::Duration>,
}
/// LinkFilter storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LinkFilter {
    /// Restrict results to incomplete link sets.
    pub incomplete: bool,
    /// Full-path boundary globs; mutually exclusive with `boundary_depth`.
    pub boundary: Vec<String>,
    /// Optional boundary depth relative to the query directory; mutually exclusive with globs.
    pub boundary_depth: Option<u32>,
}
/// FileHit storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileHit {
    /// Stored snapshot row.
    pub row: FileRow,
    /// Full path, or None if a bulk name-resolution race lost the name.
    #[serde(flatten, with = "crate::path::report_optional_path")]
    pub path: Option<RawPath>,
}
/// LinkSetHit storage contract (05 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkSetHit {
    /// Stored snapshot row.
    pub row: LinkSetRow,
    /// Full raw path of the link-set LCA, serialized as `path`/`path_b64`.
    #[serde(flatten, with = "crate::path::report_path")]
    pub lca_path: RawPath,
    /// Number of stored link instances for this set.
    pub instance_count: u64,
}

#[derive(Serialize, Deserialize)]
struct SnapshotMetaWire {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::path::present_string"
    )]
    root: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::path::present_string"
    )]
    root_b64: Option<String>,
    schema_version: u32,
    tool_version: String,
    scan_id: ScanId,
    root_display: String,
    mode: ScanMode,
    mode_reason: String,
    status: SnapshotStatus,
    degraded: bool,
    exclude: Vec<String>,
    link_instances: LinkInstances,
    started_at: i64,
    finished_at: i64,
    host: String,
    kernel: String,
    os_release: String,
    fs_uuid: String,
    fs_dev: u64,
    fs_blocksize: u64,
    threads: u32,
    throttle_profile: String,
    dirs: u64,
    files: u64,
    inodes: u64,
    errors: u64,
    races: u64,
    skipped: u64,
    noatime_fallbacks: u64,
    phase_ms: BTreeMap<String, u64>,
    gate_stats: GateStats,
}
impl From<SnapshotMeta> for SnapshotMetaWire {
    fn from(value: SnapshotMeta) -> Self {
        let (root, root_b64) = crate::path::encode_fields(Some(value.root.as_bytes()));
        Self {
            root,
            root_b64,
            schema_version: value.schema_version,
            tool_version: value.tool_version,
            scan_id: value.scan_id,
            root_display: value.root_display,
            mode: value.mode,
            mode_reason: value.mode_reason,
            status: value.status,
            degraded: value.degraded,
            exclude: value.exclude,
            link_instances: value.link_instances,
            started_at: value.started_at,
            finished_at: value.finished_at,
            host: value.host,
            kernel: value.kernel,
            os_release: value.os_release,
            fs_uuid: value.fs_uuid,
            fs_dev: value.fs_dev,
            fs_blocksize: value.fs_blocksize,
            threads: value.threads,
            throttle_profile: value.throttle_profile,
            dirs: value.dirs,
            files: value.files,
            inodes: value.inodes,
            errors: value.errors,
            races: value.races,
            skipped: value.skipped,
            noatime_fallbacks: value.noatime_fallbacks,
            phase_ms: value.phase_ms,
            gate_stats: value.gate_stats,
        }
    }
}
impl TryFrom<SnapshotMetaWire> for SnapshotMeta {
    type Error = String;
    fn try_from(value: SnapshotMetaWire) -> Result<Self, Self::Error> {
        let root =
            crate::path::decode_fields::<serde::de::value::Error>(value.root, value.root_b64)
                .map_err(|e| e.to_string())?
                .map(RawPath::new)
                .transpose()
                .map_err(|e| e.to_string())?;
        let root = root.ok_or_else(|| "root or root_b64 is required".to_owned())?;
        Ok(Self {
            root,
            schema_version: value.schema_version,
            tool_version: value.tool_version,
            scan_id: value.scan_id,
            root_display: value.root_display,
            mode: value.mode,
            mode_reason: value.mode_reason,
            status: value.status,
            degraded: value.degraded,
            exclude: value.exclude,
            link_instances: value.link_instances,
            started_at: value.started_at,
            finished_at: value.finished_at,
            host: value.host,
            kernel: value.kernel,
            os_release: value.os_release,
            fs_uuid: value.fs_uuid,
            fs_dev: value.fs_dev,
            fs_blocksize: value.fs_blocksize,
            threads: value.threads,
            throttle_profile: value.throttle_profile,
            dirs: value.dirs,
            files: value.files,
            inodes: value.inodes,
            errors: value.errors,
            races: value.races,
            skipped: value.skipped,
            noatime_fallbacks: value.noatime_fallbacks,
            phase_ms: value.phase_ms,
            gate_stats: value.gate_stats,
        })
    }
}
