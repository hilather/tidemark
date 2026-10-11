//! Owned by: W0-B1
//! Scan contracts and helpers.

use crate::path::{RawName, RawPath};
use serde::{Deserialize, Serialize};

/// Ino contract.
pub type Ino = u64;
/// DevId contract.
pub type DevId = u64;

/// Discovery-order id. Root = DirId(0). Renumbered to preorder when writing the snapshot.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
/// DirId contract.
pub struct DirId(
    /// Discovery-order directory number.
    pub u32,
);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// FileKind contract.
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// Regular variant.
    Regular,
    /// Dir variant.
    Dir,
    /// Symlink variant.
    Symlink,
    /// Other variant.
    Other,
}

/// Bytes by mtime age relative to scan start: <1d, 1–7d, 7–30d, 30–90d, 90–365d, >365d.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
/// AgeBuckets contract.
pub struct AgeBuckets(
    /// Charged bytes by increasing age.
    pub [u64; 6],
);

/// Stats for direct entries whose inode has nlink == 1.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
/// OwnStats contract.
pub struct OwnStats {
    #[doc = "files field; see the normative plan for units."]
    pub files: u64, // non-directory entries (includes symlinks and "other")
    #[doc = "symlinks field; see the normative plan for units."]
    pub symlinks: u64, // subset of `files`
    #[doc = "disk field; see the normative plan for units."]
    pub disk: u64,
    #[doc = "apparent field; see the normative plan for units."]
    pub apparent: u64,
    #[doc = "age field; see the normative plan for units."]
    pub age: AgeBuckets, // disk bytes
    #[doc = "newest_mtime field; see the normative plan for units."]
    pub newest_mtime: i64,
}

/// One direct entry whose inode has nlink > 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// LinkRef contract.
pub struct LinkRef {
    #[doc = "ino field; see the normative plan for units."]
    pub ino: Ino,
    #[doc = "nlink field; see the normative plan for units."]
    pub nlink: u32,
    #[doc = "kind field; see the normative plan for units."]
    pub kind: FileKind,
    #[doc = "disk field; see the normative plan for units."]
    pub disk: u64,
    #[doc = "apparent field; see the normative plan for units."]
    pub apparent: u64,
    #[doc = "mtime field; see the normative plan for units."]
    pub mtime: i64,
    #[doc = "name field; see the normative plan for units."]
    pub name: Option<RawName>, // Some in walk mode; None in bulk mode
}

