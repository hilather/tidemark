//! Owned by: W5-A
//! Bounded minute aggregation with deterministic top-key retention.
use super::{CompletedMinute, HotScope};
use std::collections::{BTreeMap, HashSet, VecDeque};
use tidemark_core::live::{HotEntry, HotMinute};
const CURRENT_CAP: usize = 20_000;
const COMPLETED_CAP: usize = 1_000;
// Space-saving counters are exact through eight units per row and minute;
// beyond eight, the reported leader is approximate. Zero-byte events still attribute.
const UNIT_CAP: usize = 8;
#[derive(Default)]
struct UnitCounts(BTreeMap<String, u64>);
impl UnitCounts {
    fn add(&mut self, unit: &str, weight: u64) {
        if let Some(count) = self.0.get_mut(unit) {
            *count = count.saturating_add(weight);
            return;
        }
        let baseline = if self.0.len() == UNIT_CAP {
            let victim = self
                .0
                .iter()
                .min_by(|(a, x), (b, y)| x.cmp(y).then(b.cmp(a)))
                .map(|(name, count)| (name.clone(), *count));
            if let Some((name, count)) = victim {
                self.0.remove(&name);
                count
            } else {
                0
            }
        } else {
            0
        };
        self.0.insert(unit.into(), baseline.saturating_add(weight));
    }
    fn top(&self) -> Option<&String> {
        self.0
            .iter()
            .max_by(|(a, x), (b, y)| x.cmp(y).then(b.cmp(a)))
            .map(|(unit, _)| unit)
    }
}
struct Bucket {
    minute: i64,
    maps: [BTreeMap<Vec<u8>, HotEntry>; 3],
    units: [BTreeMap<Vec<u8>, UnitCounts>; 3],
    lossy: bool,
    drained: bool,
    truncated: bool,
    drain_truncated: bool,
    distinct_capped: bool,
    current_capped: bool,
    completed_capped: bool,
    read_error: bool,
    descriptor_pressure: bool,
    overflow_start: u64,
    overflow_end: u64,
    seen: HashSet<(usize, Vec<u8>, (u64, u64))>,
}
pub(super) struct Rings {
    buckets: VecDeque<Bucket>,
    pub dropped: u64,
    overflow_total: u64,
    next_minute: Option<i64>,
    rollback_until: Option<i64>,
    last_emitted_end: Option<u64>,
    rows_drained_until: Option<i64>,
}
fn ranked(map: &BTreeMap<Vec<u8>, HotEntry>) -> Vec<HotEntry> {
    let mut rows: Vec<_> = map.values().cloned().collect();
    rows.sort_by(|a, b| b.bytes_delta.cmp(&a.bytes_delta).then(a.key.cmp(&b.key)));
    rows
}
impl Rings {
    pub fn new() -> Self {
        Self {
            buckets: VecDeque::new(),
            dropped: 0,
            overflow_total: 0,
            next_minute: None,
            rollback_until: None,
            last_emitted_end: None,
            rows_drained_until: None,
        }
    }
    pub fn advance(&mut self, now: i64) {
        let minute = now.div_euclid(60) * 60;
        self.next_minute.get_or_insert(minute.saturating_add(60));
        // Preserve the emission watermark across the existing rollback reset.
        let rollback = self.buckets.back().is_some_and(|b| b.minute > minute);
        if rollback {
            let previous = self.buckets.back().map(|b| b.minute);
            self.rollback_until = self.rollback_until.max(previous);
            self.buckets.clear();
        }
        if self.buckets.back().is_some_and(|b| b.minute == minute) {
            return;
        }
        if let Some(last) = self.buckets.back_mut() {
            last.seen.clear();
            last.seen.shrink_to_fit();
            last.overflow_end = self.overflow_total;
            for (map, units) in last.maps.iter_mut().zip(&mut last.units) {
                let rows = ranked(map);
                if rows.len() > COMPLETED_CAP {
                    last.lossy = true;
                    last.truncated = true;
                    last.completed_capped = true;
                    self.dropped += (rows.len() - COMPLETED_CAP) as u64;
                }
                *map = rows
                    .into_iter()
                    .take(COMPLETED_CAP)
                    .map(|r| (r.key.clone(), r))
                    .collect();
                units.retain(|key, _| map.contains_key(key));
            }
        }
        let next = self.buckets.back().map(|b| b.minute.saturating_add(60));
        self.buckets
            .retain(|b| b.minute > minute.saturating_sub(3600));
        // Bound both work and storage even after years without a poll or drain.
        let first = next.unwrap_or(minute).max(minute.saturating_sub(59 * 60));
        for offset in 0..60 {
            let at = first.saturating_add(offset * 60);
            if at > minute {
                break;
            }
            self.buckets.push_back(Bucket {
                minute: at,
                maps: std::array::from_fn(|_| BTreeMap::new()),
                units: std::array::from_fn(|_| BTreeMap::new()),
                lossy: self.rollback_until.is_some_and(|end| at <= end),
                drained: self.rows_drained_until.is_some_and(|end| at <= end),
                truncated: false,
                drain_truncated: false,
                distinct_capped: false,
                current_capped: false,
                completed_capped: false,
                read_error: false,
                descriptor_pressure: false,
                overflow_start: self.overflow_total,
                overflow_end: self.overflow_total,
                seen: HashSet::new(),
            });
        }
        while self.buckets.len() > 60 {
            self.buckets.pop_front();
        }
    }
    // Queue overflow totals are distinct from loss caused by recovery or caps.
    pub fn queue_overflow(&mut self, now: i64) {
        self.overflow(now);
        self.overflow_total = self.overflow_total.saturating_add(1);
        if let Some(bucket) = self.buckets.back_mut() {
            bucket.overflow_end = self.overflow_total;
        }
    }
    // All time traversed while the transport is unavailable is uncertain.
    pub fn recovery(&mut self, now: i64) {
        let start = self.buckets.back().map_or(now, |b| b.minute);
        self.advance(now);
        for bucket in &mut self.buckets {
            if bucket.minute >= start {
                bucket.lossy = true;
            }
        }
    }
    pub fn read_failure(&mut self, now: i64, descriptor_pressure: bool) {
        self.overflow(now);
        if let Some(b) = self.buckets.back_mut() {
            b.descriptor_pressure |= descriptor_pressure;
            b.read_error |= !descriptor_pressure;
        }
    }
    pub fn overflow(&mut self, now: i64) {
        self.advance(now);
        if let Some(b) = self.buckets.back_mut() {
            b.lossy = true;
        }
    }
    pub fn add(
        &mut self,
        now: i64,
        keys: [Vec<u8>; 3],
        delta: i64,
        unit: Option<String>,
        identity: (u64, u64),
    ) {
        self.advance(now);
        let Some(bucket) = self.buckets.back_mut() else {
            return;
        };
        for (index, (map, key)) in bucket.maps.iter_mut().zip(keys).enumerate() {
            let unique = if bucket.seen.len() < 200_000 {
                bucket.seen.insert((index, key.clone(), identity))
            } else {
                bucket.lossy = true;
                bucket.truncated = true;
                bucket.distinct_capped = true;
                false
            };
            if !map.contains_key(&key) && map.len() >= CURRENT_CAP {
                let rows = ranked(map);
                self.dropped += (rows.len() / 2) as u64;
                *map = rows
                    .into_iter()
                    .take(CURRENT_CAP / 2)
                    .map(|r| (r.key.clone(), r))
                    .collect();
                bucket.units[index].retain(|key, _| map.contains_key(key));
                bucket.lossy = true;
                bucket.truncated = true;
                bucket.current_capped = true;
            }
            let row = if map.contains_key(&key) {
                map.get_mut(&key)
            } else {
                Some(map.entry(key.clone()).or_insert(HotEntry {
                    key,
                    events: 0,
                    files: 0,
                    bytes_delta: 0,
                    top_unit: None,
                    lossy: false,
                }))
            };
            let Some(row) = row else {
                continue;
            };
            if let Some(unit) = unit.as_deref() {
                if !bucket.units[index].contains_key(&row.key) {
                    bucket.units[index].insert(row.key.clone(), UnitCounts::default());
                }
                let Some(counts) = bucket.units[index].get_mut(&row.key) else {
                    continue;
                };
                counts.add(unit, delta.unsigned_abs());
                let top = counts.top();
                if row.top_unit.as_ref() != top {
                    row.top_unit = top.cloned();
                }
            }
            row.events = row.events.saturating_add(1);
            row.files = row.files.saturating_add(u64::from(unique));
            row.bytes_delta = row.bytes_delta.saturating_add(delta);
        }
    }
    pub fn hot(&self, now: i64, window: u32, scope: HotScope, limit: usize) -> Vec<HotEntry> {
        let index = match scope {
            HotScope::Dir => 0,
            HotScope::Unit => 1,
            HotScope::Comm => 2,
        };
        let cutoff = now.div_euclid(60) * 60 - i64::from(window.clamp(1, 60) - 1) * 60;
        let mut sum: BTreeMap<Vec<u8>, HotEntry> = BTreeMap::new();
        let mut units: BTreeMap<Vec<u8>, BTreeMap<String, u64>> = BTreeMap::new();
        let lossy = self
            .buckets
            .iter()
            .any(|b| b.minute >= cutoff && b.minute <= now.div_euclid(60) * 60 && b.lossy);
        for bucket in self
            .buckets
            .iter()
            .filter(|b| b.minute >= cutoff && b.minute <= now.div_euclid(60) * 60)
        {
            for row in bucket.maps[index].values() {
                if let Some(counts) = bucket.units[index].get(&row.key) {
                    // At most 60 * eight candidates per retained row across the window.
                    let merged = units.entry(row.key.clone()).or_default();
                    for (unit, weight) in &counts.0 {
                        let count = merged.entry(unit.clone()).or_default();
                        *count = count.saturating_add(*weight);
                    }
                }
                let entry = sum.entry(row.key.clone()).or_insert(HotEntry {
                    key: row.key.clone(),
                    events: 0,
                    files: 0,
                    bytes_delta: 0,
                    top_unit: row.top_unit.clone(),
                    lossy,
                });
                entry.events = entry.events.saturating_add(row.events);
                entry.files = entry.files.saturating_add(row.files);
                entry.bytes_delta = entry.bytes_delta.saturating_add(row.bytes_delta);
            }
        }
        for (key, counts) in units {
            if let Some(row) = sum.get_mut(&key) {
                row.top_unit = UnitCounts(counts).top().cloned();
            }
        }
        ranked(&sum).into_iter().take(limit).collect()
    }
    pub fn detail(&self, now: i64) -> String {
        let cutoff = now.div_euclid(60) * 60 - 14 * 60;
        let mut reasons = Vec::new();
        for b in self
            .buckets
            .iter()
            .filter(|b| b.minute >= cutoff && b.minute <= now)
        {
            let minute = format!(
                "{:02}:{:02} UTC (timestamp {})",
                b.minute.div_euclid(3600).rem_euclid(24),
                b.minute.div_euclid(60).rem_euclid(60),
                b.minute
            );
            if b.distinct_capped {
                reasons.push(format!("truncated: distinct-file cap reached in the minute {minute} (events complete, per-file counts capped)"));
            }
            if b.current_capped {
                reasons.push(format!(
                    "truncated: current-key cap reached in the minute {minute}"
                ));
            }
            if b.completed_capped {
                reasons.push(format!(
                    "truncated: completed-key cap reached in the minute {minute}"
                ));
            }
            if b.drain_truncated {
                reasons.push(format!(
                    "truncated: requested-output cap reached in the minute {minute}"
                ));
            }
            if b.read_error {
                reasons.push(format!("lost events: read error in the minute {minute}"));
            }
            if b.descriptor_pressure {
                reasons.push(format!(
                    "lost events: descriptor pressure in the minute {minute}"
                ));
            }
            if b.overflow_end > b.overflow_start {
                reasons.push(format!(
                    "lost events: queue overflow in the minute {minute}"
                ));
            }
            if b.lossy
                && !b.truncated
                && !b.read_error
                && !b.descriptor_pressure
                && b.overflow_end == b.overflow_start
            {
                reasons.push(format!(
                    "lost events: {} in the minute {minute}",
                    if b.overflow_end > b.overflow_start {
                        "queue overflow"
                    } else {
                        "transport recovery or history gap"
                    }
                ));
            }
        }
        reasons.join("; ")
    }
    pub fn lossy(&self, now: i64) -> u32 {
        self.buckets
            .iter()
            .filter(|b| {
                b.minute >= now.div_euclid(60) * 60 - 14 * 60
                    && b.minute <= now.div_euclid(60) * 60
                    && b.lossy
            })
            .count() as u32
    }
    fn persisted(b: &mut Bucket, mount: &str, top_k: usize) -> Vec<HotMinute> {
        if b.drained {
            return Vec::new();
        }
        b.drained = true;
        let limit = top_k.min(COMPLETED_CAP);
        b.drain_truncated = b.maps.iter().any(|map| map.len() > limit);
        let mut result = Vec::new();
        for (scope, map) in ["dir", "unit", "comm"].into_iter().zip(&b.maps) {
            result.extend(ranked(map).into_iter().take(limit).map(|r| HotMinute {
                mount: mount.into(),
                scope: scope.into(),
                key: r.key,
                ts: b.minute,
                events: r.events,
                files: r.files,
                bytes_delta: r.bytes_delta,
                lossy: b.lossy,
            }));
        }
        result
    }
    pub fn drain(&mut self, now: i64, mount: &str, top_k: usize) -> Vec<HotMinute> {
        self.advance(now);
        let mut result = Vec::new();
        for b in &mut self.buckets {
            if b.minute < now.div_euclid(60) * 60 {
                result.extend(Self::persisted(b, mount, top_k));
                self.rows_drained_until = self.rows_drained_until.max(Some(b.minute));
            }
        }
        result
    }
    pub fn drain_minutes(&mut self, now: i64, mount: &str, top_k: usize) -> Vec<CompletedMinute> {
        self.advance(now);
        let mut result = Vec::new();
        let Some(mut next) = self.next_minute else {
            return result;
        };
        for b in &mut self.buckets {
            if b.minute < next || b.minute >= now.div_euclid(60) * 60 {
                continue;
            }
            let persisted = Self::persisted(b, mount, top_k);
            self.rows_drained_until = self.rows_drained_until.max(Some(b.minute));
            let truncated =
                b.truncated || b.drain_truncated || b.maps[0].len() > top_k.min(COMPLETED_CAP);
            let lossy = b.lossy || truncated || b.minute != next;
            let dirs = ranked(&b.maps[0])
                .into_iter()
                .take(top_k.min(COMPLETED_CAP))
                .map(|mut row| {
                    row.lossy = lossy;
                    row
                })
                .collect();
            result.push(CompletedMinute {
                started_at: b.minute,
                overflow_total_at_start: if b.minute == next {
                    self.last_emitted_end.unwrap_or(b.overflow_start)
                } else {
                    b.overflow_start
                },
                overflow_total_at_end: b.overflow_end,
                lossy,
                truncated,
                dirs,
                persisted,
            });
            self.last_emitted_end = Some(b.overflow_end);
            next = b.minute.saturating_add(60);
        }
        self.next_minute = Some(next);
        result
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn keys(n: usize) -> [Vec<u8>; 3] {
        [
            format!("/dir/{n}").into_bytes(),
            b"unit".to_vec(),
            b"comm".to_vec(),
        ]
    }
    #[test]
    fn diagnostics_distinguish_caps_from_each_transport_loss() {
        let mut r = Rings::new();
        r.advance(0);
        r.read_failure(60, true);
        r.read_failure(61, false);
        r.queue_overflow(62);
        let bucket = r.buckets.back_mut().unwrap();
        bucket.truncated = true;
        bucket.distinct_capped = true;
        let detail = r.detail(62);
        for reason in [
            "truncated: distinct-file cap",
            "lost events: queue overflow",
            "lost events: read error",
            "lost events: descriptor pressure",
            "00:01 UTC (timestamp 60)",
        ] {
            assert!(detail.contains(reason), "{detail}");
        }
    }
    #[test]
    fn completed_empty_minutes_and_overflow_boundaries() {
        let mut r = Rings::new();
        r.advance(17);
        r.queue_overflow(130);
        r.queue_overflow(131);
        r.overflow(181); // Recovery is loss, not a queue overflow.
        let minutes = r.drain_minutes(300, "/", 1000);
        assert_eq!(
            minutes.iter().map(|m| m.started_at).collect::<Vec<_>>(),
            vec![60, 120, 180, 240]
        );
        assert_eq!(
            minutes
                .iter()
                .map(|m| (m.overflow_total_at_start, m.overflow_total_at_end))
                .collect::<Vec<_>>(),
            vec![(0, 0), (0, 2), (2, 2), (2, 2)]
        );
        assert_eq!(
            minutes.iter().map(|m| m.lossy).collect::<Vec<_>>(),
            vec![false, true, true, false]
        );
        assert!(minutes
            .iter()
            .all(|m| m.dirs.is_empty() && m.persisted.is_empty() && !m.truncated));
        assert!(r.drain_minutes(300, "/", 1000).is_empty());
    }
    #[test]
    fn pause_is_bounded_and_gap_marks_only_first_returned_minute() {
        let mut r = Rings::new();
        r.advance(0);
        r.queue_overflow(61);
        let initial = r.drain_minutes(120, "/", 1000);
        assert_eq!(initial[0].overflow_total_at_end, 1);
        let minutes = r.drain_minutes(120 + 90 * 60, "/", 1000);
        assert!(minutes.len() <= 60);
        assert_eq!(r.buckets.len(), 60);
        assert!(minutes[0].started_at > 120);
        assert!(minutes[0].lossy);
        assert!(minutes[1..].iter().all(|m| !m.lossy));
        assert!(minutes
            .iter()
            .all(|m| (m.overflow_total_at_start, m.overflow_total_at_end) == (1, 1)));
        assert!(minutes
            .windows(2)
            .all(|m| m[1].started_at == m[0].started_at + 60));
    }
    #[test]
    fn completed_caps_and_output_caps_are_truncated() {
        let mut r = Rings::new();
        r.advance(0);
        for n in 0..=COMPLETED_CAP {
            r.add(60, keys(n), n as i64, Some("u".into()), (1, n as u64));
        }
        let minutes = r.drain_minutes(120, "/", usize::MAX);
        assert_eq!(minutes[0].dirs.len(), COMPLETED_CAP);
        assert!(minutes[0].lossy && minutes[0].truncated);
        assert_eq!(minutes[0].dirs[0].top_unit.as_deref(), Some("u"));
        r.add(120, keys(1), 1, None, (1, 1));
        r.add(120, keys(2), 2, None, (1, 2));
        let minutes = r.drain_minutes(180, "/", 1);
        assert_eq!(minutes[0].dirs.len(), 1);
        assert_eq!(minutes[0].persisted.len(), 3);
        assert!(minutes[0].truncated && minutes[0].lossy);
    }
    #[test]
    fn drains_share_rows_but_not_minute_facts() {
        for legacy_first in [false, true] {
            let mut r = Rings::new();
            r.advance(0);
            r.add(60, keys(1), 1, None, (1, 1));
            if legacy_first {
                assert_eq!(r.drain(120, "/", 1).len(), 3);
            }
            let minutes = r.drain_minutes(120, "/", 1);
            assert_eq!(minutes.len(), 1);
            assert_eq!(minutes[0].dirs.len(), 1);
            assert_eq!(minutes[0].persisted.len(), if legacy_first { 0 } else { 3 });
            assert!(r.drain(120, "/", 1).is_empty());
            assert!(r.drain_minutes(120, "/", 1).is_empty());
        }
    }
    #[test]
    fn rollback_does_not_repeat_minutes_or_rows_and_totals_chain() {
        let mut r = Rings::new();
        r.advance(0);
        r.add(60, keys(1), 1, None, (1, 1));
        let first = r.drain_minutes(120, "/", 1000);
        assert_eq!(first[0].started_at, 60);
        r.queue_overflow(121);
        r.advance(30);
        r.add(60, keys(1), 1, None, (1, 1));
        assert!(r.drain_minutes(120, "/", 1000).is_empty());
        assert!(r.drain(120, "/", 1000).is_empty());
        let next = r.drain_minutes(240, "/", 1000);
        assert_eq!(
            next.iter().map(|m| m.started_at).collect::<Vec<_>>(),
            vec![120, 180]
        );
        assert!(next[0].lossy);
        assert!(!next[1].lossy);
        assert_eq!(
            first[0].overflow_total_at_end,
            next[0].overflow_total_at_start
        );
        assert_eq!(
            (
                next[0].overflow_total_at_start,
                next[0].overflow_total_at_end
            ),
            (0, 1)
        );
        assert_eq!(
            next[0].overflow_total_at_end,
            next[1].overflow_total_at_start
        );
    }
    #[test]
    fn attributed_leaders_and_absolute_deltas() {
        for (events, expected) in [
            (vec![(Some("a"), 1)], Some("a")),
            (vec![(Some("a"), 1), (Some("b"), 2)], Some("b")),
            (vec![(Some("b"), 2), (Some("a"), 2)], Some("a")),
            (vec![(None, 9)], None),
            (vec![(Some("a"), 2), (Some("b"), -9)], Some("b")),
        ] {
            let mut r = Rings::new();
            for (unit, delta) in events {
                r.add(0, keys(1), delta, unit.map(str::to_owned), (1, 1));
            }
            assert_eq!(
                r.hot(0, 5, HotScope::Dir, 1)[0].top_unit.as_deref(),
                expected
            );
            r.advance(60);
            assert_eq!(
                r.buckets[0].maps[0]
                    .values()
                    .next()
                    .unwrap()
                    .top_unit
                    .as_deref(),
                expected
            );
            assert_eq!(r.drain(60, "/", 50).len(), 3);
        }
    }
    #[test]
    fn window_leader_merges_nonleaders() {
        let mut r = Rings::new();
        for (at, unit, delta) in [(0, "a", 10), (0, "b", 9), (60, "c", 10), (60, "b", 9)] {
            r.add(at, keys(1), delta, Some(unit.into()), (1, 1));
        }
        assert_eq!(
            r.hot(60, 5, HotScope::Dir, 1)[0].top_unit.as_deref(),
            Some("b")
        );
        assert_eq!(
            r.hot(60, 1, HotScope::Dir, 1)[0].top_unit.as_deref(),
            Some("c")
        );
    }
    #[test]
    fn candidates_are_bounded_and_unit_rows_identify_themselves() {
        let mut r = Rings::new();
        for n in 0..20 {
            let unit = format!("u{n:02}");
            let mut k = keys(1);
            k[1] = unit.as_bytes().to_vec();
            r.add(0, k, 1, Some(unit), (1, 1));
        }
        assert_eq!(
            r.buckets[0].units[0].values().next().unwrap().0.len(),
            UNIT_CAP
        );
        for row in r.hot(0, 1, HotScope::Unit, 50) {
            assert_eq!(row.top_unit.as_ref().unwrap().as_bytes(), row.key);
        }
        r.add(
            0,
            [b"/unknown".to_vec(), b"?".to_vec(), b"?".to_vec()],
            1,
            None,
            (1, 2),
        );
        assert!(r
            .hot(0, 1, HotScope::Unit, 50)
            .iter()
            .find(|r| r.key == b"?")
            .unwrap()
            .top_unit
            .is_none());
    }
    #[test]
    fn window_rollover_lossy_and_drain_once() {
        let mut r = Rings::new();
        r.add(60, keys(1), 12, None, (1, 1));
        r.add(61, keys(1), -2, None, (1, 1));
        assert_eq!(r.hot(61, 1, HotScope::Dir, 1)[0].files, 1);
        r.overflow(61);
        r.add(120, keys(1), 7, None, (1, 2));
        assert_eq!(r.hot(120, 1, HotScope::Dir, 1)[0].bytes_delta, 7);
        let rows = r.hot(120, 5, HotScope::Dir, 1);
        assert_eq!(
            (
                rows[0].bytes_delta,
                rows[0].events,
                rows[0].files,
                rows[0].lossy
            ),
            (17, 3, 2, true)
        );
        assert_eq!(r.drain(120, "/", 50).len(), 3);
        assert!(r.drain(120, "/", 50).is_empty());
        r.advance(4000);
        assert!(r.hot(4000, 60, HotScope::Dir, 50).is_empty());
        assert_eq!(r.lossy(4000), 0);
    }
    #[test]
    fn key_caps_shrink_and_deterministic_ties() {
        let mut r = Rings::new();
        for n in 0..=CURRENT_CAP {
            r.add(0, keys(n), n as i64, None, (1, n as u64));
        }
        assert!(r.buckets[0].maps[0].len() <= CURRENT_CAP);
        assert!(r.dropped > 0);
        r.advance(60);
        assert_eq!(r.buckets[0].maps[0].len(), COMPLETED_CAP);
        assert_eq!(
            r.hot(60, 5, HotScope::Dir, 1)[0].bytes_delta,
            CURRENT_CAP as i64
        );
        assert!(r.hot(60, 5, HotScope::Dir, 1)[0].lossy);
        assert!(r.buckets[0].seen.is_empty());
    }
    #[test]
    fn sixty_buckets_and_clock_rollback() {
        let mut r = Rings::new();
        for minute in 0..100 {
            r.add(minute * 60, keys(1), 1, None, (1, 1));
        }
        assert_eq!(r.buckets.len(), 60);
        r.advance(0);
        assert_eq!(r.buckets.len(), 1);
        assert!(r.hot(0, 60, HotScope::Comm, 1).is_empty());
        assert_eq!(r.lossy(0), 1);
        r.add(0, keys(1), 5, None, (1, 1));
        assert_eq!(r.hot(0, 1, HotScope::Dir, 1)[0].bytes_delta, 5);
        assert!(r.hot(-60, 1, HotScope::Dir, 1).is_empty());
        assert_eq!(r.lossy(-60), 0);
        assert!(r.drain(0, "/", 5).is_empty());
        assert_eq!(r.drain(60, "/", 5).len(), 3);
        assert!(r.drain(60, "/", 5).is_empty());
    }
}
