//! Owned by: W1-A
//! Cross-task signatures reserved for W1-A.

use tidemark_core::{config::Config, path::RawPath};
/// Reserved for W1-A.
pub struct ProbeReport {
    #[doc = "Field specified by the owning task."]
    pub host: HostFacts,
    #[doc = "Field specified by the owning task."]
    pub roots: Vec<RootFacts>,
    #[doc = "Field specified by the owning task."]
    pub recommendations: Vec<Recommendation>,
}
/// Reserved for W1-A.
pub fn probe(cfg: &Config, extra_roots: &[RawPath]) -> ProbeReport {
    let _ = cfg;
    let _ = extra_roots;
    ProbeReport {
        host: HostFacts,
        roots: Vec::new(),
        recommendations: Vec::new(),
    }
}

/// Shape left to W1-A.
pub struct HostFacts;
/// Shape left to W1-A.
pub struct RootFacts;
/// Shape left to W1-A.
pub struct Recommendation;

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert!(probe(&Config::default(), &[]).roots.is_empty());
    }
}
