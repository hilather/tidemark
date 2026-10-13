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

fn tcp_pair() -> Option<(TcpStream, TcpStream)> {
    let listener = match listen("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("TCP bind unavailable: {error}");
            return None;
        }
    };
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match listener.accept() {
            Ok((server, _)) => return Some((client, server)),
            Err(e) if transient(&e) => {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            Err(e) => panic!("accept: {e}"),
        }
    }
}
#[test]
fn http_ninth_idle_client_closed_and_shutdown_joins_all_workers() {
    let mut workers = HttpWorkers(Vec::new());
    let text = Arc::new(RwLock::new("tidemark_up 1\n".into()));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut clients = Vec::new();
    for _ in 0..8 {
        let Some((client, server)) = tcp_pair() else {
            return;
        };
        workers.admit(server, text.clone(), stop.clone()).unwrap();
        clients.push(client);
    }
    let Some((mut ninth, server)) = tcp_pair() else {
        return;
    };
    workers.admit(server, text, stop).unwrap();
    ninth
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    assert_eq!(ninth.read(&mut [0; 1]).unwrap(), 0);
    assert_eq!(workers.0.len(), 8);
    let start = Instant::now();
    workers.close();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(workers.0.is_empty());
    drop(clients);
}
#[test]
fn http_unending_head_expires_under_absolute_deadline() {
    let Some((mut client, server)) = tcp_pair() else {
        return;
    };
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let start = Instant::now();
    let worker = std::thread::spawn(move || respond(server, "up 1\n", &stop));
    client
        .write_all(b"GET /metrics HTTP/1.1\r\nX: never ends")
        .unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(7)))
        .unwrap();
    assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    worker.join().unwrap().unwrap();
    assert!(start.elapsed() >= Duration::from_secs(4));
    assert!(start.elapsed() < Duration::from_secs(6));
}
#[test]
fn http_head_bytes_are_bounded() {
    let Some((mut client, server)) = tcp_pair() else {
        return;
    };
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker = std::thread::spawn(move || respond(server, "up 1\n", &stop));
    client.write_all(&[b'x'; 8192]).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    worker.join().unwrap().unwrap();
}
#[test]
fn http_slow_reader_cannot_extend_write_deadline() {
    let Some((mut client, server)) = tcp_pair() else {
        return;
    };
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let start = Instant::now();
    let worker = std::thread::spawn(move || respond(server, &"x".repeat(16 * 1024 * 1024), &stop));
    client.write_all(b"GET /metrics HTTP/1.1\r\n\r\n").unwrap();
    // Hold the receive window closed; a per-write timeout loop would never end.
    let deadline = Instant::now() + Duration::from_secs(7);
    while !worker.is_finished() {
        assert!(Instant::now() < deadline, "response worker did not expire");
        std::thread::sleep(Duration::from_millis(10));
    }
    worker.join().unwrap().unwrap();
    assert!(start.elapsed() < Duration::from_secs(6));
}
