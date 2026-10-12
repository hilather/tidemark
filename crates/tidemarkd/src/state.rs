//! Owned by: W6-A1
//! Bounded shared measurements, health and injected-time report views.

use parking_lot::RwLock;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tidemark_core::{api::*, config::Config, live::*, path::RawPath};
use tidemark_live::alerts::{AlertDiagnostic, AlertSource, ScanAge};

/// Maximum configured mounts, roots, components and degradation reasons.
pub const MAX_SUBJECTS: usize = 128;
/// Maximum records in a complete measurement inventory; over-cap publications fail.
pub const MAX_INVENTORY: usize = 10_000;
/// Maximum distinct exact watcher mount/window pairs.
pub const MAX_WINDOWS: usize = 1024;
/// Maximum samples per mount, additionally evicted after two hours.
pub const MAX_FS_SAMPLES: usize = 7201;
/// Shared bounded publication handle; cloned handles observe atomic config reloads.
#[derive(Clone)]
pub struct LiveState(Arc<RwLock<Inner>>);
/// Consistent data/config snapshot. Watcher and scanner handles have their own locks.
pub struct LiveStateView {
    data: Inner,
    now: i64,
}
#[derive(Clone)]
struct Inner {
    config: Arc<Config>,
    generation: u64,
    started: i64,
    hash: Option<String>,
    fs: BTreeMap<String, VecDeque<FsSample>>,
    lvm: Option<LvmResult>,
    watchers: BTreeMap<String, Arc<tidemark_live::watch::Watcher>>,
    procscan: Option<Arc<parking_lot::Mutex<tidemark_live::procscan::ProcScanner>>>,
    alerts: Option<(Vec<AlertSummary>, Vec<AlertDiagnostic>)>,
    scans: BTreeMap<String, ScanStatus>,
    growth: BTreeMap<(String, u32), Vec<(RawPath, i64)>>,
    windows: BTreeMap<(String, Duration), WindowFacts>,
    components: BTreeMap<String, ComponentState>,
    errors: BTreeMap<String, (i64, String)>,
    degraded: Vec<String>,
    history: VecDeque<AlertEvent>,
    space: Option<bool>,
    metrics: Option<Arc<crate::metrics::SnapshotMetrics>>,
    storage: Option<Arc<std::os::fd::OwnedFd>>,
}
#[derive(Clone)]
struct WindowFacts {
    at: i64,
    hot: Option<Vec<HotEntry>>,
    overflows: Option<u64>,
}
impl Default for LiveState {
    fn default() -> Self {
        Self::new(&Config::default(), 0)
    }
}
/// Atomic service publication. Inventories are complete or rejected, never silently truncated.
pub enum Publication {
    /// Publish fs measurements.
    Fs(FsSample),
    /// Publish lvm measurements.
    Lvm(LvmResult),
    /// Publish watcher measurements.
    Watcher((String, Arc<tidemark_live::watch::Watcher>)),
    /// Publish procscan measurements.
    Procscan(Arc<parking_lot::Mutex<tidemark_live::procscan::ProcScanner>>),
    /// Publish alerts measurements.
    Alerts((Vec<AlertSummary>, Vec<AlertDiagnostic>)),
    /// Publish scan measurements.
    Scan(ScanStatus),
    /// Publish dirgrowth measurements.
    DirGrowth((String, u32, Vec<(RawPath, i64)>)),
    /// Publish cached metrics from a completed scan.
    SnapshotMetrics(crate::metrics::SnapshotMetrics),
    /// Publish the current installed config hash and daemon startup time for firstboot.
    Deployment {
        /// SHA-256 of the actual loaded config bytes.
        config_sha256: String,
        /// Injected Unix startup time.
        started_at: i64,
    },
    /// Publish snapshot space-guard refusal.
    StateSpaceRefused(bool),
    /// Exact rule-window facts supplied after a successful watcher observation.
    /// Complete inventories only; at most MAX_INVENTORY entries per window and
    /// MAX_WINDOWS distinct mount/window pairs. Duration is never rounded.
    WatchWindow {
        /// Configured mount.
        mount: String,
        /// Exact requested duration.
        window: Duration,
        /// Observation timestamp, supplied by the collector.
        measured_at: i64,
        /// Complete directory inventory, None if unavailable.
        hot: Option<Vec<HotEntry>>,
        /// Exact window overflow count, None if unavailable.
        overflows: Option<u64>,
    },
    /// Persisted alert history observation; capped at MAX_INVENTORY events.
    AlertEvent(AlertEvent),
    /// Publish degraded measurements.
    Degraded(String),
}
impl LiveState {
    /// Create unavailable measurement state. Subject inventories are capped at MAX_SUBJECTS.
    pub fn new(config: &Config, started_at: i64) -> Self {
        Self(Arc::new(RwLock::new(Inner {
            config: Arc::new(config.clone()),
            generation: 0,
            started: started_at,
            hash: None,
            fs: BTreeMap::new(),
            lvm: None,
            watchers: BTreeMap::new(),
            procscan: None,
            alerts: None,
            scans: BTreeMap::new(),
            growth: BTreeMap::new(),
            windows: BTreeMap::new(),
            components: BTreeMap::new(),
            errors: BTreeMap::new(),
            degraded: Vec::new(),
            history: VecDeque::new(),
            space: None,
            metrics: None,
            storage: None,
        })))
    }
    /// Snapshot facts; default alert clock is the newest fs observation or startup time.
    /// Alert evaluators should use view_at(now) to select their explicit logical clock.
    pub fn view(&self) -> LiveStateView {
        let data = self.0.read().clone();
        let now = data
            .fs
            .values()
            .filter_map(|r| r.back().map(|s| s.ts))
            .max()
            .unwrap_or(data.started);
        LiveStateView { data, now }
    }
    /// Snapshot facts with the caller's injected Unix timestamp.
    pub fn view_at(&self, now: i64) -> LiveStateView {
        let mut v = self.view();
        v.now = now;
        v
    }
    /// Current effective config and monotonic reload generation; poll at service boundaries.
    /// The config includes state_dir self-protection excludes. No subscription queue grows.
    pub fn config(&self) -> (Arc<Config>, u64) {
        let d = self.0.read();
        (d.config.clone(), d.generation)
    }
    /// Atomically publish validated effective configuration. Runtime state/socket relocation is refused.
    pub fn replace_config(&self, config: Config) -> Result<(), crate::DaemonError> {
        validate_bounds(&config)?;
        let mut d = self.0.write();
        if d.config.general.state_dir != config.general.state_dir
            || d.config.general.socket != config.general.socket
        {
            return Err(crate::DaemonError::Config(
                "state_dir and socket changes require restart".into(),
            ));
        }
        d.fs.retain(|m, _| {
            config
                .sampler
                .mounts
                .iter()
                .any(|p| p.to_string_lossy() == m.as_str())
        });
        d.windows.clear();
        d.config = Arc::new(config);
        d.generation = d.generation.saturating_add(1);
        Ok(())
    }
    /// Retain a verified state directory for descriptor-anchored SQLite paths.
    pub fn bind_storage(&self, fd: std::os::fd::OwnedFd) {
        self.0.write().storage = Some(Arc::new(fd));
    }
    /// Storage path anchored to the retained directory descriptor, or configured test path.
    /// The final `.` names the directory itself for consumers using `O_NOFOLLOW`.
    pub fn storage_path(&self) -> std::path::PathBuf {
        use std::os::fd::AsRawFd;
        let d = self.0.read();
        d.storage
            .as_ref()
            .map(|f| format!("/proc/self/fd/{}/.", f.as_raw_fd()).into())
            .unwrap_or_else(|| d.config.general.state_dir.clone())
    }
    /// Publish a complete bounded result and health atomically.
    pub fn publish(
        &self,
        result: Publication,
        component: ComponentState,
    ) -> Result<(), crate::DaemonError> {
        let mut d = self.0.write();
        check_key(&d.components, &component.name, MAX_SUBJECTS)?;
        match result {
            Publication::Fs(s) => {
                check_key(&d.fs, &s.mount, MAX_SUBJECTS)?;
                let r = d.fs.entry(s.mount.clone()).or_default();
                if r.back().is_some_and(|last| s.ts < last.ts) {
                    r.clear();
                }
                if r.back().is_some_and(|last| s.ts == last.ts) {
                    r.pop_back();
                }
                let cutoff = s.ts.saturating_sub(7200);
                while r.front().is_some_and(|v| v.ts < cutoff) || r.len() >= MAX_FS_SAMPLES {
                    r.pop_front();
                }
                r.push_back(s);
            }
            Publication::Lvm(mut r) => {
                check_len(r.mounts.len(), MAX_SUBJECTS)?;
                check_len(r.vgs.len(), MAX_INVENTORY)?;
                r.mounts.sort_by(|a, b| a.0.cmp(&b.0));
                r.vgs.sort_by(|a, b| a.vg.cmp(&b.vg));
                d.lvm = Some(r);
            }
            Publication::Watcher((mount, w)) => {
                check_key(&d.watchers, &mount, MAX_SUBJECTS)?;
                d.watchers.insert(mount, w);
            }
            Publication::Procscan(p) => d.procscan = Some(p),
            Publication::Alerts((mut a, mut diagnostics)) => {
                check_len(a.len(), MAX_INVENTORY)?;
                check_len(diagnostics.len(), MAX_INVENTORY)?;
                a.sort_by(|a, b| (&a.rule, &a.subject).cmp(&(&b.rule, &b.subject)));
                diagnostics.sort_by(|a, b| a.rule.cmp(&b.rule));
                d.alerts = Some((a, diagnostics));
            }
            Publication::Scan(s) => {
                check_key(&d.scans, &s.root, MAX_SUBJECTS)?;
                d.scans.insert(s.root.clone(), s);
            }
            Publication::DirGrowth((root, depth, mut rows)) => {
                check_len(rows.len(), MAX_INVENTORY)?;
                let key = (root, depth);
                check_key(&d.growth, &key, MAX_WINDOWS)?;
                rows.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
                d.growth.insert(key, rows);
            }
            Publication::SnapshotMetrics(m) => d.metrics = Some(Arc::new(m)),
            Publication::Deployment {
                config_sha256,
                started_at,
            } => {
                d.hash = Some(config_sha256);
                d.started = started_at;
            }
            Publication::StateSpaceRefused(v) => d.space = Some(v),
            Publication::Degraded(reason) => {
                if !d.degraded.contains(&reason) {
                    check_len(d.degraded.len() + 1, MAX_SUBJECTS)?;
                    d.degraded.push(reason);
                    d.degraded.sort();
                }
            }
            Publication::WatchWindow {
                mount,
                window,
                measured_at,
                hot,
                overflows,
            } => {
                if let Some(h) = &hot {
                    check_len(h.len(), MAX_INVENTORY)?;
                }
                let key = (mount, window);
                check_key(&d.windows, &key, MAX_WINDOWS)?;
                d.windows.insert(
                    key,
                    WindowFacts {
                        at: measured_at,
                        hot,
                        overflows,
                    },
                );
            }
            Publication::AlertEvent(e) => {
                if d.history.len() == MAX_INVENTORY {
                    d.history.pop_front();
                }
                d.history.push_back(e);
            }
        }
        health(&mut d, component);
        Ok(())
    }
    /// Remove a watcher publication before restart/shutdown; the final Arc drop stops its worker.
    pub fn remove_watcher(&self, mount: &str) {
        self.0.write().watchers.remove(mount);
    }
    /// Release every watcher publication during ordered shutdown.
    pub fn clear_watchers(&self) {
        self.0.write().watchers.clear();
    }
    /// Publish component health without implying a successful measurement.
    pub fn publish_component(&self, component: ComponentState) -> Result<(), crate::DaemonError> {
        let mut d = self.0.write();
        check_key(&d.components, &component.name, MAX_SUBJECTS)?;
        health(&mut d, component);
        Ok(())
    }
}
fn health(d: &mut Inner, c: ComponentState) {
    if matches!(c.state.as_str(), "error" | "failed" | "degraded") {
        d.errors.insert(
            c.name.clone(),
            (
                d.fs.values()
                    .filter_map(|r| r.back().map(|s| s.ts))
                    .max()
                    .unwrap_or(d.started),
                c.detail.clone(),
            ),
        );
    }
    d.components.insert(c.name.clone(), c);
}
fn check_len(n: usize, cap: usize) -> Result<(), crate::DaemonError> {
    if n > cap {
        Err(crate::DaemonError::Unavailable(format!(
            "complete inventory exceeds {cap} records"
        )))
    } else {
        Ok(())
    }
}
fn check_key<K: Ord, V>(
    map: &BTreeMap<K, V>,
    key: &K,
    cap: usize,
) -> Result<(), crate::DaemonError> {
    check_len(map.len() + usize::from(!map.contains_key(key)), cap)
}
/// Validate daemon inventory caps without modifying frozen core config validation.
pub fn validate_bounds(c: &Config) -> Result<(), crate::DaemonError> {
    for n in [c.sampler.mounts.len(), c.watch.mounts.len(), c.roots.len()] {
        check_len(n, MAX_SUBJECTS)?;
    }
    check_len(c.alerts.rule.len(), MAX_INVENTORY)
}
impl LiveStateView {
    /// Current snapshot config and generation.
    pub fn config(&self) -> (Arc<Config>, u64) {
        (self.data.config.clone(), self.data.generation)
    }
    /// Build available firstboot inputs; None until config identity is published.
    pub fn firstboot_inputs(&self) -> Option<crate::firstboot::FirstbootInputs> {
        Some(crate::firstboot::FirstbootInputs {
            version: env!("CARGO_PKG_VERSION").into(),
            config_sha256: self.data.hash.clone()?,
            daemon_started_at: self.data.started,
            components: self.data.components.values().cloned().collect(),
            watch_modes: self.modes(),
            first_fs_sample: self
                .data
                .fs
                .values()
                .filter_map(|r| r.front().map(|s| s.ts))
                .min(),
            first_lvm_sample: self.data.lvm.as_ref().map(|r| r.sampled_at),
            next_scans: self
                .data
                .scans
                .values()
                .filter_map(|s| Some((s.root.clone(), s.next_due?)))
                .collect(),
        })
    }
    /// Clear only successfully scanned root dirtiness.
    pub fn clear_watch_dirty(&self, root: &RawPath) -> Result<(), crate::DaemonError> {
        for w in self.data.watchers.values() {
            w.take_dirty(root);
        }
        Ok(())
    }
    /// Unknown until the space guard has run.
    pub fn state_space_refused(&self) -> Option<bool> {
        self.data.space
    }
    /// Latest bounded persisted event observations, newest first.
    pub fn alerts_history(
        &self,
        p: &AlertsHistoryParams,
    ) -> Result<Vec<AlertEvent>, crate::DaemonError> {
        Ok(self
            .data
            .history
            .iter()
            .rev()
            .take(p.limit.unwrap_or(100).min(MAX_INVENTORY))
            .cloned()
            .collect())
    }
    /// Legacy owned cache adapter. Use snapshot_metrics_handle for a shared non-Clone cache.
    /// The preparation cache cannot be copied; this adapter remains unavailable.
    pub fn snapshot_metrics(&self) -> Option<crate::metrics::SnapshotMetrics> {
        None
    }
    /// Retained cache handle without requiring the metrics owner's type to implement Clone.
    pub fn snapshot_metrics_handle(&self) -> Option<Arc<crate::metrics::SnapshotMetrics>> {
        self.data.metrics.clone()
    }
    /// Available daemon health with injected uptime.
    pub fn status(&self, now: i64) -> Option<StatusResult> {
        Some(StatusResult {
            version: env!("CARGO_PKG_VERSION").into(),
            uptime_s: now.saturating_sub(self.data.started).max(0) as u64,
            components: self.data.components.values().cloned().collect(),
            watch_modes: self.modes(),
            last_errors: self
                .data
                .errors
                .iter()
                .map(|(name, (ts, error))| (name.clone(), *ts, error.clone()))
                .collect(),
        })
    }
    fn modes(&self) -> Vec<(String, String)> {
        self.data
            .watchers
            .iter()
            .map(|(m, w)| (m.clone(), watch_mode(w.mode()).into()))
            .collect()
    }
    /// Build shared report with explicit not-measured sections and exact injected clock.
    pub fn now(&self, p: &NowParams, now: i64) -> Option<NowReport> {
        use tidemark_live::{
            now::{build_now, NowInputs},
            rates::{time_to_full, MountRates},
        };
        let mut v = LiveStateView {
            data: self.data.clone(),
            now,
        };
        if let Some(m) = &p.mount {
            v.data.fs.retain(|key, _| key == m);
        }
        let samples = v.fs_samples();
        let rates: Option<Vec<_>> = samples.map(|s| {
            s.into_iter()
                .map(|s| {
                    let rate = v.fs_rate(&s.mount, Duration::from_secs(3600));
                    MountRates {
                        rate_15m: v.fs_rate(&s.mount, Duration::from_secs(900)),
                        rate_1h: rate,
                        ttf: rate.and_then(|r| time_to_full(s.avail, r)),
                        mount: s.mount,
                        size: s.size,
                        used: s.used,
                        avail: s.avail,
                    }
                })
                .collect()
        });
        let mounts: Vec<_> = v
            .data
            .config
            .sampler
            .mounts
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let hot: Option<Vec<_>> = mounts
            .iter()
            .map(|m| Some((m.clone(), v.hot_dirs(m, Duration::from_secs(300))?)))
            .collect();
        let phantom = v.phantom(&PhantomParams {
            mount: p.mount.clone(),
        });
        let writers = v.writers(&WritersParams {
            mount: p.mount.clone(),
            window: Duration::from_secs(300),
        });
        let grouped = writers.map(|rows| {
            mounts
                .iter()
                .map(|m| {
                    (
                        m.clone(),
                        rows.iter().filter(|r| &r.mount == m).cloned().collect(),
                    )
                })
                .collect::<Vec<_>>()
        });
        let unit_hot: Option<Vec<_>> = if v.data.watchers.is_empty() {
            None
        } else {
            Some(
                v.data
                    .watchers
                    .iter()
                    .map(|(m, w)| {
                        (
                            m.clone(),
                            w.hot(5, tidemark_live::watch::HotScope::Unit, MAX_INVENTORY),
                        )
                    })
                    .collect(),
            )
        };
        let modes: Option<Vec<_>> = if v.data.watchers.is_empty() {
            None
        } else {
            Some(
                v.data
                    .watchers
                    .iter()
                    .map(|(m, w)| {
                        (
                            m.clone(),
                            watch_mode(w.mode()).into(),
                            w.stats().lossy_minutes_15m,
                        )
                    })
                    .collect(),
            )
        };
        let alerts = v.active_alerts();
        let scans = v.scans();
        let diagnostics = v.alert_diagnostics().unwrap_or_default();
        Some(build_now(
            &NowInputs {
                host: rustix::system::uname()
                    .nodename()
                    .to_str()
                    .unwrap_or("unknown"),
                mounts: rates.as_deref(),
                lvm: v.data.lvm.as_ref().map(|r| r.mounts.as_slice()),
                hot_dirs_5m: hot.as_deref(),
                writers_5m: unit_hot.as_deref(),
                phantom: phantom.as_deref(),
                open_writers: grouped.as_deref(),
                alerts: alerts.as_deref(),
                scans: scans.as_deref(),
                watch_modes: modes.as_deref(),
                unevaluable: &diagnostics,
                degraded: &v.data.degraded,
            },
            now,
        ))
    }
    /// Bounded API hot list; integral API windows map exactly to watcher minutes.
    pub fn hot(&self, p: &HotParams) -> Option<Vec<HotEntry>> {
        if self.data.watchers.is_empty() {
            return None;
        }
        let minutes = match p.window {
            HotWindow::OneMinute => 1,
            HotWindow::FiveMinutes => 5,
            HotWindow::FifteenMinutes => 15,
            HotWindow::SixtyMinutes => 60,
        };
        let scope = match p.by {
            HotBy::Dir => tidemark_live::watch::HotScope::Dir,
            HotBy::Unit => tidemark_live::watch::HotScope::Unit,
            HotBy::Comm => tidemark_live::watch::HotScope::Comm,
        };
        let mut rows = Vec::new();
        for (mount, w) in &self.data.watchers {
            if p.mount.as_ref().is_none_or(|m| m == mount) {
                rows.extend(w.hot(minutes, scope, p.limit.min(MAX_INVENTORY)));
            }
        }
        rows.sort_by(|a, b| b.bytes_delta.cmp(&a.bytes_delta).then(a.key.cmp(&b.key)));
        rows.truncate(p.limit.min(MAX_INVENTORY));
        Some(rows)
    }
    /// Latest report, preserving measured-empty LVM inventory.
    pub fn lvm_report(&self, p: &LvmParams) -> Option<LvmResult> {
        let mut r = self.data.lvm.clone()?;
        if let Some(m) = &p.mount {
            r.mounts.retain(|(mount, _)| mount == m);
        }
        Some(r)
    }
    /// Successful scanner publication implies measured empty is available.
    pub fn phantom(&self, p: &PhantomParams) -> Option<Vec<PhantomSummary>> {
        let raw = p
            .mount
            .as_ref()
            .map(|m| RawPath::new(m.as_bytes().to_vec()).ok())
            .transpose_option()?;
        Some(self.data.procscan.as_ref()?.lock().phantom(raw.as_ref()))
    }
    /// Scanner growth using the caller's exact positive duration.
    pub fn writers(&self, p: &WritersParams) -> Option<Vec<OpenWriter>> {
        let raw = p
            .mount
            .as_ref()
            .map(|m| RawPath::new(m.as_bytes().to_vec()).ok())
            .transpose_option()?;
        Some(
            self.data
                .procscan
                .as_ref()?
                .lock()
                .writers(raw.as_ref(), p.window, MAX_INVENTORY),
        )
    }
    /// None before alert evaluation, Some(empty) after healthy evaluation.
    pub fn active_alerts(&self) -> Option<Vec<AlertSummary>> {
        self.data.alerts.as_ref().map(|r| r.0.clone())
    }
    /// Unavailable rule diagnostics.
    pub fn alert_diagnostics(&self) -> Option<Vec<AlertDiagnostic>> {
        self.data.alerts.as_ref().map(|r| r.1.clone())
    }
    /// Complete known scan statuses; unavailable until every configured root was observed.
    pub fn scans(&self) -> Option<Vec<ScanStatus>> {
        if self.data.config.roots.iter().any(|r| {
            !self
                .data
                .scans
                .contains_key(&r.path.to_string_lossy().into_owned())
        }) {
            return None;
        }
        Some(self.data.scans.values().cloned().collect())
    }
    /// Latest observations no later than this view's injected clock.
    pub fn fs_samples(&self) -> Option<Vec<FsSample>> {
        if self.data.fs.is_empty() {
            None
        } else {
            Some(
                self.data
                    .fs
                    .values()
                    .filter_map(|r| r.iter().rev().find(|s| s.ts <= self.now).cloned())
                    .collect(),
            )
        }
    }
    /// Current watcher counters, bounded by MAX_SUBJECTS.
    pub fn watch_stats(&self) -> Option<Vec<(String, tidemark_live::watch::WatchStats)>> {
        if self.data.watchers.is_empty() {
            None
        } else {
            Some(
                self.data
                    .watchers
                    .iter()
                    .map(|(m, w)| (m.clone(), w.stats()))
                    .collect(),
            )
        }
    }
    /// Known degradation reasons, capped at MAX_SUBJECTS.
    pub fn degraded(&self) -> Option<Vec<String>> {
        Some(self.data.degraded.clone())
    }
}
trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(v) => v.map(Some),
        }
    }
}
impl AlertSource for LiveStateView {
    fn fs_sample(&self, mount: &str) -> Option<FsSample> {
        self.data
            .fs
            .get(mount)?
            .iter()
            .rev()
            .find(|s| s.ts <= self.now)
            .cloned()
    }
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64> {
        if window.is_zero() {
            return None;
        }
        let start = i128::from(self.now) * 1_000_000_000
            - i128::from(window.as_secs()) * 1_000_000_000
            - i128::from(window.subsec_nanos());
        let rows = self
            .data
            .fs
            .get(mount)?
            .iter()
            .filter(|s| s.ts <= self.now && i128::from(s.ts) * 1_000_000_000 >= start)
            .map(|s| Some((s.ts, i64::try_from(s.used).ok()?)))
            .collect::<Option<Vec<_>>>()?;
        tidemark_live::rates::slope_bytes_per_sec(&rows)
    }
    fn hot_dirs(&self, mount: &str, window: Duration) -> Option<Vec<HotEntry>> {
        let f = self.data.windows.get(&(mount.into(), window))?;
        if f.at != self.now {
            return None;
        }
        f.hot.clone()
    }
    fn watch_overflows(&self, mount: &str, window: Duration) -> Option<u64> {
        let f = self.data.windows.get(&(mount.into(), window))?;
        if f.at != self.now {
            return None;
        }
        f.overflows
    }
    fn phantom_bytes(&self, mount: &str) -> Option<u64> {
        self.phantom(&PhantomParams {
            mount: Some(mount.into()),
        })?
        .iter()
        .find(|r| r.mount == mount)
        .map(|r| r.total_bytes)
    }
    fn lvm(&self) -> Option<Vec<LvmNow>> {
        Some(
            self.data
                .lvm
                .as_ref()?
                .mounts
                .iter()
                .map(|r| r.1.clone())
                .collect(),
        )
    }
    fn dir_growth(&self, root: &str, depth: u32) -> Option<Vec<(RawPath, i64)>> {
        self.data.growth.get(&(root.into(), depth)).cloned()
    }
    fn scan_age(&self, root: &str) -> Option<ScanAge> {
        let cfg = self
            .data
            .config
            .roots
            .iter()
            .find(|r| r.path.to_string_lossy() == root)?;
        let mut cadence = if cfg.schedule == "off" {
            None
        } else if cfg.schedule.starts_with("daily ") {
            Some(Duration::from_secs(86400))
        } else {
            cfg.schedule
                .strip_prefix("every ")
                .and_then(|d| humantime::parse_duration(d).ok())
        };
        if cfg.window.is_some() {
            cadence = cadence.map(|d| d.max(Duration::from_secs(86400)));
        }
        let status = self.data.scans.get(root)?;
        Some(ScanAge {
            last_complete_at: status.last_complete.as_ref().map(|l| l.finished_at),
            effective_cadence: cadence,
        })
    }
}

fn watch_mode(mode: tidemark_live::watch::WatchMode) -> &'static str {
    use tidemark_live::watch::WatchMode;
    match mode {
        WatchMode::FdMount => "fd_mount",
        WatchMode::DfidNameMount => "dfid_name_mount",
        WatchMode::DfidNameFilesystem => "dfid_name_filesystem",
        WatchMode::Failed => "failed",
    }
}
