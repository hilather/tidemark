//! Owned by: W6-B
//! Bounded snapshot metrics and read-only Prometheus exporters.
use parking_lot::RwLock;
use std::{
    collections::BTreeMap,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tidemark_core::{api::*, config::MetricsConfig, scan::SnapshotStatus, snapshot::DirRow};
use tidemark_live::alerts::AlertSource;

/// Default candidate capacity per family; reconciled to the configured series cap.
const DEFAULT_CANDIDATES: usize = 500;
const MAX_ROOTS: usize = 128;
#[derive(Default)]
struct CachedRoot {
    rows: Vec<DirRow>,
    reclaimable: Vec<DirRow>,
    eligible: u64,
    source: Option<(std::path::PathBuf, u32)>,
    errors: u64,
}
/// Latest complete snapshot data, replaced atomically per root after a scan.
pub struct SnapshotMetrics {
    roots: RwLock<BTreeMap<Vec<u8>, CachedRoot>>,
    capacity: std::sync::atomic::AtomicUsize,
}
impl Default for SnapshotMetrics {
    fn default() -> Self {
        Self {
            roots: RwLock::new(BTreeMap::new()),
            capacity: std::sync::atomic::AtomicUsize::new(DEFAULT_CANDIDATES),
        }
    }
}
impl SnapshotMetrics {
    /// Refresh bounded directory aggregates from an immutable complete snapshot.
    pub fn refresh(
        &self,
        snapshot: &tidemark_store::reader::Snapshot,
        depth: u32,
    ) -> Result<(), crate::DaemonError> {
        if snapshot.meta().status != SnapshotStatus::Complete {
            return Err(crate::DaemonError::Invalid(
                "metrics requires a complete snapshot".into(),
            ));
        }
        let key = snapshot.meta().root.as_bytes().to_vec();
        let mut roots = self.roots.write();
        let capacity = self.capacity.load(Ordering::Acquire);
        if !roots.contains_key(&key) && roots.len() >= MAX_ROOTS {
            return Err(crate::DaemonError::Unavailable(
                "metrics root capacity exceeded".into(),
            ));
        }
        // Requery immutable sources so replacing/removing a root can uncover candidates
        // previously below the global cutoff. Build a replacement before publishing.
        let mut replacement = BTreeMap::new();
        let mut used = Vec::new();
        let mut reclaimable = Vec::new();
        for root in roots
            .keys()
            .filter(|root| **root != key)
            .chain(std::iter::once(&key))
        {
            let opened;
            let (reader, depth) = if *root == key {
                (snapshot, depth)
            } else {
                let Some((file, depth)) = roots[root].source.as_ref() else {
                    continue;
                };
                opened = tidemark_store::reader::Snapshot::open(file).map_err(store)?;
                (&opened, *depth)
            };
            if reader.meta().status != SnapshotStatus::Complete
                || reader.meta().root.as_bytes() != root.as_slice()
            {
                return Err(crate::DaemonError::Store(
                    "metrics source must match its complete catalog root".into(),
                ));
            }
            let (u, r, eligible, file) =
                reader.metrics_candidates(depth, capacity).map_err(store)?;
            replacement.insert(
                root.clone(),
                CachedRoot {
                    eligible,
                    source: Some((file, depth)),
                    errors: reader.meta().errors,
                    ..CachedRoot::default()
                },
            );
            used.extend(u.into_iter().map(|row| (root.clone(), row)));
            reclaimable.extend(r.into_iter().map(|row| (root.clone(), row)));
            // Each merge holds at most twice the configured cap per family;
            // retained candidates are bounded globally, independently per family.
            used.sort_by(|a, b| {
                b.1.t_disk
                    .cmp(&a.1.t_disk)
                    .then(a.0.cmp(&b.0))
                    .then(a.1.path.as_bytes().cmp(b.1.path.as_bytes()))
            });
            reclaimable.sort_by(|a, b| {
                b.1.t_reclaimable
                    .cmp(&a.1.t_reclaimable)
                    .then(a.0.cmp(&b.0))
                    .then(a.1.path.as_bytes().cmp(b.1.path.as_bytes()))
            });
            used.truncate(capacity);
            reclaimable.truncate(capacity);
        }
        for (root, row) in used {
            if let Some(cache) = replacement.get_mut(&root) {
                cache.rows.push(row);
            }
        }
        for (root, row) in reclaimable {
            if let Some(cache) = replacement.get_mut(&root) {
                cache.reclaimable.push(row);
            }
        }
        *roots = replacement;
        Ok(())
    }
}
fn reconcile(
    state: &crate::state::LiveState,
    config: &tidemark_core::config::Config,
) -> Result<(), crate::DaemonError> {
    if config.roots.len() > MAX_ROOTS {
        return Err(crate::DaemonError::Unavailable(
            "metrics root capacity exceeded".into(),
        ));
    }
    let cache = state.view().snapshot_metrics_handle();
    use tidemark_core::path::RawPath;
    let mut sources = Vec::with_capacity(config.roots.len());
    if !config.roots.is_empty() {
        let catalog =
            tidemark_store::catalog::Catalog::open(&state.storage_path(), false).map_err(store)?;
        for root in &config.roots {
            let raw = RawPath::from_path(&root.path)
                .map_err(|e| crate::DaemonError::Invalid(e.to_string()))?;
            if let Some(entry) = catalog.latest(&raw).map_err(store)? {
                if let Some(file) = entry.file {
                    sources.push((raw, file));
                }
            }
        }
    }
    if let Some(cache) = &cache {
        let roots = cache.roots.read();
        if cache.capacity.load(Ordering::Acquire) == config.metrics.max_series_per_metric
            && roots.len() == sources.len()
            && sources.iter().all(|(root, file)| {
                roots.get(root.as_bytes()).is_some_and(|r| {
                    r.source.as_ref().is_some_and(|(cached_file, depth)| {
                        cached_file == file && *depth == config.metrics.dir_metrics_depth
                    })
                })
            })
        {
            return Ok(());
        }
    }
    let fresh = SnapshotMetrics::default();
    fresh
        .capacity
        .store(config.metrics.max_series_per_metric, Ordering::Release);
    if let Some((root, file)) = sources.pop() {
        // Seed source identities first, then rank every root once. Repeated refresh
        // calls here would reread all previously loaded inventories quadratically.
        for (root, file) in sources {
            fresh.roots.write().insert(
                root.as_bytes().to_vec(),
                CachedRoot {
                    source: Some((file, config.metrics.dir_metrics_depth)),
                    ..CachedRoot::default()
                },
            );
        }
        let snapshot = tidemark_store::reader::Snapshot::open(&file).map_err(store)?;
        if snapshot.meta().root != root {
            return Err(crate::DaemonError::Store(
                "metrics snapshot root disagrees with catalog".into(),
            ));
        }
        fresh.refresh(&snapshot, config.metrics.dir_metrics_depth)?;
    }
    if let Some(cache) = cache {
        let mut roots = cache.roots.write();
        *roots = fresh.roots.into_inner();
        cache
            .capacity
            .store(config.metrics.max_series_per_metric, Ordering::Release);
    } else {
        state.publish(
            crate::state::Publication::SnapshotMetrics(fresh),
            crate::component("snapshot_metrics", "ok", "latest complete snapshots loaded"),
        )?;
    }
    Ok(())
}
fn store(e: tidemark_store::StoreError) -> crate::DaemonError {
    crate::DaemonError::Store(e.to_string())
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
fn labels(pairs: &[(&str, &str)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    format!(
        "{{{}}}",
        pairs
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", escape(v)))
            .collect::<Vec<_>>()
            .join(",")
    )
}
#[derive(Default)]
struct Series(
    BTreeMap<&'static str, BTreeMap<String, f64>>,
    BTreeMap<&'static str, u64>,
);
impl Series {
    fn add(&mut self, name: &'static str, pairs: &[(&str, &str)], value: f64) {
        if value.is_finite() {
            self.0
                .entry(name)
                .or_default()
                .entry(labels(pairs))
                .and_modify(|v| *v = v.max(value))
                .or_insert(value);
        }
    }
    fn finish(mut self, cap: usize) -> String {
        for name in self.1.keys() {
            self.0.entry(name).or_default();
        }
        let mut output = String::new();
        let mut dropped = Vec::new();
        for (name, rows) in self.0 {
            let count = if let Some(total) = self.1.get(name) {
                total.saturating_sub(rows.len().min(cap) as u64)
            } else {
                rows.len().saturating_sub(cap) as u64
            };
            if count > 0 {
                dropped.push((name, count));
            }
            let mut rows: Vec<_> = rows.into_iter().collect();
            rows.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            rows.truncate(cap);
            rows.sort_by(|a, b| a.0.cmp(&b.0));
            header(&mut output, name);
            for (label, value) in rows {
                output.push_str(&format!("{name}{label} {value}\n"));
            }
        }
        header(&mut output, "tidemark_metrics_dropped_series");
        dropped.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        dropped.truncate(cap);
        dropped.sort_by_key(|r| r.0);
        for (name, count) in dropped {
            output.push_str(&format!(
                "tidemark_metrics_dropped_series{} {count}\n",
                labels(&[("metric", name)])
            ));
        }
        output
    }
}
fn header(out: &mut String, name: &str) {
    let kind = if matches!(
        name,
        "tidemark_watch_events_total" | "tidemark_watch_overflows_total"
    ) {
        "counter"
    } else {
        "gauge"
    };
    out.push_str(&format!(
        "# HELP {name} Tidemark {}.\n# TYPE {name} {kind}\n",
        name.trim_start_matches("tidemark_").replace('_', " ")
    ));
}
// Read-only adapter keeps rendering testable without constructing fanotify or
// attributing processes on the test host. Production reads the frozen view.
trait MetricsSource {
    fn config(&self) -> (Arc<tidemark_core::config::Config>, u64);
    fn fs_samples(&self) -> Option<Vec<tidemark_core::live::FsSample>>;
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64>;
    fn hot_dirs(&self, mount: &str, window: Duration)
        -> Option<Vec<tidemark_core::live::HotEntry>>;
    fn lvm_report(&self, p: &LvmParams) -> Option<LvmResult>;
    fn hot(&self, p: &HotParams) -> Option<Vec<tidemark_core::live::HotEntry>>;
    fn phantom(&self, p: &PhantomParams) -> Option<Vec<tidemark_core::live::PhantomSummary>>;
    fn writers(&self, p: &WritersParams) -> Option<Vec<tidemark_core::live::OpenWriter>>;
    fn scans(&self) -> Option<Vec<tidemark_core::live::ScanStatus>>;
    fn watch_stats(&self) -> Option<Vec<(String, tidemark_live::watch::WatchStats)>>;
    fn active_alerts(&self) -> Option<Vec<tidemark_core::live::AlertSummary>>;
}
impl MetricsSource for crate::state::LiveStateView {
    fn config(&self) -> (Arc<tidemark_core::config::Config>, u64) {
        self.config()
    }
    fn fs_samples(&self) -> Option<Vec<tidemark_core::live::FsSample>> {
        self.fs_samples()
    }
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64> {
        AlertSource::fs_rate(self, mount, window)
    }
    fn hot_dirs(
        &self,
        mount: &str,
        window: Duration,
    ) -> Option<Vec<tidemark_core::live::HotEntry>> {
        AlertSource::hot_dirs(self, mount, window)
    }
    fn lvm_report(&self, p: &LvmParams) -> Option<LvmResult> {
        self.lvm_report(p)
    }
    fn hot(&self, p: &HotParams) -> Option<Vec<tidemark_core::live::HotEntry>> {
        self.hot(p)
    }
    fn phantom(&self, p: &PhantomParams) -> Option<Vec<tidemark_core::live::PhantomSummary>> {
        self.phantom(p)
    }
    fn writers(&self, p: &WritersParams) -> Option<Vec<tidemark_core::live::OpenWriter>> {
        self.writers(p)
    }
    fn scans(&self) -> Option<Vec<tidemark_core::live::ScanStatus>> {
        self.scans()
    }
    fn watch_stats(&self) -> Option<Vec<(String, tidemark_live::watch::WatchStats)>> {
        self.watch_stats()
    }
    fn active_alerts(&self) -> Option<Vec<tidemark_core::live::AlertSummary>> {
        self.active_alerts()
    }
}
/// Render available measurements with deterministic labels and per-family caps.
pub fn render(
    view: &crate::state::LiveStateView,
    snapshots: &SnapshotMetrics,
    config: &MetricsConfig,
    _now: i64,
) -> Result<String, crate::DaemonError> {
    render_source(view, snapshots, config)
}
fn render_source(
    view: &dyn MetricsSource,
    snapshots: &SnapshotMetrics,
    config: &MetricsConfig,
) -> Result<String, crate::DaemonError> {
    let mut s = Series::default();
    s.add("tidemark_up", &[], 1.0);
    s.add(
        "tidemark_build_info",
        &[
            ("version", env!("CARGO_PKG_VERSION")),
            ("commit", option_env!("GIT_COMMIT").unwrap_or("unknown")),
        ],
        1.0,
    );
    if let Some(rows) = view.fs_samples() {
        for r in rows {
            let l = [("mount", r.mount.as_str())];
            for (n, v) in [
                ("tidemark_fs_size_bytes", r.size),
                ("tidemark_fs_used_bytes", r.used),
                ("tidemark_fs_avail_bytes", r.avail),
                ("tidemark_fs_inodes_used", r.inodes_used),
            ] {
                s.add(n, &l, v as f64);
            }
            for (w, d) in [("15m", 900), ("1h", 3600)] {
                if let Some(rate) = view.fs_rate(&r.mount, Duration::from_secs(d)) {
                    s.add(
                        "tidemark_fs_growth_bytes_per_second",
                        &[("mount", &r.mount), ("window", w)],
                        rate,
                    );
                    if w == "15m" && rate > 0.0 {
                        s.add(
                            "tidemark_fs_time_to_full_seconds",
                            &l,
                            r.avail as f64 / rate,
                        );
                    }
                }
            }
        }
    }
    if let Some(r) = view.lvm_report(&LvmParams { mount: None }) {
        for vg in r.vgs {
            s.add(
                "tidemark_lvm_vg_free_bytes",
                &[("vg", &vg.vg)],
                vg.free as f64,
            );
        }
        for (_, lv) in r.mounts {
            s.add(
                "tidemark_lvm_lv_size_bytes",
                &[("vg", &lv.vg), ("lv", &lv.lv)],
                lv.lv_size as f64,
            );
            if let Some(pool) = lv.thin_pool {
                for (n, v) in [
                    ("tidemark_lvm_thin_data_percent", lv.data_pct),
                    ("tidemark_lvm_thin_metadata_percent", lv.meta_pct),
                ] {
                    if let Some(v) = v {
                        s.add(n, &[("vg", &lv.vg), ("pool", &pool)], v);
                    }
                }
            }
        }
    }
    let (effective, _) = view.config();
    for mount in &effective.watch.mounts {
        let mount = mount.to_string_lossy();
        if let Some(mut hot) = view.hot_dirs(&mount, Duration::from_secs(300)).or_else(|| {
            view.hot(&HotParams {
                mount: Some(mount.to_string()),
                window: HotWindow::FiveMinutes,
                by: HotBy::Dir,
                limit: 10,
            })
        }) {
            hot.sort_by(|a, b| b.bytes_delta.cmp(&a.bytes_delta).then(a.key.cmp(&b.key)));
            for h in hot.into_iter().take(10) {
                s.add(
                    "tidemark_hot_bytes",
                    &[
                        ("mount", &mount),
                        ("scope", "dir"),
                        ("key", &String::from_utf8_lossy(&h.key)),
                    ],
                    h.bytes_delta as f64,
                );
            }
        }
    }
    for mount in &effective.watch.mounts {
        let mount = mount.to_string_lossy();
        for (scope, by) in [("unit", HotBy::Unit), ("comm", HotBy::Comm)] {
            if let Some(rows) = view.hot(&HotParams {
                mount: Some(mount.to_string()),
                window: HotWindow::FiveMinutes,
                by,
                limit: 10,
            }) {
                for h in rows {
                    s.add(
                        "tidemark_hot_bytes",
                        &[
                            ("mount", &mount),
                            ("scope", scope),
                            ("key", &String::from_utf8_lossy(&h.key)),
                        ],
                        h.bytes_delta as f64,
                    );
                }
            }
        }
    }
    if let Some(rows) = view.phantom(&PhantomParams { mount: None }) {
        for r in rows {
            s.add(
                "tidemark_phantom_bytes",
                &[("mount", &r.mount)],
                r.total_bytes as f64,
            );
        }
    }
    if let Some(rows) = view.writers(&WritersParams {
        mount: None,
        window: Duration::from_secs(300),
    }) {
        let mut units: BTreeMap<(String, String), i128> = BTreeMap::new();
        for r in rows {
            if let Some(unit) = r.unit {
                let sum = units.entry((r.mount, unit)).or_default();
                // The frozen view caps inventories at 10,000 i64 values; i128
                // therefore holds the complete unit sum without saturation.
                *sum += i128::from(r.growth);
            }
        }
        let mut rows: Vec<_> = units.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for ((mount, unit), growth) in rows.into_iter().take(10) {
            s.add(
                "tidemark_open_writer_growth_bytes",
                &[("mount", &mount), ("unit", &unit)],
                growth as f64,
            );
        }
    }
    if let Some(rows) = view.scans() {
        for r in rows {
            if let Some(last) = r.last_complete {
                s.add(
                    "tidemark_scan_last_success_timestamp_seconds",
                    &[("root", &r.root)],
                    last.finished_at as f64,
                );
                s.add(
                    "tidemark_scan_duration_seconds",
                    &[("root", &r.root), ("mode", &last.mode)],
                    last.duration_ms as f64 / 1000.0,
                );
            }
        }
    }
    if let Some(rows) = view.watch_stats() {
        for (mount, r) in rows {
            s.add(
                "tidemark_watch_events_total",
                &[("mount", &mount)],
                r.events_total as f64,
            );
            s.add(
                "tidemark_watch_overflows_total",
                &[("mount", &mount)],
                r.overflows_total as f64,
            );
        }
    }
    if let Some(rows) = view.active_alerts() {
        for r in rows {
            s.add(
                "tidemark_alert_firing",
                &[("rule", &r.rule), ("subject", &r.subject)],
                1.0,
            );
        }
    }
    for (root, cached) in snapshots.roots.read().iter() {
        let root = String::from_utf8_lossy(root);
        s.add(
            "tidemark_scan_errors",
            &[("root", &root)],
            cached.errors as f64,
        );
        for r in &cached.rows {
            if r.depth <= config.dir_metrics_depth {
                // Metrics specify UTF-8 replacement, rather than terminal byte escapes.
                let path = String::from_utf8_lossy(r.path.as_bytes());
                let l = [("root", root.as_ref()), ("path", path.as_ref())];
                s.add("tidemark_dir_used_bytes", &l, r.t_disk as f64);
            }
        }
        for r in &cached.reclaimable {
            if r.depth <= config.dir_metrics_depth {
                let path = String::from_utf8_lossy(r.path.as_bytes());
                s.add(
                    "tidemark_dir_reclaimable_bytes",
                    &[("root", root.as_ref()), ("path", path.as_ref())],
                    r.t_reclaimable as f64,
                );
            }
        }
        for name in ["tidemark_dir_used_bytes", "tidemark_dir_reclaimable_bytes"] {
            *s.1.entry(name).or_default() += cached.eligible;
        }
    }
    Ok(s.finish(config.max_series_per_metric))
}
/// Atomically publish a textfile without creating the collector directory.
pub fn write_textfile(path: &Path, text: &str) -> std::io::Result<()> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    use rustix::fs::{flock, openat, renameat, FlockOperation, Mode, OFlags};
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("missing textfile name"))?;
    let dir = std::fs::File::open(parent)?;
    let mut tmp = name.to_os_string();
    tmp.push(".tmp");
    let fd = openat(
        &dir,
        &tmp,
        OFlags::WRONLY | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?;
    let mut f = std::fs::File::from(fd);
    use std::os::unix::fs::MetadataExt;
    let md = f.metadata()?;
    if !md.is_file() || md.uid() != rustix::process::geteuid().as_raw() || md.nlink() != 1 {
        return Err(std::io::Error::other(
            "metrics temporary must be an owned regular single-link file",
        ));
    }
    flock(&f, FlockOperation::NonBlockingLockExclusive)?;
    // Another writer may have renamed the inode between open and flock. Never
    // truncate a published textfile through such a stale temporary handle.
    let named =
        rustix::fs::statat(&dir, &tmp, rustix::fs::AtFlags::SYMLINK_NOFOLLOW).map_err(|e| {
            if e == rustix::io::Errno::NOENT {
                std::io::Error::from(std::io::ErrorKind::WouldBlock)
            } else {
                e.into()
            }
        })?;
    if (named.st_dev, named.st_ino) != (md.dev(), md.ino()) {
        return Err(std::io::ErrorKind::WouldBlock.into());
    }
    f.set_len(0)?;
    f.write_all(text.as_bytes())?;
    f.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    f.sync_all()?;
    renameat(&dir, &tmp, &dir, name)?;
    Ok(())
}
/// Start the reload-aware textfile exporter with bounded shutdown polling.
pub fn start(
    _config: Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
) -> Result<crate::ServiceHandle, crate::DaemonError> {
    std::thread::Builder::new()
        .name("metrics".into())
        .spawn(move || {
            let empty = SnapshotMetrics::default();
            let (initial_config, _) = state.config();
            // Failed seeding is retried and reported by the first export below.
            let _ = reconcile(&state, &initial_config);
            let mut generation = None;
            let mut next = Some(Instant::now());
            let mut last_log: Option<Instant> = None;
            while !shutdown.load(Ordering::Acquire) {
                let (config, g) = state.config();
                if generation != Some(g) {
                    generation = Some(g);
                    next = if Instant::now()
                        .checked_add(config.metrics.interval)
                        .is_some()
                    {
                        Some(Instant::now())
                    } else {
                        state.publish_component(crate::component(
                            "metrics",
                            "error",
                            "metrics.interval cannot be represented by the monotonic clock",
                        ))?;
                        None
                    };
                }
                if next.is_some_and(|deadline| Instant::now() >= deadline) {
                    // Compare catalog identities at each export to repair startup/reload
                    // races with a post-scan publication; unchanged snapshots need no read.
                    let cache_error = reconcile(&state, &config).err().map(|e| e.to_string());
                    let now = crate::unix_now();
                    let view = state.view_at(now);
                    let cache = view.snapshot_metrics_handle();
                    let output = render(
                        &view,
                        cache.as_deref().unwrap_or(&empty),
                        &config.metrics,
                        now,
                    )?;
                    let error = write_textfile(&config.metrics.textfile, &output)
                        .err()
                        .map(|e| e.to_string())
                        .or_else(|| cache_error.clone());
                    if let Some(error) = error {
                        if last_log.is_none_or(|t| t.elapsed() >= Duration::from_secs(3600)) {
                            tracing::warn!(%error,"metrics exporter degraded");
                            last_log = Some(Instant::now());
                        }
                        state.publish_component(crate::component("metrics", "degraded", error))?;
                    } else {
                        let disabled = config.metrics.textfile.as_os_str().is_empty();
                        state.publish_component(crate::component(
                            "metrics",
                            if disabled { "disabled" } else { "ok" },
                            "exported",
                        ))?;
                    }
                    if let Some(deadline) = Instant::now()
                        .checked_add(config.metrics.interval.max(Duration::from_millis(1)))
                    {
                        next = Some(deadline);
                    } else {
                        next = None;
                        state.publish_component(crate::component(
                            "metrics",
                            "error",
                            "metrics.interval cannot be represented by the monotonic clock",
                        ))?;
                    }
                }
                crate::wait_service(&state, g, &shutdown, Duration::from_millis(50));
            }
            Ok(())
        })
        .map_err(|source| crate::DaemonError::Io {
            context: "start metrics".into(),
            source,
        })
}

#[cfg(test)]
#[path = "../tests/w6b/source.rs"]
mod tests;
