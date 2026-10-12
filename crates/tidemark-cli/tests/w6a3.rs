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
    let dir = tempfile::tempdir().unwrap();
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
    std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
            let dir = tempfile::tempdir().unwrap();
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
            let out = cli(&p, &["scan", "/"]);
            server.join().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let p = config(&dir);
    std::fs::write(dir.path().join("api.sock"), b"not a socket").unwrap();
    let config = std::fs::read_to_string(&p)
        .unwrap()
        .replace("api.sock'", "api.sock/child'");
    std::fs::write(&p, config).unwrap();
    let out = cli(&p, &["scan", "/"]);
    assert_eq!(out.status.code(), Some(4));
    assert!(!dir.path().join("state").exists());
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
        assert!(text.lines().all(|line| line.chars().count() <= width));
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
        connect, socket_with, AddressFamily, SocketAddrUnix, SocketFlags, SocketType,
    };
    let dir = tempfile::tempdir().unwrap();
    let p = config(&dir);
    let socket = dir.path().join("api.sock");
    let _listener = UnixListener::bind(&socket).unwrap();
    let address = SocketAddrUnix::new(&socket).unwrap();
    let mut pending = Vec::new();
    let mut full = false;
    for _ in 0..256 {
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
    assert!(full, "did not fill bounded listener backlog");
    let out = cli(&p, &["scan", "/"]);
    assert_eq!(
        out.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("timed out"));
    assert!(!dir.path().join("state").exists());
}