/// A single-link file that may belong in the snapshot's `files` table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// NotableFile contract.
pub struct NotableFile {
    #[doc = "ino field; see the normative plan for units."]
    pub ino: Ino,
    #[doc = "name field; see the normative plan for units."]
    pub name: Option<RawName>,
    #[doc = "disk field; see the normative plan for units."]
    pub disk: u64,
    #[doc = "apparent field; see the normative plan for units."]
    pub apparent: u64,
    #[doc = "mtime field; see the normative plan for units."]
    pub mtime: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// SkipReason contract.
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Mount variant.
    Mount,
    /// OtherFs variant.
    OtherFs,
    /// Excluded variant.
    Excluded,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Skip contract.
pub struct Skip {
    #[doc = "name field; see the normative plan for units."]
    pub name: RawName,
    #[doc = "reason field; see the normative plan for units."]
    pub reason: SkipReason,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// ErrOp contract.
#[serde(rename_all = "snake_case")]
pub enum ErrOp {
    /// Open variant.
    Open,
    /// Getdents variant.
    Getdents,
    /// Statx variant.
    Statx,
    /// Fstat variant.
    Fstat,
    /// Bulkstat variant.
    Bulkstat,
    /// Readlink variant.
    Readlink,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// ScanErrorRec contract.
pub struct ScanErrorRec {
    #[doc = "path field; see the normative plan for units."]
    #[serde(flatten, with = "crate::path::report_path")]
    pub path: RawPath,
    #[doc = "op field; see the normative plan for units."]
    pub op: ErrOp,
    #[doc = "errno field; see the normative plan for units."]
    pub errno: i32,
}

/// Walk mode: one per directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// DirRecord contract.
pub struct DirRecord {
    #[doc = "id field; see the normative plan for units."]
    pub id: DirId,
    #[doc = "parent field; see the normative plan for units."]
    pub parent: Option<DirId>,
    #[doc = "name field; see the normative plan for units."]
    pub name: Option<RawName>, // None only for the root record (the root path is AggConfig.root / WalkConfig.root)
    #[doc = "ino field; see the normative plan for units."]
    pub ino: Ino,
    #[doc = "dev field; see the normative plan for units."]
    pub dev: DevId,
    #[doc = "mtime field; see the normative plan for units."]
    pub mtime: i64,
    #[doc = "dir_disk field; see the normative plan for units."]
    pub dir_disk: u64,
    #[doc = "dir_apparent field; see the normative plan for units."]
    pub dir_apparent: u64,
    #[doc = "projid field; see the normative plan for units."]
    pub projid: Option<u32>, // walk mode: None
    #[doc = "entries field; see the normative plan for units."]
    pub entries: u32, // all direct entries incl. subdirs and skipped
    #[doc = "own field; see the normative plan for units."]
    pub own: OwnStats,
    #[doc = "links field; see the normative plan for units."]
    pub links: Vec<LinkRef>,
    #[doc = "notable field; see the normative plan for units."]
    pub notable: Vec<NotableFile>,
    #[doc = "skipped field; see the normative plan for units."]
    pub skipped: Vec<Skip>,
    #[doc = "errors field; see the normative plan for units."]
    pub errors: Vec<ScanErrorRec>,
    #[doc = "races field; see the normative plan for units."]
    pub races: u32, // entries that vanished or changed type mid-scan
}

/// Bulk mode phase 1: one per directory, no per-file stat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// NsRecord contract.
pub struct NsRecord {
    #[doc = "id field; see the normative plan for units."]
    pub id: DirId,
    #[doc = "parent field; see the normative plan for units."]
    pub parent: Option<DirId>,
    #[doc = "name field; see the normative plan for units."]
    pub name: Option<RawName>, // None only for the root
    #[doc = "ino field; see the normative plan for units."]
    pub ino: Ino,
    #[doc = "dev field; see the normative plan for units."]
    pub dev: DevId,
    #[doc = "entries field; see the normative plan for units."]
    pub entries: u32,
    #[doc = "children field; see the normative plan for units."]
    pub children: Vec<Ino>, // non-directory entries' inode numbers (from d_ino)
    #[doc = "skipped field; see the normative plan for units."]
    pub skipped: Vec<Skip>,
    #[doc = "errors field; see the normative plan for units."]
    pub errors: Vec<ScanErrorRec>,
    #[doc = "races field; see the normative plan for units."]
    pub races: u32,
}

/// A child directory that was queued (so its DirId exists) but turned out not to be scannable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// GoneRecord contract.
pub struct GoneRecord {
    #[doc = "id field; see the normative plan for units."]
    pub id: DirId,
    #[doc = "parent field; see the normative plan for units."]
    pub parent: DirId,
    #[doc = "name field; see the normative plan for units."]
    pub name: RawName,
    #[doc = "reason field; see the normative plan for units."]
    pub reason: GoneReason,
}
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// GoneReason contract.
#[serde(rename_all = "snake_case")]
pub enum GoneReason {
    /// Vanished variant.
    Vanished,
    /// Replaced variant.
    Replaced, /* no longer a dir, or inode changed */
    /// OtherFs variant.
    OtherFs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// WalkOutput contract.
#[serde(rename_all = "snake_case")]
pub enum WalkOutput {
    /// Dir variant.
    Dir(DirRecord),
    /// Ns variant.
    Ns(NsRecord),
    /// Gone variant.
    Gone(GoneRecord),
}

/// Inode attributes as delivered by the bulk join (and used by the aggregator's bulk inputs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// InodeAttrs contract.
pub struct InodeAttrs {
    #[doc = "ino field; see the normative plan for units."]
    pub ino: Ino,
    #[doc = "kind field; see the normative plan for units."]
    pub kind: FileKind,
    #[doc = "nlink field; see the normative plan for units."]
    pub nlink: u32,
    #[doc = "disk field; see the normative plan for units."]
    pub disk: u64,
    #[doc = "apparent field; see the normative plan for units."]
    pub apparent: u64,
    #[doc = "mtime field; see the normative plan for units."]
    pub mtime: i64,
    #[doc = "projid field; see the normative plan for units."]
    pub projid: Option<u32>,
}

/// Throttle seam. `cost` is in "metadata I/O units" (see §6).
/// Gate contract.
pub trait Gate: Send + Sync {
    /// Acquire metadata I/O units before a syscall.
    fn acquire(&self, cost: u32);
    /// Current allowed units per second.
    fn current_rate(&self) -> f64; // units/s currently allowed (for progress output)
}
#[derive(Debug, Clone, Copy, Default)]
/// NoopGate contract.
pub struct NoopGate;

impl Gate for NoopGate {
    fn acquire(&self, _cost: u32) {}
    fn current_rate(&self) -> f64 {
        f64::INFINITY
    }
}
/// Snapshot completion state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStatus {
    /// Finished scanning.
    /// Complete variant.
    Complete,
    /// Stopped before completion.
    /// Partial variant.
    Partial,
}
/// Actual scanner implementation used (not the auto selection policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    /// Directory walk.
    /// Walk variant.
    Walk,
    /// XFS bulkstat.
    /// Bulk variant.
    Bulk,
}

/// Controller statistics persisted in snapshot metadata (W1-D contract).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Shared throttle statistics; `tidemark_fs::throttle` must re-export this type
/// rather than define an incompatible copy.
pub struct GateStats {
    /// Mean allowed metadata I/O units per second.
    pub mean_rate: f64,
    /// Minimum allowed metadata I/O units per second.
    pub min_rate: f64,
    /// Total time blocked by throttling, milliseconds.
    pub throttled_ms: u64,
    /// Number of rate decreases.
    pub decreases: u64,
    /// Number of rate increases.
    pub increases: u64,
}
