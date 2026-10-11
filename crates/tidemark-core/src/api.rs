//! Owned by: W0-B1
//! Api contracts and helpers.

use crate::path::RawPath;
use crate::{
    config::{RootMode, ThrottlePreset},
    live::*,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
/// Request API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// v field.
    pub v: u32,
    /// id field.
    pub id: String,
    /// method field.
    pub method: String,
    /// params field.
    pub params: Value,
}
/// Response API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// v field.
    pub v: u32,
    /// id field.
    pub id: String,
    /// ok field.
    pub ok: bool,
    /// result field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// error field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}
/// ApiError API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    /// code field.
    pub code: ApiErrorCode,
    /// message field.
    pub message: String,
}
/// Stable wire error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    /// Malformed request or parameters.
    /// BadRequest variant.
    BadRequest,
    /// Peer is not allowed access.
    /// Forbidden variant.
    Forbidden,
    /// Requested object is missing.
    /// NotFound variant.
    NotFound,
    /// Component is unavailable.
    /// Unavailable variant.
    Unavailable,
    /// Another scan holds the host lock.
    /// Busy variant.
    Busy,
    /// Internal failure.
    /// Internal variant.
    Internal,
}
/// ComponentState API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentState {
    /// name field.
    pub name: String,
    /// state field.
    pub state: String,
    /// detail field.
    pub detail: String,
}
/// VgInfo API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VgInfo {
    /// vg field.
    pub vg: String,
    /// size field.
    pub size: u64,
    /// free field.
    pub free: u64,
}
/// StatusResult API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusResult {
    /// version field.
    pub version: String,
    /// uptime_s field.
    pub uptime_s: u64,
    /// components field.
    pub components: Vec<ComponentState>,
    /// watch_modes field.
    pub watch_modes: Vec<(String, String)>,
    /// last_errors field.
    pub last_errors: Vec<(String, i64, String)>,
}
/// LvmResult API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LvmResult {
    /// mounts field.
    pub mounts: Vec<(String, LvmNow)>,
    /// vgs field.
    pub vgs: Vec<VgInfo>,
    /// sampled_at field.
    pub sampled_at: i64,
}
/// StatusParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StatusParams {}
/// NowParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NowParams {
    /// mount field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
}
/// HotParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HotParams {
    /// mount field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
    /// window field.
    pub window: HotWindow,
    /// by field.
    pub by: HotBy,
    /// limit field.
    pub limit: usize,
}
/// LvmParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LvmParams {
    /// mount field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
}
/// PhantomParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhantomParams {
    /// mount field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
}
/// WritersParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WritersParams {
    /// mount field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
    /// Positive humantime duration; no additional maximum is imposed by core.
    #[serde(with = "positive_duration")]
    pub window: std::time::Duration,
}
/// PgParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PgParams {
    /// relations field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relations: Option<bool>,
    /// db field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<String>,
}
/// AlertsActiveParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertsActiveParams {
    /// limit field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}
/// AlertsHistoryParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertsHistoryParams {
    /// limit field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}
