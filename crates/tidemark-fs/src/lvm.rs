//! Owned by: W4-A
//! Cross-task signatures reserved for W4-A.

use crate::mount::MountInfo;
/// Stub error reserved for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum LvmError {
    /// Cross-task implementation is not available yet.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}
/// Shared core VG information.
pub use tidemark_core::api::VgInfo;
use tidemark_core::{
    config::LvmConfig,
    live::{LvmNow, LvmSample},
    path::RawPath,
};
/// Reserved for W4-A.
pub struct LvInfo {
    #[doc = "Field specified by the owning task."]
    pub vg: String,
    #[doc = "Field specified by the owning task."]
    pub lv: String,
    #[doc = "Field specified by the owning task."]
    pub attr: String,
    #[doc = "Field specified by the owning task."]
    pub segtype: String,
    #[doc = "Field specified by the owning task."]
    pub size: u64,
    #[doc = "Field specified by the owning task."]
    pub data_pct: Option<f64>,
    #[doc = "Field specified by the owning task."]
    pub meta_pct: Option<f64>,
    #[doc = "Field specified by the owning task."]
    pub pool_lv: Option<String>,
    #[doc = "Field specified by the owning task."]
    pub dm_path: Option<String>,
    #[doc = "Field specified by the owning task."]
    pub major: Option<u32>,
    #[doc = "Field specified by the owning task."]
    pub minor: Option<u32>,
}

/// Reserved for W4-A.
pub struct LvmReport {
    #[doc = "Field specified by the owning task."]
    pub lvs: Vec<LvInfo>,
    #[doc = "Field specified by the owning task."]
    pub vgs: Vec<VgInfo>,
    #[doc = "Field specified by the owning task."]
    pub sampled_at: i64,
}
/// Reserved for W4-A.
pub struct MountLv {
    #[doc = "Field specified by the owning task."]
    pub mount: RawPath,
    #[doc = "Field specified by the owning task."]
    pub vg: String,
    #[doc = "Field specified by the owning task."]
    pub lv: String,
    #[doc = "Field specified by the owning task."]
    pub thin_pool: Option<String>,
}

/// Reserved for W4-A.
pub fn parse_lvs_json(bytes: &[u8]) -> Result<Vec<LvInfo>, LvmError> {
    let _ = bytes;
    Err(LvmError::NotYetImplemented)
}
/// Reserved for W4-A.
pub fn parse_vgs_json(bytes: &[u8]) -> Result<Vec<VgInfo>, LvmError> {
    let _ = bytes;
    Err(LvmError::NotYetImplemented)
}
/// Reserved for W4-A.
pub fn collect(cfg: &LvmConfig, now: i64) -> Result<LvmReport, LvmError> {
    let _ = cfg;
    let _ = now;
    Err(LvmError::NotYetImplemented)
}
/// Reserved for W4-A.
pub fn map_mounts(report: &LvmReport, mounts: &[MountInfo]) -> Vec<MountLv> {
    let _ = report;
    let _ = mounts;
    Vec::new()
}
/// Reserved for W4-A.
pub fn lvm_now(report: &LvmReport, m: &MountLv) -> LvmNow {
    let _ = report;
    let _ = m;
    LvmNow {
        vg: String::new(),
        lv: String::new(),
        lv_size: 0,
        vg_free: 0,
        thin_pool: None,
        data_pct: None,
        meta_pct: None,
        sampled_at: report.sampled_at,
    }
}
/// Reserved for W4-A.
pub fn to_samples(report: &LvmReport, tracked: &[MountLv]) -> Vec<LvmSample> {
    let _ = report;
    let _ = tracked;
    Vec::new()
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        let raw = tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap();
        assert_eq!(
            parse_lvs_json(b"").err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            parse_vgs_json(b"").err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            collect(&LvmConfig::default(), 0).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert!(map_mounts(
            &LvmReport {
                lvs: Vec::new(),
                vgs: Vec::new(),
                sampled_at: 0
            },
            &[]
        )
        .is_empty());
        assert!(lvm_now(
            &LvmReport {
                lvs: Vec::new(),
                vgs: Vec::new(),
                sampled_at: 0
            },
            &MountLv {
                mount: raw.clone(),
                vg: String::new(),
                lv: String::new(),
                thin_pool: None
            }
        )
        .data_pct
        .is_none());
        assert!(to_samples(
            &LvmReport {
                lvs: Vec::new(),
                vgs: Vec::new(),
                sampled_at: 0
            },
            &[]
        )
        .is_empty());
    }
}
