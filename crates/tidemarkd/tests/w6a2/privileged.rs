//! Owned by: W6-A2a
//! Disposable-VM SIGHUP and SIGTERM scan-publication gate, compile-only here.
use std::{
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};
fn wait(mut f: impl FnMut() -> bool, deadline: Instant, logs: &impl Fn() -> String) {
    while !f() {
        assert!(
            Instant::now() < deadline,
            "VM daemon deadline exceeded; stderr: {}",
            logs()
        );
        std::thread::yield_now();
    }
}
struct StopChild(Child);
impl Drop for StopChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn signal(child: &Child, signal: rustix::process::Signal) {
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
        signal,
    )
    .unwrap();
}
fn write_config(path: &Path, threshold: u32) {
    let p = path.parent().unwrap();
    let table = tidemark_fs::mount::read_mountinfo().unwrap();
    let raw = tidemark_core::path::RawPath::from_path(&std::fs::canonicalize(p).unwrap()).unwrap();
    let mount = tidemark_fs::mount::mount_for_path(&table, &raw)
        .unwrap()
        .mount_point
        .as_path();
    std::fs::write(path,format!("[general]\nstate_dir='{}'\nsocket='{}'\n[sampler]\nmounts=['{}']\ninterval='1s'\n[lvm]\nenabled=false\n[watch]\nenabled=false\nmounts=[]\n[procscan]\nenabled=false\n[metrics]\ntextfile='{}'\n[scan]\nthreads=1\n[throttle]\nmin_ops_per_sec=10\nmax_ops_per_sec=10\n[[roots]]\npath='{}'\nmode='walk'\nschedule='every 1h'\nthrottle='normal'\n[[alerts.rule]]\nname='vm-reload'\nkind='fs_used_pct'\nmount='{}'\nabove={}\nfor='0s'\nresolve_for='0s'\n",p.join("state").display(),p.join("api.sock").display(),mount.display(),p.join("metrics.prom").display(),p.join("mount").display(),mount.display(),threshold)).unwrap();
}
#[test]
fn daemon_reload_and_shutdown() {
    assert_eq!(
        std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(),
        Ok("1"),
        "disposable VM only"
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("mount")).unwrap();
    for n in 0..1024 {
        std::fs::write(dir.path().join(format!("mount/file-{n}")), b"fixture").unwrap();
    }
    let path = dir.path().join("config.toml");
    write_config(&path, 90);
    let log = dir.path().join("daemon.log");
    let stderr = std::fs::File::create(&log).unwrap();
    let mut child = StopChild(
        Command::new(assert_cmd::cargo::cargo_bin!("tidemarkd"))
            .arg("--config")
            .arg(&path)
            .env_remove("INVOCATION_ID")
            .env("RUST_LOG", "info")
            .env("NO_COLOR", "1")
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    let logs = || std::fs::read_to_string(&log).unwrap();
    wait(
        || logs().contains("daemon ready"),
        Instant::now() + Duration::from_secs(15),
        &logs,
    );
    write_config(&path, 99);
    signal(&child.0, rustix::process::Signal::HUP);
    wait(
        || logs().contains("reloaded=true"),
        Instant::now() + Duration::from_secs(10),
        &logs,
    );
    std::fs::write(&path, "[sampler]\ninterval='0s'\n").unwrap();
    signal(&child.0, rustix::process::Signal::HUP);
    wait(
        || logs().contains("reloaded=false"),
        Instant::now() + Duration::from_secs(10),
        &logs,
    );
    wait(
        || logs().contains("scan memory admission estimate"),
        Instant::now() + Duration::from_secs(15),
        &logs,
    );
    let started = Instant::now();
    signal(&child.0, rustix::process::Signal::TERM);
    let mut exit = None;
    wait(
        || {
            exit = child.0.try_wait().unwrap();
            exit.is_some()
        },
        started + Duration::from_secs(10),
        &logs,
    );
    assert!(exit.unwrap().success(), "stderr: {}", logs());
    let snapshots = dir.path().join("state/snapshots");
    if snapshots.exists() {
        for root in std::fs::read_dir(snapshots).unwrap() {
            for file in std::fs::read_dir(root.unwrap().path()).unwrap() {
                assert!(!file
                    .unwrap()
                    .file_name()
                    .as_encoded_bytes()
                    .ends_with(b".db.tmp"));
            }
        }
    }
}
