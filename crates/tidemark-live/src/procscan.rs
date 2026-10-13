//! Owned by: W5-B
//! Metadata-only phantom aggregation and bounded open-writer history.

use std::{
    collections::{BTreeMap, BinaryHeap, VecDeque},
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use tidemark_core::{
    live::{Attribution, OpenWriter, PhantomHolder, PhantomSample, PhantomSummary, WriterSample},
    path::RawPath,
    scan::DevId,
};
use tidemark_fs::{
    attribution::AttributionCache,
    procfs::{self, OpenFile},
};

const MAX_WRITERS: usize = 10_000;
const HISTORY: usize = 60;
const MAX_FDS: usize = 1_048_576;
// Each persistence/report row set is at most 3.75 MiB, including vector slack.
// Temporary trees are separately bounded; history tuples are capped by MAX_WRITERS/HISTORY.
const MAX_PASS_BYTES: usize = 15 * 1024 * 1024 / 4;
const MAX_SCRATCH_BYTES: usize = 16 * 1024 * 1024;
const ROW_OVERHEAD: usize = 512;

#[derive(Debug, thiserror::Error)]
#[error("procscan copy byte budget exceeded: {count} descriptors, {bytes} bytes charged")]
struct CopyBudgetExceeded {
    count: usize,
    bytes: usize,
}
fn charge(total: &mut usize, bytes: usize, count: usize, limit: usize) -> io::Result<()> {
    let next = total.saturating_add(bytes);
    if next > limit {
        return Err(io::Error::new(
            io::ErrorKind::OutOfMemory,
            CopyBudgetExceeded {
                count,
                bytes: *total,
            },
        ));
    }
    *total = next;
    Ok(())
}
fn row_charge(path: usize, comm: usize, unit: usize, mount: usize) -> usize {
    // Output vectors reserve their full admitted length, with no geometric slack.
    std::mem::size_of::<OpenWriter>()
        .max(std::mem::size_of::<WriterSample>())
        .max(std::mem::size_of::<PhantomSample>())
        .max(std::mem::size_of::<PhantomHolder>())
        + path
        + comm
        + unit
        + mount
}

/// Devices and exclusions for a single shared procfs pass.
pub struct ProcScannerOptions {
    /// Watched mount paths and kernel devices.
    pub mounts: Vec<(RawPath, DevId)>,
    /// Read fd flags and track writable descriptors.
    pub open_writers: bool,
    /// Component-aware writer exclusions.
    pub exclude_prefixes: Vec<RawPath>,
}
/// Diagnostics for a completed pass.
pub struct ProcScanResult {
    /// Injected Unix timestamp.
    pub at: i64,
    /// Regular-file descriptors returned by the source, before deduplication.
    pub fds_seen: u64,
    /// Whether writer candidates exceeded the history cap.
    pub writers_full: bool,
    /// Histories evicted for admission this pass.
    pub writer_evictions: u64,
    /// Elapsed scan and aggregation time.
    pub duration: Duration,
}
trait Source: Send {
    fn scan(&mut self, devs: &[DevId], flags: bool) -> io::Result<Vec<OpenFile>>;
    fn attribution(&self, pid: u32) -> Attribution;
}
struct ProcSource(Arc<AttributionCache>);
impl Source for ProcSource {
    fn scan(&mut self, devs: &[DevId], flags: bool) -> io::Result<Vec<OpenFile>> {
        procfs::scan_open_files(devs, flags)
    }
    fn attribution(&self, pid: u32) -> Attribution {
        self.0.lookup(pid)
    }
}
#[derive(Clone)]
struct Tracked {
    row: WriterSample,
    samples: VecDeque<(i64, u64, u64)>,
    missed: u8,
}
fn delta(a: u64, b: u64) -> i64 {
    (i128::from(a) - i128::from(b)).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}
fn growth(t: &Tracked) -> i64 {
    t.samples.front().map_or(0, |s| delta(t.row.disk, s.1))
}
/// One-pass phantom detector with at most 10,000 writer histories of 60 samples.
/// Procfs does no disk I/O, so Gate charging and O_NOATIME do not apply.
/// Pending samples hold only the latest pass; callers should drain after each pass.
pub struct ProcScanner {
    cfg: ProcScannerOptions,
    source: Box<dyn Source>,
    phantom: Vec<PhantomSummary>,
    tracked: BTreeMap<(u64, u64), Tracked>,
    pending: (Vec<PhantomSample>, Vec<WriterSample>),
    now: Option<i64>,
    admission_cursor: Option<(u64, u64)>,
    rotate: bool,
}
/// Immutable completed process report with bounded retained writer histories.
/// Sharing this value never exposes the mutable procfs collector to readers.
pub struct ProcReport {
    mounts: Vec<(RawPath, DevId)>,
    phantom: Vec<PhantomSummary>,
    tracked: BTreeMap<(u64, u64), Tracked>,
    now: i64,
}
impl ProcReport {
    /// Latest completed phantom totals, including measured empty mounts.
    pub fn phantom(&self, mount: Option<&RawPath>) -> Vec<PhantomSummary> {
        phantom_rows(&self.phantom, &self.mounts, mount)
    }
    /// Compute growth over any positive window using the retained timestamp history.
    pub fn writers(
        &self,
        mount: Option<&RawPath>,
        window: Duration,
        limit: usize,
    ) -> Vec<OpenWriter> {
        writer_rows(
            &self.tracked,
            &self.mounts,
            Some(self.now),
            mount,
            window,
            limit,
        )
    }
}
impl ProcScanner {
    /// Construct a real procfs scanner using the shared attribution cache.
    pub fn new(cfg: ProcScannerOptions, attribution: Arc<AttributionCache>) -> Self {
        Self {
            cfg,
            source: Box::new(ProcSource(attribution)),
            phantom: Vec::new(),
            tracked: BTreeMap::new(),
            pending: (Vec::new(), Vec::new()),
            now: None,
            admission_cursor: None,
            rotate: false,
        }
    }
    /// Scan once. Failed passes preserve the last successful state.
    /// Timestamps must not decrease so that window baselines remain ordered.
    pub fn scan_once(&mut self, now: i64) -> io::Result<ProcScanResult> {
        if self.now.is_some_and(|last| now < last) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "scan timestamps must not decrease",
            ));
        }
        let started = Instant::now();
        let devs: Vec<_> = self.cfg.mounts.iter().map(|m| m.1).collect();
        let mut files = self.source.scan(&devs, self.cfg.open_writers)?;
        if files.len() > MAX_FDS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "proc descriptor cap exceeded",
            ));
        }
        // Validate injected sources too, before any collector state changes.
        let inventory_bytes = files.iter().fold(
            files
                .capacity()
                .saturating_mul(std::mem::size_of::<OpenFile>()),
            |n, f| n.saturating_add(f.target.capacity()),
        );
        if inventory_bytes > procfs::MAX_INVENTORY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                procfs::InventoryBudgetExceeded {
                    count: files.len(),
                    bytes: inventory_bytes,
                },
            ));
        }
        let fds_seen = files.len() as u64;
        files.sort_unstable_by_key(|a| (a.dev, a.ino, a.pid, a.fd));
        let mut scratch = MAX_WRITERS * std::mem::size_of::<usize>();
        let mut phantom_bytes = 0;
        let mut writer_bytes = 0;
        let mut history_rows = BinaryHeap::with_capacity(MAX_WRITERS);
        for t in self.tracked.values() {
            history_rows.push(std::cmp::Reverse(row_charge(
                t.row.path.capacity(),
                t.row.comm.capacity(),
                t.row.unit.as_ref().map_or(0, String::capacity),
                t.row.mount.capacity(),
            )));
        }
        let mount_bytes = self
            .cfg
            .mounts
            .iter()
            .map(|m| m.0.as_bytes().len().saturating_mul(4))
            .sum::<usize>();
        charge(
            &mut scratch,
            mount_bytes + self.cfg.mounts.len() * ROW_OVERHEAD,
            0,
            MAX_SCRATCH_BYTES,
        )?;
        charge(
            &mut phantom_bytes,
            mount_bytes + self.cfg.mounts.len() * ROW_OVERHEAD,
            0,
            MAX_PASS_BYTES,
        )?;
        let mut attrs = BTreeMap::new();
        let mut holder_counts = vec![0usize; self.cfg.mounts.len()];
        let mut last_holder = None;
        let mut last_writer = None;
        // Admit all copies before changing histories, reports, or pending samples.
        for (count, file) in files.iter().enumerate() {
            let Some(index) = self.cfg.mounts.iter().position(|m| m.1 == file.dev) else {
                continue;
            };
            if !file.target.starts_with(b"/") {
                continue;
            }
            let path = if file.nlink == 0 {
                file.target
                    .strip_suffix(b" (deleted)")
                    .unwrap_or(&file.target)
            } else {
                &file.target
            };
            let writable = self.cfg.open_writers
                && file.flags.is_some_and(|f| matches!(f & 3, 1 | 2))
                && !self
                    .cfg
                    .exclude_prefixes
                    .iter()
                    .any(|p| under(path, p.as_bytes()));
            if file.nlink != 0 && !writable {
                continue;
            }
            if !attrs.contains_key(&file.pid) {
                charge(&mut scratch, ROW_OVERHEAD, count, MAX_SCRATCH_BYTES)?;
                let attr = self.source.attribution(file.pid);
                charge(
                    &mut scratch,
                    attr.comm.capacity() + attr.unit.as_ref().map_or(0, String::capacity),
                    count,
                    MAX_SCRATCH_BYTES,
                )?;
                attrs.insert(file.pid, attr);
            }
            let attr = &attrs[&file.pid];
            let bytes = row_charge(
                path.len(),
                attr.comm.capacity().max(1),
                attr.unit.as_ref().map_or(0, String::capacity),
                self.cfg.mounts[index].0.as_bytes().len() * 4,
            );
            let key = (file.dev, file.ino);
            if file.nlink == 0 && last_holder != Some((key, file.pid)) {
                charge(&mut phantom_bytes, bytes, count, MAX_PASS_BYTES)?;
                holder_counts[index] += 1;
                last_holder = Some((key, file.pid));
            }
            if writable && last_writer != Some(key) {
                charge(&mut writer_bytes, bytes, count, MAX_PASS_BYTES)?;
                charge(&mut scratch, ROW_OVERHEAD + bytes, count, MAX_SCRATCH_BYTES)?;
                // The largest MAX_WRITERS rows bound every rotation/admission choice.
                if history_rows.len() < MAX_WRITERS {
                    history_rows.push(std::cmp::Reverse(bytes));
                } else if let Some(mut smallest) = history_rows.peek_mut() {
                    if bytes > smallest.0 {
                        *smallest = std::cmp::Reverse(bytes);
                    }
                }
                last_writer = Some(key);
            }
        }
        let mut history_bytes = 0;
        for bytes in history_rows {
            charge(&mut history_bytes, bytes.0, files.len(), MAX_PASS_BYTES)?;
        }
        let mut summaries: Vec<_> = self
            .cfg
            .mounts
            .iter()
            .map(|(p, _)| PhantomSummary {
                mount: p.display_lossy(),
                total_bytes: 0,
                files: 0,
                holders: Vec::new(),
            })
            .collect();
        for (summary, count) in summaries.iter_mut().zip(&holder_counts) {
            summary.holders.try_reserve_exact(*count).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    CopyBudgetExceeded {
                        count: files.len(),
                        bytes: phantom_bytes,
                    },
                )
            })?;
        }
        let mut phantom_samples = Vec::new();
        phantom_samples
            .try_reserve_exact(holder_counts.iter().sum())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    CopyBudgetExceeded {
                        count: files.len(),
                        bytes: phantom_bytes,
                    },
                )
            })?;
        let mut previous_phantom = None;
        let mut previous_holder = None;
        let mut seen_writers = BTreeMap::new();
        // Attribution was admitted before aggregation; errors preserve the old report.
        for file in files {
            let Some(index) = self.cfg.mounts.iter().position(|m| m.1 == file.dev) else {
                continue;
            };
            if !file.target.starts_with(b"/") {
                continue;
            }
            let path = if file.nlink == 0 {
                file.target
                    .strip_suffix(b" (deleted)")
                    .unwrap_or(&file.target)
            } else {
                &file.target
            };
            let key = (file.dev, file.ino);
            let writable = self.cfg.open_writers
                && file.flags.is_some_and(|f| matches!(f & 3, 1 | 2))
                && !self
                    .cfg
                    .exclude_prefixes
                    .iter()
                    .any(|p| under(path, p.as_bytes()));
            if file.nlink != 0 && !writable {
                continue;
            }
            let attr = attrs
                .entry(file.pid)
                .or_insert_with(|| self.source.attribution(file.pid));
            let unknown = (attr.starttime != file.starttime).then(|| Attribution {
                pid: file.pid,
                starttime: file.starttime,
                comm: "?".into(),
                unit: None,
                uid: None,
            });
            let attr = unknown.as_ref().unwrap_or(attr);
            if file.nlink == 0 {
                if previous_phantom != Some(key) {
                    summaries[index].total_bytes =
                        summaries[index].total_bytes.saturating_add(file.disk);
                    summaries[index].files += 1;
                    previous_phantom = Some(key);
                }
                let holder = (key, file.pid);
                if previous_holder != Some(holder) {
                    summaries[index].holders.push(PhantomHolder {
                        pid: file.pid,
                        comm: attr.comm.clone(),
                        unit: attr.unit.clone(),
                        path: path.to_vec(),
                        dev: file.dev,
                        ino: file.ino,
                        bytes: file.disk,
                    });
                    previous_holder = Some(holder);
                }
            }
            if writable && !seen_writers.contains_key(&key) {
                seen_writers.insert(
                    key,
                    WriterSample {
                        ts: now,
                        mount: summaries[index].mount.clone(),
                        pid: file.pid,
                        comm: attr.comm.clone(),
                        unit: attr.unit.clone(),
                        path: path.to_vec(),
                        dev: file.dev,
                        ino: file.ino,
                        size: file.size,
                        disk: file.disk,
                    },
                );
            }
        }
        for t in self.tracked.values_mut() {
            t.missed = t.missed.saturating_add(1);
        }
        let mut candidates: Vec<_> = self
            .tracked
            .iter()
            .filter(|(k, t)| t.missed < 2 || seen_writers.contains_key(k))
            .map(|(k, t)| {
                (
                    *k,
                    seen_writers.get(k).map_or_else(
                        || growth(t),
                        |r| t.samples.front().map_or(0, |s| delta(r.disk, s.1)),
                    ),
                )
            })
            .collect();
        let mut newcomers: Vec<_> = seen_writers
            .keys()
            .filter(|k| !self.tracked.contains_key(k))
            .copied()
            .collect();
        // Rotate admission in key order, wrapping after the last admitted key.
        if let Some(cursor) = self.admission_cursor {
            let split = newcomers.partition_point(|k| *k <= cursor);
            newcomers.rotate_left(split);
        }
        let writers_full = candidates.len() + newcomers.len() > MAX_WRITERS;
        candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        // Every other successful pass replaces at most half the table's smallest
        // growers. The intervening pass guarantees newcomers a second observation.
        let admissions = if writers_full && self.rotate {
            newcomers.len().min(MAX_WRITERS / 2)
        } else {
            newcomers
                .len()
                .min(MAX_WRITERS.saturating_sub(candidates.len()))
        };
        candidates.truncate(MAX_WRITERS - admissions);
        let mut keep: std::collections::BTreeSet<_> = candidates.into_iter().map(|c| c.0).collect();
        for key in newcomers.into_iter().take(admissions) {
            self.admission_cursor = Some(key);
            keep.insert(key);
        }
        self.rotate = !self.rotate;
        let writer_evictions = self
            .tracked
            .iter()
            .filter(|(k, t)| !keep.contains(k) && (t.missed < 2 || seen_writers.contains_key(k)))
            .count() as u64;
        self.tracked.retain(|k, _| keep.contains(k));
        seen_writers.retain(|k, _| keep.contains(k));
        for (key, row) in seen_writers {
            let t = self.tracked.entry(key).or_insert_with(|| Tracked {
                row: row.clone(),
                samples: VecDeque::with_capacity(HISTORY),
                missed: 0,
            });
            t.missed = 0;
            t.row = row;
            if t.samples.len() == HISTORY {
                t.samples.pop_front();
            }
            t.samples.push_back((now, t.row.disk, t.row.size));
        }
        for summary in &mut summaries {
            summary.holders.sort_unstable_by(|a, b| {
                b.bytes
                    .cmp(&a.bytes)
                    .then(a.path.cmp(&b.path))
                    .then((a.dev, a.ino, a.pid).cmp(&(b.dev, b.ino, b.pid)))
            });
        }
        for s in &summaries {
            for h in &s.holders {
                phantom_samples.push(PhantomSample {
                    ts: now,
                    mount: s.mount.clone(),
                    pid: h.pid,
                    comm: h.comm.clone(),
                    unit: h.unit.clone(),
                    path: h.path.clone(),
                    dev: h.dev,
                    ino: h.ino,
                    bytes: h.bytes,
                });
            }
        }
        self.pending.0 = phantom_samples;
        self.pending.1 = Vec::with_capacity(self.tracked.len());
        self.pending.1.extend(
            self.tracked
                .values()
                .filter(|t| t.missed == 0)
                .map(|t| t.row.clone()),
        );
        self.phantom = summaries;
        self.now = Some(now);
        Ok(ProcScanResult {
            at: now,
            fds_seen,
            writers_full,
            writer_evictions,
            duration: started.elapsed(),
        })
    }
    /// Snapshot the completed report without exposing collector synchronization.
    /// None before the first successful pass; failures retain private collector history.
    pub fn report(&self) -> Option<ProcReport> {
        Some(ProcReport {
            mounts: self.cfg.mounts.clone(),
            phantom: self.phantom.clone(),
            tracked: self.tracked.clone(),
            now: self.now?,
        })
    }
    /// Latest totals with each inode counted once and each process holder listed once.
    pub fn phantom(&self, mount: Option<&RawPath>) -> Vec<PhantomSummary> {
        phantom_rows(&self.phantom, &self.cfg.mounts, mount)
    }
    /// Positive allocated growth over the caller's exact window and retained history.
    pub fn writers(
        &self,
        mount: Option<&RawPath>,
        window: Duration,
        limit: usize,
    ) -> Vec<OpenWriter> {
        writer_rows(
            &self.tracked,
            &self.cfg.mounts,
            self.now,
            mount,
            window,
            limit,
        )
    }
    /// Drain the latest pass's persistence samples exactly once.
    pub fn take_samples(&mut self) -> (Vec<PhantomSample>, Vec<WriterSample>) {
        std::mem::take(&mut self.pending)
    }
}
fn phantom_rows(
    rows: &[PhantomSummary],
    mounts: &[(RawPath, DevId)],
    mount: Option<&RawPath>,
) -> Vec<PhantomSummary> {
    rows.iter()
        .enumerate()
        .filter(|(i, _)| mount.is_none_or(|p| p == &mounts[*i].0))
        .map(|(_, s)| s.clone())
        .collect()
}
fn writer_rows(
    tracked: &BTreeMap<(u64, u64), Tracked>,
    mounts: &[(RawPath, DevId)],
    now: Option<i64>,
    mount: Option<&RawPath>,
    window: Duration,
    limit: usize,
) -> Vec<OpenWriter> {
    if window.is_zero() {
        return Vec::new();
    }
    let Some(now) = now else {
        return Vec::new();
    };
    let cutoff = now.saturating_sub(
        i64::try_from(
            window
                .as_secs()
                .saturating_add(u64::from(window.subsec_nanos() != 0)),
        )
        .unwrap_or(i64::MAX),
    );
    let mut rows = Vec::with_capacity(tracked.len());
    for t in tracked.values().filter(|t| t.missed == 0) {
        let r = &t.row;
        if mount.is_some_and(|p| !mounts.iter().any(|(m, d)| m == p && *d == r.dev)) {
            continue;
        }
        let Some(base) = t
            .samples
            .iter()
            .rev()
            .find(|s| s.0 <= cutoff)
            .or_else(|| t.samples.front())
        else {
            continue;
        };
        let growth = delta(r.disk, base.1);
        let elapsed = r.ts.saturating_sub(base.0);
        if growth <= 0 || elapsed <= 0 {
            continue;
        }
        rows.push(OpenWriter {
            mount: r.mount.clone(),
            pid: r.pid,
            comm: r.comm.clone(),
            unit: r.unit.clone(),
            path: r.path.clone(),
            dev: r.dev,
            ino: r.ino,
            size: r.size,
            disk: r.disk,
            growth,
            window_secs: elapsed as u64,
        });
    }
    rows.sort_unstable_by(|a, b| {
        b.growth
            .cmp(&a.growth)
            .then(a.path.cmp(&b.path))
            .then((a.dev, a.ino, a.pid).cmp(&(b.dev, b.ino, b.pid)))
    });
    rows.truncate(limit);
    rows
}
fn under(path: &[u8], prefix: &[u8]) -> bool {
    path == prefix
        || prefix == b"/"
        || path
            .strip_prefix(prefix)
            .is_some_and(|s| s.starts_with(b"/"))
}

#[cfg(test)]
#[path = "../tests/w5b/unit.rs"]
mod tests;
