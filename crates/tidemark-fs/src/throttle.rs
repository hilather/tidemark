//! Owned by: W1-D
//! Thread-safe metadata token buckets and an interruptible AIMD controller.

use parking_lot::{Condvar, Mutex};
use std::{
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
/// Shared core statistics.
pub use tidemark_core::scan::GateStats;
use tidemark_core::{
    config::{ConfigIssue, ThrottleConfig},
    dev::{BlockDevInfo, DiskStat},
    scan::{Gate, NoopGate},
};

/// Parameters for additive increase, multiplicative decrease.
pub struct AdaptiveParams {
    /// Maximum metadata units per second.
    pub max_ops: u32,
    /// Minimum metadata units per second.
    pub min_ops: u32,
    /// Maximum single-queue rotational device utilization, percent.
    pub max_util_pct: f64,
    /// Maximum average request latency, milliseconds.
    pub target_latency_ms: f64,
    /// Time between samples.
    pub interval: Duration,
}
// Maximum retained sample count; oversize sampler results are rejected.
const MAX_LEAVES: usize = 4096;

/// A bounded-duration sampler, returning leaves in stable order.
pub type StatSampler = Box<dyn FnMut() -> io::Result<Vec<DiskStat>> + Send>;

struct Bucket {
    state: Mutex<TokenState>,
    wake: Condvar,
    blocked_ns: AtomicU64,
}
// Constant-size accounting: issued - earned is outstanding token debt.
// Each caller keeps its own FIFO target, so no waiter collection is needed.
struct TokenState {
    last: Instant,
    rate: f64,
    issued: f64,
    earned: f64,
}
impl TokenState {
    fn account(&mut self, now: Instant) {
        self.earned = (self.earned + self.rate * now.duration_since(self.last).as_secs_f64())
            .min(self.issued + self.rate * 0.25);
        self.last = now;
    }
}
impl Bucket {
    fn new(rate: f64) -> Self {
        Self {
            state: Mutex::new(TokenState {
                last: Instant::now(),
                rate,
                issued: 0.0,
                earned: rate * 0.25,
            }),
            wake: Condvar::new(),
            blocked_ns: AtomicU64::new(0),
        }
    }
    fn rate(&self) -> f64 {
        self.state.lock().rate
    }
    fn set_rate(&self, rate: f64) {
        let mut state = self.state.lock();
        // Earn tokens at the old rate up to the transition, never retroactively.
        state.account(Instant::now());
        state.rate = rate;
        state.earned = state.earned.min(state.issued + rate * 0.25);
        self.wake.notify_all();
    }
    fn acquire(&self, cost: u32) {
        if cost == 0 {
            return;
        }
        let before = Instant::now();
        let mut state = self.state.lock();
        state.account(Instant::now());
        state.issued += f64::from(cost);
        let target = state.issued;
        let mut blocked = false;
        loop {
            state.account(Instant::now());
            let debt = target - state.earned;
            if debt <= 0.0 {
                break;
            }
            blocked = true;
            let wait = Duration::from_secs_f64((debt / state.rate).min(u32::MAX as f64));
            self.wake.wait_for(&mut state, wait);
        }
        if blocked {
            self.blocked_ns
                .fetch_add(nanos(before.elapsed()), Ordering::Relaxed);
        }
    }
}
fn nanos(d: Duration) -> u64 {
    d.as_nanos().min(u128::from(u64::MAX)) as u64
}

/// Fixed-rate token bucket with a quarter-second burst allowance.
pub struct FixedGate {
    bucket: Bucket,
}
impl FixedGate {
    /// Construct a bucket. Invalid/nonpositive rates are normalized to one unit/s.
    fn new(rate: f64) -> Self {
        Self {
            bucket: Bucket::new(if rate.is_finite() && rate > 0.0 {
                rate
            } else {
                1.0
            }),
        }
    }
}
impl Gate for FixedGate {
    fn acquire(&self, cost: u32) {
        self.bucket.acquire(cost);
    }
    fn current_rate(&self) -> f64 {
        self.bucket.rate()
    }
}

struct State {
    bucket: Bucket,
    stopped: Mutex<bool>,
    wake: Condvar,
    metrics: Mutex<Metrics>,
}
struct Metrics {
    stats: GateStats,
    last: Instant,
    started: Instant,
    integral: f64,
    rate: f64,
}
impl Metrics {
    fn account(&mut self, now: Instant) {
        self.integral += self.rate * now.duration_since(self.last).as_secs_f64();
        self.last = now;
    }
}
/// Adaptive gate. Dropping the last gate or calling stop joins its controller.
pub struct AdaptiveGate {
    state: Arc<State>,
    controller: Mutex<Option<JoinHandle<()>>>,
}
impl AdaptiveGate {
    /// Start at the maximum rate. The sampler must finish within one interval.
    pub fn start(mut p: AdaptiveParams, mut sampler: StatSampler) -> Arc<AdaptiveGate> {
        p.min_ops = p.min_ops.max(1);
        p.max_ops = p.max_ops.max(p.min_ops);
        p.interval = p.interval.max(Duration::from_millis(1));
        let rate = f64::from(p.max_ops);
        let now = Instant::now();
        let state = Arc::new(State {
            bucket: Bucket::new(rate),
            stopped: Mutex::new(false),
            wake: Condvar::new(),
            metrics: Mutex::new(Metrics {
                stats: GateStats {
                    mean_rate: rate,
                    min_rate: rate,
                    ..GateStats::default()
                },
                last: now,
                started: now,
                integral: 0.0,
                rate,
            }),
        });
        let worker = Arc::clone(&state);
        let controller = thread::spawn(move || {
            let mut previous: Option<(Vec<DiskStat>, Instant)> = None;
            let mut warned = false;
            loop {
                if *worker.stopped.lock() {
                    break;
                }
                match sampler() {
                    Ok(current) if current.len() <= MAX_LEAVES => {
                        let now = Instant::now();
                        if let Some((prev, sampled_at)) = previous.take() {
                            let rate = worker.bucket.rate();
                            let next = controller_step(
                                rate,
                                &prev,
                                &current,
                                now.duration_since(sampled_at).as_secs_f64() * 1000.0,
                                &p,
                            );
                            let mut metrics = worker.metrics.lock();
                            metrics.account(now);
                            if next < rate {
                                metrics.stats.decreases += 1;
                            }
                            if next > rate {
                                metrics.stats.increases += 1;
                            }
                            metrics.stats.min_rate = metrics.stats.min_rate.min(next);
                            metrics.rate = next;
                            worker.bucket.set_rate(next);
                        }
                        previous = Some((current, now));
                    }
                    Ok(_) => {
                        if !warned {
                            tracing::warn!(
                                "disk sampler exceeded 4096 leaves; keeping throttle rate"
                            );
                            warned = true;
                        }
                        previous = None;
                    }
                    Err(e) => {
                        if !warned {
                            tracing::warn!(error = %e, "disk sampler failed; keeping throttle rate");
                            warned = true;
                        }
                        // Don't compare a stale baseline across an unavailable sample.
                        previous = None;
                    }
                }
                let mut stopped = worker.stopped.lock();
                if *stopped {
                    break;
                }
                worker.wake.wait_for(&mut stopped, p.interval);
                if *stopped {
                    break;
                }
            }
            worker.metrics.lock().account(Instant::now());
        });
        Arc::new(Self {
            state,
            controller: Mutex::new(Some(controller)),
        })
    }
    /// Time-weighted rates and total blocked worker time.
    pub fn stats(&self) -> GateStats {
        let mut metrics = self.state.metrics.lock();
        let now = if *self.state.stopped.lock() {
            metrics.last
        } else {
            Instant::now()
        };
        metrics.account(now);
        let mut stats = metrics.stats.clone();
        let elapsed = now.duration_since(metrics.started).as_secs_f64();
        stats.mean_rate = if elapsed > 0.0 {
            metrics.integral / elapsed
        } else {
            metrics.rate
        };
        stats.throttled_ms = self.state.bucket.blocked_ns.load(Ordering::Relaxed) / 1_000_000;
        stats
    }
    /// Wake and join the controller; safe to call repeatedly or concurrently.
    pub fn stop(&self) {
        *self.state.stopped.lock() = true;
        self.state.wake.notify_all();
        if let Some(handle) = self.controller.lock().take() {
            if handle.join().is_err() {
                tracing::warn!("throttle controller panicked");
            }
        }
    }
}
impl Drop for AdaptiveGate {
    fn drop(&mut self) {
        self.stop();
    }
}
impl Gate for AdaptiveGate {
    fn acquire(&self, cost: u32) {
        self.state.bucket.acquire(cost);
    }
    fn current_rate(&self) -> f64 {
        self.state.bucket.rate()
    }
}

/// Pure AIMD step; paired samples must have the same stable leaf order.
/// Missing baselines, counter resets and invalid intervals preserve the rate.
pub fn controller_step(
    rate: f64,
    prev: &[DiskStat],
    cur: &[DiskStat],
    dt_ms: f64,
    p: &AdaptiveParams,
) -> f64 {
    let min = f64::from(p.min_ops.max(1));
    let max = f64::from(p.max_ops.max(p.min_ops).max(1));
    let rate = if rate.is_finite() {
        rate.clamp(min, max)
    } else {
        min
    };
    if !dt_ms.is_finite() || dt_ms <= 0.0 || prev.len() != cur.len() || cur.is_empty() {
        return rate;
    }
    let mut overloaded = false;
    for (old, new) in prev.iter().zip(cur) {
        let deltas = (
            new.io_ticks_ms.checked_sub(old.io_ticks_ms),
            new.read_ticks_ms.checked_sub(old.read_ticks_ms),
            new.write_ticks_ms.checked_sub(old.write_ticks_ms),
            new.read_ios.checked_sub(old.read_ios),
            new.write_ios.checked_sub(old.write_ios),
        );
        let (Some(io), Some(read_ms), Some(write_ms), Some(reads), Some(writes)) = deltas else {
            return rate;
        };
        let util = io as f64 / dt_ms * 100.0;
        let latency = (read_ms as f64 + write_ms as f64) / (reads as f64 + writes as f64).max(1.0);
        overloaded |= util > p.max_util_pct || latency > p.target_latency_ms;
    }
    if overloaded {
        (rate * 0.5).max(min)
    } else {
        (rate + 0.05 * max).min(max)
    }
}

/// Select a configured adaptive profile, conservative fixed fallback, or max.
/// No leaves (including unresolved W1-A discovery) use the configured minimum.
pub fn gate_from_config(
    profile: &str,
    cfg: &ThrottleConfig,
    leaves: &[BlockDevInfo],
    sampler: StatSampler,
) -> Result<Arc<dyn Gate>, ConfigIssue> {
    if profile == "max" {
        return Ok(Arc::new(NoopGate));
    }
    let issue = |message: &str| ConfigIssue {
        field: "throttle".into(),
        message: message.into(),
    };
    let params = cfg
        .profiles
        .get(profile)
        .ok_or_else(|| issue("unknown throttle profile"))?;
    if cfg.min_ops_per_sec == 0
        || cfg.max_ops_per_sec < cfg.min_ops_per_sec
        || cfg.sample_interval.is_zero()
        || !params.max_util_pct.is_finite()
        || !(0.0..=100.0).contains(&params.max_util_pct)
        || !params.target_latency_ms.is_finite()
        || params.target_latency_ms <= 0.0
    {
        return Err(issue("invalid throttle parameters"));
    }
    if leaves.len() > MAX_LEAVES {
        return Err(issue("more than 4096 leaf devices"));
    }
    if leaves.is_empty() {
        tracing::warn!("no leaf devices known; using minimum fixed metadata rate");
        return Ok(Arc::new(FixedGate::new(f64::from(cfg.min_ops_per_sec))));
    }
    // %util is trustworthy only on known rotational, single-queue leaves.
    // Missing mq directories on legacy devices mean a single queue; unknown
    // rotational or inaccessible sysfs information disables the util signal.
    let use_util: Vec<bool> = leaves
        .iter()
        .map(|leaf| {
            if leaf.rotational != Some(true) {
                return false;
            }
            match std::fs::read_dir(format!("/sys/dev/block/{}:{}/mq", leaf.major, leaf.minor)) {
                Ok(mut entries) => matches!((entries.next(), entries.next()), (Some(Ok(_)), None)),
                Err(e) => e.kind() == io::ErrorKind::NotFound,
            }
        })
        .collect();
    let mut sampler = sampler;
    let masked: StatSampler = Box::new(move || {
        let mut stats = sampler()?;
        if stats.len() != use_util.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "leaf sample count changed",
            ));
        }
        for (stat, enabled) in stats.iter_mut().zip(&use_util) {
            if !enabled {
                stat.io_ticks_ms = 0;
            }
        }
        Ok(stats)
    });
    Ok(AdaptiveGate::start(
        AdaptiveParams {
            max_ops: cfg.max_ops_per_sec,
            min_ops: cfg.min_ops_per_sec,
            max_util_pct: params.max_util_pct,
            target_latency_ms: params.target_latency_ms,
            interval: cfg.sample_interval,
        },
        masked,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn time_weighted_debt_and_burst() {
        let start = Instant::now();
        let mut state = TokenState {
            last: start,
            rate: 1000.0,
            issued: 1000.0,
            earned: 250.0,
        };
        state.account(start + Duration::from_millis(100));
        state.rate = 1.0;
        state.account(start + Duration::from_millis(800));
        assert!((state.earned - 350.7).abs() < 1e-9);
        assert!(state.earned < state.issued);
        state.rate = 1000.0;
        state.account(start + Duration::from_millis(1500));
        assert!((state.earned - 1050.7).abs() < 1e-9);
        state.account(start + Duration::from_secs(100));
        assert_eq!(state.earned, state.issued + 250.0);
    }

    // Real wait/notification tests allow 2 units for floating point and scheduling.
    // Integrate the actual rate transition times; late scheduling adds no credit.
    #[test]
    fn concurrent_waiters_follow_rate_decrease() {
        let bucket = Arc::new(Bucket::new(100.0));
        bucket.acquire(25);
        let start = Instant::now();
        let count = Arc::new(AtomicU64::new(0));
        let handles: Vec<_> = (0..64)
            .map(|_| {
                let bucket = Arc::clone(&bucket);
                let count = Arc::clone(&count);
                thread::spawn(move || {
                    bucket.acquire(1);
                    count.fetch_add(1, Ordering::Relaxed);
                })
            })
            .collect();
        thread::sleep(Duration::from_millis(50));
        bucket.set_rate(1.0);
        let changed = start.elapsed().as_secs_f64();
        thread::sleep(Duration::from_millis(700));
        let end = start.elapsed().as_secs_f64();
        let granted = count.load(Ordering::Relaxed) as f64;
        assert!(
            granted <= 100.0 * changed + (end - changed) + 25.0 + 2.0,
            "granted {granted}, change {changed}, end {end}"
        );
        bucket.set_rate(1000.0);
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(count.load(Ordering::Relaxed), 64);
    }

    #[test]
    fn bulk_waiter_follows_decrease_and_recovery() {
        let bucket = Arc::new(Bucket::new(1000.0));
        let start = Instant::now();
        let clone = Arc::clone(&bucket);
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            clone.acquire(1000);
            send.send(start.elapsed()).unwrap();
        });
        thread::sleep(Duration::from_millis(100));
        bucket.set_rate(1.0);
        let decreased = start.elapsed().as_secs_f64();
        thread::sleep(Duration::from_millis(800));
        assert!(
            recv.try_recv().is_err(),
            "bulk request ignored the decrease"
        );
        bucket.set_rate(1000.0);
        let recovered = start.elapsed().as_secs_f64();
        let finished = recv
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .as_secs_f64();
        let earned = 1000.0 * decreased + (recovered - decreased) + 1000.0 * (finished - recovered);
        assert!(1000.0 <= earned + 250.0 + 2.0, "earned {earned}");
        handle.join().unwrap();
    }

    #[test]
    fn four_thread_fixed_rate() {
        let gate = Arc::new(FixedGate::new(1000.0));
        // Spend the documented initial 250ms burst before measuring steady state.
        gate.acquire(250);
        let start = Instant::now();
        let count = Arc::new(AtomicU64::new(0));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let gate = Arc::clone(&gate);
                let count = Arc::clone(&count);
                std::thread::spawn(move || {
                    while start.elapsed() < Duration::from_secs(2) {
                        gate.acquire(1);
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let count = count.load(Ordering::Relaxed);
        assert!((1900..=2100).contains(&count), "count {count}");
    }
}
