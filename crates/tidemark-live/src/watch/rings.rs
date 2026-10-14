//! Owned by: W5-A
//! Bounded minute aggregation with deterministic top-key retention.
use super::{
    budget::{copy, Budget, Reservation},
    CompletedMinute, HotScope,
};
use std::collections::{BTreeMap, HashSet, VecDeque};
use tidemark_core::live::{HotEntry, HotMinute};
// Conservative heap credits include sparse B-tree nodes, hash-table growth,
// reference ranking scratch and allocation headers, in addition to actual keys.
const ROW_NODE_BYTES: usize = 512;
const ROW_MAP_BYTES: usize = 4096;
const UNIT_NODE_BYTES: usize = 768;
const UNIT_MAP_BYTES: usize = 2048;
const SEEN_NODE_BYTES: usize = 256;
const CURRENT_CAP: usize = 20_000;
const COMPLETED_CAP: usize = 1_000;
// Space-saving counters are exact through eight units per row and minute;
// beyond eight, the reported leader is approximate. Zero-byte events still attribute.
const UNIT_CAP: usize = 8;
#[derive(Default)]
struct UnitCounts(BTreeMap<String, u64>);
impl UnitCounts {
    fn add(&mut self, unit: &str, weight: u64) -> bool {
        if let Some(count) = self.0.get_mut(unit) {
            *count = count.saturating_add(weight);
            return true;
        }
        let Some(name) = super::budget::copy_string(unit) else {
            return false;
        };
        let baseline = if self.0.len() == UNIT_CAP {
            let victim = self
                .0
                .iter()
                .min_by(|(a, x), (b, y)| x.cmp(y).then(b.cmp(a)))
                .map(|(name, _)| name.as_str() as *const str);
            // Extract the selected owned name; comparing pointers never dereferences them.
            self.0
                .extract_if(.., |name, _| Some(name.as_str() as *const str) == victim)
                .next()
                .map_or(0, |(_, count)| count)
        } else {
            0
        };
        self.0.insert(name, baseline.saturating_add(weight));
        true
    }
    fn top(&self) -> Option<&String> {
        self.0
            .iter()
            .max_by(|(a, x), (b, y)| x.cmp(y).then(b.cmp(a)))
            .map(|(unit, _)| unit)
    }
}
struct Bucket {
    budget: Budget,
    charged: usize,
    nodes: usize,
    key_budget: bool,
    output_budget: std::cell::Cell<bool>,
    dirty_full: bool,
    dirty_budget: bool,
    events: u64,
    bytes_delta: i64,
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
    transport_loss: bool,
    overflow_start: u64,
    overflow_end: u64,
    seen: HashSet<(usize, Vec<u8>, (u64, u64))>,
}
impl Bucket {
    fn release(&mut self, bytes: usize) {
        self.charged -= bytes;
        self.budget.release(bytes);
    }
    fn release_nodes(&mut self, bytes: usize) {
        self.nodes -= bytes;
        self.budget.release_nodes(bytes);
    }
    fn compact(&mut self, index: usize, limit: usize) {
        // Rank references, then move retained rows: no duplicate pathname payload.
        let mut rows: Vec<_> = self.maps[index].iter().collect();
        rows.sort_unstable_by(|(_, a), (_, b)| {
            b.bytes_delta.cmp(&a.bytes_delta).then(a.key.cmp(&b.key))
        });
        let keep: HashSet<_> = rows
            .into_iter()
            .take(limit)
            .map(|(k, _)| k.as_slice())
            .collect();
        let old_rows = self.maps[index].len();
        let old_units = self.units[index].len();
        let removed: usize = self.maps[index]
            .iter()
            .filter(|(k, _)| !keep.contains(k.as_slice()))
            .map(|(k, r)| {
                k.capacity() + r.key.capacity() + r.top_unit.as_ref().map_or(0, String::capacity)
            })
            .sum();
        let keep: HashSet<_> = keep.into_iter().map(|k| k as *const [u8]).collect();
        self.maps[index].retain(|k, _| keep.contains(&(k.as_slice() as *const [u8])));
        let mut removed = removed;
        self.units[index].retain(|k, v| {
            let retain = self.maps[index].contains_key(k);
            if !retain {
                removed += k.capacity() + v.0.keys().map(String::capacity).sum::<usize>();
            }
            retain
        });
        self.release(removed);
        let removed_rows = old_rows - self.maps[index].len();
        let removed_units = old_units - self.units[index].len();
        let nodes = removed_rows * ROW_NODE_BYTES
            + removed_units * UNIT_NODE_BYTES
            + if old_rows > 0 && self.maps[index].is_empty() {
                ROW_MAP_BYTES
            } else {
                0
            }
            + if old_units > 0 && self.units[index].is_empty() {
                UNIT_MAP_BYTES
            } else {
                0
            };
        self.release_nodes(nodes);
    }
}
impl Drop for Bucket {
    fn drop(&mut self) {
        self.maps = std::array::from_fn(|_| BTreeMap::new());
        self.units = std::array::from_fn(|_| BTreeMap::new());
        self.seen = HashSet::new();
        self.budget.release(self.charged);
        self.budget.release_nodes(self.nodes);
    }
}
pub(super) struct Rings {
    budget: Budget,
    root: Vec<u8>,
    buckets: VecDeque<Bucket>,
    pub dropped: u64,
    overflow_total: u64,
    next_minute: Option<i64>,
    rollback_until: Option<i64>,
    last_emitted_end: Option<u64>,
    rows_drained_until: Option<i64>,
}
impl Drop for Rings {
    fn drop(&mut self) {
        let bytes = self.root.capacity();
        self.buckets.clear();
        self.root = Vec::new();
        self.budget.release(bytes);
    }
}

