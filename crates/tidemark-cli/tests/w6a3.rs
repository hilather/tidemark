//! Owned by: W6-A3
//! Binary live command output and unavailable endpoint evidence.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    process::Command,
};
use tidemark_core::api::{Request, Response};
fn config(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(
        &p,
        format!(
            "[general]\nsocket='{}'\nstate_dir='{}'\n",
            dir.path().join("api.sock").display(),
            dir.path().join("state").display()
        ),
    )
    .unwrap();
    p
}
fn cli(path: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new("timeout")
        .arg("20s")
        .arg(env!("CARGO_BIN_EXE_tidemark"))
        .arg("-c")
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn live_commands_without_server_exit_four() {
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    for args in [
        vec!["now"],
        vec!["hot"],
        vec!["alerts"],
        vec!["alerts", "--all"],
        vec!["status"],
    ] {
        let out = cli(&p, &args);
        assert_eq!(
            out.status.code(),
            Some(4),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stderr)
            .contains("daemon unavailable — is tidemarkd running? (systemctl status tidemarkd)"));
    }
}
fn fake(
    p: &std::path::Path,
    replies: Vec<(&'static str, serde_json::Value)>,
) -> std::thread::JoinHandle<()> {
    let socket = p.parent().unwrap().join("api.sock");
    let listener = UnixListener::bind(socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let s = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                Err(error) => panic!("fake daemon accept deadline: {error}"),
            }
        };
        s.set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut s = BufReader::new(s);
        for (method, result) in replies {
            let mut line = String::new();
            s.read_line(&mut line).unwrap();
            let r: Request = serde_json::from_str(&line).unwrap();
            assert_eq!(r.method, method);
            serde_json::to_writer(
                s.get_mut(),
                &Response {
                    v: 1,
                    id: r.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                },
            )
            .unwrap();
            s.get_mut().write_all(b"\n").unwrap();
        }
    })
}
#[test]
fn live_tables_and_shared_byte_path_machine_output() {
    if !socket_available() {
        return;
    }
    let cases = [
        (
            vec!["scan", "/", "--mode", "walk"],
            vec![
                ("scan.start", serde_json::json!({"scan_id":"fixture"})),
                (
                    "scan.status",
                    serde_json::json!([{"root":"/","running":null,"last_complete":{"scan_id":"fixture","finished_at":0,"mode":"walk","duration_ms":0},"next_due":null}]),
                ),
            ],
            "scan fixture completed",
        ),
        (
            vec!["scan", "/", "--cancel"],
            vec![("scan.cancel", serde_json::json!({"cancelled":true}))],
            "scan cancellation requested: true",
        ),
        (
            vec!["lvm"],
            vec![(
                "lvm",
                serde_json::json!({"mounts":[],"vgs":[],"sampled_at":0}),
            )],
            "View from daemon",
        ),
        (
            vec!["writers"],
            vec![("writers", serde_json::json!([]))],
            "GROWTH  DISK",
        ),
        (
            vec!["phantom"],
            vec![(
                "phantom",
                serde_json::json!([{"mount":"/fixture","total_bytes":0,"files":0,"holders":[],"limited":false}]),
            )],
            "deleted open files",
        ),
        (
            vec!["now"],
            vec![(
                "now",
                serde_json::json!({"generated_at":0,"host":"fixture","mounts":[],"alerts_firing":[],"scans":[],"degraded":[]}),
            )],
            "tidemark now",
        ),
        (
            vec!["--json", "now"],
            vec![(
                "now",
                serde_json::json!({"generated_at":0,"host":"fixture","mounts":[],"alerts_firing":[],"scans":[],"degraded":[]}),
            )],
            "tidemark.now.v1",
        ),
        (
            vec!["status", "--show-config"],
            vec![
                (
                    "status",
                    serde_json::json!({"version":"fixture", "uptime_s":42,"components":[{"name":"api","state":"ok","detail":"ready"}],"watch_modes":[],"last_errors":[]}),
                ),
                ("scan.status", serde_json::json!([])),
            ],
            "the running daemon may hold an older configuration until it is reloaded",
        ),
        (
            vec!["hot"],
            vec![(
                "hot",
                serde_json::json!([{"path_b64":"L2JhZP8=","events":2,"files":1,"bytes_delta":12,"top_unit":"unit","lossy":true}]),
            )],
            "DELTA BYTES",
        ),
        (
            vec!["alerts"],
            vec![(
                "alerts.active",
                serde_json::json!([{"rule":"full","subject":"/data","since":1,"value":90,"message":"full"}]),
            )],
            "RULE  SUBJECT",
        ),
        (
            vec!["alerts", "--all"],
            vec![("alerts.history", serde_json::json!([]))],
            "History covers the current daemon run only.",
        ),
    ];
    for (args, replies, expected) in cases {
        let dir = ::test_support::private_tempdir().unwrap();
        let p = config(&dir);
        if args.first() == Some(&"scan") && !args.contains(&"--cancel") {
            catalog_entry(&dir, false, "complete");
        }
        let server = fake(&p, replies);
        let out = cli(&p, &args);
        server.join().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains(expected));
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    let server = fake(
        &p,
        vec![(
            "hot",
            serde_json::json!([{"path_b64":"L2JhZP8=","events":2,"files":1,"bytes_delta":12,"top_unit":null,"lossy":false}]),
        )],
    );
    let out = cli(&p, &["--json", "hot"]);
    server.join().unwrap();
    assert!(out.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["data"][0]["path_b64"], "L2JhZP8=");
    assert!(doc["data"][0].get("path").is_none());
}
#[test]
fn renderers_work_without_socket_permissions() {
    let context = tidemark_cli::Context {
        config: tidemark_core::config::Config::default(),
        config_path: "/fixture".into(),
        json: false,
        color: false,
    };
    let report = tidemark_core::live::NowReport {
        generated_at: 0,
        host: "fixture".into(),
        mounts: vec![],
        alerts_firing: vec![],
        scans: vec![],
        degraded: vec![],
    };
    let mut out = Vec::new();
    tidemark_cli::render::now::write(&report, &context, &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("tidemark now"));
    let mut out = Vec::new();
    tidemark_cli::render::hot::write(
        &[tidemark_core::live::HotEntry {
            key: b"/bad\xff".to_vec(),
            events: 2,
            files: 1,
            bytes_delta: 12,
            top_unit: None,
            lossy: true,
        }],
        true,
        &mut out,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["data"][0]["path_b64"], "L2JhZP8=");
    let mut out = Vec::new();
    tidemark_cli::render::alerts::history(&[], false, &mut out).unwrap();
    assert!(String::from_utf8(out)
        .unwrap()
        .contains("current daemon run only"));
}

