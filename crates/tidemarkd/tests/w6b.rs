//! Owned by: W6-B
//! Exporter contract, snapshot cache, atomic publication and HTTP evidence.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
#[path = "w6b/fixtures.rs"]
mod fixtures;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tidemark_core::{api::*, config::Config, live::*};
use tidemarkd::{
    component,
    metrics::{self, SnapshotMetrics},
    state::{LiveState, Publication},
};
fn wait(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
fn publish(state: &LiveState, value: Publication) {
    state
        .publish(value, component("test", "ok", "measured"))
        .unwrap();
}
// A small strict parser: validates metadata, unique sample identities and quoted
// labels, including the three allowed escape sequences, before parsing numbers.
fn parse(text: &str) -> BTreeMap<String, Vec<(String, f64)>> {
    assert!(text.ends_with('\n'));
    let mut types = BTreeMap::new();
    let mut help = BTreeSet::new();
    let mut samples: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("# HELP ") {
            let (name, _) = v.split_once(' ').unwrap();
            assert!(help.insert(name.to_owned()));
            continue;
        }
        if let Some(v) = line.strip_prefix("# TYPE ") {
            let (name, kind) = v.split_once(' ').unwrap();
            assert!(matches!(kind, "gauge" | "counter"));
            assert!(types.insert(name.to_owned(), kind).is_none());
            continue;
        }
        let (key, value) = line.rsplit_once(' ').unwrap();
        let name = key.split('{').next().unwrap();
        assert!(types.contains_key(name));
        assert!(help.contains(name));
        assert!(seen.insert(key.to_owned()));
        if let Some((_, labels)) = key.split_once('{') {
            assert!(labels.ends_with('}'));
            let mut chars = labels[..labels.len() - 1].chars().peekable();
            while chars.peek().is_some() {
                let mut label = String::new();
                for c in chars.by_ref() {
                    if c == '=' {
                        break;
                    }
                    label.push(c);
                }
                assert!(
                    !label.is_empty() && label.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                );
                assert_eq!(chars.next(), Some('"'));
                loop {
                    match chars.next().unwrap() {
                        '"' => break,
                        '\\' => assert!(matches!(chars.next(), Some('\\' | '"' | 'n'))),
                        '\n' => panic!("raw newline"),
                        _ => {}
                    }
                }
                if chars.peek().is_some() {
                    assert_eq!(chars.next(), Some(','));
                }
            }
        }
        samples
            .entry(name.into())
            .or_default()
            .push((key.into(), value.parse::<f64>().unwrap()));
    }
    samples
}
fn snapshot(n: u32) -> (tempfile::TempDir, tidemark_store::reader::Snapshot) {
    let tmp = tempfile::tempdir().unwrap();
    let mut meta = fixtures::meta();
    meta.dirs = n as u64;
    meta.errors = 7;
    let mut writer = tidemark_store::writer::SnapshotWriter::create(tmp.path(), &meta).unwrap();
    let mut rows: Vec<_> = (0..n).map(fixtures::dir).collect();
    rows[0].subtree_end = n - 1;
    writer.put_dirs(&rows).unwrap();
    let file = writer.finish(&meta).unwrap();
    let snapshot = tidemark_store::reader::Snapshot::open(&file).unwrap();
    (tmp, snapshot)
}
#[test]
fn cardinality_10000_hostile_names_and_cached_depth() {
    let (_tmp, snapshot) = snapshot(10_000);
    let cache = SnapshotMetrics::default();
    cache.refresh(&snapshot, 1).unwrap();
    let mut config = Config::default();
    config.roots.clear();
    config.metrics.max_series_per_metric = 20;
    let state = LiveState::new(&config, 0);
    let output = metrics::render(&state.view_at(0), &cache, &config.metrics, 0).unwrap();
    let parsed = parse(&output);
    for name in ["tidemark_dir_used_bytes", "tidemark_dir_reclaimable_bytes"] {
        assert_eq!(parsed[name].len(), 20);
    }
    assert!(
        output.contains("tidemark_metrics_dropped_series{metric=\"tidemark_dir_used_bytes\"} 9980")
    );
    assert!(output.contains("�\\\"\\\\\\n"));
    assert_eq!(
        output,
        metrics::render(&state.view_at(0), &cache, &config.metrics, 0).unwrap()
    );
    assert!(parsed["tidemark_dir_used_bytes"]
        .iter()
        .all(|(_, v)| *v >= 4096.0 + 9980.0));
    cache.refresh(&snapshot, 0).unwrap();
    let parsed = parse(&metrics::render(&state.view_at(0), &cache, &config.metrics, 0).unwrap());
    assert_eq!(parsed["tidemark_dir_used_bytes"].len(), 1);
    assert_eq!(
        parsed["tidemark_dir_used_bytes"][0].0.matches('�').count(),
        2
    );
    assert_eq!(parsed["tidemark_scan_errors"][0].1, 7.0);
}
#[test]
fn measured_state_and_unavailable_omission() {
    let mut config = Config::default();
    config.roots.clear();
    config.watch.mounts = vec!["/fixture".into()];
    let state = LiveState::new(&config, 0);
    let cache = SnapshotMetrics::default();
    let empty = parse(&metrics::render(&state.view_at(0), &cache, &config.metrics, 0).unwrap());
    assert_eq!(empty.len(), 2);
    for ts in [0, 60, 120] {
        publish(
            &state,
            Publication::Fs(FsSample {
                ts,
                mount: "/fixture".into(),
                size: 10000,
                used: 1000 + ts as u64,
                avail: 9000 - ts as u64,
                inodes: 100,
                inodes_used: 3,
            }),
        );
    }
    publish(
        &state,
        Publication::Lvm(LvmResult {
            mounts: vec![(
                "/fixture".into(),
                LvmNow {
                    vg: "vg".into(),
                    lv: "lv".into(),
                    lv_size: 10000,
                    vg_free: 1000,
                    thin_pool: Some("pool".into()),
                    data_pct: Some(10.0),
                    meta_pct: Some(20.0),
                    sampled_at: 120,
                },
            )],
            vgs: vec![VgInfo {
                vg: "vg".into(),
                size: 20000,
                free: 1000,
            }],
            sampled_at: 120,
        }),
    );
    publish(
        &state,
        Publication::Scan(ScanStatus {
            root: "/fixture".into(),
            running: None,
            last_complete: Some(LastScan {
                scan_id: "test".into(),
                finished_at: 120,
                mode: "walk".into(),
                duration_ms: 1500,
            }),
            next_due: None,
        }),
    );
    publish(
        &state,
        Publication::Alerts((
            vec![AlertSummary {
                rule: "rule".into(),
                subject: "bad\"\\\n".into(),
                since: 1,
                value: None,
                message: String::new(),
            }],
            vec![],
        )),
    );
    publish(
        &state,
        Publication::WatchWindow {
            mount: "/fixture".into(),
            window: Duration::from_secs(300),
            measured_at: 120,
            hot: Some(vec![HotEntry {
                key: b"/hot/\xff".to_vec(),
                events: 1,
                files: 1,
                bytes_delta: 5,
                top_unit: None,
                lossy: false,
            }]),
            overflows: Some(0),
        },
    );
    let parsed =
        parse(&metrics::render(&state.view_at(120), &cache, &config.metrics, 120).unwrap());
    assert_eq!(parsed["tidemark_fs_growth_bytes_per_second"].len(), 2);
    assert_eq!(parsed["tidemark_fs_time_to_full_seconds"][0].1, 8880.0);
    assert_eq!(parsed["tidemark_alert_firing"][0].1, 1.0);
    assert_eq!(parsed["tidemark_scan_duration_seconds"][0].1, 1.5);
    assert_eq!(parsed["tidemark_hot_bytes"][0].1, 5.0);
    assert!(!parsed.contains_key("tidemark_phantom_bytes"));
    assert!(!parsed.contains_key("tidemark_watch_events_total"));
    let spec = include_str!("../../../docs/plan/08-interfaces.md");
    for name in parsed.keys() {
        assert!(spec.contains(name), "undocumented {name}");
    }
}
#[test]
fn atomic_publication_and_modes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("metrics.prom");
    let a = "a".repeat(100000);
    let b = "b".repeat(100000);
    metrics::write_textfile(&path, &a).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    let p = path.clone();
    let aa = a.clone();
    let bb = b.clone();
    let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
    let reader = std::thread::spawn(move || {
        let first = std::fs::read_to_string(&p).unwrap();
        assert!(first == aa || first == bb);
        ready_tx.send(()).unwrap();
        let mut reads = 1;
        while !stop.load(Ordering::Acquire) {
            let v = std::fs::read_to_string(&p).unwrap();
            assert!(v == aa || v == bb);
            reads += 1;
        }
        reads
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    for i in 0..100 {
        metrics::write_textfile(&path, if i % 2 == 0 { &b } else { &a }).unwrap();
    }
    done.store(true, Ordering::Release);
    assert!(reader.join().unwrap() > 0);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert!(!path.with_extension("prom.tmp").exists());
    assert!(metrics::write_textfile(&tmp.path().join("absent/file"), "x").is_err());
    let staging = path.with_extension("prom.tmp");
    std::fs::write(&staging, "reserved").unwrap();
    assert!(metrics::write_textfile(&path, "new").is_err());
    assert_eq!(std::fs::read_to_string(staging).unwrap(), "reserved");
}
#[test]
fn loopback_http_only_get_metrics() {
    assert!(metrics::listen("0.0.0.0:0").is_err());
    let server = metrics::listen("127.0.0.1:0").unwrap();
    let address = server.server_addr().to_ip().unwrap();
    let worker = std::thread::spawn(move || {
        for _ in 0..4 {
            let request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            metrics::respond(request, "tidemark_up 1\n").unwrap();
        }
    });
    for (method, url, code) in [
        ("GET", "/metrics", 200),
        ("POST", "/metrics", 404),
        ("GET", "/", 404),
        ("GET", "/metrics?x", 404),
    ] {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(format!("{method} {url} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {code}")),
            "{response}"
        );
        if code == 200 {
            assert!(response.contains("version=0.0.4"));
            assert!(response.ends_with("tidemark_up 1\n"));
        }
    }
    worker.join().unwrap();
}
#[test]
fn exporter_reload_health_and_shutdown() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.metrics.textfile = tmp.path().join("absent/out.prom");
    config.metrics.interval = Duration::from_millis(10);
    config.roots.clear();
    let state = LiveState::new(&config, 0);
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = metrics::start(Arc::new(config.clone()), state.clone(), shutdown.clone()).unwrap();
    wait(|| {
        state
            .view_at(0)
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "metrics" && c.state == "degraded")
    });
    config.metrics.textfile = tmp.path().join("out.prom");
    state.replace_config(config).unwrap();
    wait(|| {
        std::fs::read_to_string(tmp.path().join("out.prom"))
            .is_ok_and(|s| s.contains("tidemark_up 1"))
    });
    shutdown.store(true, Ordering::Release);
    wait(|| handle.is_finished());
    handle.join().unwrap().unwrap();
}
#[test]
fn dashboard_and_yaml_rules_contract() {
    let dashboard: serde_json::Value =
        serde_json::from_str(include_str!("../../../packaging/grafana/tidemark.json")).unwrap();
    // JSON is a YAML 1.2 subset. Parsing it validates the exact shipped YAML bytes.
    let rules: serde_json::Value = serde_json::from_str(include_str!(
        "../../../packaging/prometheus/tidemark-alerts.yml"
    ))
    .unwrap();
    let spec = include_str!("../../../docs/plan/08-interfaces.md");
    let mut exprs = Vec::new();
    for p in dashboard["panels"].as_array().unwrap() {
        for t in p["targets"].as_array().unwrap() {
            exprs.push(t["expr"].as_str().unwrap());
        }
    }
    let rules = rules["groups"][0]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 7);
    assert_eq!(dashboard["panels"].as_array().unwrap().len(), 9);
    for r in rules {
        exprs.push(r["expr"].as_str().unwrap());
    }
    for expr in exprs {
        for token in expr.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if token.starts_with("tidemark_") {
                assert!(spec.contains(token), "unknown metric {token}");
            }
        }
    }
    assert!(!dashboard.to_string().to_lowercase().contains("postgres"));
}

#[cfg(feature = "privileged")]
#[test]
fn vm_xfs_textfile_publication_root_owned_and_readable() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let xfs = test_support::xfs::LoopXfs::new(1 << 30, &[], "");
    let output = xfs.mountpoint.join("tidemark.prom");
    metrics::write_textfile(&output, "tidemark_up 1\n").unwrap();
    let md = std::fs::metadata(&output).unwrap();
    assert_eq!((md.uid(), md.gid()), (0, 0));
    assert_eq!(md.permissions().mode() & 0o777, 0o644);
    assert_eq!(std::fs::read_to_string(output).unwrap(), "tidemark_up 1\n");
}