fn ranked(map: &BTreeMap<Vec<u8>, HotEntry>, limit: usize) -> Vec<HotEntry> {
    if limit == 0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    if rows.try_reserve_exact(map.len()).is_err() {
        return Vec::new();
    }
    rows.extend(map.values());
    rows.sort_unstable_by(|a, b| b.bytes_delta.cmp(&a.bytes_delta).then(a.key.cmp(&b.key)));
    let mut result = Vec::new();
    if result.try_reserve_exact(rows.len().min(limit)).is_err() {
        return result;
    }
    for r in rows.into_iter().take(limit) {
        let Some(key) = copy(&r.key) else {
            break;
        };
        let top_unit = match r.top_unit.as_deref() {
            Some(unit) => match super::budget::copy_string(unit) {
                Some(unit) => Some(unit),
                None => break,
            },
            None => None,
        };
        result.push(HotEntry {
            key,
            events: r.events,
            files: r.files,
            bytes_delta: r.bytes_delta,
            top_unit,
            lossy: r.lossy,
        });
    }
    result
}

impl Rings {
    pub fn new() -> Self {
        Self {
            budget: Budget::new(),
            root: Vec::new(),
            buckets: VecDeque::new(),
            dropped: 0,
            overflow_total: 0,
            next_minute: None,
            rollback_until: None,
            last_emitted_end: None,
            rows_drained_until: None,
        }
    }
    pub fn budget(&self) -> &Budget {
        &self.budget
    }
    pub fn configure(&mut self, budget: Budget, root: &[u8]) -> std::io::Result<()> {
        let bytes = copy(root).ok_or_else(|| std::io::Error::from(rustix::io::Errno::NOMEM))?;
        if !budget.reserve_root(bytes.capacity()) {
            return Err(rustix::io::Errno::NOMEM.into());
        }
        self.buckets.clear();
        let old = std::mem::take(&mut self.root);
        let old_bytes = old.capacity();
        drop(old);
        self.budget.release(old_bytes);
        self.budget = budget;
        self.root = bytes;
        Ok(())
    }
    pub fn clear_retained(&mut self) {
        self.buckets.clear();
        self.next_minute = None;
        self.rollback_until = None;
        self.last_emitted_end = None;
        self.rows_drained_until = None;
    }
    pub fn key_memory_loss(&mut self) {
        if let Some(b) = self.buckets.back_mut() {
            b.key_budget = true;
            b.truncated = true;
            b.lossy = true;
        }
    }
    pub fn dirty_loss(&mut self, now: i64, memory: bool) {
        self.advance(now);
        if let Some(b) = self.buckets.back_mut() {
            b.lossy = true;
            b.dirty_budget |= memory;
            b.dirty_full |= !memory;
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
            let seen_bytes: usize = last.seen.iter().map(|(_, k, _)| k.capacity()).sum();
            let seen_nodes = last.seen.len() * SEEN_NODE_BYTES;
            last.seen.clear();
            last.seen.shrink_to_fit();
            last.release(seen_bytes);
            last.release_nodes(seen_nodes);
            last.overflow_end = self.overflow_total;
            for index in 0..3 {
                if last.maps[index].len() > COMPLETED_CAP {
                    last.lossy = true;
                    last.truncated = true;
                    last.completed_capped = true;
                    self.dropped += (last.maps[index].len() - COMPLETED_CAP) as u64;
                    last.compact(index, COMPLETED_CAP);
                }
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
                budget: self.budget.clone(),
                charged: 0,
                nodes: 0,
                key_budget: false,
                output_budget: std::cell::Cell::new(false),
                dirty_full: false,
                dirty_budget: false,
                events: 0,
                bytes_delta: 0,
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
                transport_loss: self.rollback_until.is_some_and(|end| at <= end),
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
                bucket.transport_loss = true;
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
            b.transport_loss = true;
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
        bucket.events = bucket.events.saturating_add(1);
        bucket.bytes_delta = bucket.bytes_delta.saturating_add(delta);
        for (index, key) in keys.into_iter().enumerate() {
            if !bucket.maps[index].contains_key(&key) && bucket.maps[index].len() >= CURRENT_CAP {
                self.dropped += (bucket.maps[index].len() / 2) as u64;
                bucket.compact(index, CURRENT_CAP / 2);
                bucket.lossy = true;
                bucket.truncated = true;
                bucket.current_capped = true;
            }
            let new_row = !bucket.maps[index].contains_key(&key);
            let new_unit = unit.is_some() && !bucket.units[index].contains_key(&key);
            let seen_scratch = Reservation::new(&bucket.budget, key.len());
            let seen_key = seen_scratch
                .as_ref()
                .and_then(|_| copy(&key))
                .map(|k| (index, k, identity));
            let new_seen = seen_key.as_ref().is_some_and(|k| !bucket.seen.contains(k))
                && bucket.seen.len() < 200_000;
            let copies = 2 * usize::from(new_row) + usize::from(new_unit) + usize::from(new_seen);
            let unit_reserve = unit.as_ref().map_or(0, |u| u.len() * 2);
            let needed = key
                .len()
                .saturating_mul(copies)
                .saturating_add(unit_reserve);
            let nodes = usize::from(new_row)
                * (ROW_NODE_BYTES
                    + if bucket.maps[index].is_empty() {
                        ROW_MAP_BYTES
                    } else {
                        0
                    })
                + usize::from(new_unit)
                    * (UNIT_NODE_BYTES
                        + if bucket.units[index].is_empty() {
                            UNIT_MAP_BYTES
                        } else {
                            0
                        })
                + usize::from(new_seen) * SEEN_NODE_BYTES;
            let admitted_payload = bucket.budget.reserve(needed);
            let admitted = admitted_payload && bucket.budget.reserve_nodes(nodes, false);
            if admitted_payload && !admitted {
                bucket.budget.release(needed);
            }
            if admitted {
                bucket.nodes += nodes;
            }
            let key = if admitted {
                bucket.charged += needed;
                key
            } else {
                bucket.lossy = true;
                bucket.truncated = true;
                bucket.key_budget = true;
                // Directory scope already counts this event exactly. Dropping
                // optional unit/command attribution must not count it twice there.
                if index != 0 {
                    continue;
                }
                self.root.clone()
            };
            let mut actual = 0;
            let unique = if admitted && new_seen {
                if bucket.seen.try_reserve(1).is_err() {
                    bucket.key_budget = true;
                    bucket.lossy = true;
                    bucket.truncated = true;
                    false
                } else if let Some(k) = copy(&key) {
                    actual += k.capacity();
                    bucket.seen.insert((index, k, identity))
                } else {
                    false
                }
            } else {
                if admitted && bucket.seen.len() >= 200_000 {
                    bucket.lossy = true;
                    bucket.truncated = true;
                    bucket.distinct_capped = true;
                }
                false
            };
            if !bucket.maps[index].contains_key(&key) {
                let pair = copy(&key).zip(copy(&key));
                if let Some((map_key, row_key)) = pair {
                    let bytes = map_key.capacity() + row_key.capacity();
                    if !admitted {
                        let nodes = ROW_NODE_BYTES
                            + if bucket.maps[index].is_empty() {
                                ROW_MAP_BYTES
                            } else {
                                0
                            };
                        if !bucket.budget.reserve_nodes(nodes, true) {
                            continue;
                        }
                        if !bucket.budget.reserve_root(bytes) {
                            bucket.budget.release_nodes(nodes);
                            continue;
                        }
                        bucket.nodes += nodes;
                    }
                    if !admitted {
                        bucket.charged += bytes;
                    } else {
                        actual += bytes;
                    }
                    bucket.maps[index].insert(
                        map_key,
                        HotEntry {
                            key: row_key,
                            events: 0,
                            files: 0,
                            bytes_delta: 0,
                            top_unit: None,
                            lossy: false,
                        },
                    );
                }
            }
            if admitted {
                if let Some(unit) = unit.as_deref() {
                    if !bucket.units[index].contains_key(&key) {
                        if let Some(k) = copy(&key) {
                            actual += k.capacity();
                            bucket.units[index].insert(k, UnitCounts::default());
                        }
                    }
                    if let Some(counts) = bucket.units[index].get_mut(&key) {
                        let before: usize = counts.0.keys().map(String::capacity).sum();
                        if !counts.add(unit, delta.unsigned_abs()) {
                            bucket.key_budget = true;
                            bucket.truncated = true;
                            bucket.lossy = true;
                        }
                        let after: usize = counts.0.keys().map(String::capacity).sum();
                        actual += after.saturating_sub(before);
                        if before > after {
                            bucket.charged -= before - after;
                            bucket.budget.release(before - after);
                        }
                    }
                }
                if let Some(row) = bucket.maps[index].get_mut(&key) {
                    let previous = row.top_unit.as_ref().map_or(0, String::capacity);
                    let top = bucket.units[index].get(&key).and_then(UnitCounts::top);
                    if row.top_unit.as_ref() != top {
                        row.top_unit = top.and_then(|v| super::budget::copy_string(v));
                        if top.is_some() && row.top_unit.is_none() {
                            bucket.key_budget = true;
                            bucket.truncated = true;
                            bucket.lossy = true;
                        }
                    }
                    let next = row.top_unit.as_ref().map_or(0, String::capacity);
                    actual += next.saturating_sub(previous);
                    if previous > next {
                        bucket.charged -= previous - next;
                        bucket.budget.release(previous - next);
                    }
                }
                bucket.release(needed.saturating_sub(actual));
            }
            if let Some(row) = bucket.maps[index].get_mut(&key) {
                row.events = row.events.saturating_add(1);
                row.files = row.files.saturating_add(u64::from(unique));
                row.bytes_delta = row.bytes_delta.saturating_add(delta);
            }
        }
    }
    pub fn hot(&self, now: i64, window: u32, scope: HotScope, limit: usize) -> Vec<HotEntry> {
        let index = match scope {
            HotScope::Dir => 0,
            HotScope::Unit => 1,
            HotScope::Comm => 2,
        };
        let cutoff = now.div_euclid(60) * 60 - i64::from(window.clamp(1, 60) - 1) * 60;
        let selected = || {
            self.buckets
                .iter()
                .filter(|b| b.minute >= cutoff && b.minute <= now.div_euclid(60) * 60)
        };
        let scratch = selected()
            .map(|b| {
                b.maps[index]
                    .values()
                    .map(|r| {
                        4 * r.key.capacity() + 2 * r.top_unit.as_ref().map_or(0, String::capacity)
                    })
                    .sum::<usize>()
                    + b.units[index]
                        .iter()
                        .map(|(k, v)| {
                            k.capacity() + v.0.keys().map(String::capacity).sum::<usize>()
                        })
                        .sum::<usize>()
            })
            .sum();
        let payload = Reservation::new(&self.budget, scratch);
        let node_bytes: usize = selected()
            .map(|b| {
                b.maps[index].len()
                    * (ROW_NODE_BYTES + UNIT_MAP_BYTES + 2 * std::mem::size_of::<HotEntry>())
                    + b.units[index]
                        .values()
                        .map(|v| v.0.len() * 128)
                        .sum::<usize>()
            })
            .sum();
        let nodes = payload
            .as_ref()
            .and_then(|_| Reservation::nodes(&self.budget, node_bytes));
        let Some(_scratch) = payload.zip(nodes) else {
            if limit == 0 {
                return Vec::new();
            }
            if selected().any(|b| b.events > 0) {
                if let Some(b) = self
                    .buckets
                    .iter()
                    .find(|b| b.minute == now.div_euclid(60) * 60)
                {
                    b.output_budget.set(true);
                }
            }
            let Some(_root) = Reservation::new(&self.budget, self.root.len()) else {
                return Vec::new();
            };
            let Some(_nodes) = Reservation::emergency_nodes(
                &self.budget,
                2 * std::mem::size_of::<HotEntry>() + 128,
            ) else {
                return Vec::new();
            };
            let Some(key) = copy(&self.root) else {
                return Vec::new();
            };
            let mut row = HotEntry {
                key,
                events: 0,
                files: 0,
                bytes_delta: 0,
                top_unit: None,
                lossy: true,
            };
            for b in selected() {
                row.events = row.events.saturating_add(b.events);
                row.bytes_delta = row.bytes_delta.saturating_add(b.bytes_delta);
            }
            return if row.events == 0 {
                Vec::new()
            } else {
                vec![row]
            };
        };
        let mut sum: BTreeMap<Vec<u8>, HotEntry> = BTreeMap::new();
        let mut units: BTreeMap<Vec<u8>, BTreeMap<String, u64>> = BTreeMap::new();
        let lossy = self.buckets.iter().any(|b| {
            b.minute >= cutoff
                && b.minute <= now.div_euclid(60) * 60
                && (b.lossy || b.output_budget.get())
        });
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
        ranked(&sum, limit)
    }
    pub fn detail(&self, now: i64) -> String {
        let cutoff = now.div_euclid(60) * 60 - 14 * 60;
        // One latest occurrence per cause bounds status text independently of
        // the number of affected minutes and configured watchers.
        let mut reported = [false; 12];
        let mut reasons = Vec::with_capacity(reported.len() + 1);
        for b in self
            .buckets
            .iter()
            .rev()
            .filter(|b| b.minute >= cutoff && b.minute <= now)
        {
            let causes = [
                (
                    b.output_budget.get(),
                    "truncated: output over the memory budget",
                ),
                (b.key_budget, "truncated: key bytes over the memory budget"),
                (b.dirty_full, "lost events: dirty set full"),
                (
                    b.dirty_budget,
                    "lost events: dirty set over its memory budget",
                ),
                (b.distinct_capped, "truncated: distinct-file cap reached"),
                (b.current_capped, "truncated: current-key cap reached"),
                (b.completed_capped, "truncated: completed-key cap reached"),
                (b.drain_truncated, "truncated: requested-output cap reached"),
                (b.read_error, "lost events: read error"),
                (b.descriptor_pressure, "lost events: descriptor pressure"),
                (
                    b.overflow_end > b.overflow_start,
                    "lost events: queue overflow",
                ),
                (
                    b.transport_loss
                        && !b.read_error
                        && !b.descriptor_pressure
                        && b.overflow_end == b.overflow_start,
                    "lost events: transport recovery or history gap",
                ),
            ];
            for (index, (occurred, reason)) in causes.into_iter().enumerate() {
                if !occurred || reported[index] {
                    continue;
                }
                reported[index] = true;
                let minute = format!(
                    "{:02}:{:02} UTC",
                    b.minute.div_euclid(3600).rem_euclid(24),
                    b.minute.div_euclid(60).rem_euclid(60)
                );
                let qualifier = if index == 4 && !b.transport_loss {
                    " (events complete, per-file counts capped)"
                } else if index == 4 {
                    " (per-file counts capped)"
                } else {
                    ""
                };
                reasons.push(format!("{reason} in the minute {minute}{qualifier}"));
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
                    && (b.lossy || b.output_budget.get())
            })
            .count() as u32
    }
    fn output_bytes(b: &Bucket, limit: usize, mount: &str, dirs: bool) -> usize {
        // Admission must precede ranking scratch allocation. The largest row
        // cost times the requested count conservatively covers any ranking.
        b.maps
            .iter()
            .enumerate()
            .map(|(index, map)| {
                let largest = map
                    .values()
                    .map(|row| {
                        let payload = row.key.len() + row.top_unit.as_ref().map_or(0, String::len);
                        usize::from(!b.drained) * (payload + mount.len() + [3, 4, 4][index])
                            + usize::from(dirs && index == 0) * payload
                    })
                    .max()
                    .unwrap_or(0);
                largest.saturating_mul(map.len().min(limit))
            })
            .sum()
    }
    fn output_reservation(
        b: &mut Bucket,
        top_k: usize,
        mount: &str,
        dirs: bool,
    ) -> (usize, Option<(Reservation, Reservation)>) {
        let limit = top_k.min(COMPLETED_CAP);
        let payload = Reservation::normal(&b.budget, Self::output_bytes(b, limit, mount, dirs));
        let count: usize = b.maps.iter().map(|m| m.len().min(limit)).sum();
        let ranking_bytes =
            b.maps.iter().map(BTreeMap::len).max().unwrap_or(0) * std::mem::size_of::<&HotEntry>();
        let nodes = payload.as_ref().and_then(|_| {
            Reservation::nodes(
                &b.budget,
                ranking_bytes
                    + count
                        * (5 * std::mem::size_of::<HotEntry>()
                            + 2 * std::mem::size_of::<HotMinute>()),
            )
        });
        if let Some(lease) = payload.zip(nodes) {
            return (limit, Some(lease));
        }
        b.output_budget.set(true);
        b.truncated = true;
        b.lossy = true;
        let payload = Reservation::new(&b.budget, 3 * mount.len() + 3);
        let nodes = Reservation::emergency_nodes(
            &b.budget,
            4 * std::mem::size_of::<HotEntry>() + 4 * std::mem::size_of::<HotMinute>() + 256,
        );
        (usize::MAX, payload.zip(nodes))
    }
    fn aggregate(b: &Bucket, root: &[u8]) -> HotEntry {
        let mut row = HotEntry {
            key: root.to_vec(),
            events: 0,
            files: 0,
            bytes_delta: 0,
            top_unit: None,
            lossy: true,
        };
        row.events = b.events;
        row.bytes_delta = b.bytes_delta;
        row
    }

    fn persisted(b: &mut Bucket, mount: &str, top_k: usize, root: &[u8]) -> Vec<HotMinute> {
        if b.drained {
            return Vec::new();
        }
        b.drained = true;
        if top_k == usize::MAX {
            let row = Self::aggregate(b, root);
            return vec![HotMinute {
                mount: mount.into(),
                scope: "dir".into(),
                key: row.key,
                ts: b.minute,
                events: row.events,
                files: 0,
                bytes_delta: row.bytes_delta,
                lossy: true,
            }];
        }
        let limit = top_k.min(COMPLETED_CAP);
        b.drain_truncated |= b.maps.iter().any(|map| map.len() > limit);
        let mut result = Vec::new();
        for (scope, map) in ["dir", "unit", "comm"].into_iter().zip(&b.maps) {
            result.extend(ranked(map, limit).into_iter().map(|r| HotMinute {
                mount: mount.into(),
                scope: scope.into(),
                key: r.key,
                ts: b.minute,
                events: r.events,
                files: r.files,
                bytes_delta: r.bytes_delta,
                lossy: b.lossy || b.output_budget.get(),
            }));
        }
        result
    }
    pub fn drain(&mut self, now: i64, mount: &str, top_k: usize) -> Vec<HotMinute> {
        self.advance(now);
        let mut result = Vec::new();
        let mut reservations = Vec::new();
        for b in &mut self.buckets {
            if !b.drained && b.minute < now.div_euclid(60) * 60 {
                let (limit, lease) = Self::output_reservation(b, top_k, mount, false);
                reservations.push(lease);
                result.extend(Self::persisted(b, mount, limit, &self.root));
                self.rows_drained_until = self.rows_drained_until.max(Some(b.minute));
            }
        }
        result
    }
    pub fn drain_minutes(&mut self, now: i64, mount: &str, top_k: usize) -> Vec<CompletedMinute> {
        self.advance(now);
        let mut result = Vec::new();
        let mut reservations = Vec::new();
        let Some(mut next) = self.next_minute else {
            return result;
        };
        for b in &mut self.buckets {
            if b.minute < next || b.minute >= now.div_euclid(60) * 60 {
                continue;
            }
            let (limit, lease) = Self::output_reservation(b, top_k, mount, true);
            reservations.push(lease);
            let persisted = Self::persisted(b, mount, limit, &self.root);
            self.rows_drained_until = self.rows_drained_until.max(Some(b.minute));
            let truncated = b.truncated
                || b.output_budget.get()
                || b.drain_truncated
                || b.maps[0].len() > top_k.min(COMPLETED_CAP);
            let lossy = b.lossy || truncated || b.minute != next;
            let dirs = if limit == usize::MAX {
                vec![Self::aggregate(b, &self.root)]
            } else {
                ranked(&b.maps[0], limit)
            }
            .into_iter()
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
    fn payload(r: &Rings) -> usize {
        r.root.capacity()
            + r.buckets
                .iter()
                .map(|b| {
                    b.maps
                        .iter()
                        .flat_map(|m| m.iter())
                        .map(|(k, v)| {
                            k.capacity()
                                + v.key.capacity()
                                + v.top_unit.as_ref().map_or(0, String::capacity)
                        })
                        .sum::<usize>()
                        + b.units
                            .iter()
                            .flat_map(|m| m.iter())
                            .map(|(k, v)| {
                                k.capacity() + v.0.keys().map(String::capacity).sum::<usize>()
                            })
                            .sum::<usize>()
                        + b.seen.iter().map(|(_, k, _)| k.capacity()).sum::<usize>()
                })
                .sum::<usize>()
    }
    #[test]
    fn emergency_root_headroom_preserves_sixty_undrained_minutes() {
        let mut r = Rings::new();
        let mut root = vec![b'r'; super::super::WATCH_ROOT_BYTES];
        root[0] = b'/';
        r.configure(Budget::new(), &root).unwrap();
        for n in 0..300 {
            let mut key = format!("/{n}/").into_bytes();
            key.resize(256 * 1024, b'x');
            r.add(0, [key, b"u".to_vec(), b"c".to_vec()], 7, None, (1, n));
        }
        for minute in 1..60 {
            let key = vec![b'a' + (minute % 26) as u8; 4000];
            r.add(
                minute * 60,
                [key, b"u".to_vec(), b"c".to_vec()],
                7,
                None,
                (2, minute as u64),
            );
            let rows = r.hot(minute * 60, 1, HotScope::Dir, usize::MAX);
            assert_eq!(rows.iter().map(|row| row.events).sum::<u64>(), 1);
            assert_eq!(rows.iter().map(|row| row.bytes_delta).sum::<i64>(), 7);
            assert_eq!(r.budget.used(), payload(&r));
            assert!(r.budget.committed() <= super::super::WATCH_MEMORY_BYTES);
        }
        let rows = r.hot(59 * 60, 60, HotScope::Dir, usize::MAX);
        assert_eq!(rows.iter().map(|row| row.events).sum::<u64>(), 359);
        assert_eq!(rows.iter().map(|row| row.bytes_delta).sum::<i64>(), 359 * 7);
    }
    #[test]
    fn emergency_headroom_covers_full_history_and_aggregate_export() {
        let mut r = Rings::new();
        let root = vec![0xff; super::super::WATCH_ROOT_BYTES];
        r.configure(Budget::new(), &root).unwrap();
        let available = super::super::WATCH_MEMORY_BYTES - 3 * 1024 * 1024 - r.budget.committed();
        let held = Reservation::nodes(&r.budget, available).unwrap();
        for minute in 0..60 {
            r.add(
                minute * 60,
                keys(minute as usize),
                7,
                None,
                (1, minute as u64),
            );
        }
        let mount = String::from_utf8_lossy(&root);
        let rows = r.drain(60 * 60, &mount, 1000);
        // The 60th boundary expires the oldest bucket, leaving 59 full minutes.
        assert_eq!(rows.len(), 59);
        assert!(rows
            .iter()
            .all(|row| row.key == root && row.events == 1 && row.bytes_delta == 7));
        assert!(r.budget.committed() <= super::super::WATCH_MEMORY_BYTES);
        drop(held);
    }
    #[test]
    fn output_admission_bounds_unequal_rows_before_ranking() {
        let mut r = Rings::new();
        r.configure(Budget::new(), b"/mount").unwrap();
        for n in 0..10 {
            r.add(
                0,
                [vec![b'x'; n * 100 + 1], b"unit".to_vec(), b"comm".to_vec()],
                n as i64,
                Some("writer".into()),
                (1, n as u64),
            );
        }
        let bucket = r.buckets.back().unwrap();
        for limit in [0, 1, 5, 1000] {
            let admitted = Rings::output_bytes(bucket, limit, "/mount", true);
            let persisted = bucket
                .maps
                .iter()
                .enumerate()
                .map(|(index, map)| {
                    ranked(map, limit)
                        .iter()
                        .map(|row| {
                            row.key.len()
                                + row.top_unit.as_ref().map_or(0, String::len)
                                + "/mount".len()
                                + [3, 4, 4][index]
                        })
                        .sum::<usize>()
                })
                .sum::<usize>();
            let dirs = ranked(&bucket.maps[0], limit)
                .iter()
                .map(|row| row.key.len() + row.top_unit.as_ref().map_or(0, String::len))
                .sum::<usize>();
            assert!(admitted >= persisted + dirs);
        }
    }
    #[test]
    fn byte_budget_falls_back_to_root_and_retains_completed_maps() {
        let mut r = Rings::new();
        let budget = Budget::new();
        r.configure(budget.clone(), b"/mount").unwrap();
        for minute in 0..70 {
            for n in 0..8 {
                let mut key = vec![b'x'; 150_000];
                key[0] = b'/';
                key[1] = b'a' + n;
                r.add(
                    minute * 60,
                    [key, b"unit".to_vec(), b"comm".to_vec()],
                    7,
                    Some("writer".into()),
                    (1, n as u64),
                );
            }
            let rows = r.hot(minute * 60, 1, HotScope::Dir, 20);
            assert_eq!(rows.iter().map(|r| r.events).sum::<u64>(), 8);
            assert_eq!(rows.iter().map(|r| r.bytes_delta).sum::<i64>(), 56);
            if minute > 30 {
                assert!(rows.iter().any(|r| r.key == b"/mount"));
            }

            assert_eq!(budget.used(), payload(&r)); // exact, zero tolerance
            assert!(budget.committed() <= super::super::WATCH_MEMORY_BYTES);
            r.drain_minutes((minute + 1) * 60, "/mount", 1000);
            assert_eq!(
                r.hot((minute + 1) * 60, 2, HotScope::Dir, 20)
                    .iter()
                    .map(|r| r.events)
                    .sum::<u64>(),
                8
            );
            assert_eq!(budget.used(), payload(&r));
        }
    }
    #[test]
    fn every_scope_retains_history_after_both_delivery_paths() {
        for legacy in [false, true] {
            let mut r = Rings::new();
            for minute in 0..60 {
                r.add(
                    minute * 60,
                    [b"/dir".to_vec(), b"unit".to_vec(), b"comm".to_vec()],
                    7,
                    None,
                    (1, minute as u64),
                );
                let at = (minute + 1) * 60;
                r.advance(at);
                for window in [5, 15, 60] {
                    for scope in [HotScope::Dir, HotScope::Unit, HotScope::Comm] {
                        let before = r.hot(at, window, scope, 1000);
                        if legacy {
                            r.drain(at, "/", 1000);
                        } else {
                            r.drain_minutes(at, "/", 1000);
                        }
                        assert_eq!(before, r.hot(at, window, scope, 1000));
                    }
                }
                let remaining = r.drain(at, "/", 1000);
                assert_eq!(remaining.len(), if !legacy && minute == 0 { 3 } else { 0 });
                assert!(r
                    .drain_minutes(at, "/", 1000)
                    .iter()
                    .all(|m| m.persisted.is_empty()));
            }
        }
    }
    #[test]
    fn exhausted_output_keeps_exact_aggregate_and_expiring_reason() {
        for legacy in [false, true] {
            let mut r = Rings::new();
            r.configure(Budget::new(), b"/mount").unwrap();
            r.advance(0);
            r.add(
                60,
                [b"/a".to_vec(), b"u".to_vec(), b"c".to_vec()],
                71,
                None,
                (1, 1),
            );
            r.add(
                61,
                [b"/b".to_vec(), b"u".to_vec(), b"c".to_vec()],
                -9,
                None,
                (1, 2),
            );
            r.advance(120);
            let available =
                super::super::WATCH_MEMORY_BYTES - 3 * 1024 * 1024 - r.budget.committed();
            let held = Reservation::nodes(&r.budget, available).unwrap();
            let hot = r.hot(120, 5, HotScope::Dir, 1000);
            assert_eq!((hot[0].events, hot[0].bytes_delta), (2, 62));
            assert!(r.detail(120).contains("output over the memory budget"));
            assert_eq!(r.lossy(120), 1);
            if legacy {
                let rows = r.drain(120, "/mount", 1000);
                assert_eq!(rows.len(), 1);
                assert_eq!((rows[0].events, rows[0].bytes_delta), (2, 62));
                assert_eq!(rows[0].key, b"/mount");
                assert!(r.drain(120, "/mount", 1000).is_empty());
            } else {
                let rows = r.drain_minutes(120, "/mount", 1000);
                assert_eq!(rows[0].persisted.len(), 1);
                assert_eq!(
                    (rows[0].dirs[0].events, rows[0].dirs[0].bytes_delta),
                    (2, 62)
                );
                assert!(rows[0].truncated && rows[0].lossy);
                assert!(r.drain_minutes(120, "/mount", 1000).is_empty());
            }
            drop(held);
            assert!(r.detail(17 * 60).is_empty());
            assert_eq!(r.lossy(17 * 60), 0);
        }
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
        assert!(!detail.contains("events complete"));
        assert!(!detail.contains("(timestamp "), "{detail}");
        for reason in [
            "truncated: distinct-file cap",
            "lost events: queue overflow",
            "lost events: read error",
            "lost events: descriptor pressure",
            "00:01 UTC",
        ] {
            assert!(detail.contains(reason), "{detail}");
        }
    }
    #[test]
    fn diagnostics_retain_only_latest_occurrence_per_cause() {
        let mut r = Rings::new();
        for at in 0..15 {
            r.read_failure(at * 60, true);
        }
        let detail = r.detail(14 * 60);
        assert_eq!(detail.matches("descriptor pressure").count(), 1);
        assert!(detail.contains("00:14 UTC"));
        assert!(!detail.contains("(timestamp "), "{detail}");
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
