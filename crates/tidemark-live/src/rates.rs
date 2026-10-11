//! Owned by: W5-D
//! Cross-task signatures reserved for W5-D.

use std::time::Duration;
/// Reserved for W5-D.
pub fn slope_bytes_per_sec(samples: &[(i64, i64)]) -> Option<f64> {
    let _ = samples;
    None
}
/// Reserved for W5-D.
pub fn time_to_full(avail: u64, rate: f64) -> Option<Duration> {
    let _ = avail;
    let _ = rate;
    None
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert!(slope_bytes_per_sec(&[]).is_none());
        assert!(time_to_full(0, 0.0).is_none());
    }
}