fn socket_available() -> bool {
    let dir = ::test_support::private_tempdir().unwrap();
    match std::os::unix::net::UnixListener::bind(dir.path().join("probe.sock")) {
        Ok(_) => true,
        Err(e) if e.raw_os_error() == Some(1) => {
            eprintln!("pending unrestricted host: Unix socket bind denied by sandbox (EPERM)");
            false
        }
        Err(e) => panic!("socket probe failed: {e}"),
    }
}
#[test]
fn lvm_fallback_notice_with_collector_disabled() {
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    let mut text = std::fs::read_to_string(&p).unwrap();
    text.push_str("\n[lvm]\nenabled=false\n");
    std::fs::write(&p, text).unwrap();
    let out = cli(&p, &["lvm"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("falling back to direct lvm mode"));
}

#[test]
fn phantom_and_writers_fallback_notice_and_direct_setup_seam() {
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    let missing = dir.path().join("missing-mount");
    let missing = missing.to_str().unwrap();
    for command in ["phantom", "writers"] {
        let fallback = cli(&p, &[command, "--mount", missing]);
        let direct = cli(&p, &[command, "--direct", "--mount", missing]);
        let notice = format!("falling back to direct {command} mode");
        let stderr = String::from_utf8_lossy(&fallback.stderr);
        assert_eq!(stderr.matches(&notice).count(), 1, "{stderr}");
        assert!(!String::from_utf8_lossy(&direct.stderr).contains("falling back"));
        // The same direct canonicalization failure occurs before any /proc pass.
        assert_eq!(fallback.status.code(), direct.status.code(), "{stderr}");
        assert!(!fallback.status.success(), "missing mount accepted");
        assert_ne!(fallback.status.code(), Some(4), "{stderr}");
        let direct_error = String::from_utf8_lossy(&direct.stderr);
        assert!(
            stderr.ends_with(direct_error.as_ref()),
            "fallback: {stderr}; direct: {direct_error}"
        );
    }
}

#[test]
fn explicit_direct_never_connects_to_listening_daemon() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    let listener = UnixListener::bind(dir.path().join("api.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let missing = dir.path().join("missing-mount");
    for command in ["phantom", "writers"] {
        let out = cli(
            &p,
            &[command, "--direct", "--mount", missing.to_str().unwrap()],
        );
        assert!(
            !out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!String::from_utf8_lossy(&out.stderr).contains("falling back"));
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "{command} contacted daemon"
        );
    }
}

fn catalog_entry(dir: &tempfile::TempDir, degraded: bool, status: &str) {
    let entry = tidemark_store::catalog::ScanEntry {
        scan_id: "fixture".into(),
        root: tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap(),
        mode: tidemark_core::scan::ScanMode::Walk,
        started_at: 0,
        finished_at: Some(1),
        status: status.into(),
        degraded,
        file: None,
        t_disk: None,
        t_inodes: None,
        t_dirs: None,
        errors: None,
        duration_ms: None,
    };
    let catalog = tidemark_store::catalog::Catalog::open(&dir.path().join("state"), true).unwrap();
    catalog.record_start(&entry).unwrap();
    catalog.record_finish(&entry).unwrap();
}
#[test]
fn scan_resolves_only_its_catalog_outcome() {
    if !socket_available() {
        return;
    }
    for (degraded, status, expected) in [
        (false, "complete", 0),
        (true, "complete", 3),
        (false, "partial", 3),
        (false, "missing", 3),
    ] {
        for slot in ["completed", "later_running", "later_complete", "absent"] {
            let dir = ::test_support::private_tempdir().unwrap();
            let p = config(&dir);
            if status != "missing" {
                catalog_entry(&dir, degraded, status);
            }
            let running = if slot == "later_running" {
                serde_json::json!({"scan_id":"later","root":"/","mode":"walk","phase":"walk","dirs":0,"entries":0,"started_at":0,"rate":0.0})
            } else {
                serde_json::Value::Null
            };
            let latest = if slot == "later_complete" {
                "later"
            } else {
                "fixture"
            };
            let statuses = if slot == "absent" {
                serde_json::json!([])
            } else {
                serde_json::json!([{"root":"/","running":running,"last_complete":{"scan_id":latest,"finished_at":1,"mode":"walk","duration_ms":0},"next_due":null}])
            };
            let server = fake(
                &p,
                vec![
                    ("scan.start", serde_json::json!({"scan_id":"fixture"})),
                    ("scan.status", statuses),
                ],
            );
            let out = cli(
                &p,
                if degraded && status == "complete" {
                    &["--json", "scan", "/"]
                } else {
                    &["scan", "/"]
                },
            );
            server.join().unwrap();
            if status == "complete" {
                if degraded {
                    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
                    assert_eq!(doc["data"]["last_complete"]["scan_id"], "fixture");
                } else {
                    assert!(String::from_utf8_lossy(&out.stdout).contains("scan fixture completed"));
                }
            }
            assert_eq!(
                out.status.code(),
                Some(expected),
                "{slot} {status}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}
#[test]
fn scan_invalid_endpoint_never_writes_state() {
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    std::fs::write(dir.path().join("api.sock"), b"not a socket").unwrap();
    let config = std::fs::read_to_string(&p)
        .unwrap()
        .replace("api.sock'", "api.sock/child'");
    std::fs::write(&p, config).unwrap();
    for args in [
        vec!["scan", "/"],
        vec!["snapshots", "rebuild-catalog"],
        vec!["snapshots", "prune"],
    ] {
        let out = cli(&p, &args);
        assert_eq!(out.status.code(), Some(4));
        assert!(!dir.path().join("state").exists());
    }
}
#[test]
fn cells_and_prose_fit_injected_widths_without_splitting_escapes() {
    let path = format!("/begin/{}/last", "a".repeat(200));
    let escaped = tidemark_core::path::display_lossy_bytes(b"/bad\xff\n/last");
    for width in [80, 100, 200] {
        let table = tidemark_cli::render::table::aligned_width(
            &[
                vec!["PATH".into(), "VALUE".into()],
                vec![path.clone(), escaped.clone()],
            ],
            width,
        );
        assert!(table.lines().all(|line| line.chars().count() <= width));
        assert!(table.contains("/begin/") && table.contains("/last") && table.contains("..."));
        let text = tidemark_cli::render::table::wrap(
            &format!("{path} {escaped} full in ~2h (1h trend)"),
            width,
        );
        assert!(text
            .lines()
            .all(|line| line.chars().count() <= width || line.split_whitespace().count() == 1));
        assert!(text.contains(&escaped));
        assert!(!text.contains("tre\nnd"));
    }
}

#[test]
fn scan_full_backlog_expires_without_direct_state_writes() {
    if !socket_available() {
        return;
    }
    use rustix::net::{
        bind, connect, listen, socket_with, AddressFamily, SocketAddrUnix, SocketFlags, SocketType,
    };
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    let socket = dir.path().join("api.sock");
    let address = SocketAddrUnix::new(&socket).unwrap();
    let listener = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    bind(&listener, &address).unwrap();
    // Keep the queue unaccepted and independent of std's system-sized backlog.
    // Linux admits two pending connections for this requested backlog of one.
    listen(&listener, 1).unwrap();
    let mut pending = Vec::new();
    let mut full = false;
    for _ in 0..64 {
        let fd = socket_with(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::NONBLOCK | SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        match connect(&fd, &address) {
            Ok(()) => pending.push(fd),
            Err(rustix::io::Errno::AGAIN) => {
                full = true;
                break;
            }
            Err(error) => panic!("backlog fill failed: {error}"),
        }
    }
    if !full {
        eprintln!("pending unrestricted host: kernel did not bound the listener backlog within 64 connections");
        return;
    }
    // cli() allows 20 seconds, longer than the shared 11-second connect deadline.
    for args in [
        vec!["scan", "/"],
        vec!["snapshots", "rebuild-catalog"],
        vec!["snapshots", "prune"],
    ] {
        let start = std::time::Instant::now();
        let out = cli(&p, &args);
        assert_eq!(
            out.status.code(),
            Some(4),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(15));
        assert!(String::from_utf8_lossy(&out.stderr).contains("timed out"));
        assert!(!dir.path().join("state").exists());
    }
}

fn example_now() -> tidemark_core::live::NowReport {
    serde_json::from_value(serde_json::json!({
        "generated_at":1791845576,"host":"fixture","alerts_firing":[],"scans":[{"root":"/data","running":null,"next_due":null,"last_complete":{"scan_id":"a","finished_at":1791845576,"mode":"walk","duration_ms":5}}],"degraded":[],
        "mounts":[{"mount":"/data","size":1000000000,"used":50000000,"avail":950000000,"rate_15m":2362647.27,"rate_1h":500.0,"ttf_secs":189,"lvm":null,"hot_dirs_5m":[{"path":"/data/hot","events":3,"files":2,"bytes_delta":50000000,"top_unit":"writer.service","lossy":false},{"path_b64":"L2JhZP8=","events":1,"files":1,"bytes_delta":1,"top_unit":null,"lossy":false}],"writers_5m":[{"path":"other.service","events":3,"files":2,"bytes_delta":100,"top_unit":"other.service","lossy":false}],"phantom":{"mount":"/data","total_bytes":0,"files":0,"holders":[],"limited":false},"open_writers_top":[],"watch_mode":"fd/mount","lossy_minutes_15m":0}]
    })).unwrap()
}
#[test]
fn now_uses_report_rates_hot_directories_units_and_utc() {
    let report = example_now();
    for width in [80, 100, 200] {
        let text = tidemark_cli::render::now::render_text(&report, width);
        assert!(text.lines().all(|line| line.chars().count() <= width));
        assert!(text.contains("2026-10-12 22:52:56Z"));
        assert!(text.contains("writers (5m) other.service +100 B"));
        assert_eq!(text.matches("other.service").count(), 1);
        assert!(text.contains("7.92 GiB/h (15m)"), "{text}");
        assert!(text.contains("full in ~3m 9s (1h trend)"), "{text}");
        assert!(text.contains("hot dirs (5m) /data/hot") && text.contains("writer.service"));
        assert!(
            text.contains("/bad\\xFF") && text.contains("1 files  ?"),
            "{text}"
        );
    }
    let mut out = Vec::new();
    tidemark_cli::render::json("tidemark.now.v1", &report, &mut out).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(doc["data"]["mounts"][0]["rate_15m"], 2362647.27);
    assert_eq!(doc["data"]["mounts"][0]["ttf_secs"], 189);
    assert_eq!(doc["data"]["generated_at"], 1791845576);
}
#[test]
fn status_schedules_are_human_readable_and_fit_widths() {
    let context = tidemark_cli::Context {
        config: tidemark_core::config::Config::default(),
        config_path: "/fixture".into(),
        json: false,
        color: false,
    };
    let report: tidemark_core::api::StatusResult = serde_json::from_value(serde_json::json!({"version":"fixture","uptime_s":1,"components":[{"name":"api","state":"ok","detail":"a".repeat(200)}],"watch_modes":[],"last_errors":[["api",1791845576,"bad\nmessage"]]})).unwrap();
    let scans = vec![tidemark_core::live::ScanStatus {
        root: "/data".into(),
        running: None,
        next_due: Some(1791845576),
        last_complete: None,
    }];
    for width in [80, 100, 200] {
        let mut out = Vec::new();
        tidemark_cli::render::status::write_width(
            &report, &scans, &context, false, width, &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text
            .lines()
            .all(|line| line.chars().count() <= width || line.split_whitespace().count() == 1));
        assert!(
            text.contains("/data  no       2026-10-12 22:52:56Z  -"),
            "{text}"
        );
        assert!(text.contains("bad\\x0Amessage"), "{text}");
        assert!(!text.contains("Some(") && !text.contains("None"));
    }
    assert_eq!(
        tidemark_cli::render::size::timestamp(0),
        "1970-01-01 00:00:00Z"
    );
    assert_eq!(
        tidemark_cli::render::size::timestamp(-1),
        "1969-12-31 23:59:59Z"
    );
}
#[test]
fn hot_units_are_visible_without_repeating_unit_group_keys() {
    let mut rows = example_now().mounts.remove(0).hot_dirs_5m;
    rows[0].key = b"writer.service".to_vec();
    for width in [80, 100, 200] {
        let mut out = Vec::new();
        tidemark_cli::render::hot::write_grouped(&rows, false, width, true, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("writer.service").count(), 1);
        assert!(!text.contains("TOP UNIT"));
        let mut out = Vec::new();
        tidemark_cli::render::hot::write_width(&rows, false, width, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out)
                .unwrap()
                .matches("writer.service")
                .count(),
            2
        );
    }
}

#[test]
fn every_live_renderer_fits_long_byte_paths_and_controls() {
    let mut key = format!("/begin/{}/last", "a".repeat(200)).into_bytes();
    key.extend_from_slice("世界é".as_bytes());
    key.extend_from_slice(b"\xff\n");
    let row = tidemark_core::live::HotEntry {
        key: key.clone(),
        events: 1,
        files: 1,
        bytes_delta: 1,
        top_unit: Some("unit\nname".into()),
        lossy: false,
    };
    let phantom: tidemark_core::live::PhantomSummary = serde_json::from_value(serde_json::json!({"mount":"/data","total_bytes":1,"files":1,"limited":false,"holders":[{"pid":1,"comm":"writer","unit":null,"path":"/placeholder","dev":1,"ino":1,"bytes":1}]})).unwrap();
    let mut phantom = phantom;
    phantom.holders[0].path = key.clone();
    let mut writer: tidemark_core::live::OpenWriter = serde_json::from_value(serde_json::json!({"mount":"/data","pid":1,"comm":"writer","unit":null,"path":"/placeholder","dev":1,"ino":1,"size":1,"disk":1,"growth":1,"window_secs":1})).unwrap();
    writer.path = key.clone();
    let alerts = vec![tidemark_core::live::AlertSummary {
        rule: "rule".into(),
        subject: tidemark_core::path::display_lossy_bytes(&key),
        since: 0,
        value: Some(1.0),
        message: "m".repeat(200),
    }];
    let lvm: tidemark_core::api::LvmResult = serde_json::from_value(serde_json::json!({"mounts":[["/data",{"vg":"v".repeat(200),"lv":"lv","lv_size":1,"vg_free":1,"thin_pool":null,"data_pct":null,"meta_pct":null,"sampled_at":0}]],"vgs":[],"sampled_at":0})).unwrap();
    for width in [40, 80, 100, 200] {
        let mut outputs = Vec::new();
        let mut out = Vec::new();
        tidemark_cli::render::hot::write_width(std::slice::from_ref(&row), false, width, &mut out)
            .unwrap();
        outputs.push(out);
        let mut out = Vec::new();
        tidemark_cli::render::alerts::active_width(&alerts, false, width, &mut out).unwrap();
        outputs.push(out);
        let mut out = Vec::new();
        tidemark_cli::render::now::phantom_width(
            std::slice::from_ref(&phantom),
            false,
            width,
            &mut out,
        )
        .unwrap();
        outputs.push(out);
        let mut out = Vec::new();
        tidemark_cli::render::now::writers_width(
            std::slice::from_ref(&writer),
            false,
            width,
            &mut out,
        )
        .unwrap();
        outputs.push(out);
        let mut out = Vec::new();
        tidemark_cli::render::now::lvm_width(&lvm, &[], false, width, &mut out).unwrap();
        outputs.push(out);
        let mut report = example_now();
        report.mounts[0].hot_dirs_5m = vec![row.clone()];
        outputs.push(tidemark_cli::render::now::render_text(&report, width).into_bytes());
        for out in outputs {
            let text = String::from_utf8(out).unwrap();
            assert!(
                text.lines().all(
                    |line| tidemark_cli::render::table::display_width(line) <= width
                        || line.split_whitespace().count() == 1
                ),
                "{text}"
            );
            assert!(
                text.contains("...")
                    || text
                        .split_whitespace()
                        .any(|word| tidemark_cli::render::table::display_width(word) > width),
                "{text}"
            );
        }
        let mut out = Vec::new();
        tidemark_cli::render::hot::write_width(std::slice::from_ref(&row), true, width, &mut out)
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let decoded: Vec<tidemark_core::live::HotEntry> =
            serde_json::from_value(value["data"].clone()).unwrap();
        assert_eq!(decoded[0].key, key);
    }
}

#[test]
fn scan_permission_error_never_falls_back() {
    use std::os::unix::fs::PermissionsExt;
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    let text = std::fs::read_to_string(&p)
        .unwrap()
        .replace("api.sock'", "locked/api.sock'");
    std::fs::write(&p, text).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o0)).unwrap();
    let denied = std::fs::symlink_metadata(locked.join("api.sock"))
        .err()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied);
    if !denied {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("permission coverage unavailable: process can traverse mode-000 directory");
        return;
    }
    for args in [
        vec!["scan", "/"],
        vec!["snapshots", "rebuild-catalog"],
        vec!["snapshots", "prune"],
    ] {
        let out = cli(&p, &args);
        assert_eq!(out.status.code(), Some(4));
        assert!(!dir.path().join("state").exists());
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
}

// CLI-display2: packaged-daemon regressions, independent of sockets or terminal state.
const WATCHER_DETAIL: &str = "mode=fd/mount recovering=false overflows_total=0 read_errors=0 persistence_dropped=false; exact minute windows unavailable: collector omits empty-minute/loss records and boundary counters";
const DEGRADED_REASON: &str = "alert watch_lossy:/mnt/tidemark-install: not evaluable: exact completed-minute observation unavailable: collector lacks empty-minute/loss records and boundary overflow counters";

#[test]
fn packaged_status_wraps_details_and_aligns_roots() {
    let context = tidemark_cli::Context {
        config: tidemark_core::config::Config::default(),
        config_path: "/fixture".into(),
        json: false,
        color: false,
    };
    let report = serde_json::from_value(serde_json::json!({
        "version":"fixture", "uptime_s":1,
        "components":[{"name":"watcher:/mnt/tidemark-install","state":"ok","detail":WATCHER_DETAIL}],
        "watch_modes":[], "last_errors":[]
    })).unwrap();
    let scans = vec![tidemark_core::live::ScanStatus {
        root: "/mnt/tidemark-install".into(),
        running: None,
        next_due: None,
        last_complete: Some(tidemark_core::live::LastScan {
            scan_id: "a".into(),
            finished_at: 1791858543,
            mode: "walk".into(),
            duration_ms: 1,
        }),
    }];
    for width in [80, 100, 160] {
        let mut out = Vec::new();
        tidemark_cli::render::status::write_width(
            &report, &scans, &context, false, width, &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.lines().all(|line| line.len() <= width), "{text}");
        let lines: Vec<_> = text.lines().collect();
        let detail_column = lines[1].find("DETAIL").unwrap();
        let root_index = lines
            .iter()
            .position(|line| line.starts_with("ROOT"))
            .unwrap();
        let mut detail = Vec::new();
        for (i, line) in lines[2..root_index].iter().enumerate() {
            if i > 0 {
                assert!(line.starts_with(&" ".repeat(detail_column)), "{line}");
            }
            detail.extend(line[detail_column..].split_whitespace());
        }
        assert_eq!(detail.join(" "), WATCHER_DETAIL);
        for counter in [
            "recovering=false",
            "overflows_total=0",
            "read_errors=0",
            "persistence_dropped=false;",
        ] {
            assert!(text.contains(counter), "{text}");
        }
        let header = lines[root_index];
        let row = lines[root_index + 1];
        assert_eq!(header.find("RUNNING"), row.find("no"));
        let next_column = header.find("NEXT RUN").unwrap();
        assert!(row[next_column..].starts_with("-"));
        assert_eq!(
            header.find("LAST COMPLETE"),
            row.find("2026-10-13 02:29:03Z")
        );
    }
}

#[test]
fn packaged_now_continuations_belong_to_their_degraded_item() {
    let mut report = example_now();
    report.mounts.clear();
    report.scans.clear();
    report.degraded = vec![
        DEGRADED_REASON.into(),
        "watcher /mnt/tidemark-install: fd mode only".into(),
    ];
    for width in [80, 100, 160] {
        let text = tidemark_cli::render::now::render_text(&report, width);
        assert!(text.lines().all(|line| line.len() <= width));
        let lines: Vec<_> = text.lines().skip(1).collect();
        let next = lines
            .iter()
            .position(|line| line.starts_with("degraded: watcher"))
            .unwrap();
        assert!(next > 1);
        assert!(lines[1..next]
            .iter()
            .all(|line| line.starts_with("          ")));
        let reason = std::iter::once(lines[0].strip_prefix("degraded: ").unwrap())
            .chain(lines[1..next].iter().map(|line| line.trim_start()))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(reason, DEGRADED_REASON);
    }
}

#[test]
fn free_text_preserves_oversized_words_and_alert_columns() {
    let long_word = "x".repeat(180);
    for width in [80, 100, 160] {
        let text = tidemark_cli::render::table::wrap_prefixed(
            "degraded: ",
            &format!("before {long_word} after"),
            width,
        );
        assert_eq!(
            text,
            format!("degraded: before\n          {long_word}\n          after\n")
        );
        let rows = vec![tidemark_core::live::AlertSummary {
            rule: "watch_lossy".into(),
            subject: "/mnt/tidemark-install".into(),
            since: 0,
            value: None,
            message: WATCHER_DETAIL.into(),
        }];
        let mut out = Vec::new();
        tidemark_cli::render::alerts::active_width(&rows, false, width, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.lines().all(|line| line.len() <= width));
        let column = text.lines().next().unwrap().find("MESSAGE").unwrap();
        let lines: Vec<_> = text.lines().skip(1).collect();
        assert!(lines[1..]
            .iter()
            .all(|line| line.starts_with(&" ".repeat(column))));
        assert_eq!(
            lines
                .iter()
                .map(|line| line[column..].trim())
                .collect::<Vec<_>>()
                .join(" "),
            WATCHER_DETAIL
        );
        let events = vec![tidemark_core::live::AlertEvent {
            ts: 0,
            rule: "watch_lossy".into(),
            subject: "/mnt/tidemark-install".into(),
            state: tidemark_core::live::AlertState::Firing,
            value: None,
            message: WATCHER_DETAIL.into(),
        }];
        let mut out = Vec::new();
        tidemark_cli::render::alerts::history_width(&events, false, width, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.lines().all(|line| line.len() <= width), "{text}");
        let column = text.lines().next().unwrap().find("MESSAGE").unwrap();
        let message = text
            .lines()
            .skip(1)
            .take_while(|line| !line.starts_with("History"))
            .filter_map(|line| line.get(column..))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(message, WATCHER_DETAIL);
    }
}

#[test]
fn daemon_lvm_json_has_its_own_schema_and_unchanged_fields() {
    let report: tidemark_core::api::LvmResult =
        serde_json::from_value(serde_json::json!({"mounts":[],"vgs":[],"sampled_at":42})).unwrap();
    let mut out = Vec::new();
    tidemark_cli::render::now::lvm_width(&report, &[], true, 100, &mut out).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(doc["schema"], "tidemark.lvm.daemon.v1");
    assert_eq!(doc["data"], serde_json::to_value(report).unwrap());
}

#[test]
fn status_error_continuations_keep_the_message_and_prefix_indent() {
    let context = tidemark_cli::Context {
        config: tidemark_core::config::Config::default(),
        config_path: "/fixture".into(),
        json: false,
        color: false,
    };
    let report = serde_json::from_value(serde_json::json!({
        "version":"fixture","uptime_s":1,"components":[],"watch_modes":[],
        "last_errors":[["api",0,WATCHER_DETAIL]]
    }))
    .unwrap();
    let prefix = "last error api at 1970-01-01 00:00:00Z: ";
    for width in [80, 100, 160] {
        let mut out = Vec::new();
        tidemark_cli::render::status::write_width(&report, &[], &context, false, width, &mut out)
            .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.lines().all(|line| line.len() <= width));
        let lines: Vec<_> = text
            .lines()
            .skip_while(|line| !line.starts_with(prefix))
            .take_while(|line| !line.starts_with("ROOT"))
            .collect();
        assert!(lines.len() > 1);
        assert!(lines[1..]
            .iter()
            .all(|line| line.starts_with(&" ".repeat(prefix.len()))));
        assert_eq!(
            lines
                .iter()
                .map(|line| line[prefix.len()..].trim())
                .collect::<Vec<_>>()
                .join(" "),
            WATCHER_DETAIL
        );
    }
}

#[test]
fn direct_scan_space_refusal_has_state_context() {
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    // Refuse in preflight, before walking /, without exhausting the filesystem.
    let mut text = std::fs::read_to_string(&p).unwrap();
    text.push_str("[retention]\nmin_free_pct=100.0\n");
    std::fs::write(&p, text).unwrap();
    let out = cli(&p, &["-q", "--json", "scan", "/", "--mode", "walk"]);
    assert_eq!(out.status.code(), Some(5));
    let error = String::from_utf8_lossy(&out.stderr);
    let state = dir.path().join("state");
    if error
        == format!(
            "scan: /: scan state: {}: root mount changed during setup\n",
            state.display()
        )
    {
        eprintln!("pending unrestricted host: root mount identity differs in sandbox");
        return;
    }
    assert_eq!(
        error,
        format!(
            "scan: /: scan state: {}: NoSpace\n",
            dir.path().join("state").display()
        )
    );
}

#[test]
fn daemon_scan_space_refusal_matches_direct_exit_and_wording() {
    if !socket_available() {
        return;
    }
    use tidemark_core::api::{ApiError, ApiErrorCode};
    for (code, message, exit, expected) in [
        (
            ApiErrorCode::Unavailable,
            "scan: NoSpace",
            5,
            "scan: /: NoSpace\n",
        ),
        (
            ApiErrorCode::Unavailable,
            "scan: NoSpace: floor",
            5,
            "scan: /: NoSpace\n",
        ),
        (
            ApiErrorCode::Unavailable,
            "measurement unavailable",
            4,
            "scan: /: daemon request scan.start: measurement unavailable\n",
        ),
        (
            ApiErrorCode::Internal,
            "scan: NoSpace",
            1,
            "scan: /: daemon request scan.start: daemon error: scan: NoSpace\n",
        ),
    ] {
        let dir = ::test_support::private_tempdir().unwrap();
        let p = config(&dir);
        let listener = UnixListener::bind(dir.path().join("api.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(e) => panic!("fake daemon accept: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            let r: Request = serde_json::from_str(&line).unwrap();
            assert_eq!(r.method, "scan.start");
            serde_json::to_writer(
                stream.get_mut(),
                &Response {
                    v: 1,
                    id: r.id,
                    ok: false,
                    result: None,
                    error: Some(ApiError {
                        code,
                        message: message.into(),
                    }),
                },
            )
            .unwrap();
            stream.get_mut().write_all(b"\n").unwrap();
        });
        let out = cli(&p, &["--json", "scan", "/", "--mode", "walk"]);
        server.join().unwrap();
        assert_eq!(out.status.code(), Some(exit));
        let error = String::from_utf8_lossy(&out.stderr);
        assert_eq!(error, expected);
        if exit == 5 {
            // Direct mode additionally includes scan state context; see the
            // separate direct-mode test and GATE-fix2 handoff for this gap.
            assert!(!error.contains("scan: scan:"), "{error}");
            assert!(!error.contains("daemon error"), "{error}");
        }
    }
}

#[test]
fn catalog_mutations_preserve_existing_corrupt_state_on_uncertain_endpoint() {
    use std::os::unix::fs::MetadataExt;
    let dir = ::test_support::private_tempdir().unwrap();
    let p = config(&dir);
    std::fs::write(dir.path().join("api.sock"), b"not a directory").unwrap();
    let text = std::fs::read_to_string(&p)
        .unwrap()
        .replace("api.sock'", "api.sock/child'");
    std::fs::write(&p, text).unwrap();
    let state = dir.path().join("state");
    ::test_support::private_dir(&state).unwrap();
    let mut original = Vec::new();
    for name in ["timeseries.db", "timeseries.db-wal", "timeseries.db-shm"] {
        let path = state.join(name);
        std::fs::write(&path, b"preserve corrupt state").unwrap();
        original.push((path.clone(), std::fs::metadata(&path).unwrap().ino()));
    }
    for operation in ["rebuild-catalog", "prune"] {
        let out = cli(&p, &["snapshots", operation]);
        assert_eq!(out.status.code(), Some(4));
        assert_eq!(std::fs::read_dir(&state).unwrap().count(), 3);
        for (path, inode) in &original {
            assert_eq!(std::fs::metadata(path).unwrap().ino(), *inode);
            assert_eq!(std::fs::read(path).unwrap(), b"preserve corrupt state");
        }
    }
}

#[test]
fn live_human_state_and_scan_labels_are_lowercase() {
    let mut report = example_now();
    report.scans[0].last_complete.as_mut().unwrap().mode = "Walk".into();
    let context = tidemark_cli::Context {
        config: Default::default(),
        config_path: "/fixture".into(),
        json: false,
        color: false,
    };
    let status = serde_json::from_value(serde_json::json!({"version":"fixture","uptime_s":1,"components":[{"name":"scan","state":"Complete","detail":"ready"}],"watch_modes":[["/data","Fid"]],"last_errors":[]})).unwrap();
    for width in [80, 100] {
        let text = tidemark_cli::render::now::render_text(&report, width);
        assert!(text.contains("walk"), "{text}");
        assert!(!text.contains("Walk"));
        let mut out = Vec::new();
        tidemark_cli::render::status::write_width(&status, &[], &context, false, width, &mut out)
            .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("complete") && text.contains("fid"), "{text}");
        assert!(!text.contains("Complete") && !text.contains("Fid"));
    }
}

#[test]
fn writer_unit_key_json_uses_the_frozen_core_serializer() {
    let mut report = example_now();
    report.mounts[0].writers_5m[0].key = b"user-1000".to_vec();
    let context = tidemark_cli::Context {
        config: Default::default(),
        config_path: "/fixture".into(),
        json: true,
        color: false,
    };
    let mut out = Vec::new();
    tidemark_cli::render::now::write(&report, &context, &mut out).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let entry = &report.mounts[0].writers_5m[0];
    assert_eq!(
        doc["data"]["mounts"][0]["writers_5m"][0],
        serde_json::to_value(entry).unwrap()
    );
    assert_eq!(
        doc["data"]["mounts"][0]["writers_5m"][0]["key"],
        serde_json::json!(b"user-1000".to_vec())
    );
    assert!(doc["data"]["mounts"][0]["writers_5m"][0]
        .get("path")
        .is_none());
}

#[test]
fn appended_watcher_cap_detail_wraps_without_losing_reason() {
    const DETAIL: &str = "truncated: distinct-file cap reached in the minute 10:42 UTC (timestamp 1791888120) (events complete, per-file counts capped)";
    let context = tidemark_cli::Context {
        config: Default::default(),
        config_path: "/fixture".into(),
        json: false,
        color: false,
    };
    let status = serde_json::from_value(serde_json::json!({"version":"fixture","uptime_s":1,"components":[{"name":"watcher:/data","state":"degraded","detail":DETAIL}],"watch_modes":[],"last_errors":[]})).unwrap();
    let mut report = example_now();
    report.mounts.clear();
    report.scans.clear();
    report.degraded = vec![DETAIL.into()];
    for width in [80, 100] {
        let mut out = Vec::new();
        tidemark_cli::render::status::write_width(&status, &[], &context, false, width, &mut out)
            .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.lines().all(|line| line.len() <= width), "{text}");
        let lines: Vec<_> = text.lines().collect();
        let column = lines[1].find("DETAIL").unwrap();
        let end = lines
            .iter()
            .position(|line| line.starts_with("ROOT"))
            .unwrap();
        assert!(end > 3, "{text}");
        assert!(lines[3..end]
            .iter()
            .all(|line| line.starts_with(&" ".repeat(column))));
        assert_eq!(
            lines[2..end]
                .iter()
                .flat_map(|line| line[column..].split_whitespace())
                .collect::<Vec<_>>()
                .join(" "),
            DETAIL
        );
        let text = tidemark_cli::render::now::render_text(&report, width);
        assert!(text.lines().all(|line| line.len() <= width), "{text}");
        let lines: Vec<_> = text.lines().skip(1).collect();
        assert!(lines.len() > 1);
        assert!(lines[1..].iter().all(|line| line.starts_with("          ")));
        let detail = std::iter::once(lines[0].strip_prefix("degraded: ").unwrap())
            .chain(lines[1..].iter().map(|line| line.trim_start()))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(detail, DETAIL);
    }
}

