//! Owned by: W1-E
//! Cross-task signatures reserved for W1-E.

use crate::catalog::ScanEntry;
use tidemark_core::config::SnapshotRetention;
/// Reserved for W1-E.
pub fn retention_plan(entries: &[ScanEntry], policy: &SnapshotRetention, now: i64) -> Vec<String> {
    let _ = entries;
    let _ = policy;
    let _ = now;
    Vec::new()
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert!(retention_plan(&[], &SnapshotRetention::default(), 0).is_empty());
    }
}
