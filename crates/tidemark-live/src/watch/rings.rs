//! Owned by: W5-A
//! Bounded minute aggregation with deterministic top-key retention.
use super::HotScope;
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
    seen: HashSet<(usize, Vec<u8>, (u64, u64))>,
}
pub(super) struct Rings {
    buckets: VecDeque<Bucket>,
    pub dropped: u64,
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
        }
    }
    pub fn advance(&mut self, now: i64) {
        let minute = now.div_euclid(60) * 60;
        // Reset on rollback: discarded history cannot be attributed to the new timeline.
        let rollback = self.buckets.back().is_some_and(|b| b.minute > minute);
        if rollback {
            self.buckets.clear();
        }
        if self.buckets.back().is_some_and(|b| b.minute == minute) {
            return;
        }
        if let Some(last) = self.buckets.back_mut() {
            last.seen.clear();
            last.seen.shrink_to_fit();
            for (map, units) in last.maps.iter_mut().zip(&mut last.units) {
                let rows = ranked(map);
                if rows.len() > COMPLETED_CAP {
                    last.lossy = true;
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
        self.buckets
            .retain(|b| b.minute > minute.saturating_sub(3600));
        self.buckets.push_back(Bucket {
            minute,
            maps: std::array::from_fn(|_| BTreeMap::new()),
            units: std::array::from_fn(|_| BTreeMap::new()),
            lossy: rollback,
            drained: false,
            seen: HashSet::new(),
        });
        while self.buckets.len() > 60 {
            self.buckets.pop_front();
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
    pub fn drain(&mut self, now: i64, mount: &str, top_k: usize) -> Vec<HotMinute> {
        self.advance(now);
        let mut result = Vec::new();
        for b in &mut self.buckets {
            if b.minute >= now.div_euclid(60) * 60 || b.drained {
                continue;
            }
            b.drained = true;
            for (scope, map) in ["dir", "unit", "comm"].into_iter().zip(&b.maps) {
                result.extend(
                    ranked(map)
                        .into_iter()
                        .take(top_k.min(COMPLETED_CAP))
                        .map(|r| HotMinute {
                            mount: mount.into(),
                            scope: scope.into(),
                            key: r.key,
                            ts: b.minute,
                            events: r.events,
                            files: r.files,
                            bytes_delta: r.bytes_delta,
                            lossy: b.lossy,
                        }),
                );
            }
        }
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
