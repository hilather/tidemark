//! Owned by: W4-B
//! Compiling cross-task interfaces; runtime behavior belongs to W4-B.

use std::{io, path::Path, time::Duration};
use tidemark_core::config::Config;

/// Cross-task contract reserved for the owning task.
pub enum CheckStatus {
    #[doc = "Contract variant."]
    Pass,
    #[doc = "Contract variant."]
    Warn,
    #[doc = "Contract variant."]
    Fail,
}
/// Cross-task contract reserved for the owning task.
pub struct CheckResult {
    #[doc = "Contract field."]
    pub name: &'static str,
    #[doc = "Contract field."]
    pub status: CheckStatus,
    #[doc = "Contract field."]
    pub detail: String,
}
/// Cross-task contract reserved for the owning task.
pub struct StagedRecord {
    #[doc = "Contract field."]
    pub version: String,
    #[doc = "Contract field."]
    pub staged_at: i64,
    #[doc = "Contract field."]
    pub config_sha256: String,
    #[doc = "Contract field."]
    pub checks: Vec<CheckResult>,
    #[doc = "Contract field."]
    pub start_now: bool,
}
/// Cross-task contract reserved for the owning task.
pub trait Systemctl {
    /// System service operation.
    fn is_enabled(&self, unit: &str) -> io::Result<bool>;
    /// System service operation.
    fn is_active(&self, unit: &str) -> io::Result<bool>;
    /// System service operation.
    fn enable(&self, unit: &str) -> io::Result<()>;
    /// System service operation.
    fn disable_now(&self, unit: &str) -> io::Result<()>;
    /// System service operation.
    fn restart(&self, unit: &str) -> io::Result<()>;
}
/// Cross-task contract reserved for the owning task.
pub fn preflight(cfg: &Config, env: &dyn Env) -> Vec<CheckResult> {
    let _ = cfg;
    let _ = env;
    Vec::new()
}
/// Cross-task contract reserved for the owning task.
pub fn stage(
    cfg_path: &Path,
    opts: &StageOptions,
    sys: &dyn Systemctl,
    env: &dyn Env,
) -> Result<StagedRecord, DeployError> {
    let _ = cfg_path;
    let _ = opts;
    let _ = sys;
    let _ = env;
    Err(DeployError::NotYetImplemented)
}
/// Cross-task contract reserved for the owning task.
pub fn verify(
    state_dir: &Path,
    wait: Duration,
    sys: &dyn Systemctl,
    api: &dyn ApiPing,
) -> Result<VerifyResult, DeployError> {
    let _ = state_dir;
    let _ = wait;
    let _ = sys;
    let _ = api;
    Err(DeployError::NotYetImplemented)
}
/// Cross-task contract reserved for the owning task.
pub fn unstage(
    state_dir: &Path,
    purge_state: bool,
    sys: &dyn Systemctl,
) -> Result<(), DeployError> {
    let _ = state_dir;
    let _ = purge_state;
    let _ = sys;
    Err(DeployError::NotYetImplemented)
}

/// Cross-task contract reserved for the owning task.
pub trait Env {}
/// Cross-task contract reserved for the owning task.
pub trait ApiPing {}
/// Cross-task contract reserved for the owning task.
pub struct StageOptions;
/// Cross-task contract reserved for the owning task.
pub struct VerifyResult;
#[derive(Debug, thiserror::Error)]
/// Cross-task contract reserved for the owning task.
pub enum DeployError {
    #[doc = "Contract variant."]
    #[error("NotYetImplemented")]
    NotYetImplemented,
}
