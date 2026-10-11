//! Owned by: W2-C
//! Progress counters and deterministic reporting cadence.

use std::time::Duration;
/// Live scan counters and timing.
pub struct Progress {
    /// Contract field.
    pub phase: &'static str,
    /// Contract field.
    pub dirs: u64,
    /// Contract field.
    pub entries: u64,
    /// Contract field.
    pub errors: u64,
    /// Contract field.
    pub races: u64,
    /// Contract field.
    pub rate: f64,
    /// Contract field.
    pub elapsed: Duration,
    /// Contract field.
    pub eta: Option<Duration>,
}

/// Monotonic cadence, with injected elapsed time for repeatable tests.
pub struct Cadence {
    interval: Duration,
    last: Duration,
}
impl Cadence {
    /// Start a cadence without an immediate report.
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: Duration::ZERO,
        }
    }
    /// Return true once at each elapsed interval; never emit catch-up bursts.
    pub fn due(&mut self, elapsed: Duration) -> bool {
        if elapsed.saturating_sub(self.last) < self.interval {
            return false;
        }
        self.last = elapsed;
        true
    }
}
/// Estimate remaining duration from a previous entry count and current throughput.
pub fn estimate_eta(entries: u64, previous: Option<u64>, elapsed: Duration) -> Option<Duration> {
    let total = previous?;
    if entries == 0 || elapsed.is_zero() || total < entries {
        return None;
    }
    Duration::try_from_secs_f64(elapsed.as_secs_f64() * (total - entries) as f64 / entries as f64)
        .ok()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cadence_and_eta() {
        let mut c = Cadence::new(Duration::from_secs(2));
        assert!(!c.due(Duration::from_millis(1999)));
        assert!(c.due(Duration::from_secs(2)));
        assert!(!c.due(Duration::from_secs(2)));
        assert!(c.due(Duration::from_secs(20)));
        assert!(!c.due(Duration::from_secs(20)));
        assert_eq!(
            estimate_eta(50, Some(100), Duration::from_secs(2)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(estimate_eta(0, Some(100), Duration::from_secs(2)), None);
        assert_eq!(estimate_eta(50, None, Duration::from_secs(2)), None);
    }
}
