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
    metrics::write_textfile(&path, "new").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    assert!(!staging.exists());
    std::os::unix::fs::symlink(&path, &staging).unwrap();
    assert!(metrics::write_textfile(&path, "refused").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
}
#[test]
fn loopback_http_only_get_metrics() {
    assert!(metrics::listen("0.0.0.0:0").is_err());
    let server = match metrics::listen("127.0.0.1:0") {
        Ok(server) => server,
        Err(error) => {
            eprintln!("TCP bind unavailable: {error}");
            return;
        }
    };
    let address = server.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        for _ in 0..6 {
            let deadline = Instant::now() + Duration::from_secs(10);
            let stream = loop {
                match server.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        std::thread::yield_now();
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            };
            metrics::respond(stream, "tidemark_up 1\n", &Arc::new(AtomicBool::new(false))).unwrap();
        }
    });
    for (method, url, code) in [
        ("GET", "/metrics", 200),
        ("HEAD", "/metrics", 200),
        ("POST", "/metrics", 405),
        ("G ET", "/metrics", 400),
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
            if method == "HEAD" {
                assert!(response.ends_with("\r\n\r\n"));
            } else {
                assert!(response.ends_with("tidemark_up 1\n"));
            }
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
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "tidemark_up 1\n");
    let staging = output.with_extension("prom.tmp");
    std::fs::write(&staging, "other owner").unwrap();
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o666)).unwrap();
    rustix::fs::chown(&staging, Some(rustix::process::Uid::from_raw(65534)), None).unwrap();
    assert!(metrics::write_textfile(&output, "refused").is_err());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "tidemark_up 1\n");
}

