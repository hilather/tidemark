//! Owned by: W5-D
//! Cross-task interface stubs.

use std::time::Duration;
use tidemark_core::path::RawPath;

/// Reserved for W5-D.
pub fn slope_bytes_per_sec(samples: &[(i64, i64)]) -> Option<f64> {
    let _ = samples;
    None
}
/// Reserved for W5-D.
pub fn window_rate(samples: &[(i64, i64)], now: i64, window: Duration) -> Option<f64> {
    let _ = samples;
    let _ = now;
    let _ = window;
    None
}
/// Reserved for W5-D.
pub fn time_to_full(avail: u64, rate: f64) -> Option<Duration> {
    let _ = avail;
    let _ = rate;
    None
}
/// Reserved for W5-D.
pub struct MountRates {
    /// Contract field.
    pub mount: String,
    /// Contract field.
    pub size: u64,
    /// Contract field.
    pub used: u64,
    /// Contract field.
    pub avail: u64,
    /// Contract field.
    pub rate_15m: Option<f64>,
    /// Contract field.
    pub rate_1h: Option<f64>,
    /// Contract field.
    pub ttf: Option<Duration>,
}
/// Reserved for W5-D.
pub struct DirGrowth {
    /// Contract field.
    pub root: String,
    /// Contract field.
    pub path: RawPath,
    /// Contract field.
    pub depth: u32,
    /// Contract field.
    pub delta: i64,
    /// Contract field.
    pub between: (String, String),
}
