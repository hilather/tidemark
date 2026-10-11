//! Owned by: W1-E
//! Pure per-root snapshot retention using UTC calendar and ISO-week buckets.
use crate::catalog::ScanEntry;
use std::collections::{BTreeMap, BTreeSet};
use tidemark_core::config::SnapshotRetention;

/// Return scan ids to delete, retaining recent scans and calendar representatives.
///
/// Days use UTC; ISO weeks start Monday. Running scans are never removed.
pub fn retention_plan(entries: &[ScanEntry], policy: &SnapshotRetention, now: i64) -> Vec<String> {
    let today = now.div_euclid(86400);
    // Unix epoch was Thursday; adding three gives Monday-based week numbers.
    let this_week = (today + 3).div_euclid(7);
    let mut roots: BTreeMap<&[u8], Vec<&ScanEntry>> = BTreeMap::new();
    let mut deleted = Vec::new();
    for e in entries {
        match e.status.as_str() {
            "partial" | "failed" => {
                if now.saturating_sub(e.finished_at.unwrap_or(e.started_at)) > 86400 {
                    deleted.push(e.scan_id.clone());
                }
            }
            "complete" => roots.entry(e.root.as_bytes()).or_default().push(e),
            _ => {}
        }
    }
    for rows in roots.values_mut() {
        rows.sort_by(|a, b| {
            b.started_at
                .cmp(&a.started_at)
                .then_with(|| b.scan_id.cmp(&a.scan_id))
        });
        let mut days = BTreeSet::new();
        let mut weeks = BTreeSet::new();
        for (i, e) in rows.iter().enumerate() {
            let day = e.started_at.div_euclid(86400);
            let week = (day + 3).div_euclid(7);
            let daily = day <= today
                && usize::try_from(today.saturating_sub(day)).is_ok_and(|n| n < policy.daily)
                && days.insert(day);
            let weekly = week <= this_week
                && usize::try_from(this_week.saturating_sub(week)).is_ok_and(|n| n < policy.weekly)
                && weeks.insert(week);
            if i >= policy.recent && !daily && !weekly {
                deleted.push(e.scan_id.clone());
            }
        }
    }
    deleted.sort();
    deleted
}
