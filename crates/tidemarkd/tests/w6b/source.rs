//! Owned by: W6-B
//! Deterministic complete measured-source fixture and exact family contract oracle.
#![allow(clippy::unwrap_used)]
use super::*;
use tidemark_core::live::*;
#[allow(dead_code)]
#[path = "fixtures.rs"]
mod fixtures;
struct Populated;
impl MetricsSource for Populated {
    fn config(&self) -> (Arc<tidemark_core::config::Config>, u64) {
        let mut c = tidemark_core::config::Config::default();
        c.watch.mounts = vec!["/fixture".into()];
        (Arc::new(c), 0)
    }
    fn fs_samples(&self) -> Option<Vec<FsSample>> {
        Some(vec![FsSample {
            ts: 10,
            mount: "/fixture".into(),
            size: 100,
            used: 20,
            avail: 80,
            inodes: 10,
            inodes_used: 2,
        }])
    }
    fn fs_rate(&self, _mount: &str, _window: Duration) -> Option<f64> {
        Some(2.0)
    }
    fn hot_dirs(&self, _mount: &str, _window: Duration) -> Option<Vec<HotEntry>> {
        self.hot(&HotParams {
            mount: None,
            window: HotWindow::FiveMinutes,
            by: HotBy::Dir,
            limit: 10,
        })
    }
    fn lvm_report(&self, _p: &LvmParams) -> Option<LvmResult> {
        Some(LvmResult {
            mounts: vec![(
                "/fixture".into(),
                LvmNow {
                    vg: "vg".into(),
                    lv: "lv".into(),
                    lv_size: 100,
                    vg_free: 20,
                    thin_pool: Some("pool".into()),
                    data_pct: Some(20.0),
                    meta_pct: Some(30.0),
                    sampled_at: 10,
                },
            )],
            vgs: vec![VgInfo {
                vg: "vg".into(),
                size: 200,
                free: 20,
            }],
            sampled_at: 10,
        })
    }
    fn hot(&self, _p: &HotParams) -> Option<Vec<HotEntry>> {
        Some(vec![HotEntry {
            key: b"/hostile/\xff\"\\\n".to_vec(),
            events: 1,
            files: 1,
            bytes_delta: 20,
            top_unit: Some("unit".into()),
            lossy: false,
        }])
    }
    fn phantom(&self, _p: &PhantomParams) -> Option<Vec<PhantomSummary>> {
        Some(vec![PhantomSummary {
            mount: "/fixture".into(),
            total_bytes: 30,
            files: 1,
            holders: vec![],
        }])
    }
    fn writers(&self, _p: &WritersParams) -> Option<Vec<OpenWriter>> {
        Some(vec![OpenWriter {
            mount: "/fixture".into(),
            pid: 1,
            comm: "test".into(),
            unit: Some("unit".into()),
            path: b"/fixture/file".to_vec(),
            dev: 1,
            ino: 1,
            size: 40,
            disk: 40,
            growth: 20,
            window_secs: 300,
        }])
    }
    fn scans(&self) -> Option<Vec<ScanStatus>> {
        Some(vec![ScanStatus {
            root: "/fixture".into(),
            running: None,
            last_complete: Some(LastScan {
                scan_id: "test".into(),
                finished_at: 10,
                mode: "walk".into(),
                duration_ms: 500,
            }),
            next_due: None,
        }])
    }
    fn watch_stats(&self) -> Option<Vec<(String, tidemark_live::watch::WatchStats)>> {
        Some(vec![(
            "/fixture".into(),
            tidemark_live::watch::WatchStats {
                mode: tidemark_live::watch::WatchMode::FdMount,
                events_total: 40,
                overflows_total: 1,
                lossy_minutes_15m: 1,
                dropped_keys: 0,
                dropped_events: 0,
                recovering: false,
                read_errors: 0,
            },
        )])
    }
    fn active_alerts(&self) -> Option<Vec<AlertSummary>> {
        Some(vec![AlertSummary {
            rule: "test".into(),
            subject: "/fixture".into(),
            since: 1,
            value: None,
            message: String::new(),
        }])
    }
}
#[test]
fn every_contract_metric_and_exact_labels_from_populated_source() {
    let spec = include_str!("../../../../docs/plan/08-interfaces.md");
    let section = spec
        .split("## 4. Metrics")
        .nth(1)
        .unwrap()
        .split("## 5.")
        .next()
        .unwrap();
    let mut expected = BTreeMap::new();
    for line in section.lines().filter(|l| l.starts_with("| `tidemark_")) {
        let cells: Vec<_> = line.split('|').collect();
        let names: Vec<_> = cells[1]
            .split('`')
            .filter(|t| t.starts_with("tidemark_"))
            .collect();
        let labels: Vec<_> = cells[3]
            .split('(')
            .next()
            .unwrap()
            .split('`')
            .enumerate()
            .filter_map(|(i, t)| {
                (i % 2 == 1 && t.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
                    .then_some(t.to_owned())
            })
            .collect();
        for name in names {
            expected.insert(
                name.to_owned(),
                (cells[2].trim().to_owned(), labels.clone()),
            );
        }
    }
    expected.insert(
        "tidemark_metrics_dropped_series".into(),
        ("gauge".into(), vec!["metric".into()]),
    );
    let cache = SnapshotMetrics::default();
    cache.roots.write().insert(
        b"/fixture".to_vec(),
        CachedRoot {
            rows: vec![fixtures::dir(0), fixtures::dir(1)],
            reclaimable: vec![fixtures::dir(0), fixtures::dir(1)],
            eligible: 2,
            errors: 1,
            ..CachedRoot::default()
        },
    );
    let config = MetricsConfig {
        max_series_per_metric: 1,
        ..MetricsConfig::default()
    };
    let text = render_source(&Populated, &cache, &config).unwrap();
    let mut actual = std::collections::BTreeSet::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("# TYPE ") {
            let (name, kind) = v.split_once(' ').unwrap();
            assert_eq!(expected[name].0, kind);
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let (key, value) = line.rsplit_once(' ').unwrap();
        assert!(value.parse::<f64>().unwrap().is_finite());
        let name = key.split('{').next().unwrap();
        actual.insert(name.to_owned());
        let labels: Vec<_> = key
            .split_once('{')
            .map(|(_, l)| {
                l[..l.len() - 1]
                    .split(",")
                    .map(|pair| pair.split('=').next().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(expected[name].1, labels, "labels for {name}");
    }
    assert_eq!(actual, expected.into_keys().collect());
}