#[test]
fn unavailable_process_measurements_keep_daemon_reason_and_name_method() {
    if !socket_available() {
        return;
    }
    for method in ["writers", "phantom"] {
        let dir = test_support::private_tempdir().unwrap();
        let p = config(&dir);
        let listener = UnixListener::bind(dir.path().join("api.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(error) => panic!("fake unavailable daemon accept: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            let request: Request = serde_json::from_str(&line).unwrap();
            assert_eq!(request.method, method);
            serde_json::to_writer(stream.get_mut(), &serde_json::json!({"v":1,"id":request.id,"ok":false,"error":{"code":"unavailable","message":"unavailable: measurement unavailable"}})).unwrap();
            stream.get_mut().write_all(b"\n").unwrap();
        });
        let out = cli(&p, &[method]);
        server.join().unwrap();
        let text = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(4), "{text}");
        assert!(
            text.contains(&format!("daemon request {method}"))
                && text.contains("unavailable: measurement unavailable"),
            "{text}"
        );
        assert!(
            text.contains(&dir.path().join("api.sock").display().to_string()),
            "{text}"
        );
    }
}

#[test]
fn pre_api_writer_child() {
    let Some(state) = std::env::var_os("TIDEMARK_CLI_LEASE_CHILD") else {
        return;
    };
    let state = std::path::PathBuf::from(state);
    let _catalog = tidemark_store::catalog::Catalog::open(&state, true).unwrap();
    let _series =
        tidemark_store::timeseries::TimeSeries::open(&state.join("timeseries.db"), true).unwrap();
    test_support::private_write(state.join("ready"), b"ready").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(60));
}

