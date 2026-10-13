//! Owned by: TEST-e2e-scan
//! Cancellation, retained partials, lock contention and invalid preflight.
use super::support::{direct_config, no_temporaries, Harness};
use std::time::Duration;
use tidemark_core::scan::SnapshotStatus;
struct ChildGuard(Option<std::process::Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn interrupt_scan(
    keep_partial: bool,
) -> (std::process::Output, tempfile::TempDir, tempfile::TempDir) {
    let fixture = ::test_support::private_tempdir().unwrap();
    for i in 0..100 {
        let dir = fixture.path().join(format!("dir{i}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::File::create(dir.join("file")).unwrap();
    }
    let state = ::test_support::private_tempdir().unwrap();
    let config = direct_config(
        state.path(),
        "[throttle]\nmin_ops_per_sec=1\nmax_ops_per_sec=1\n",
    );
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"));
    command
        .arg("--config")
        .arg(&config)
        .arg("--state-dir")
        .arg(state.path())
        .arg("--json")
        .arg("scan")
        .arg(fixture.path())
        .args(["--mode", "walk", "--threads", "1", "--throttle", "gentle"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if keep_partial {
        command.arg("--keep-partial");
    }
    let mut child = command.spawn().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let stderr_reader = std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stderr);
        let mut bytes = Vec::new();
        let mut line = Vec::new();
        let mut reported = false;
        while reader.read_until(b'\n', &mut line).unwrap() != 0 {
            let text = String::from_utf8_lossy(&line);
            if !reported && text.starts_with("walk: ") {
                let dirs = text
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .parse::<u64>()
                    .unwrap();
                if dirs > 0 {
                    ready_tx.send(()).unwrap();
                    reported = true;
                }
            }
            bytes.extend_from_slice(&line);
            line.clear();
        }
        bytes
    });
    let pid = child.id();
    let mut guard = ChildGuard(Some(child));
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(catalog) = tidemark_store::catalog::Catalog::open(state.path(), false) {
            if catalog
                .list(None)
                .unwrap()
                .iter()
                .any(|e| e.status == "running")
            {
                break;
            }
        }
        assert!(std::time::Instant::now() < deadline, "scan never started");
        assert!(
            guard.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "scan exited before interruption"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Interrupt only after the collector has accepted at least one directory.
    ready_rx
        .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        .expect("scan never reported a collected directory");
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid as i32).unwrap(),
        rustix::process::Signal::INT,
    )
    .unwrap();
    // The cancellation diagnostic is emitted only after durable catalog/snapshot
    // bookkeeping. This bounds test completion, not cancellation of scan work.
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    loop {
        if guard.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "cancelled scan did not exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut output = guard.0.take().unwrap().wait_with_output().unwrap();
    output.stderr = stderr_reader.join().unwrap();
    (output, state, fixture)
}

#[test]
fn interrupted_cancelled_and_failed_scans() {
    for keep in [false, true] {
        let (output, state, _fixture) = interrupt_scan(keep);
        assert_eq!(
            output.status.code(),
            Some(3),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let catalog = tidemark_store::catalog::Catalog::open(state.path(), false).unwrap();
        let rows = catalog.list(None).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].finished_at.is_some());
        assert_eq!(rows[0].status, if keep { "partial" } else { "failed" });
        assert!(catalog.latest(&rows[0].root).unwrap().is_none());
        if keep {
            let file = rows[0].file.as_ref().unwrap();
            assert!(file.is_file());
            let snapshot = tidemark_store::Snapshot::open(file).unwrap();
            assert_eq!(snapshot.meta().status, SnapshotStatus::Partial);
            // Publish a complete current scan, then move the retained partial into
            // the historical window. A wrongly admitted partial would now be the
            // only possible growth baseline, so the expected refusal is decisive.
            let mut old = catalog.get(&rows[0].scan_id).unwrap().unwrap();
            drop(catalog);
            let scan = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
                .arg("--config")
                .arg(direct_config(state.path(), ""))
                .arg("--state-dir")
                .arg(state.path())
                .arg("--json")
                .arg("scan")
                .arg(rows[0].root.as_path())
                .args(["--mode", "walk", "--throttle", "max"])
                .output()
                .unwrap();
            assert_eq!(scan.status.code(), Some(0));
            let scan: serde_json::Value = serde_json::from_slice(&scan.stdout).unwrap();
            let current = scan["data"]["scan_id"].as_str().unwrap();
            old.started_at = tidemark_core::time::unix_now() - 86400;
            let writable = tidemark_store::catalog::Catalog::open(state.path(), true).unwrap();
            writable.record_finish(&old).unwrap();
            let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
                .arg("--config")
                .arg(direct_config(state.path(), ""))
                .arg("--state-dir")
                .arg(state.path())
                .arg("top")
                .arg(rows[0].root.as_path())
                .args(["--snapshot", current, "--sort", "growth"])
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(5),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        } else {
            assert!(rows[0].file.is_none());
            assert!(!state.path().join("snapshots").exists());
        }
        no_temporaries(state.path());
    }
    let h = Harness::new();
    let root = ::test_support::private_tempdir().unwrap();
    super::support::accessible(root.path());
    for opts in [["--threads", "0"], ["--threads", "65"], ["--exclude", "["]] {
        h.run(
            true,
            &[
                "scan".as_ref(),
                root.path().as_os_str(),
                opts[0].as_ref(),
                opts[1].as_ref(),
            ],
            2,
        );
        assert!(!h.state.exists(), "invalid overrides created state");
    }
    // A separate child acquires the lock and explicitly signals readiness.
    // Its bounded stdin lifetime releases it even if an assertion panics.
    h.prepare_state();
    let mut child = h.fixture_command("python3")
        .args(["-c", "import fcntl,sys; import os; f=os.fdopen(os.open(sys.argv[1], os.O_WRONLY|os.O_CREAT, 0o600), 'w'); fcntl.flock(f, fcntl.LOCK_EX); print('locked', flush=True); sys.stdin.read()"])
        .arg(h.state.join("scan.lock"))
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    // The lock file is test-owned; make it openable by the CLI child as well.
    let stdout = child.stdout.take().unwrap();
    let mut guard = ChildGuard(Some(child));
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        use std::io::BufRead;
        let mut line = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .unwrap();
        tx.send(line).unwrap();
    });
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(30)).unwrap().trim(),
        "locked"
    );
    h.run(
        false,
        &[
            "scan".as_ref(),
            root.path().as_os_str(),
            "--mode".as_ref(),
            "walk".as_ref(),
        ],
        5,
    );
    drop(guard.0.as_mut().unwrap().stdin.take());
    assert!(guard.0.take().unwrap().wait().unwrap().success());
    reader.join().unwrap();
}

