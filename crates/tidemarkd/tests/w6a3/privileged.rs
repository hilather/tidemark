//! Owned by: W6-A3
//! Disposable VM end-to-end watch attribution and thin-pool API checks.
use super::*;
use std::process::Command;
struct Unit(String);
impl Drop for Unit {
    fn drop(&mut self) {
        let _ = Command::new("systemctl").args(["stop", &self.0]).status();
    }
}
fn call(socket: &std::path::Path, method: &str, params: &str) -> Response {
    let stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(11)))
        .unwrap();
    let mut stream = BufReader::new(stream);
    write(&mut stream, &request(method, params));
    read(&mut stream)
}
#[test]
fn daemon_end_to_end() {
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let vg = test_support::lvm::LoopVg::new(
        "w6a3",
        2 * 1024 * 1024 * 1024,
        test_support::lvm::LvLayout {
            linear: None,
            thin_pool: Some((1024 * 1024 * 1024, 512 * 1024 * 1024)),
        },
    );
    let thin = &vg.mounts[0].mountpoint;
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("api.sock");
    let config = dir.path().join("config.toml");
    std::fs::write(&config, format!("[general]\nstate_dir='{}'\nsocket='{}'\n[sampler]\nmounts=['{}','{}']\ninterval='1s'\n[watch]\nmounts=['{}','{}']\n[lvm]\ninterval='1s'\n[metrics]\ntextfile='{}'\n[[roots]]\npath='{}'\nmode='walk'\nschedule='off'\n", dir.path().join("state").display(), socket.display(), fs.mountpoint.display(), thin.display(), fs.mountpoint.display(), thin.display(), dir.path().join("metrics.prom").display(), fs.mountpoint.display())).unwrap();
    let unit = Unit(format!("tmtest-w6a3-daemon-{}.service", std::process::id()));
    assert!(Command::new("systemd-run").args(["--unit", &unit.0,"--property=Type=notify","--property=User=root","--property=UMask=0077","--property=NoNewPrivileges=yes","--property=CapabilityBoundingSet=CAP_SYS_ADMIN CAP_DAC_READ_SEARCH CAP_FOWNER CAP_SYS_PTRACE","--property=RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6","--property=MemoryMax=2G","--property=CPUQuota=200%","--property=LimitNOFILE=65536","--"]).arg(env!("CARGO_BIN_EXE_tidemarkd")).arg("--config").arg(&config).status().unwrap().success());
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !socket.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "socket did not become ready"
        );
        std::thread::yield_now();
    }
    let hotdir = fs.mountpoint.join("hot");
    std::fs::create_dir(&hotdir).unwrap();
    let writer_unit = format!("tmtest-w6a3-writer-{}", std::process::id());
    assert!(Command::new("systemd-run")
        .args([
            "--wait",
            "--unit",
            &writer_unit,
            "--",
            "python3",
            "-c",
            "import sys; f=open(sys.argv[1],'wb'); f.write(b'x'*(50*1024*1024)); f.close()"
        ])
        .arg(hotdir.join("file"))
        .status()
        .unwrap()
        .success());
    loop {
        let response = call(&socket, "now", "{}");
        assert!(response.ok);
        let report: tidemark_core::live::NowReport =
            serde_json::from_value(response.result.unwrap()).unwrap();
        if report.mounts.len() == 2
            && report
                .mounts
                .iter()
                .any(|m| m.lvm.as_ref().is_some_and(|l| l.data_pct.is_some()))
            && report.mounts.iter().flat_map(|m| &m.hot_dirs_5m).any(|h| {
                h.key == hotdir.as_os_str().as_encoded_bytes()
                    && h.top_unit
                        .as_deref()
                        .is_some_and(|u| u.contains(&writer_unit))
            })
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "hot directory, transient unit and thin-pool measurements missing"
        );
        std::thread::yield_now();
    }
    drop(unit);
}
