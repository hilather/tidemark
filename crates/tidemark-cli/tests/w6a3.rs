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
    Command::new(env!("CARGO_BIN_EXE_tidemark"))
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
            vec!["status"],
            vec![
                (
                    "status",
                    serde_json::json!({"version":"fixture", "uptime_s":42,"components":[{"name":"api","state":"ok","detail":"ready"}],"watch_modes":[],"last_errors":[]}),
                ),
                ("scan.status", serde_json::json!([])),
            ],
            "COMPONENT  STATE  DETAIL",
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
