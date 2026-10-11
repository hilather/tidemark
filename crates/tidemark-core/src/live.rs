//! Owned by: W0-B1
//! Live contracts and helpers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// NowReport contract.
pub struct NowReport {
    #[doc = "generated_at field; see the normative plan for units."]
    pub generated_at: i64,
    #[doc = "host field; see the normative plan for units."]
    pub host: String,
    #[doc = "mounts field; see the normative plan for units."]
    pub mounts: Vec<MountNow>,
    #[doc = "pg field; see the normative plan for units."]
    pub pg: Option<PgNow>,
    #[doc = "alerts_firing field; see the normative plan for units."]
    pub alerts_firing: Vec<AlertSummary>,
    #[doc = "scans field; see the normative plan for units."]
    pub scans: Vec<ScanStatus>, // last success per root, running scan progress
    #[doc = "degraded field; see the normative plan for units."]
    pub degraded: Vec<String>, // e.g. "watcher: fd mode only", "pg: unreachable since …"
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// MountNow contract.
pub struct MountNow {
    #[doc = "mount field; see the normative plan for units."]
    pub mount: String,
    #[doc = "size field; see the normative plan for units."]
    pub size: u64,
    #[doc = "used field; see the normative plan for units."]
    pub used: u64,
    #[doc = "avail field; see the normative plan for units."]
    pub avail: u64,
    #[doc = "rate_15m field; see the normative plan for units."]
    pub rate_15m: Option<f64>,
    #[doc = "rate_1h field; see the normative plan for units."]
    pub rate_1h: Option<f64>,
    #[doc = "ttf_secs field; see the normative plan for units."]
    pub ttf_secs: Option<u64>,
    #[doc = "lvm field; see the normative plan for units."]
    pub lvm: Option<LvmNow>, // LV size, VG free, thin-pool data/meta % (if thin)
    #[doc = "hot_dirs_5m field; see the normative plan for units."]
    pub hot_dirs_5m: Vec<HotEntry>,
    #[doc = "writers_5m field; see the normative plan for units."]
    pub writers_5m: Vec<HotEntry>,
    #[doc = "phantom field; see the normative plan for units."]
    pub phantom: PhantomSummary,
    #[doc = "open_writers_top field; see the normative plan for units."]
    pub open_writers_top: Vec<OpenWriter>,
    #[doc = "watch_mode field; see the normative plan for units."]
    pub watch_mode: String,
    #[doc = "lossy_minutes_15m field; see the normative plan for units."]
    pub lossy_minutes_15m: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// FsSample contract.
pub struct FsSample {
    #[doc = "ts field; see the normative plan for units."]
    pub ts: i64,
    #[doc = "mount field; see the normative plan for units."]
    pub mount: String,
    #[doc = "size field; see the normative plan for units."]
    pub size: u64,
    #[doc = "used field; see the normative plan for units."]
    pub used: u64,
    #[doc = "avail field; see the normative plan for units."]
    pub avail: u64,
    #[doc = "inodes field; see the normative plan for units."]
    pub inodes: u64,
    #[doc = "inodes_used field; see the normative plan for units."]
    pub inodes_used: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// LvmSample contract.
pub struct LvmSample {
    #[doc = "ts field; see the normative plan for units."]
    pub ts: i64,
    #[doc = "vg field; see the normative plan for units."]
    pub vg: String,
    #[doc = "lv field; see the normative plan for units."]
    pub lv: String,
    #[doc = "lv_size field; see the normative plan for units."]
    pub lv_size: u64,
    #[doc = "vg_free field; see the normative plan for units."]
    pub vg_free: u64,
    #[doc = "data_pct field; see the normative plan for units."]
    pub data_pct: Option<f64>,
    #[doc = "meta_pct field; see the normative plan for units."]
    pub meta_pct: Option<f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// LvmNow contract.
pub struct LvmNow {
    #[doc = "vg field; see the normative plan for units."]
    pub vg: String,
    #[doc = "lv field; see the normative plan for units."]
    pub lv: String,
    #[doc = "lv_size field; see the normative plan for units."]
    pub lv_size: u64,
    #[doc = "vg_free field; see the normative plan for units."]
    pub vg_free: u64,
    #[doc = "thin_pool field; see the normative plan for units."]
    pub thin_pool: Option<String>,
    #[doc = "data_pct field; see the normative plan for units."]
    pub data_pct: Option<f64>,
    #[doc = "meta_pct field; see the normative plan for units."]
    pub meta_pct: Option<f64>,
    #[doc = "sampled_at field; see the normative plan for units."]
    pub sampled_at: i64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Attribution contract.
pub struct Attribution {
    #[doc = "pid field; see the normative plan for units."]
    pub pid: u32,
    #[doc = "starttime field; see the normative plan for units."]
    pub starttime: u64,
    #[doc = "comm field; see the normative plan for units."]
    pub comm: String,
    #[doc = "unit field; see the normative plan for units."]
    pub unit: Option<String>,
    #[doc = "uid field; see the normative plan for units."]
    pub uid: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "HotEntryWire", into = "HotEntryWire")]
/// HotEntry contract. Absolute directory keys serialize as `path`/`path_b64`;
/// unit/comm labels retain byte-oriented `key` because this type has no scope field.
pub struct HotEntry {
    #[doc = "key field; see the normative plan for units."]
    pub key: Vec<u8>, /* dir path, unit or comm */
    #[doc = "events field; see the normative plan for units."]
    pub events: u64,
    #[doc = "files field; see the normative plan for units."]
    pub files: u64,
    #[doc = "bytes_delta field; see the normative plan for units."]
    pub bytes_delta: i64,
    #[doc = "top_unit field; see the normative plan for units."]
    pub top_unit: Option<String>,
    #[doc = "lossy field; see the normative plan for units."]
    pub lossy: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "HotMinuteWire", into = "HotMinuteWire")]
/// HotMinute contract. Scope `dir` serializes its key as `path`/`path_b64`;
/// unit/comm scopes retain byte-oriented `key`.
pub struct HotMinute {
    #[doc = "mount field; see the normative plan for units."]
    pub mount: String,
    #[doc = "scope field; see the normative plan for units."]
    pub scope: String, /* dir|unit|comm */
    #[doc = "key field; see the normative plan for units."]
    pub key: Vec<u8>,
    #[doc = "ts field; see the normative plan for units."]
    pub ts: i64,
    #[doc = "events field; see the normative plan for units."]
    pub events: u64,
    #[doc = "files field; see the normative plan for units."]
    pub files: u64,
    #[doc = "bytes_delta field; see the normative plan for units."]
    pub bytes_delta: i64,
    #[doc = "lossy field; see the normative plan for units."]
    pub lossy: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// PhantomHolder contract.
pub struct PhantomHolder {
    #[doc = "pid field; see the normative plan for units."]
    pub pid: u32,
    #[doc = "comm field; see the normative plan for units."]
    pub comm: String,
    #[doc = "unit field; see the normative plan for units."]
    pub unit: Option<String>,
    /// Raw path bytes, encoded as mutually exclusive `path`/`path_b64` report fields.
    #[serde(flatten, with = "crate::path::report_bytes")]
    pub path: Vec<u8>,
    #[doc = "dev field; see the normative plan for units."]
    pub dev: u64,
    #[doc = "ino field; see the normative plan for units."]
    pub ino: u64,
    #[doc = "bytes field; see the normative plan for units."]
    pub bytes: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// PhantomSummary contract.
pub struct PhantomSummary {
    #[doc = "mount field; see the normative plan for units."]
    pub mount: String,
    #[doc = "total_bytes field; see the normative plan for units."]
    pub total_bytes: u64,
    #[doc = "files field; see the normative plan for units."]
    pub files: u64,
    #[doc = "holders field; see the normative plan for units."]
    pub holders: Vec<PhantomHolder>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// PhantomSample contract.
pub struct PhantomSample {
    #[doc = "ts field; see the normative plan for units."]
    pub ts: i64,
    #[doc = "mount field; see the normative plan for units."]
    pub mount: String,
    #[doc = "pid field; see the normative plan for units."]
    pub pid: u32,
    #[doc = "comm field; see the normative plan for units."]
    pub comm: String,
    #[doc = "unit field; see the normative plan for units."]
    pub unit: Option<String>,
    /// Raw path bytes, encoded as mutually exclusive `path`/`path_b64` report fields.
    #[serde(flatten, with = "crate::path::report_bytes")]
    pub path: Vec<u8>,
    #[doc = "dev field; see the normative plan for units."]
    pub dev: u64,
    #[doc = "ino field; see the normative plan for units."]
    pub ino: u64,
    #[doc = "bytes field; see the normative plan for units."]
    pub bytes: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// OpenWriter contract.
pub struct OpenWriter {
    #[doc = "mount field; see the normative plan for units."]
    pub mount: String,
    #[doc = "pid field; see the normative plan for units."]
    pub pid: u32,
    #[doc = "comm field; see the normative plan for units."]
    pub comm: String,
    #[doc = "unit field; see the normative plan for units."]
    pub unit: Option<String>,
    /// Raw path bytes, encoded as mutually exclusive `path`/`path_b64` report fields.
    #[serde(flatten, with = "crate::path::report_bytes")]
    pub path: Vec<u8>,
    #[doc = "dev field; see the normative plan for units."]
    pub dev: u64,
    #[doc = "ino field; see the normative plan for units."]
    pub ino: u64,
    #[doc = "size field; see the normative plan for units."]
    pub size: u64,
    #[doc = "disk field; see the normative plan for units."]
    pub disk: u64,
    #[doc = "growth field; see the normative plan for units."]
    pub growth: i64,
    #[doc = "window_secs field; see the normative plan for units."]
    pub window_secs: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// WriterSample contract.
pub struct WriterSample {
    #[doc = "ts field; see the normative plan for units."]
    pub ts: i64,
    #[doc = "mount field; see the normative plan for units."]
    pub mount: String,
    #[doc = "pid field; see the normative plan for units."]
    pub pid: u32,
    #[doc = "comm field; see the normative plan for units."]
    pub comm: String,
    #[doc = "unit field; see the normative plan for units."]
    pub unit: Option<String>,
    /// Raw path bytes, encoded as mutually exclusive `path`/`path_b64` report fields.
    #[serde(flatten, with = "crate::path::report_bytes")]
    pub path: Vec<u8>,
    #[doc = "dev field; see the normative plan for units."]
    pub dev: u64,
    #[doc = "ino field; see the normative plan for units."]
    pub ino: u64,
    #[doc = "size field; see the normative plan for units."]
    pub size: u64,
    #[doc = "disk field; see the normative plan for units."]
    pub disk: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// PgSample contract.
pub struct PgSample {
    #[doc = "ts field; see the normative plan for units."]
    pub ts: i64,
    #[doc = "kind field; see the normative plan for units."]
    pub kind: String, /* 05 §6 kinds */
    #[doc = "name field; see the normative plan for units."]
    pub name: String,
    #[doc = "value field; see the normative plan for units."]
    pub value: i64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// PgSlot contract.
pub struct PgSlot {
    #[doc = "name field; see the normative plan for units."]
    pub name: String,
    #[doc = "slot_type field; see the normative plan for units."]
    pub slot_type: String,
    #[doc = "active field; see the normative plan for units."]
    pub active: bool,
    #[doc = "retained_bytes field; see the normative plan for units."]
    pub retained_bytes: Option<u64>,
    #[doc = "wal_status field; see the normative plan for units."]
    pub wal_status: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// PgNow contract.
pub struct PgNow {
    #[doc = "sampled_at field; see the normative plan for units."]
    pub sampled_at: Option<i64>,
    #[doc = "unreachable_since field; see the normative plan for units."]
    pub unreachable_since: Option<i64>,
    #[doc = "server_version_num field; see the normative plan for units."]
    pub server_version_num: Option<u32>,
    #[doc = "databases field; see the normative plan for units."]
    pub databases: Vec<(String, u64)>,
    #[doc = "wal_dir_bytes field; see the normative plan for units."]
    pub wal_dir_bytes: Option<u64>,
    #[doc = "slots field; see the normative plan for units."]
    pub slots: Vec<PgSlot>,
    #[doc = "archiver_failing field; see the normative plan for units."]
    pub archiver_failing: bool,
    #[doc = "archiver_last_failed field; see the normative plan for units."]
    pub archiver_last_failed: Option<i64>,
    #[doc = "temp_bytes_rate field; see the normative plan for units."]
    pub temp_bytes_rate: Option<f64>,
    #[doc = "top_relations field; see the normative plan for units."]
    pub top_relations: Vec<(String /* db.schema.rel */, u64)>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Alert lifecycle state.
#[serde(rename_all = "snake_case")]
pub enum AlertState {
    /// Firing variant.
    Firing,
    /// Resolved variant.
    Resolved,
    /// Renotify variant.
    Renotify,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// AlertEvent contract.
pub struct AlertEvent {
    #[doc = "ts field; see the normative plan for units."]
    pub ts: i64,
    #[doc = "rule field; see the normative plan for units."]
    pub rule: String,
    #[doc = "subject field; see the normative plan for units."]
    pub subject: String,
    #[doc = "state field; see the normative plan for units."]
    pub state: AlertState,
    #[doc = "value field; see the normative plan for units."]
    pub value: Option<f64>,
    #[doc = "message field; see the normative plan for units."]
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// AlertSummary contract.
pub struct AlertSummary {
    #[doc = "rule field; see the normative plan for units."]
    pub rule: String,
    #[doc = "subject field; see the normative plan for units."]
    pub subject: String,
    #[doc = "since field; see the normative plan for units."]
    pub since: i64,
    #[doc = "value field; see the normative plan for units."]
    pub value: Option<f64>,
    #[doc = "message field; see the normative plan for units."]
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// ScanProgress contract.
pub struct ScanProgress {
    #[doc = "scan_id field; see the normative plan for units."]
    pub scan_id: String,
    #[doc = "phase field; see the normative plan for units."]
    pub phase: String,
    #[doc = "dirs field; see the normative plan for units."]
    pub dirs: u64,
    #[doc = "entries field; see the normative plan for units."]
    pub entries: u64,
    #[doc = "started_at field; see the normative plan for units."]
    pub started_at: i64,
    #[doc = "rate field; see the normative plan for units."]
    pub rate: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// ScanStatus contract.
pub struct ScanStatus {
    #[doc = "root field; see the normative plan for units."]
    pub root: String,
    #[doc = "running field; see the normative plan for units."]
    pub running: Option<ScanProgress>,
    #[doc = "last_complete field; see the normative plan for units."]
    pub last_complete: Option<LastScan>,
    #[doc = "next_due field; see the normative plan for units."]
    pub next_due: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// LastScan contract.
pub struct LastScan {
    #[doc = "scan_id field; see the normative plan for units."]
    pub scan_id: String,
    #[doc = "finished_at field; see the normative plan for units."]
    pub finished_at: i64,
    #[doc = "mode field; see the normative plan for units."]
    pub mode: String,
    #[doc = "duration_ms field; see the normative plan for units."]
    pub duration_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct HotEntryWire {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::path::present_string"
    )]
    path: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::path::present_string"
    )]
    path_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<Vec<u8>>,
    events: u64,
    files: u64,
    bytes_delta: i64,
    top_unit: Option<String>,
    lossy: bool,
}
impl From<HotEntry> for HotEntryWire {
    fn from(value: HotEntry) -> Self {
        let (path, path_b64, key) = if value.key.starts_with(b"/") {
            let (path, path_b64) = crate::path::encode_fields(Some(&value.key));
            (path, path_b64, None)
        } else {
            (None, None, Some(value.key))
        };
        Self {
            path,
            path_b64,
            key,
            events: value.events,
            files: value.files,
            bytes_delta: value.bytes_delta,
            top_unit: value.top_unit,
            lossy: value.lossy,
        }
    }
}
impl TryFrom<HotEntryWire> for HotEntry {
    type Error = String;
    fn try_from(value: HotEntryWire) -> Result<Self, Self::Error> {
        let path =
            crate::path::decode_fields::<serde::de::value::Error>(value.path, value.path_b64)
                .map_err(|e| e.to_string())?;
        let key = match (path, value.key) {
            (Some(_), Some(_)) => return Err("key conflicts with path representation".into()),
            (Some(path), None) => path,
            (None, Some(key)) => key,
            (None, None) => return Err("key or path representation is required".into()),
        };
        Ok(Self {
            key,
            events: value.events,
            files: value.files,
            bytes_delta: value.bytes_delta,
            top_unit: value.top_unit,
            lossy: value.lossy,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct HotMinuteWire {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::path::present_string"
    )]
    path: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::path::present_string"
    )]
    path_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<Vec<u8>>,
    mount: String,
    scope: String,
    ts: i64,
    events: u64,
    files: u64,
    bytes_delta: i64,
    lossy: bool,
}
impl From<HotMinute> for HotMinuteWire {
    fn from(value: HotMinute) -> Self {
        let (path, path_b64, key) = if value.scope == "dir" {
            let (path, path_b64) = crate::path::encode_fields(Some(&value.key));
            (path, path_b64, None)
        } else {
            (None, None, Some(value.key))
        };
        Self {
            path,
            path_b64,
            key,
            mount: value.mount,
            scope: value.scope,
            ts: value.ts,
            events: value.events,
            files: value.files,
            bytes_delta: value.bytes_delta,
            lossy: value.lossy,
        }
    }
}
impl TryFrom<HotMinuteWire> for HotMinute {
    type Error = String;
    fn try_from(value: HotMinuteWire) -> Result<Self, Self::Error> {
        let path =
            crate::path::decode_fields::<serde::de::value::Error>(value.path, value.path_b64)
                .map_err(|e| e.to_string())?;
        let key = match (path, value.key) {
            (Some(_), Some(_)) => return Err("key conflicts with path representation".into()),
            (Some(path), None) => path,
            (None, Some(key)) => key,
            (None, None) => return Err("key or path representation is required".into()),
        };
        Ok(Self {
            key,
            mount: value.mount,
            scope: value.scope,
            ts: value.ts,
            events: value.events,
            files: value.files,
            bytes_delta: value.bytes_delta,
            lossy: value.lossy,
        })
    }
}
