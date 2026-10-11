//! Owned by: W2-C
//! Cross-task interface stubs.

use std::time::Duration;
/// Reserved for W2-C.
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