#[test]
fn huge_interval_reports_error_and_recovers_after_reload() {
    let mut config = Config::default();
    config.roots.clear();
    config.metrics.textfile = Default::default();
    config.metrics.interval = Duration::from_secs(u64::MAX);
    let state = LiveState::new(&config, 0);
    let stop = Arc::new(AtomicBool::new(false));
    let worker = metrics::start(Arc::new(config.clone()), state.clone(), stop.clone()).unwrap();
    wait(|| {
        state
            .view()
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "metrics" && c.state == "error" && c.detail.contains("interval"))
    });
    config.metrics.interval = Duration::from_millis(10);
    state.replace_config(config).unwrap();
    wait(|| {
        state
            .view()
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "metrics" && c.state == "disabled")
    });
    stop.store(true, Ordering::Release);
    worker.join().unwrap().unwrap();
}
#[test]
fn temporary_live_writer_and_nonregular_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("out.prom");
    let staging = tmp.path().join("out.prom.tmp");
    std::fs::write(&staging, "live").unwrap();
    let file = std::fs::File::open(&staging).unwrap();
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    assert!(metrics::write_textfile(&output, "new").is_err());
    assert_eq!(std::fs::read_to_string(&staging).unwrap(), "live");
    drop(file);
    metrics::write_textfile(&output, "recovered").unwrap();
    std::fs::create_dir(&staging).unwrap();
    assert!(metrics::write_textfile(&output, "refused").is_err());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "recovered");
}
#[test]
fn ranked_large_inventory_keeps_both_families_and_exact_counts() {
    let (_tmp, snapshot) = snapshot(100_001);
    let (used, reclaimable, count, _) = snapshot.metrics_candidates(1, 3).unwrap();
    assert_eq!(count, 100_001);
    assert_eq!(used.len(), 3);
    assert_eq!(reclaimable.len(), 3);
    assert_eq!(used[0].id, 100_000);
    let cache = SnapshotMetrics::default();
    let (_previous_dir, previous) = self::snapshot(5);
    cache.refresh(&previous, 1).unwrap();
    cache.refresh(&snapshot, 1).unwrap();
    let config = Config::default();
    let state = LiveState::new(&config, 0);
    let output = metrics::render(&state.view(), &cache, &config.metrics, 0).unwrap();
    let parsed = parse(&output);
    for name in ["tidemark_dir_used_bytes", "tidemark_dir_reclaimable_bytes"] {
        assert_eq!(parsed[name].len(), 500);
        assert!(output.contains(&format!(
            "tidemark_metrics_dropped_series{{metric=\"{name}\"}} 99501"
        )));
    }
    cache.refresh(&snapshot, 0).unwrap();
    let output = metrics::render(&state.view(), &cache, &config.metrics, 0).unwrap();
    assert_eq!(parse(&output)["tidemark_dir_used_bytes"].len(), 1);
}
#[test]
fn exporter_seeds_restart_and_reconciles_depth_and_root_removal() {
    let (tmp, snapshot) = snapshot(20);
    let catalog = tidemark_store::catalog::Catalog::open(tmp.path(), true).unwrap();
    let meta = snapshot.meta();
    let entry = tidemark_store::catalog::ScanEntry {
        scan_id: meta.scan_id.to_string(),
        root: meta.root.clone(),
        mode: meta.mode,
        started_at: meta.started_at,
        finished_at: Some(meta.finished_at),
        status: "complete".into(),
        degraded: false,
        file: Some(snapshot.metrics_candidates(0, 1).unwrap().3),
        t_disk: None,
        t_inodes: None,
        t_dirs: None,
        errors: Some(7),
        duration_ms: None,
    };
    catalog.record_start(&entry).unwrap();
    catalog.record_finish(&entry).unwrap();
    let mut second_meta = fixtures::meta();
    second_meta.root = tidemark_core::path::RawPath::new(b"/second".to_vec()).unwrap();
    second_meta.scan_id = serde_json::from_str("\"20260101T000001Z-00000000\"").unwrap();
    second_meta.dirs = 20;
    second_meta.errors = 8;
    let mut writer =
        tidemark_store::writer::SnapshotWriter::create(tmp.path(), &second_meta).unwrap();
    let rows: Vec<_> = (0..20)
        .map(|id| {
            let mut row = fixtures::dir(id);
            row.path = if id == 0 {
                second_meta.root.clone()
            } else {
                second_meta.root.join(
                    &tidemark_core::path::RawName::new(format!("{id:08}").into_bytes()).unwrap(),
                )
            };
            row.path_hash = row.path.hash64();
            row.subtree_end = if id == 0 { 19 } else { id };
            row
        })
        .collect();
    writer.put_dirs(&rows).unwrap();
    let second_file = writer.finish(&second_meta).unwrap();
    let second_entry = tidemark_store::catalog::ScanEntry {
        scan_id: second_meta.scan_id.to_string(),
        root: second_meta.root.clone(),
        mode: second_meta.mode,
        started_at: second_meta.started_at,
        finished_at: Some(second_meta.finished_at),
        status: "complete".into(),
        degraded: false,
        file: Some(second_file),
        t_disk: None,
        t_inodes: None,
        t_dirs: None,
        errors: Some(8),
        duration_ms: None,
    };
    catalog.record_start(&second_entry).unwrap();
    catalog.record_finish(&second_entry).unwrap();
    drop(catalog);
    let mut config = Config::default();
    config.general.state_dir = tmp.path().into();
    config.roots.truncate(1);
    use std::os::unix::ffi::OsStringExt;
    config.roots[0].path = std::ffi::OsString::from_vec(meta.root.as_bytes().to_vec()).into();
    config.roots[0].schedule = "off".into();
    let mut second_root = config.roots[0].clone();
    second_root.path = "/second".into();
    config.roots.push(second_root);
    config.metrics.textfile = tmp.path().join("out.prom");
    config.metrics.interval = Duration::from_millis(10);
    config.metrics.dir_metrics_depth = 0;
    for _ in 0..2 {
        let state = LiveState::new(&config, 0);
        let stop = Arc::new(AtomicBool::new(false));
        let worker = metrics::start(Arc::new(config.clone()), state.clone(), stop.clone()).unwrap();
        wait(|| {
            std::fs::read_to_string(&config.metrics.textfile).is_ok_and(|s| {
                s.contains("tidemark_scan_errors")
                    && parse(&s)["tidemark_dir_used_bytes"].len() == 2
            })
        });
        let mut changed = config.clone();
        changed.metrics.dir_metrics_depth = 1;
        state.replace_config(changed.clone()).unwrap();
        wait(|| {
            std::fs::read_to_string(&config.metrics.textfile).is_ok_and(|s| {
                parse(&s)
                    .get("tidemark_dir_used_bytes")
                    .is_some_and(|r| r.len() == 40)
            })
        });
        changed.roots.clear();
        state.replace_config(changed).unwrap();
        wait(|| {
            std::fs::read_to_string(&config.metrics.textfile)
                .is_ok_and(|s| !s.contains("tidemark_dir_used_bytes"))
        });
        stop.store(true, Ordering::Release);
        worker.join().unwrap().unwrap();
        std::fs::remove_file(&config.metrics.textfile).unwrap();
    }
}

