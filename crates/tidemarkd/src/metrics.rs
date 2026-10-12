//! Owned by: W6-B
//! Bounded snapshot metrics and read-only Prometheus exporters.
use parking_lot::RwLock;
use std::{
    collections::BTreeMap,
    io::Write,
    net::SocketAddr,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tidemark_core::{
    api::*,
    config::MetricsConfig,
    scan::SnapshotStatus,
    snapshot::{DirRow, SortKey},
};
use tidemark_live::alerts::AlertSource;

/// Maximum cached directories across all roots; oversized refreshes preserve the old cache.
const MAX_DIRS: usize = 100_000;
const MAX_ROOTS: usize = 128;
#[derive(Default)]
struct CachedRoot {
    rows: Vec<DirRow>,
    errors: u64,
}
/// Latest complete snapshot data, replaced atomically per root after a scan.
#[derive(Default)]
pub struct SnapshotMetrics {
    roots: RwLock<BTreeMap<Vec<u8>, CachedRoot>>,
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
        let rows = snapshot
            .subtree_dirs(
                snapshot.root().map_err(store)?.id,
                depth,
                SortKey::Name,
                MAX_DIRS + 1,
            )
            .map_err(store)?;
        if rows.len() > MAX_DIRS {
            return Err(crate::DaemonError::Unavailable(
                "metrics directory cache capacity exceeded".into(),
            ));
        }
        let key = snapshot.meta().root.as_bytes().to_vec();
        let mut roots = self.roots.write();
        if !roots.contains_key(&key) && roots.len() >= MAX_ROOTS {
            return Err(crate::DaemonError::Unavailable(
                "metrics root capacity exceeded".into(),
            ));
        }
        let retained: usize = roots
            .iter()
            .filter(|(root, _)| **root != key)
            .map(|(_, r)| r.rows.len())
            .sum();
        if retained.saturating_add(rows.len()) > MAX_DIRS {
            return Err(crate::DaemonError::Unavailable(
                "metrics total directory capacity exceeded".into(),
            ));
        }
        roots.insert(
            key,
            CachedRoot {
                rows,
                errors: snapshot.meta().errors,
            },
        );
        Ok(())
    }
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
struct Series(BTreeMap<&'static str, BTreeMap<String, f64>>);
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
    fn finish(self, cap: usize) -> String {
        let mut output = String::new();
        let mut dropped = Vec::new();
        for (name, rows) in self.0 {
            let count = rows.len().saturating_sub(cap);
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
                s.add("tidemark_dir_reclaimable_bytes", &l, r.t_reclaimable as f64);
            }
        }
    }
    Ok(s.finish(config.max_series_per_metric))
}
/// Atomically publish a textfile without creating the collector directory.
pub fn write_textfile(path: &Path, text: &str) -> std::io::Result<()> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let result = (|| {
        f.write_all(text.as_bytes())?;
        f.set_permissions(std::fs::Permissions::from_mode(0o644))?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
/// Start the loopback-only listener; exposed for ephemeral-port integration tests.
pub fn listen(address: &str) -> Result<tiny_http::Server, crate::DaemonError> {
    let address: SocketAddr = address.parse().map_err(|_| {
        crate::DaemonError::Config("metrics.listen must be a loopback IP:port".into())
    })?;
    if !address.ip().is_loopback() {
        return Err(crate::DaemonError::Config(
            "metrics.listen must be loopback".into(),
        ));
    }
    tiny_http::Server::http(address).map_err(|e| crate::DaemonError::Unavailable(e.to_string()))
}
/// Answer only GET /metrics, with the Prometheus content type.
pub fn respond(request: tiny_http::Request, text: &str) -> std::io::Result<()> {
    if request.method() != &tiny_http::Method::Get || request.url() != "/metrics" {
        return request.respond(tiny_http::Response::empty(404));
    }
    let header =
        tiny_http::Header::from_bytes("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
            .map_err(|_| std::io::Error::other("invalid content type"))?;
    request.respond(tiny_http::Response::from_string(text).with_header(header))
}
/// Start reload-aware textfile and HTTP exporters with bounded shutdown polling.
pub fn start(
    _config: Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    shutdown: crate::Shutdown,
) -> Result<crate::ServiceHandle, crate::DaemonError> {
    std::thread::Builder::new()
        .name("metrics".into())
        .spawn(move || {
            let empty = SnapshotMetrics::default();
            let initial_now = crate::unix_now();
            let initial_view = state.view_at(initial_now);
            let (initial_config, _) = initial_view.config();
            let initial_cache = initial_view.snapshot_metrics_handle();
            let text = Arc::new(RwLock::new(render(
                &initial_view,
                initial_cache.as_deref().unwrap_or(&empty),
                &initial_config.metrics,
                initial_now,
            )?));
            let http_error = Arc::new(RwLock::new(None::<String>));
            let http_text = text.clone();
            let http_health = http_error.clone();
            let http_state = state.clone();
            let http_shutdown = shutdown.clone();
            // One response worker isolates tiny_http's potentially blocking socket writes.
            // No per-request threads or application queue are created.
            let http = std::thread::Builder::new()
                .name("metrics-http".into())
                .spawn(move || {
                    let mut listener = None;
                    let mut generation = None;
                    while !http_shutdown.load(Ordering::Acquire) {
                        let (config, g) = http_state.config();
                        if generation != Some(g) {
                            listener = None;
                            *http_health.write() = None;
                            if !config.metrics.listen.is_empty() {
                                match listen(&config.metrics.listen) {
                                    Ok(server) => listener = Some(server),
                                    Err(error) => *http_health.write() = Some(error.to_string()),
                                }
                            }
                            generation = Some(g);
                        }
                        if let Some(server) = &listener {
                            match server.recv_timeout(Duration::from_millis(50)) {
                                Ok(Some(request)) => {
                                    let output = http_text.read().clone();
                                    let _ = respond(request, &output);
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    *http_health.write() = Some(error.to_string());
                                    listener = None;
                                }
                            }
                        } else {
                            crate::wait_service(
                                &http_state,
                                g,
                                &http_shutdown,
                                Duration::from_millis(50),
                            );
                        }
                    }
                })
                .map_err(|source| crate::DaemonError::Io {
                    context: "start metrics HTTP".into(),
                    source,
                })?;
            let mut generation = None;
            let mut next = Instant::now();
            let mut last_log: Option<Instant> = None;
            while !shutdown.load(Ordering::Acquire) {
                let (config, g) = state.config();
                if generation != Some(g) {
                    generation = Some(g);
                    next = Instant::now();
                }
                if Instant::now() >= next {
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
                        .or_else(|| http_error.read().clone());
                    *text.write() = output;
                    if let Some(error) = error {
                        if last_log.is_none_or(|t| t.elapsed() >= Duration::from_secs(3600)) {
                            tracing::warn!(%error,"metrics exporter degraded");
                            last_log = Some(Instant::now());
                        }
                        state.publish_component(crate::component("metrics", "degraded", error))?;
                    } else {
                        let disabled = config.metrics.textfile.as_os_str().is_empty()
                            && config.metrics.listen.is_empty();
                        state.publish_component(crate::component(
                            "metrics",
                            if disabled { "disabled" } else { "ok" },
                            "exported",
                        ))?;
                    }
                    next = Instant::now() + config.metrics.interval.max(Duration::from_millis(1));
                }
                crate::wait_service(&state, g, &shutdown, Duration::from_millis(50));
            }
            // tiny_http exposes no socket write deadline or cancellation API. Never let
            // a stalled client delay daemon shutdown; at most one responder is detached.
            let deadline = Instant::now() + Duration::from_millis(250);
            while !http.is_finished() && Instant::now() < deadline {
                std::thread::yield_now();
            }
            if http.is_finished() {
                let _ = http.join();
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
