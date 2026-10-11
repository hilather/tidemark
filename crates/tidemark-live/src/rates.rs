//! Owned by: W5-D
//! Least-squares growth rates and checked time-to-full estimates.

use std::time::Duration;
use tidemark_core::path::RawPath;

/// Growth-rate contract using Unix seconds and disk bytes.
pub fn slope_bytes_per_sec(samples: &[(i64, i64)]) -> Option<f64> {
    slope(samples.iter().copied())
}
/// Fit samples within the inclusive window, excluding future samples.
pub fn window_rate(samples: &[(i64, i64)], now: i64, window: Duration) -> Option<f64> {
    let start = i128::from(now) - i128::from(window.as_secs());
    slope(
        samples
            .iter()
            .copied()
            .filter(|(ts, _)| i128::from(*ts) >= start && *ts <= now),
    )
}
/// Estimate exhaustion time for a finite positive growth rate.
pub fn time_to_full(avail: u64, rate: f64) -> Option<Duration> {
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }
    Duration::try_from_secs_f64(avail as f64 / rate).ok()
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

fn slope(samples: impl Iterator<Item = (i64, i64)>) -> Option<f64> {
    // Translate before conversion to preserve precision near extreme timestamps/values.
    let mut origin = None;
    let (mut n, mut mx, mut my, mut xx, mut xy) = (0u64, 0.0, 0.0, 0.0, 0.0);
    for (ts, bytes) in samples {
        let (ot, ob) = *origin.get_or_insert((ts, bytes));
        let x = (i128::from(ts) - i128::from(ot)) as f64;
        let y = (i128::from(bytes) - i128::from(ob)) as f64;
        n += 1;
        let dx = x - mx;
        let dy = y - my;
        mx += dx / n as f64;
        my += dy / n as f64;
        xx += dx * (x - mx);
        xy += dx * (y - my);
    }
    if n < 3 || xx <= 0.0 {
        None
    } else {
        Some(xy / xx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn series_and_edges() {
        assert_eq!(slope_bytes_per_sec(&[(0, 0), (1, 10), (2, 20)]), Some(10.0));
        assert_eq!(slope_bytes_per_sec(&[(0, 7), (1, 7), (2, 7)]), Some(0.0));
        assert_eq!(slope_bytes_per_sec(&[(0, 0), (1, 12), (2, 20)]), Some(10.0));
        assert_eq!(
            slope_bytes_per_sec(&[(0, 0), (1, 10), (2, 0), (3, 10), (4, 0)]),
            Some(0.0)
        );
        assert_eq!(slope_bytes_per_sec(&[(0, 0), (1, 1)]), None);
        assert_eq!(slope_bytes_per_sec(&[(0, 0), (0, 1), (0, 2)]), None);
        assert_eq!(
            slope_bytes_per_sec(&[
                (i64::MAX - 2, i64::MAX - 2),
                (i64::MAX - 1, i64::MAX - 1),
                (i64::MAX, i64::MAX)
            ]),
            Some(1.0)
        );
        assert_eq!(time_to_full(10, 2.0), Some(Duration::from_secs(5)));
        for r in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE] {
            assert_eq!(time_to_full(10, r), None);
        }
        assert_eq!(time_to_full(0, 1.0), Some(Duration::ZERO));
        assert_eq!(
            window_rate(
                &[(0, 0), (1, 1), (2, 2), (3, 99)],
                2,
                Duration::from_secs(2)
            ),
            Some(1.0)
        );
    }
}
