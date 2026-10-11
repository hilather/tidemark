//! Owned by: W5-C
//! Cross-task signatures reserved for W5-C.

use crate::PgError;
use tidemark_core::config::PostgresConfig;
/// Reserved for W5-C.
pub struct PgProbe {
    #[doc = "Field specified by the owning task."]
    pub reachable: bool,
    #[doc = "Field specified by the owning task."]
    pub server_version_num: Option<u32>,
    #[doc = "Field specified by the owning task."]
    pub in_recovery: Option<bool>,
    #[doc = "Field specified by the owning task."]
    pub data_directory: Option<String>,
    #[doc = "Field specified by the owning task."]
    pub missing_privileges: Vec<String>,
    #[doc = "Field specified by the owning task."]
    pub error: Option<String>,
}
/// Reserved for W5-C.
pub fn probe_from_config(cfg: &PostgresConfig) -> Result<PgProbe, PgError> {
    let _ = cfg;
    Err(PgError::NotYetImplemented)
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert_eq!(
            probe_from_config(&PostgresConfig::default())
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
    }
}