#[test]
fn unrelated_listener_does_not_redirect_direct_scan() {
    use std::os::unix::net::UnixListener;
    let h = Harness::new();
    let socket = h.temp.path().join("unrelated-api.sock");
    let listener = match UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipped unrelated listener: Unix socket bind denied by sandbox ({error})");
            return;
        }
        Err(error) => panic!("unrelated listener bind failed: {error}"),
    };
    listener.set_nonblocking(true).unwrap();
    let root = h.temp.path().join("fixture");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("file"), b"fixture").unwrap();
    h.run(
        true,
        &[
            "scan".as_ref(),
            root.as_os_str(),
            "--mode".as_ref(),
            "walk".as_ref(),
            "--throttle".as_ref(),
            "max".as_ref(),
        ],
        0,
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    let catalog = tidemark_store::catalog::Catalog::open(&h.state, false).unwrap();
    let rows = catalog.list(None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "complete");
}

#[cfg(feature = "privileged")]
#[test]
fn strict_ancestry_refuses_second_uid_before_writes() {
    use std::os::unix::{fs::PermissionsExt, process::CommandExt};
    let temp = test_support::private_tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let foreign = temp.path().join("foreign");
    test_support::private_dir(&foreign).unwrap();
    std::fs::set_permissions(&foreign, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = std::process::Command::new("true")
        .uid(65533)
        .gid(65533)
        .output()
        .unwrap();
    assert!(out.status.success());
    rustix::fs::chown(&foreign, Some(rustix::process::Uid::from_raw(65533)), None).unwrap();
    let sentinel = temp.path().join("sentinel");
    test_support::private_write(&sentinel, b"unchanged").unwrap();
    let state = foreign.join("state");
    let config = direct_config(temp.path(), "");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
        .arg("--config")
        .arg(config)
        .env("TIDEMARK_STRICT_ANCESTRY", "1")
        .arg("--state-dir")
        .arg(&state)
        .arg("scan")
        .arg(&foreign)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("untrusted owner"));
    assert!(!state.exists());
    assert_eq!(std::fs::read(sentinel).unwrap(), b"unchanged");
}
