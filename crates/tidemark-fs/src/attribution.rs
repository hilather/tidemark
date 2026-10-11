//! Owned by: W1-D
//! Cross-task signatures reserved for W1-D.

use std::time::Duration;
use tidemark_core::live::Attribution;
/// Reserved for W1-D.
pub struct AttributionCache;
impl AttributionCache {
    /// Reserved for W1-D.
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        let _ = ttl;
        let _ = capacity;
        Self
    }
    /// Reserved for W1-D.
    pub fn lookup(&self, pid: u32) -> Attribution {
        let _ = pid;
        Attribution {
            pid,
            starttime: 0,
            comm: "?".into(),
            unit: None,
            uid: None,
        }
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert_eq!(
            AttributionCache::new(std::time::Duration::ZERO, 0)
                .lookup(0)
                .comm,
            "?"
        );
        assert_eq!((AttributionCache).lookup(0).comm, "?");
    }
}
