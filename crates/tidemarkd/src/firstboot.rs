//! Owned by: W4-B
//! Compiling cross-task interfaces; runtime behavior belongs to W4-B.

use std::{io, path::Path};
/// Cross-task contract reserved for the owning task.
pub use tidemark_core::api::ComponentState;
/// Cross-task contract reserved for the owning task.
pub struct FirstbootInputs {
    #[doc = "Contract field."]
    pub version: String,
    #[doc = "Contract field."]
    pub config_sha256: String,
    #[doc = "Contract field."]
    pub daemon_started_at: i64,
    #[doc = "Contract field."]
    pub components: Vec<ComponentState>,
    #[doc = "Contract field."]
    pub watch_modes: Vec<(String, String)>,
    #[doc = "Contract field."]
    pub first_fs_sample: Option<i64>,
    #[doc = "Contract field."]
    pub first_lvm_sample: Option<i64>,
    #[doc = "Contract field."]
    pub next_scans: Vec<(String, i64)>,
}
/// Cross-task contract reserved for the owning task.
pub struct FirstbootReport {
    #[doc = "Contract field."]
    pub version: String,
    #[doc = "Contract field."]
    pub config_sha256: String,
    #[doc = "Contract field."]
    pub daemon_started_at: i64,
    #[doc = "Contract field."]
    pub components: Vec<ComponentState>,
    #[doc = "Contract field."]
    pub watch_modes: Vec<(String, String)>,
    #[doc = "Contract field."]
    pub first_fs_sample: Option<i64>,
    #[doc = "Contract field."]
    pub first_lvm_sample: Option<i64>,
    #[doc = "Contract field."]
    pub next_scans: Vec<(String, i64)>,
    #[doc = "Contract field."]
    pub booted_at: i64,
    #[doc = "Contract field."]
    pub healthy: bool,
    #[doc = "Contract field."]
    pub written_at: i64,
}
/// Cross-task contract reserved for the owning task.
pub fn needs_report(state_dir: &Path, config_sha256: &str) -> bool {
    let _ = state_dir;
    let _ = config_sha256;
    false
}
/// Cross-task contract reserved for the owning task.
pub fn build(inputs: &FirstbootInputs, now: i64) -> FirstbootReport {
    let _ = (inputs, now);
    FirstbootReport {
        version: String::new(),
        config_sha256: String::new(),
        daemon_started_at: 0,
        components: Vec::new(),
        watch_modes: Vec::new(),
        first_fs_sample: None,
        first_lvm_sample: None,
        next_scans: Vec::new(),
        booted_at: 0,
        healthy: false,
        written_at: 0,
    }
}
/// Cross-task contract reserved for the owning task.
pub fn write(state_dir: &Path, r: &FirstbootReport) -> io::Result<()> {
    let _ = state_dir;
    let _ = r;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
