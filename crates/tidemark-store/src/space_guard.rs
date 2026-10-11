//! Owned by: W1-E
//! Cross-task signatures reserved for W1-E.

use std::{io, path::Path};
use tidemark_core::config::RetentionConfig;
/// Reserved for W1-E.
pub enum SpaceVerdict {
    /// Space policy permits writing state.
    Ok,
    /// Space policy refuses writing state.
    Refuse {
        /// Explanation of the refusal.
        reason: String,
    },
}
/// Reserved for W1-E.
pub fn space_check(
    state_dir: &Path,
    estimate: u64,
    policy: &RetentionConfig,
) -> io::Result<SpaceVerdict> {
    let _ = state_dir;
    let _ = estimate;
    let _ = policy;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-E.
pub fn space_check_with(
    statvfs: impl Fn() -> io::Result<FsTotals>,
    used_by_state: u64,
    estimate: u64,
    policy: &RetentionConfig,
) -> SpaceVerdict {
    let _ = statvfs;
    let _ = used_by_state;
    let _ = estimate;
    let _ = policy;
    SpaceVerdict::Refuse {
        reason: "NotYetImplemented".into(),
    }
}

/// Shape left to W1-E.
pub struct FsTotals;

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert_eq!(
            space_check(std::path::Path::new("/"), 0, &RetentionConfig::default())
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(
            matches!(space_check_with(|| Ok(FsTotals), 0, 0, &RetentionConfig::default()), SpaceVerdict::Refuse { reason } if reason == "NotYetImplemented")
        );
    }
}