/// ScanStartParams API contract (08 §2).
/// Exactly one UTF-8 `root` or padded standard-base64 `root_b64` is required;
/// deserialization validates and lexically normalizes through `RawPath`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ScanStartParamsWire", into = "ScanStartParamsWire")]
#[serde(deny_unknown_fields)]
pub struct ScanStartParams {
    /// root field.
    pub root: RawPath,
    /// mode field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RootMode>,
    /// throttle field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throttle: Option<ThrottlePreset>,
}
/// ScanStatusParams API contract (08 §2).
/// At most one UTF-8 `root` or padded standard-base64 `root_b64`; omit both for all roots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ScanStatusParamsWire", into = "ScanStatusParamsWire")]
#[serde(deny_unknown_fields)]
pub struct ScanStatusParams {
    /// root field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<RawPath>,
}
/// ScanCancelParams API contract (08 §2).
/// Exactly one UTF-8 `root` or padded standard-base64 `root_b64` is required.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ScanCancelParamsWire", into = "ScanCancelParamsWire")]
#[serde(deny_unknown_fields)]
pub struct ScanCancelParams {
    /// root field.
    pub root: RawPath,
}
/// ConfigReloadParams API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigReloadParams {}
/// ScanStartResult API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanStartResult {
    /// scan_id field.
    pub scan_id: String,
}
/// ScanCancelResult API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanCancelResult {
    /// cancelled field.
    pub cancelled: bool,
}
/// ConfigReloadResult API contract (08 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigReloadResult {
    /// reloaded field.
    pub reloaded: bool,
    /// warnings field.
    pub warnings: Vec<String>,
}
/// Typed result for NowResult.
pub type NowResult = NowReport;
/// Typed result for HotResult.
pub type HotResult = Vec<HotEntry>;
/// Typed result for PhantomResult.
pub type PhantomResult = Vec<PhantomSummary>;
/// Typed result for WritersResult.
pub type WritersResult = Vec<OpenWriter>;
/// Typed result for PgResult.
pub type PgResult = Option<PgNow>;
/// Typed result for AlertsActiveResult.
pub type AlertsActiveResult = Vec<AlertSummary>;
/// Typed result for AlertsHistoryResult.
pub type AlertsHistoryResult = Vec<AlertEvent>;
/// Typed result for ScanStatusResult.
pub type ScanStatusResult = Vec<ScanStatus>;
/// Available ring-buffer query windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HotWindow {
    /// One minute.
    #[serde(rename = "1m")]
    /// OneMinute variant.
    OneMinute,
    /// Five minutes.
    #[serde(rename = "5m")]
    /// FiveMinutes variant.
    FiveMinutes,
    /// Fifteen minutes.
    #[serde(rename = "15m")]
    /// FifteenMinutes variant.
    FifteenMinutes,
    /// Sixty minutes.
    #[serde(rename = "60m")]
    /// SixtyMinutes variant.
    SixtyMinutes,
}
/// Attribution grouping for hot queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotBy {
    /// Directory.
    /// Dir variant.
    Dir,
    /// systemd unit.
    /// Unit variant.
    Unit,
    /// Process command name.
    /// Comm variant.
    Comm,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn example_wire_requests_and_errors() {
        let request: Request =
            serde_json::from_str(r#"{"v":1,"id":"c1","method":"now","params":{"mount":"/data"}}"#)
                .unwrap();
        let params: NowParams = serde_json::from_value(request.params).unwrap();
        assert_eq!(params.mount.as_deref(), Some("/data"));
        let response: Response = serde_json::from_str(r#"{"v":1,"id":"c2","ok":false,"error":{"code":"unavailable","message":"watcher disabled"}}"#).unwrap();
        assert_eq!(
            response.error.as_ref().unwrap().code,
            ApiErrorCode::Unavailable
        );
        assert!(!serde_json::to_value(response)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("result"));
        assert!(serde_json::from_value::<HotParams>(
            serde_json::json!({"window":"2m","by":"dir","limit":10})
        )
        .is_err());
    }
}

mod positive_duration {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;
    pub fn serialize<S: Serializer>(duration: &Duration, s: S) -> Result<S::Ok, S::Error> {
        if duration.is_zero() {
            return Err(serde::ser::Error::custom("window must be positive"));
        }
        humantime_serde::serialize(duration, s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let duration = humantime_serde::Serde::<Duration>::deserialize(d)?.into_inner();
        if duration.is_zero() {
            return Err(serde::de::Error::custom("window must be positive"));
        }
        Ok(duration)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanStartParamsWire {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<RootMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    throttle: Option<ThrottlePreset>,
}
impl From<ScanStartParams> for ScanStartParamsWire {
    fn from(value: ScanStartParams) -> Self {
        let (root, root_b64) = crate::path::encode_fields(Some(value.root.as_bytes()));
        Self {
            root,
            root_b64,
            mode: value.mode,
            throttle: value.throttle,
        }
    }
}
impl TryFrom<ScanStartParamsWire> for ScanStartParams {
    type Error = String;
    fn try_from(value: ScanStartParamsWire) -> Result<Self, Self::Error> {
        let root =
            crate::path::decode_fields::<serde::de::value::Error>(value.root, value.root_b64)
                .map_err(|e| e.to_string())?
                .map(RawPath::new)
                .transpose()
                .map_err(|e| e.to_string())?;
        let root = root.ok_or_else(|| "root or root_b64 is required".to_owned())?;
        Ok(Self {
            root,
            mode: value.mode,
            throttle: value.throttle,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanStatusParamsWire {
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
}
impl From<ScanStatusParams> for ScanStatusParamsWire {
    fn from(value: ScanStatusParams) -> Self {
        let (root, root_b64) =
            crate::path::encode_fields(value.root.as_ref().map(RawPath::as_bytes));
        Self { root, root_b64 }
    }
}
impl TryFrom<ScanStatusParamsWire> for ScanStatusParams {
    type Error = String;
    fn try_from(value: ScanStatusParamsWire) -> Result<Self, Self::Error> {
        let root =
            crate::path::decode_fields::<serde::de::value::Error>(value.root, value.root_b64)
                .map_err(|e| e.to_string())?
                .map(RawPath::new)
                .transpose()
                .map_err(|e| e.to_string())?;
        Ok(Self { root })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanCancelParamsWire {
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
}
impl From<ScanCancelParams> for ScanCancelParamsWire {
    fn from(value: ScanCancelParams) -> Self {
        let (root, root_b64) = crate::path::encode_fields(Some(value.root.as_bytes()));
        Self { root, root_b64 }
    }
}
impl TryFrom<ScanCancelParamsWire> for ScanCancelParams {
    type Error = String;
    fn try_from(value: ScanCancelParamsWire) -> Result<Self, Self::Error> {
        let root =
            crate::path::decode_fields::<serde::de::value::Error>(value.root, value.root_b64)
                .map_err(|e| e.to_string())?
                .map(RawPath::new)
                .transpose()
                .map_err(|e| e.to_string())?;
        let root = root.ok_or_else(|| "root or root_b64 is required".to_owned())?;
        Ok(Self { root })
    }
}