#[test]
fn rebuild_refuses_pre_api_writer_without_renaming() {
    use std::{
        os::unix::fs::MetadataExt,
        time::{Duration, Instant},
    };
    let dir = test_support::private_tempdir().unwrap();
    let p = config(&dir);
    let state = dir.path().join("state");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "pre_api_writer_child"])
        .env("TIDEMARK_CLI_LEASE_CHILD", &state)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !state.join("ready").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut originals = Vec::new();
    for name in ["timeseries.db", "timeseries.db-wal", "timeseries.db-shm"] {
        let path = state.join(name);
        test_support::private_write(&path, b"deliberate corruption").unwrap();
        originals.push((path.clone(), std::fs::metadata(path).unwrap().ino()));
    }
    let start = Instant::now();
    let out = Command::new("timeout")
        .arg("40s")
        .arg(env!("CARGO_BIN_EXE_tidemark"))
        .arg("-c")
        .arg(&p)
        .args(["snapshots", "rebuild-catalog"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr)
        .contains("state directory in use by another tidemark process"));
    assert!(start.elapsed() < Duration::from_secs(35));
    for (path, inode) in originals {
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(std::fs::read(path).unwrap(), b"deliberate corruption");
    }
    assert!(!state.join("timeseries-recovery-0").exists());
    child.kill().unwrap();
    child.wait().unwrap();
    let out = cli(&p, &["snapshots", "rebuild-catalog"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(state.join("timeseries-recovery-0/timeseries.db").exists());
}
