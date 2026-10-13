//! Owned by: W6-A3
//! One real-binary operator scenario, requiring Unix bind permission.
use super::*;
use std::{
    os::unix::ffi::OsStrExt,
    process::{Child, Command, Output},
    time::Instant,
};
fn containing_mount(path: &std::path::Path) -> std::path::PathBuf {
    let mounts = tidemark_fs::mount::read_mountinfo().unwrap();
    let raw =
        tidemark_core::path::RawPath::from_path(&std::fs::canonicalize(path).unwrap()).unwrap();
    tidemark_fs::mount::mount_for_path(&mounts, &raw)
        .unwrap()
        .mount_point
        .as_path()
        .to_path_buf()
}
struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(stderr: &std::path::Path, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !f() {
        assert!(
            Instant::now() < deadline,
            "operator state deadline: {}",
            std::fs::read_to_string(stderr).unwrap()
        );
        std::thread::yield_now();
    }
}
fn cli(config: &std::path::Path, args: &[&str]) -> Output {
    let executable =
        std::path::Path::new(env!("CARGO_BIN_EXE_tidemarkd")).with_file_name("tidemark");
    let out = Command::new(executable)
        .arg("-c")
        .arg(config)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn query(socket: &std::path::Path, method: &str, params: serde_json::Value) -> Response {
    let s = UnixStream::connect(socket).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(11))).unwrap();
    let mut s = BufReader::new(s);
    write(
        &mut s,
        &Request {
            v: 1,
            id: "operator".into(),
            method: method.into(),
            params,
        },
    );
    read(&mut s)
}
#[test]
fn real_operator_status_scan_top_now_reload_cancel_and_sigterm() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir(&root).unwrap();
    let bad = root.join(std::ffi::OsStr::from_bytes(b"nonutf\xff"));
    std::fs::write(&bad, b"fixture").unwrap();
    std::fs::hard_link(&bad, root.join("linked")).unwrap();
    let config = dir.path().join("config.toml");
    let socket = dir.path().join("api.sock");
    let state = dir.path().join("state");
    let text=format!("[general]\nstate_dir='{}'\nsocket='{}'\n[sampler]\nmounts=['{}']\ninterval='1s'\n[lvm]\nenabled=false\n[watch]\nenabled=false\nmounts=[]\n[procscan]\nenabled=false\n[metrics]\ntextfile='{}'\n[throttle]\nmin_ops_per_sec=10\nmax_ops_per_sec=10\n[[roots]]\npath='{}'\nmode='walk'\nschedule='off'\n",state.display(),socket.display(),containing_mount(&root).display(),dir.path().join("metrics.prom").display(),root.display());
    std::fs::write(&config, &text).unwrap();
    // Root-only production API requires root for this full operator scenario.
    if rustix::process::geteuid().as_raw() != 0 {
        eprintln!("pending root host: production API serves uid 0 only");
        return;
    }
    let stderr_path = dir.path().join("daemon.stderr");
    let stderr = std::fs::File::create(&stderr_path).unwrap();
    let mut daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_tidemarkd"))
            .arg("--config")
            .arg(&config)
            .stdout(std::process::Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    wait(&stderr_path, || {
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "daemon exited before API readiness: {}",
            std::fs::read_to_string(&stderr_path).unwrap()
        );
        socket.exists()
    });
    let out = cli(&config, &["status"]);
    let status = String::from_utf8_lossy(&out.stdout);
    assert!(status.contains("api") && status.contains("sampler") && status.contains("disabled"));
    let path = root.to_str().unwrap();
    cli(
        &config,
        &["scan", path, "--mode", "walk", "--throttle", "max"],
    );
    cli(&config, &["top", path]);
    cli(&config, &["now"]);
    let out = cli(&config, &["--json", "now"]);
    let now: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(now["schema"], "tidemark.now.v1");
    cli(&config, &["alerts"]);
    cli(&config, &["alerts", "--all"]);
    let changed=format!("{text}\n[[alerts.rule]]\nname='operator_changed'\nkind='fs_used_pct'\nmount='{}'\nabove=99\nfor='1s'\n",root.display());
    std::fs::write(&config, &changed).unwrap();
    let reload = query(&socket, "config.reload", serde_json::json!({}));
    assert!(reload.ok);
    assert_eq!(reload.result.unwrap()["reloaded"], true);
    wait(&stderr_path, || {
        let now = query(&socket, "now", serde_json::json!({}));
        now.ok && now.result.unwrap().to_string().contains("operator_changed")
    });
    std::fs::write(&config, "unknown_config_key=true").unwrap();
    let reload = query(&socket, "config.reload", serde_json::json!({}));
    assert!(reload.ok);
    let result = reload.result.unwrap();
    assert_eq!(result["reloaded"], false);
    assert!(result["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().is_some_and(|w| w.contains("unknown_config_key"))));
    // The retained rule remains visible in now's unevaluable diagnostics even
    // because the rule targets the unsampled temporary scan root.
    let now = query(&socket, "now", serde_json::json!({}));
    assert!(now.ok);
    assert!(now.result.unwrap().to_string().contains("operator_changed"));
    std::fs::write(&config, &changed).unwrap();
    // A large inventory with a ten-op gate ensures cancellation is observed
    // before completion without guessing a sleep duration.
    for n in 0..1000 {
        std::fs::create_dir(root.join(format!("cancel-{n:04}"))).unwrap();
    }
    let started = query(
        &socket,
        "scan.start",
        serde_json::json!({"root":path,"mode":"walk","throttle":"gentle"}),
    );
    assert!(started.ok);
    let id = started.result.unwrap()["scan_id"]
        .as_str()
        .unwrap()
        .to_owned();
    cli(&config, &["scan", path, "--cancel"]);
    wait(&stderr_path, || {
        let r = query(&socket, "scan.status", serde_json::json!({"root":path}));
        assert!(r.ok);
        let statuses: Vec<tidemark_core::live::ScanStatus> =
            serde_json::from_value(r.result.unwrap()).unwrap();
        statuses.first().is_some_and(|s| s.running.is_none())
    });
    let catalog = tidemark_store::catalog::Catalog::open(&state, false).unwrap();
    let rows = catalog.list(None).unwrap();
    let failed = rows.iter().find(|r| r.scan_id == id).unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.file.is_none());
    drop(catalog);
    let snapshot_root = state.join("snapshots");
    for d in std::fs::read_dir(snapshot_root).unwrap() {
        for f in std::fs::read_dir(d.unwrap().path()).unwrap() {
            assert!(!f
                .unwrap()
                .file_name()
                .as_encoded_bytes()
                .ends_with(b".db.tmp"));
        }
    }
    let start = Instant::now();
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(daemon.0.id() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    let mut exit = None;
    wait(&stderr_path, || {
        exit = daemon.0.try_wait().unwrap();
        exit.is_some()
    });
    assert!(exit.unwrap().success());
    assert!(start.elapsed() < Duration::from_secs(10));
    assert!(!socket.exists());
}

#[test]
fn real_binary_scheduled_scan_publishes_then_exits_cleanly() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("file"), b"fixture").unwrap();
    let state = dir.path().join("state");
    let config = dir.path().join("config.toml");
    let stderr_path = dir.path().join("daemon.stderr");
    let text = format!("[general]\nstate_dir='{}'\nsocket='{}'\n[sampler]\nmounts=['{}']\ninterval='1h'\n[lvm]\nenabled=false\n[watch]\nenabled=false\nmounts=[]\n[procscan]\nenabled=false\n[metrics]\ntextfile='{}'\n[[roots]]\npath='{}'\nmode='walk'\nthrottle='max'\nschedule='every 1h'\n", state.display(), dir.path().join("api.sock").display(), containing_mount(&root).display(), dir.path().join("metrics.prom").display(), root.display());
    std::fs::write(&config, text).unwrap();
    let diagnostics = || std::fs::read_to_string(&stderr_path).unwrap_or_default();
    let mut daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_tidemarkd"))
            .arg("--config")
            .arg(&config)
            .env_remove("INVOCATION_ID")
            .env("RUST_LOG", "info")
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(&stderr_path).unwrap())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn: {e}; {}", diagnostics())),
    );
    let snapshots = state
        .join("snapshots")
        .join(tidemark_store::catalog::root_slug(
            &tidemark_core::path::RawPath::from_path(&root).unwrap(),
        ));
    wait(&stderr_path, || {
        assert!(
            daemon
                .0
                .try_wait()
                .unwrap_or_else(|e| panic!("poll: {e}; {}", diagnostics()))
                .is_none(),
            "daemon exited before snapshot: {}",
            diagnostics()
        );
        std::fs::read_dir(&snapshots).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "db"))
        })
    });
    let pid = rustix::process::Pid::from_raw(daemon.0.id() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::TERM)
        .unwrap_or_else(|e| panic!("SIGTERM: {e}; {}", diagnostics()));
    wait(&stderr_path, || {
        daemon
            .0
            .try_wait()
            .unwrap_or_else(|e| panic!("poll shutdown: {e}; {}", diagnostics()))
            .is_some()
    });
    let status = daemon
        .0
        .wait()
        .unwrap_or_else(|e| panic!("wait: {e}; {}", diagnostics()));
    assert_eq!(status.code(), Some(0), "{}", diagnostics());
}
