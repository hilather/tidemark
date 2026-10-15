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
/// Consistent data/config snapshot. Process reports are immutable; watchers own their locks.
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
    presence: BTreeMap<String, Option<crate::mounts::Identity>>,
    fs_available: BTreeMap<String, bool>,
    fs_failures: BTreeMap<String, u64>,
    fs_failure_epoch: u64,
    lvm_failures: u64,
    procscan_failures: u64,
    procscan_completed_at: Option<i64>,
    procscan_available: bool,
    alert_revision: u64,
    lvm_available: bool,
    lvm: Option<LvmResult>,
    watchers: BTreeMap<String, Arc<tidemark_live::watch::Watcher>>,
    procscan: Option<Arc<tidemark_live::procscan::ProcReport>>,
    alerts: Option<(Vec<AlertSummary>, Vec<AlertDiagnostic>)>,
    scans: BTreeMap<String, ScanStatus>,
    retired_scans: std::collections::BTreeSet<String>,
    pending_scan: Option<String>,
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
    warming: bool,
    at: i64,
    hot: Option<Arc<tidemark_live::watch::WatcherRows>>,
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
    /// Generation-bound collector attempt; failure retains display history.
    FsAttempt {
        /// Configuration observed before collection.
        generation: u64,
        /// Configured mount whose collector attempted measurement.
        mount: String,
        /// Successful sample, or unavailable after failure.
        sample: Option<FsSample>,
    },
    /// Generation-bound LVM attempt; failure retains display history.
    LvmAttempt {
        /// Configuration observed before collection.
        generation: u64,
        /// Successful report, or unavailable after failure.
        report: Option<LvmResult>,
    },
    /// Publish lvm measurements.
    Lvm(LvmResult),
    /// Publish watcher measurements.
    Watcher((String, Arc<tidemark_live::watch::Watcher>)),
    /// Publish procscan measurements.
    Procscan(Arc<tidemark_live::procscan::ProcReport>),
    /// Generation-bound completed process report, or unavailable after failure.
    ProcscanAttempt {
        /// Configuration observed before collection.
        generation: u64,
        /// Immutable completed report; None retains display history but invalidates alert use.
        report: Option<Arc<tidemark_live::procscan::ProcReport>>,
    },
    /// Generation-bound process report with daemon completion time for alert freshness.
    ProcscanCompleted {
        /// Configuration observed before collection.
        generation: u64,
        /// Immutable successful report.
        report: Arc<tidemark_live::procscan::ProcReport>,
        /// Injected Unix completion timestamp.
        completed_at: i64,
    },
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
            presence: BTreeMap::new(),
            fs_available: BTreeMap::new(),
            fs_failures: BTreeMap::new(),
            fs_failure_epoch: 0,
            lvm_failures: 0,
            procscan_failures: 0,
            procscan_completed_at: None,
            procscan_available: false,
            alert_revision: 0,
            lvm_available: false,
            lvm: None,
            watchers: BTreeMap::new(),
            procscan: None,
            alerts: None,
            scans: BTreeMap::new(),
            retired_scans: Default::default(),
            pending_scan: None,
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
        let mounts_changed = |old: &[std::path::PathBuf], new: &[std::path::PathBuf]| {
            old.iter().collect::<std::collections::BTreeSet<_>>()
                != new.iter().collect::<std::collections::BTreeSet<_>>()
        };
        let sampler_requirements_changed =
            mounts_changed(&d.config.sampler.mounts, &config.sampler.mounts)
                || mounts_changed(&d.config.watch.mounts, &config.watch.mounts);
        if sampler_requirements_changed {
            check_key(&d.components, &"sampler".to_owned(), MAX_SUBJECTS)?;
        }
        // Remove retired configured roots, retaining an in-flight scan until completion.
        // Reserve slots for every new root before swapping config; inactive manual
        // history is expendable, active work is not.
        let current: std::collections::BTreeSet<String> = config
            .roots
            .iter()
            .map(|r| r.path.to_string_lossy().into_owned())
            .collect();
        let old: std::collections::BTreeSet<String> = d
            .config
            .roots
            .iter()
            .map(|r| r.path.to_string_lossy().into_owned())
            .collect();
        let mut scans = d.scans.clone();
        scans.retain(|name, status| {
            !old.contains(name)
                || current.contains(name)
                || status.running.is_some()
                || d.pending_scan.as_ref() == Some(name)
        });
        let needed = current
            .iter()
            .filter(|name| !scans.contains_key(*name))
            .count();
        while scans.len() + needed > MAX_SUBJECTS {
            let candidate = scans
                .iter()
                .find(|(name, status)| {
                    !current.contains(*name)
                        && status.running.is_none()
                        && d.pending_scan.as_ref() != Some(*name)
                })
                .map(|(name, _)| name.clone());
            let Some(candidate) = candidate else {
                return Err(crate::DaemonError::Config(
                    "scan inventory capacity occupied by active scans".into(),
                ));
            };
            scans.remove(&candidate);
        }
        for root in &current {
            scans.entry(root.clone()).or_insert_with(|| ScanStatus {
                root: root.clone(),
                running: None,
                last_complete: None,
                next_due: None,
            });
        }
        d.retired_scans = scans
            .iter()
            .filter(|(name, status)| {
                old.contains(*name)
                    && !current.contains(*name)
                    && (status.running.is_some() || d.pending_scan.as_ref() == Some(*name))
            })
            .map(|(name, _)| name.clone())
            .chain(
                d.retired_scans
                    .iter()
                    .filter(|name| scans.contains_key(*name) && !current.contains(*name))
                    .cloned(),
            )
            .collect();
        d.scans = scans;
        d.fs.retain(|m, _| {
            config
                .sampler
                .mounts
                .iter()
                .any(|p| p.to_string_lossy() == m.as_str())
        });
        d.fs_failures.retain(|m, _| {
            config
                .sampler
                .mounts
                .iter()
                .any(|p| p.to_string_lossy() == m.as_str())
        });
        d.fs_available.retain(|m, _| {
            config
                .sampler
                .mounts
                .iter()
                .any(|p| p.to_string_lossy() == m.as_str())
        });
        for path in &config.sampler.mounts {
            if !d.config.sampler.mounts.contains(path) {
                interrupt_fs(&mut d, &path.to_string_lossy())?;
            }
        }
        if d.config.lvm != config.lvm || d.config.sampler.mounts != config.sampler.mounts {
            d.lvm_failures = d.lvm_failures.saturating_add(1);
            d.lvm_available = false;
        }
        if d.config.procscan != config.procscan || d.config.watch.mounts != config.watch.mounts {
            d.procscan = None;
            d.procscan_available = false;
            d.procscan_failures = d.procscan_failures.saturating_add(1);
        }
        d.presence.retain(|m, _| {
            config
                .sampler
                .mounts
                .iter()
                .chain(&config.watch.mounts)
                .any(|p| p.to_string_lossy() == m.as_str())
        });
        let old_watch = d.config.watch.clone();
        // Map presentation keys back to their configured raw identity once.
        d.windows.retain(|(mount, window), _| {
            let Some(raw) = old_watch
                .mounts
                .iter()
                .find(|path| RawPath::from_path(path).is_ok_and(|p| p.display_lossy() == *mount))
            else {
                return false;
            };
            config.watch.enabled
                && config.watch.mounts.iter().any(|p| p == raw)
                && crate::services::watcher::same_mount_settings(&old_watch, &config.watch, raw)
                && config.alerts.rule.iter().any(|r| {
                    r.enabled
                        && r.mount.as_ref().is_some_and(|p| p == raw)
                        && r.window
                            .as_deref()
                            .and_then(|w| humantime::parse_duration(w).ok())
                            == Some(*window)
                })
        });
        // Failed startup attempts also have components, even without watcher handles.
        let retired: Vec<_> = d
            .components
            .keys()
            .filter(|name| {
                name.strip_prefix("watcher:").is_some_and(|mount| {
                    !config.watch.mounts.iter().any(|p| {
                        RawPath::from_path(p).is_ok_and(|raw| raw.display_lossy() == mount)
                    })
                })
            })
            .cloned()
            .collect();
        for name in retired {
            d.components.remove(&name);
            d.errors.remove(&name);
        }
        if sampler_requirements_changed {
            health(
                &mut d,
                crate::component(
                    "sampler",
                    "degraded",
                    "waiting for a complete pass under the new configuration",
                ),
            );
        }
        d.config = Arc::new(config);
        d.generation = d.generation.saturating_add(1);
        Ok(())
    }
    /// Atomically record configured presence and discard history across identities.
    pub fn publish_mount_presence(
        &self,
        generation: u64,
        mounts: &[tidemark_fs::mount::MountInfo],
    ) -> Result<(), crate::DaemonError> {
        self.publish_mount_presence_with(
            generation,
            mounts,
            crate::mounts::live_identity,
            |changes| crate::mounts::log(changes, "presence"),
        )
    }
    fn publish_mount_presence_with(
        &self,
        generation: u64,
        mounts: &[tidemark_fs::mount::MountInfo],
        resolve: impl Fn(
            &[tidemark_fs::mount::MountInfo],
            &std::path::Path,
        ) -> Result<Option<crate::mounts::Identity>, crate::DaemonError>,
        log: impl FnOnce(&[crate::mounts::Transition]),
    ) -> Result<(), crate::DaemonError> {
        let (config, captured_generation) = self.config();
        if generation != captured_generation {
            return Ok(());
        }
        let mut paths = config.sampler.mounts.clone();
        paths.extend(config.watch.mounts.clone());
        paths.sort();
        paths.dedup();
        let mut resolved = BTreeMap::new();
        for path in &paths {
            resolved.insert(path.to_string_lossy().into_owned(), resolve(mounts, path)?);
        }
        let mut changes = Vec::new();
        {
            let mut d = self.0.write();
            if generation != d.generation {
                return Ok(());
            }
            for (mount, current) in &resolved {
                if d.presence.get(mount) != Some(current) {
                    let previous = d.presence.get(mount).copied().flatten();
                    if config
                        .sampler
                        .mounts
                        .iter()
                        .any(|p| p.to_string_lossy() == mount.as_str())
                    {
                        interrupt_fs(&mut d, mount)?;
                    }
                    d.fs.remove(mount);
                    d.fs_available.remove(mount);
                    changes.push(crate::mounts::Transition {
                        path: mount.into(),
                        previous,
                        current: *current,
                    });
                }
            }
            d.presence = resolved;
        }
        log(&changes);
        Ok(())
    }
    /// Clear a mount's current samples on a presence/identity transition.
    /// Obsolete passes cannot change the current generation's measurements.
    pub fn reset_fs_mount(&self, generation: u64, mount: &str) -> Result<(), crate::DaemonError> {
        let mut d = self.0.write();
        if generation == d.generation
            && d.config
                .sampler
                .mounts
                .iter()
                .any(|p| p.to_string_lossy() == mount)
        {
            interrupt_fs(&mut d, mount)?;
            d.fs.remove(mount);
            d.fs_available.remove(mount);
        }
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
        self.publish_window_health(result, component, false, None)
    }
    pub(crate) fn publish_window_health(
        &self,
        result: Publication,
        mut component: ComponentState,
        warming: bool,
        observed_config: Option<&Config>,
    ) -> Result<(), crate::DaemonError> {
        let mut d = self.0.write();
        if observed_config
            .is_some_and(|c| c.watch != d.config.watch || c.alerts.rule != d.config.alerts.rule)
        {
            return Ok(());
        }
        check_key(&d.components, &component.name, MAX_SUBJECTS)?;
        let result = match result {
            Publication::FsAttempt {
                generation,
                mount,
                sample,
            } => {
                if generation != d.generation
                    || !d
                        .config
                        .sampler
                        .mounts
                        .iter()
                        .any(|p| p.to_string_lossy() == mount)
                {
                    return Ok(());
                }
                check_key(&d.fs_available, &mount, MAX_SUBJECTS)?;
                if let Some(sample) = sample {
                    if sample.mount != mount {
                        return Err(crate::DaemonError::Invalid(
                            "sample mount does not match attempt".into(),
                        ));
                    }
                    Publication::Fs(sample)
                } else {
                    interrupt_fs(&mut d, &mount)?;
                    d.fs_available.insert(mount, false);
                    health(&mut d, component);
                    return Ok(());
                }
            }
            Publication::LvmAttempt { generation, report } => {
                if generation != d.generation {
                    return Ok(());
                }
                if let Some(report) = report {
                    Publication::Lvm(report)
                } else {
                    d.lvm_failures = d.lvm_failures.saturating_add(1);
                    d.lvm_available = false;
                    d.alert_revision = d.alert_revision.saturating_add(1);
                    health(&mut d, component);
                    return Ok(());
                }
            }
            other => other,
        };
        match result {
            Publication::FsAttempt { .. } | Publication::LvmAttempt { .. } => {
                unreachable!("attempts normalized above")
            }
            Publication::Fs(s) => {
                check_key(&d.fs, &s.mount, MAX_SUBJECTS)?;
                check_key(&d.fs_available, &s.mount, MAX_SUBJECTS)?;
                if d.fs
                    .get(&s.mount)
                    .and_then(|rows| rows.back())
                    .is_some_and(|last| {
                        s.ts >= last.ts && !fresh(last.ts, s.ts, d.config.sampler.interval)
                    })
                {
                    interrupt_fs(&mut d, &s.mount)?;
                }
                d.fs_available.insert(s.mount.clone(), true);
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
                if d.lvm.as_ref().is_some_and(|last| {
                    r.sampled_at >= last.sampled_at
                        && !fresh(last.sampled_at, r.sampled_at, d.config.lvm.interval)
                }) {
                    d.lvm_failures = d.lvm_failures.saturating_add(1);
                }
                d.lvm_available = true;
                d.lvm = Some(r);
                d.alert_revision = d.alert_revision.saturating_add(1);
            }
            Publication::Watcher((mount, w)) => {
                check_key(&d.watchers, &mount, MAX_SUBJECTS)?;
                d.watchers.insert(mount, w);
                d.alert_revision = d.alert_revision.saturating_add(1);
            }
            Publication::Procscan(p) => complete_procscan(&mut d, p, crate::unix_now()),
            Publication::ProcscanCompleted {
                generation,
                report,
                completed_at,
            } => {
                if generation != d.generation {
                    return Ok(());
                }
                complete_procscan(&mut d, report, completed_at);
            }
            Publication::ProcscanAttempt { generation, report } => {
                if generation != d.generation {
                    return Ok(());
                }
                if let Some(report) = report {
                    complete_procscan(&mut d, report, crate::unix_now());
                } else {
                    d.procscan_available = false;
                    d.procscan_failures = d.procscan_failures.saturating_add(1);
                    d.alert_revision = d.alert_revision.saturating_add(1);
                }
            }
            Publication::Alerts((mut a, mut diagnostics)) => {
                let internal = a
                    .iter()
                    .filter(|a| a.rule == "state_space" && a.subject == "state")
                    .count()
                    .min(1);
                check_len(
                    a.len().saturating_sub(internal),
                    tidemark_live::alerts::MAX_ALERT_SUBJECTS,
                )?;
                check_len(diagnostics.len(), MAX_INVENTORY)?;
                a.sort_by(|a, b| (&a.rule, &a.subject).cmp(&(&b.rule, &b.subject)));
                diagnostics.sort_by(|a, b| a.rule.cmp(&b.rule));
                d.alerts = Some((a, diagnostics));
            }
            Publication::Scan(s) => {
                if s.running.is_none() && d.retired_scans.remove(&s.root) {
                    d.scans.remove(&s.root);
                } else {
                    check_key(&d.scans, &s.root, MAX_SUBJECTS)?;
                    d.scans.insert(s.root.clone(), s);
                }
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
                let hot = hot.and_then(|rows| {
                    let retained = match d.watchers.get(&key.0) {
                        Some(w) => w.retain_window_rows(rows),
                        None => tidemark_live::watch::WatcherRows::from_unwatched_rows(rows),
                    };
                    if retained.is_none() {
                        component.state = "degraded".into();
                        component
                            .detail
                            .push_str("; truncated: key bytes over the memory budget");
                        if let Some(w) = d.watchers.get(&key.0) {
                            w.key_memory_budget_exceeded();
                        }
                    }
                    retained
                });
                d.windows.insert(
                    key,
                    WindowFacts {
                        warming,
                        at: measured_at,
                        hot,
                        overflows,
                    },
                );
                d.alert_revision = d.alert_revision.saturating_add(1);
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
    pub(crate) fn reserve_scan(&self, root: String) -> Result<(), crate::DaemonError> {
        let mut d = self.0.write();
        check_key(&d.scans, &root, MAX_SUBJECTS)?;
        d.scans.entry(root.clone()).or_insert_with(|| ScanStatus {
            root: root.clone(),
            running: None,
            last_complete: None,
            next_due: None,
        });
        d.pending_scan = Some(root);
        Ok(())
    }
    pub(crate) fn reserve_scheduled(
        &self,
        root: String,
        generation: u64,
    ) -> Result<bool, crate::DaemonError> {
        let mut d = self.0.write();
        if d.generation != generation {
            return Ok(false);
        }
        check_key(&d.scans, &root, MAX_SUBJECTS)?;
        d.scans.entry(root.clone()).or_insert_with(|| ScanStatus {
            root: root.clone(),
            running: None,
            last_complete: None,
            next_due: None,
        });
        d.pending_scan = Some(root);
        Ok(true)
    }
    pub(crate) fn release_scan(&self) {
        let mut d = self.0.write();
        if let Some(root) = d.pending_scan.take() {
            if d.retired_scans.remove(&root) {
                d.scans.remove(&root);
            }
        }
    }
    pub(crate) fn publish_scheduled(
        &self,
        status: ScanStatus,
        generation: u64,
        component: ComponentState,
    ) -> Result<(), crate::DaemonError> {
        let mut d = self.0.write();
        if d.generation != generation {
            return Ok(());
        }
        check_key(&d.components, &component.name, MAX_SUBJECTS)?;
        check_key(&d.scans, &status.root, MAX_SUBJECTS)?;
        d.scans.insert(status.root.clone(), status);
        health(&mut d, component);
        Ok(())
    }
    /// Remove a watcher publication before restart/shutdown; the final Arc drop stops its worker.
    pub fn remove_watcher(&self, mount: &str) {
        let mut d = self.0.write();
        d.watchers.remove(mount);
        d.windows.retain(|(m, _), _| m != mount);
        d.alert_revision = d.alert_revision.saturating_add(1);
    }
    /// Release every watcher publication during ordered shutdown.
    pub fn clear_watchers(&self) {
        let mut d = self.0.write();
        d.watchers.clear();
        d.windows.clear();
        d.alert_revision = d.alert_revision.saturating_add(1);
    }
    /// Remove retired component health and its error history, releasing bounded slots.
    pub fn remove_component(&self, name: &str) {
        let mut d = self.0.write();
        d.components.remove(name);
        d.errors.remove(name);
    }
    pub(crate) fn alert_revision(&self) -> u64 {
        self.0.read().alert_revision
    }
    /// Publish health only if the pass still belongs to the effective configuration.
    pub fn publish_component_for_generation(
        &self,
        generation: u64,
        component: ComponentState,
    ) -> Result<(), crate::DaemonError> {
        let mut d = self.0.write();
        if d.generation != generation {
            return Ok(());
        }
        check_key(&d.components, &component.name, MAX_SUBJECTS)?;
        health(&mut d, component);
        Ok(())
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
    // Reserve the 17 non-mount names used by lifecycle, services and post-scan:
    // daemon, catalog, state_space, config, sampler, lvm, watcher, procscan,
    // alerts, scheduler, metrics, api, tswriter, growth, snapshot_metrics,
    // post_scan, firstboot (reserved for firstboot diagnostics).
    check_len(c.watch.mounts.len().saturating_add(17), MAX_SUBJECTS)?;
    let windows: std::collections::BTreeSet<_> = c
        .alerts
        .rule
        .iter()
        .filter(|r| {
            r.enabled
                && matches!(
                    r.kind,
                    tidemark_core::config::AlertKind::HotDirGrowth
                        | tidemark_core::config::AlertKind::WatchLossy
                )
        })
        .filter_map(|r| {
            Some((
                r.mount.clone()?,
                humantime::parse_duration(r.window.as_deref()?).ok()?,
            ))
        })
        .filter(|(_, w)| crate::services::alerts::minute_window(*w))
        .collect();
    check_len(windows.len(), MAX_WINDOWS)?;
    check_len(c.alerts.rule.len(), MAX_INVENTORY)
}
fn watch_start_label(start: i64) -> String {
    format!(
        "{:02}:{:02} UTC",
        start.div_euclid(3600).rem_euclid(24),
        start.div_euclid(60).rem_euclid(60)
    )
}

impl LiveStateView {
    /// Timestamp of a supported exact watcher observation usable at this view's clock.
    /// Completed-minute observations remain usable for 0 through 90 seconds inclusive;
    /// future observations and other durations are unavailable, never rounded.
    pub fn watch_window_at(&self, mount: &str, window: Duration) -> Option<i64> {
        if !crate::services::alerts::minute_window(window) {
            return None;
        }
        let facts = self.data.windows.get(&(mount.into(), window))?;
        (facts.at <= self.now && i128::from(self.now) - i128::from(facts.at) <= 90)
            .then_some(facts.at)
    }
    /// Whether missing watcher facts can be attributed solely to initial collection.
    /// Startup's partial minute is excluded, so n full minutes fill at the next
    /// minute boundary plus n minutes. Known unavailable facts are never warmup.
    pub(crate) fn watch_warming_up(&self, mount: &str, window: Duration) -> bool {
        if !self.data.config.watch.enabled || !crate::services::alerts::minute_window(window) {
            return false;
        }
        let running = self.data.watchers.get(mount).is_some_and(|w| {
            let stats = w.stats();
            stats.mode != tidemark_live::watch::WatchMode::Failed
                && !stats.recovering
                && stats.overflows_total == 0
                && stats.lossy_minutes_15m == 0
        });
        // Compute the next boundary in the wider domain, including extreme injected clocks.
        let first_boundary = i128::from(self.data.started).div_euclid(60) * 60 + 60;
        let elapsed = i128::from(self.now) - first_boundary;
        running
            && elapsed >= -60
            && elapsed < i128::from(window.as_secs())
            && self
                .data
                .windows
                .get(&(mount.into(), window))
                .is_none_or(|f| f.warming && self.watch_window_at(mount, window).is_some())
    }
    pub(crate) fn rate_warming_up(&self, mount: &str, window: Duration) -> bool {
        !window.is_zero()
            && !self.rate_ready(mount)
            && self.fs_sample(mount).is_some()
            && self.fs_rate(mount, window).is_none()
    }
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
        let hot: Vec<_> = v
            .data
            .watchers
            .iter()
            .filter(|(_, w)| w.mode() != tidemark_live::watch::WatchMode::Failed)
            .filter_map(|(m, w)| {
                w.query_at(now, 5, tidemark_live::watch::HotScope::Dir, MAX_INVENTORY)
                    .ok()
                    .map(|(_, rows)| (m.clone(), rows))
            })
            .collect();
        let unit_hot: Option<Vec<_>> = if v.data.watchers.is_empty() {
            None
        } else {
            Some(
                v.data
                    .watchers
                    .iter()
                    .filter_map(|(m, w)| {
                        w.query_at(now, 5, tidemark_live::watch::HotScope::Unit, MAX_INVENTORY)
                            .ok()
                            .map(|(_, rows)| (m.clone(), rows))
                    })
                    .collect(),
            )
        };
        let mut degraded = v.data.degraded.clone();
        for (mount, watcher) in &v.data.watchers {
            if p.mount.as_ref().is_none_or(|m| m == mount) {
                if let Some(at) = watcher.retired_at() {
                    degraded.push(format!(
                        "watcher for {mount} was replaced at {}",
                        watch_start_label(at)
                    ));
                    continue;
                }
            }
            if p.mount.as_ref().is_none_or(|m| m == mount) && !watcher.history_covers_at(now, 60) {
                if let Some(start) = watcher.history_start() {
                    degraded.push(format!(
                        "watcher {mount}: history starts at {}",
                        watch_start_label(start)
                    ));
                }
            }
        }
        for (mount, identity) in &v.data.presence {
            if identity.is_none() && p.mount.as_ref().is_none_or(|m| m == mount) {
                degraded.push(format!("mount not present: {mount}"));
            }
        }
        for mount in rates.as_deref().unwrap_or(&[]) {
            if !v.rate_ready(&mount.mount) {
                degraded.push(format!("rate: collecting {}", mount.mount));
            }
        }
        for mount in rates.as_deref().unwrap_or(&[]) {
            if !hot.iter().any(|(m, _)| m == &mount.mount) {
                degraded.push(format!("hot_dirs_5m {}: not measured", mount.mount));
            }
        }
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
        let mut diagnostics = v.alert_diagnostics().unwrap_or_default();
        diagnostics.retain(|d| {
            if d.reason.starts_with("warming up") {
                degraded.push(format!("alert {}: {}", d.rule, d.reason));
                false
            } else {
                true
            }
        });
        Some(build_now(
            &NowInputs {
                host: rustix::system::uname()
                    .nodename()
                    .to_str()
                    .unwrap_or("unknown"),
                mounts: rates.as_deref(),
                lvm: v.data.lvm.as_ref().map(|r| r.mounts.as_slice()),
                hot_dirs_5m: Some(&hot),
                writers_5m: unit_hot.as_deref(),
                phantom: phantom.as_deref(),
                open_writers: grouped.as_deref(),
                alerts: alerts.as_deref(),
                scans: scans.as_deref(),
                watch_modes: modes.as_deref(),
                unevaluable: &diagnostics,
                degraded: &degraded,
            },
            now,
        ))
    }
    fn rate_ready(&self, mount: &str) -> bool {
        let Some(rows) = self.data.fs.get(mount) else {
            return false;
        };
        let mut samples = rows.iter().filter(|s| s.ts <= self.now);
        let Some(first) = samples.next() else {
            return false;
        };
        samples
            .next_back()
            .is_some_and(|last| last.ts.saturating_sub(first.ts) >= 300)
    }
    /// Explain a requested mount outside the daemon's effective watch configuration.
    pub fn hot_unwatched_detail(&self, p: &HotParams) -> Option<String> {
        let mount = p.mount.as_ref()?;
        if self
            .data
            .config
            .watch
            .mounts
            .iter()
            .any(|path| path.as_path() == std::path::Path::new(mount))
            && self.data.presence.get(mount) != Some(&None)
        {
            return None;
        }
        let mounts: Vec<_> = self
            .data
            .config
            .watch
            .mounts
            .iter()
            .map(|path| {
                tidemark_core::path::display_lossy_bytes(path.as_os_str().as_encoded_bytes())
            })
            .collect();
        Some(format!("{mount} is not watched (watch.mounts = {mounts:?}); add it to [watch].mounts and run: tidemark deploy stage --from <file>"))
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
        let mut incomplete = false;
        for (mount, w) in &self.data.watchers {
            if p.mount.as_ref().is_none_or(|m| m == mount) {
                let (covered, entries) = w
                    .query_at(self.now, minutes, scope, p.limit.min(MAX_INVENTORY))
                    .ok()?;
                incomplete |= !covered;
                rows.extend(entries);
            }
        }
        if incomplete {
            if rows.is_empty() {
                return None;
            }
            for row in &mut rows {
                row.lossy = true;
            }
        }
        rows.sort_by(|a, b| b.bytes_delta.cmp(&a.bytes_delta).then(a.key.cmp(&b.key)));
        rows.truncate(p.limit.min(MAX_INVENTORY));
        Some(rows)
    }
    /// Explain why a hot window cannot be answered before watcher startup.
    pub fn hot_unavailable_detail(&self, p: &HotParams) -> String {
        let minutes = match p.window {
            HotWindow::OneMinute => 1,
            HotWindow::FiveMinutes => 5,
            HotWindow::FifteenMinutes => 15,
            HotWindow::SixtyMinutes => 60,
        };
        for (mount, watcher) in &self.data.watchers {
            if p.mount.as_ref().is_none_or(|m| m == mount) {
                if let Some(at) = watcher.retired_at() {
                    return format!(
                        "watcher for {mount} was replaced at {}",
                        watch_start_label(at)
                    );
                }
            }
        }
        for (mount, watcher) in &self.data.watchers {
            if p.mount.as_ref().is_none_or(|m| m == mount)
                && !watcher.history_covers_at(self.now, minutes)
            {
                if let Some(start) = watcher.history_start() {
                    let available = ((self.now.div_euclid(60) * 60 - start) / 60 + 1).clamp(0, 60);
                    if available == 0 {
                        return format!(
                            "watch history for {mount} starts at {}; no complete window yet",
                            watch_start_label(start)
                        );
                    }
                    return format!("watch history for {mount} starts at {}; ask for a window of at most {available} {}", watch_start_label(start), if available == 1 { "minute" } else { "minutes" });
                }
            }
        }
        "measurement unavailable".into()
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
        Some(self.data.procscan.as_ref()?.phantom(raw.as_ref()))
    }
    pub(crate) fn process_unavailable_detail(&self) -> String {
        if !self.data.config.procscan.enabled {
            return "process scan not configured: disabled by config".into();
        }
        if let Some(component) = self.data.components.get("procscan") {
            if matches!(component.state.as_str(), "error" | "degraded")
                && !component.detail.contains("awaiting completed scan")
            {
                return if component.detail.starts_with("process scan failed:")
                    || component
                        .detail
                        .starts_with("process inventory over its memory budget")
                {
                    component.detail.clone()
                } else {
                    format!("process scan failed: {}", component.detail)
                };
            }
        }
        "process scan has not completed yet".into()
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
    pub(crate) fn collector_failures(&self) -> (BTreeMap<String, u64>, u64) {
        (self.data.fs_failures.clone(), self.data.lvm_failures)
    }
    pub(crate) fn procscan_failure_epoch(&self) -> u64 {
        self.data.procscan_failures
    }
    /// Known degradation reasons, capped at MAX_SUBJECTS.
    pub fn degraded(&self) -> Option<Vec<String>> {
        Some(self.data.degraded.clone())
    }
}
fn complete_procscan(d: &mut Inner, report: Arc<tidemark_live::procscan::ProcReport>, now: i64) {
    if d.procscan_completed_at
        .is_some_and(|last| !fresh(last, now, d.config.procscan.interval))
    {
        d.procscan_failures = d.procscan_failures.saturating_add(1);
    }
    d.procscan = Some(report);
    d.procscan_completed_at = Some(now);
    d.procscan_available = true;
    d.alert_revision = d.alert_revision.saturating_add(1);
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
        if self.data.fs_available.get(mount) != Some(&true) {
            return None;
        }
        let sample = self.data.fs.get(mount)?.back()?;
        if !fresh(sample.ts, self.now, self.data.config.sampler.interval) {
            return None;
        }
        self.data
            .fs
            .get(mount)?
            .iter()
            .rev()
            .find(|s| s.ts <= self.now)
            .cloned()
    }
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64> {
        if window.is_zero() || !self.rate_ready(mount) || self.fs_sample(mount).is_none() {
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
        self.watch_window_at(mount, window)?;
        f.hot.as_ref()?.copy_rows()
    }
    fn watch_overflows(&self, mount: &str, window: Duration) -> Option<u64> {
        let f = self.data.windows.get(&(mount.into(), window))?;
        self.watch_window_at(mount, window)?;
        f.overflows
    }
    fn phantom_bytes(&self, mount: &str) -> Option<u64> {
        if !self.data.procscan_available
            || !self.data.config.procscan.enabled
            || !fresh(
                self.data.procscan_completed_at?,
                self.now,
                self.data.config.procscan.interval,
            )
        {
            return None;
        }
        self.phantom(&PhantomParams {
            mount: Some(mount.into()),
        })?
        .iter()
        .find(|r| r.mount == mount)
        .map(|r| r.total_bytes)
    }
    fn lvm(&self) -> Option<Vec<LvmNow>> {
        if !self.data.lvm_available
            || !self.data.config.lvm.enabled
            || !fresh(
                self.data.lvm.as_ref()?.sampled_at,
                self.now,
                self.data.config.lvm.interval,
            )
        {
            return None;
        }
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
        WatchMode::FdFilesystem => "fd_filesystem",
        WatchMode::DfidNameMount => "dfid_name_mount",
        WatchMode::DfidNameFilesystem => "dfid_name_filesystem",
        WatchMode::Failed => "failed",
    }
}

fn fresh(at: i64, now: i64, interval: Duration) -> bool {
    let age = i128::from(now) - i128::from(at);
    age >= 0
        && age * 1_000_000_000
            <= (interval.as_nanos().saturating_mul(3).min(i128::MAX as u128) as i128)
}

fn interrupt_fs(d: &mut Inner, mount: &str) -> Result<(), crate::DaemonError> {
    check_key(&d.fs_failures, &mount.to_owned(), MAX_SUBJECTS)?;
    d.fs_failure_epoch = d.fs_failure_epoch.saturating_add(1);
    d.fs_failures.insert(mount.into(), d.fs_failure_epoch);
    Ok(())
}

#[cfg(test)]
mod presence_lock_tests {
    #[test]
    fn reload_health_tracks_mount_sets_and_rejects_stale_passes() {
        let mut cfg = tidemark_core::config::Config::default();
        let state = super::LiveState::new(&cfg, 100);
        state
            .publish_component(crate::component("sampler", "ok", "fixture"))
            .unwrap();
        let old = state.config().1;
        cfg.sampler.interval = std::time::Duration::from_secs(42);
        cfg.sampler.mounts.reverse();
        state.replace_config(cfg.clone()).unwrap();
        let health = || {
            state
                .view_at(100)
                .status(100)
                .unwrap()
                .components
                .into_iter()
                .find(|c| c.name == "sampler")
                .unwrap()
        };
        assert_eq!(health().state, "ok");
        cfg.watch.mounts.push("/new-watch".into());
        state.replace_config(cfg).unwrap();
        assert_eq!(health().state, "degraded");
        assert_eq!(
            health().detail,
            "waiting for a complete pass under the new configuration"
        );
        state
            .publish_component_for_generation(old, crate::component("sampler", "ok", "stale"))
            .unwrap();
        assert_eq!(health().state, "degraded");
        state
            .publish_component_for_generation(
                state.config().1,
                crate::component("sampler", "ok", "current"),
            )
            .unwrap();
        assert_eq!(health().state, "ok");
    }

    use super::*;
    #[test]
    fn filesystem_watch_mode_state_string() {
        assert_eq!(
            watch_mode(tidemark_live::watch::WatchMode::FdFilesystem),
            "fd_filesystem"
        );
        assert_eq!(
            watch_mode(tidemark_live::watch::WatchMode::FdMount),
            "fd_mount"
        );
    }
    #[test]
    fn now_rates_are_missing_until_five_minutes_for_every_window() {
        let mut config = Config::default();
        config.sampler.mounts = vec!["/fixture".into()];
        let state = LiveState::new(&config, 0);
        for ts in [0, 33, 66, 270, 299, 300] {
            state
                .publish(
                    Publication::Fs(FsSample {
                        ts,
                        mount: "/fixture".into(),
                        size: 10_000,
                        used: ts as u64,
                        avail: 10_000 - ts as u64,
                        inodes: 100,
                        inodes_used: 1,
                    }),
                    crate::component("sampler", "ok", "sampled"),
                )
                .unwrap();
            let view = state.view_at(ts);
            let report = view.now(&NowParams { mount: None }, ts).unwrap();
            if ts < 300 {
                assert!(report.mounts[0].rate_15m.is_none());
                assert!(report.mounts[0].rate_1h.is_none());
                assert!(report.mounts[0].ttf_secs.is_none());
                for window in [60, 300, 900, 3600] {
                    assert!(view
                        .fs_rate("/fixture", Duration::from_secs(window))
                        .is_none());
                }
            } else {
                assert_eq!(report.mounts[0].rate_15m, Some(1.0));
                assert_eq!(report.mounts[0].rate_1h, Some(1.0));
                for window in [60, 300, 900, 3600] {
                    assert_eq!(
                        view.fs_rate("/fixture", Duration::from_secs(window)),
                        Some(1.0)
                    );
                }
            }
        }
    }
    #[test]
    fn blocked_resolution_and_logging_leave_readers_free_and_reject_stale_generation() {
        for phase in [0, 1, 2] {
            let mut config = Config::default();
            config.sampler.mounts = vec!["/".into()];
            config.watch.mounts.clear();
            let state = LiveState::new(&config, 0);
            let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
            let (release_tx, release_rx) = crossbeam_channel::bounded(1);
            let worker_state = state.clone();
            let worker = std::thread::spawn(move || {
                worker_state
                    .publish_mount_presence_with(
                        0,
                        &[],
                        |_, _| {
                            if phase != 1 {
                                entered_tx.send(()).unwrap();
                                release_rx.recv().unwrap();
                            }
                            Ok(Some(crate::mounts::Identity {
                                id: 1,
                                major: 8,
                                minor: 0,
                            }))
                        },
                        |_| {
                            if phase == 1 {
                                entered_tx.send(()).unwrap();
                                release_rx.recv().unwrap();
                            }
                        },
                    )
                    .unwrap();
            });
            let entered = entered_rx.recv_timeout(Duration::from_secs(10));
            let (read_tx, read_rx) = crossbeam_channel::bounded(1);
            let reader_state = state.clone();
            let reader = std::thread::spawn(move || {
                read_tx.send(reader_state.view_at(0).status(0)).unwrap();
            });
            let read = read_rx.recv_timeout(Duration::from_secs(10));
            let (reload, reload_worker) = if phase == 2 {
                let reload_state = state.clone();
                let (reload_tx, reload_rx) = crossbeam_channel::bounded(1);
                let reload_worker = std::thread::spawn(move || {
                    let _ = reload_tx.send(reload_state.replace_config(config));
                });
                let progress = reload_rx.recv_timeout(Duration::from_secs(10));
                (Some(progress), Some(reload_worker))
            } else {
                (None, None)
            };
            // Even a regressed publisher holding the global lock can now finish:
            // no assertion or synchronous reload can retain its release signal.
            let release = release_tx.send(());
            let published = worker.join();
            let read_joined = reader.join();
            let reload_joined = reload_worker.map(std::thread::JoinHandle::join);
            assert!(
                entered.is_ok(),
                "publisher did not enter during phase {phase}"
            );
            assert!(release.is_ok());
            assert!(published.is_ok() && read_joined.is_ok());
            assert!(reload_joined.is_none_or(|result| result.is_ok()));
            assert!(read.is_ok(), "reader blocked during phase {phase}");
            if phase == 2 {
                assert!(
                    matches!(reload, Some(Ok(Ok(())))),
                    "reload blocked during phase 2"
                );
            }
            assert_eq!(state.0.read().presence.is_empty(), phase == 2);
        }
    }
}
