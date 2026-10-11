//! Owned by: W0-B1
//! Injected scan time, age buckets and scan identifiers.

use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Age bucket boundaries in seconds; equality belongs to the older bucket.
pub const AGE_BOUNDS_SECS: [i64; 5] = [86_400, 604_800, 2_592_000, 7_776_000, 31_536_000];
/// Bucket an inode mtime relative to scan start; future mtimes are youngest.
pub fn age_bucket(scan_start: i64, mtime: i64) -> usize {
    let age = scan_start.saturating_sub(mtime).max(0);
    AGE_BOUNDS_SECS.partition_point(|&bound| age >= bound)
}
/// UTC scan identifier in `YYYYMMDDTHHMMSSZ-<8 hex>` form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScanId(
    /// Formatted scan identifier.
    pub String,
);
impl ScanId {
    /// Build from an injected Unix time; suffix hashes the process id and a per-process sequence.
    pub fn new(now: i64) -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let suffix = xxhash_rust::xxh3::xxh3_64(
            &[
                now.to_le_bytes(),
                sequence.to_le_bytes(),
                u64::from(std::process::id()).to_le_bytes(),
            ]
            .concat(),
        ) as u32;
        let date = chrono::DateTime::from_timestamp(now, 0)
            .filter(|d| (0..=9999).contains(&chrono::Datelike::year(d)))
            .map(|d| d.format("%Y%m%dT%H%M%SZ").to_string())
            .unwrap_or_else(|| "19700101T000000Z".into());
        Self(format!("{date}-{suffix:08x}"))
    }
    /// Borrow the identifier string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for ScanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
/// Current Unix seconds; callers inject this into deterministic logic.
pub fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs().min(i64::MAX as u64) as i64,
        Err(e) => -(e.duration().as_secs().min(i64::MAX as u64) as i64),
    }
}
/// Parse a human duration.
pub fn parse_duration(input: &str) -> Result<Duration, CoreError> {
    humantime::parse_duration(input).map_err(|e| CoreError::Parse(e.to_string()))
}
/// Format a duration using humantime.
pub fn fmt_duration(duration: Duration) -> String {
    humantime::format_duration(duration).to_string()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn age_boundaries() {
        assert_eq!(age_bucket(0, 1), 0);
        for (i, b) in AGE_BOUNDS_SECS.into_iter().enumerate() {
            assert_eq!(age_bucket(b - 1, 0), i);
            assert_eq!(age_bucket(b, 0), i + 1);
        }
        assert_eq!(age_bucket(i64::MAX, i64::MIN), 5);
    }
    #[test]
    fn scan_id_format() {
        let id = ScanId::new(0);
        assert_eq!(&id.as_str()[..17], "19700101T000000Z-");
        assert_eq!(id.as_str().len(), 25);
        assert!(id.as_str()[17..].chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(id, ScanId::new(0));
    }
}
