//! Owned by: W5-C
//! Cross-task interface stubs.

use tidemark_core::{
    config::PostgresConfig,
    live::{PgNow, PgSample},
    path::RawPath,
};
/// Reserved for W5-C.
pub struct PgProbe {
    /// Contract field.
    pub reachable: bool,
    /// Contract field.
    pub server_version_num: Option<u32>,
    /// Contract field.
    pub in_recovery: Option<bool>,
    /// Contract field.
    pub data_directory: Option<String>,
    /// Contract field.
    pub missing_privileges: Vec<String>,
    /// Contract field.
    pub error: Option<String>,
}
/// Reserved for W5-C.
pub fn probe_from_config(cfg: &PostgresConfig) -> Result<PgProbe, PgError> {
    let _ = cfg;
    Err(PgError::NotYetImplemented)
}

/// Reserved for W5-C.
pub type PgRow = Vec<Option<String>>;
/// Reserved for W5-C.
pub trait PgConn {
    /// Reserved for W5-C.
    fn query(&mut self, sql: &str, params: &[&str]) -> Result<Vec<PgRow>, PgError>;
}
/// Reserved for W5-C.
pub trait PgConnector: Send {
    /// Reserved for W5-C.
    fn connect(&self, dbname: &str) -> Result<Box<dyn PgConn>, PgError>;
}
/// Reserved for W5-C.
pub struct PgCollector;
impl PgCollector {
    /// Reserved for W5-C.
    pub fn new(cfg: PostgresConfig) -> Self {
        let _ = cfg;
        Self
    }
    /// Reserved for W5-C.
    pub fn with_connector(cfg: PostgresConfig, c: Box<dyn PgConnector>) -> Self {
        let _ = cfg;
        let _ = c;
        Self
    }
    /// Reserved for W5-C.
    pub fn collect_fast(&mut self, now: i64) -> Result<Vec<PgSample>, PgError> {
        let _ = now;
        Err(PgError::NotYetImplemented)
    }
    /// Reserved for W5-C.
    pub fn collect_relations(&mut self, now: i64) -> Result<Vec<PgSample>, PgError> {
        let _ = now;
        Err(PgError::NotYetImplemented)
    }
    /// Reserved for W5-C.
    pub fn path_labels(&mut self) -> Result<Vec<(RawPath, String, String)>, PgError> {
        Err(PgError::NotYetImplemented)
    }
    /// Reserved for W5-C.
    pub fn latest(&self) -> Option<PgNow> {
        None
    }
    /// Reserved for W5-C.
    pub fn backoff_until(&self) -> Option<i64> {
        None
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    struct Fake;
    impl PgConn for Fake {
        fn query(&mut self, _: &str, _: &[&str]) -> Result<Vec<PgRow>, PgError> {
            Err(PgError::NotYetImplemented)
        }
    }
    impl PgConnector for Fake {
        fn connect(&self, _: &str) -> Result<Box<dyn PgConn>, PgError> {
            Err(PgError::NotYetImplemented)
        }
    }
    #[test]
    fn every_stub_has_an_inert_result() {
        assert!(matches!(
            probe_from_config(&PostgresConfig::default()),
            Err(PgError::NotYetImplemented)
        ));
        assert!(matches!(
            Fake.query("", &[]),
            Err(PgError::NotYetImplemented)
        ));
        assert!(matches!(Fake.connect(""), Err(PgError::NotYetImplemented)));
        let mut c = PgCollector::new(PostgresConfig::default());
        assert!(matches!(c.collect_fast(0), Err(PgError::NotYetImplemented)));
        assert!(matches!(
            c.collect_relations(0),
            Err(PgError::NotYetImplemented)
        ));
        assert!(matches!(c.path_labels(), Err(PgError::NotYetImplemented)));
        assert!(c.latest().is_none());
        assert!(c.backoff_until().is_none());
        let c = PgCollector::with_connector(PostgresConfig::default(), Box::new(Fake));
        assert!(c.latest().is_none());
    }
}

/// Stub error reserved for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum PgError {
    /// Cross-task implementation is not available yet.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}