#[test]
fn global_candidates_across_roots_and_replacement_uncover_previous_rows() {
    use tidemark_core::path::{RawName, RawPath};
    let mut sources = Vec::new();
    for (root, offset, errors) in [
        (b"/a".as_slice(), 10000u64, 8),
        (b"/b".as_slice(), 0, 9),
        (b"/a".as_slice(), 0, 10),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let mut meta = fixtures::meta();
        meta.root = RawPath::new(root.to_vec()).unwrap();
        meta.dirs = 601;
        meta.errors = errors;
        let mut writer = tidemark_store::writer::SnapshotWriter::create(tmp.path(), &meta).unwrap();
        let rows: Vec<_> = (0..601)
            .map(|id| {
                let mut row = fixtures::dir(id);
                row.path = meta
                    .root
                    .join(&RawName::new(format!("{id:08}").into_bytes()).unwrap());
                row.path_hash = row.path.hash64();
                row.subtree_end = if id == 0 { 600 } else { id };
                row.t_disk = offset + u64::from(id);
                row.t_reclaimable = offset + u64::from(601 - id);
                row
            })
            .collect();
        writer.put_dirs(&rows).unwrap();
        let file = writer.finish(&meta).unwrap();
        sources.push((tmp, tidemark_store::reader::Snapshot::open(&file).unwrap()));
    }
    let cache = SnapshotMetrics::default();
    let config = Config::default();
    let state = LiveState::new(&config, 0);
    cache.refresh(&sources[0].1, 1).unwrap();
    cache.refresh(&sources[1].1, 1).unwrap();
    let output = metrics::render(&state.view(), &cache, &config.metrics, 0).unwrap();
    let parsed = parse(&output);
    for name in ["tidemark_dir_used_bytes", "tidemark_dir_reclaimable_bytes"] {
        assert_eq!(parsed[name].len(), 500);
        assert!(parsed[name]
            .iter()
            .all(|(label, _)| label.contains("root=\"/a\"")));
        assert!(output.contains(&format!(
            "tidemark_metrics_dropped_series{{metric=\"{name}\"}} 702"
        )));
    }
    cache.refresh(&sources[2].1, 1).unwrap();
    let parsed = parse(&metrics::render(&state.view(), &cache, &config.metrics, 0).unwrap());
    assert!(parsed["tidemark_dir_used_bytes"]
        .iter()
        .any(|(label, _)| label.contains("root=\"/b\"")));
    assert!(parsed["tidemark_scan_errors"]
        .iter()
        .any(|(label, value)| label.contains("root=\"/a\"") && *value == 10.0));
}

#[test]
fn exporter_http_scrape_matches_textfile_and_idle_shutdown_is_bounded() {
    let listener = match metrics::listen("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("TCP bind unavailable: {error}");
            return;
        }
    };
    let address = listener.local_addr().unwrap();
    drop(listener);
    let tmp = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.roots.clear();
    config.metrics.listen = address.to_string();
    config.metrics.textfile = tmp.path().join("out.prom");
    config.metrics.interval = Duration::from_millis(10);
    let state = LiveState::new(&config, 0);
    let stop = Arc::new(AtomicBool::new(false));
    let worker = metrics::start(Arc::new(config.clone()), state.clone(), stop.clone()).unwrap();
    wait(|| config.metrics.textfile.exists());
    let readiness_deadline = Instant::now() + Duration::from_secs(2);
    let mut scrape = loop {
        if let Ok(stream) = std::net::TcpStream::connect(address) {
            break stream;
        }
        assert!(
            !worker.is_finished() && Instant::now() < readiness_deadline,
            "HTTP readiness failed: {:?}",
            state.view().status(0)
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    scrape
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    scrape
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    scrape.read_to_string(&mut response).unwrap();
    let body = response.split_once("\r\n\r\n").unwrap().1;
    assert_eq!(
        body,
        std::fs::read_to_string(&config.metrics.textfile).unwrap()
    );
    drop(scrape);
    let clients: Vec<_> = (0..8)
        .map(|_| std::net::TcpStream::connect(address).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(100));
    config.metrics.listen.clear();
    state.replace_config(config.clone()).unwrap();
    wait(|| std::net::TcpStream::connect(address).is_err());
    for mut client in clients {
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    }
    config.metrics.listen = address.to_string();
    state.replace_config(config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let first = loop {
        if let Ok(stream) = std::net::TcpStream::connect(address) {
            break stream;
        }
        assert!(
            Instant::now() < deadline,
            "same-address metrics reload did not bind"
        );
        std::thread::yield_now();
    };
    let mut clients = vec![first];
    clients.extend((0..7).map(|_| std::net::TcpStream::connect(address).unwrap()));
    std::thread::sleep(Duration::from_millis(100));
    let start = Instant::now();
    stop.store(true, Ordering::Release);
    while !worker.is_finished() {
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "HTTP idle clients delayed shutdown"
        );
        std::thread::yield_now();
    }
    worker.join().unwrap().unwrap();
    assert!(std::net::TcpStream::connect(address).is_err());
    drop(clients);
}
