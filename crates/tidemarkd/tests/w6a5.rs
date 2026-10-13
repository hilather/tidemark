//! Owned by: WATCH-fd-budget
//! Real daemon survival with a low descriptor limit; socket-capable root CI only.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    time::{Duration, Instant},
};

#[test]
fn low_descriptor_limit_binary_serves_twenty_status_requests() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    match UnixListener::bind(dir.path().join("probe.sock")) {
        Ok(listener) => drop(listener),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("pending coordinator CI: sandbox denies Unix sockets");
            return;
        }
        Err(error) => panic!("socket probe: {error}"),
    }
    if rustix::process::geteuid().as_raw() != 0 {
        eprintln!("pending root CI: production API requires root peers");
        return;
    }
    let socket = dir.path().join("api.sock");
    let config = dir.path().join("config.toml");
    std::fs::write(&config, format!("[general]\nstate_dir='{}'\nsocket='{}'\n[sampler]\nmounts=['/']\ninterval='1s'\n[lvm]\nenabled=false\n[watch]\nenabled=false\nmounts=[]\n[procscan]\nenabled=false\n[metrics]\ntextfile='{}'\n[[roots]]\npath='/'\nschedule='off'\n", dir.path().join("state").display(), socket.display(), dir.path().join("metrics.prom").display())).unwrap();
    let mut child = std::process::Command::new("sh")
        .args([
            "-c",
            "ulimit -n 64; exec \"$1\" --config \"$2\"",
            "tidemark-low-fd",
        ])
        .arg(env!("CARGO_BIN_EXE_tidemarkd"))
        .arg(config)
        .env_remove("NOTIFY_SOCKET")
        .env_remove("INVOCATION_ID")
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "daemon exited before ready"
            );
            if UnixStream::connect(&socket).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline, "readiness deadline");
            std::thread::sleep(Duration::from_millis(20));
        }
        for n in 0..20 {
            let mut stream = UnixStream::connect(&socket).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            writeln!(
                stream,
                "{{\"v\":1,\"id\":\"{n}\",\"method\":\"status\",\"params\":{{}}}}"
            )
            .unwrap();
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).unwrap();
            let response: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert!(response.get("error").is_none(), "{response}");
            assert!(response.get("result").is_some(), "{response}");
        }
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
            rustix::process::Signal::TERM,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(exit) = child.try_wait().unwrap() {
                assert!(exit.success(), "{exit}");
                break;
            }
            assert!(Instant::now() < deadline, "shutdown deadline");
            std::thread::sleep(Duration::from_millis(20));
        }
    }));
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
