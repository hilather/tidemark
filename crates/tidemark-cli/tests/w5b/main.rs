//! Owned by: W5-B
//! Executable direct modes with empty device selection and no daemon/state files.
#[test]
fn direct_modes_json_without_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[watch]\nmounts = []\n").unwrap();
    for (command, schema, extra) in [
        ("phantom", "tidemark.phantom.v1", vec![]),
        ("writers", "tidemark.writers.v1", vec!["--interval", "1ms"]),
    ] {
        let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"))
            .arg("--config")
            .arg(&config)
            .arg("--state-dir")
            .arg(dir.path().join("absent-state"))
            .args(["--json", command, "--direct"])
            .args(extra)
            .assert()
            .success()
            .get_output()
            .clone();
        let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(doc["schema"], schema);
        assert_eq!(doc["data"], serde_json::json!([]));
        assert!(!dir.path().join("absent-state").exists());
    }
}

#[test]
fn direct_writer_reports_observed_interval() {
    use std::{
        io::Write,
        os::unix::fs::MetadataExt,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("growing");
    let mut file = std::fs::File::create(&path).unwrap();
    let ino = file.metadata().unwrap().ino();
    let raw =
        tidemark_core::path::RawPath::from_path(&std::fs::canonicalize(&path).unwrap()).unwrap();
    let table = tidemark_fs::mount::read_mountinfo().unwrap();
    let mount = tidemark_fs::mount::mount_for_path(&table, &raw).unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[watch]\nmounts = []\n").unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let writer = std::thread::spawn(move || {
        let block = [0u8; 65536];
        // Bounded fixture: at most 32 MiB, exclusively inside the temp directory.
        for _ in 0..512 {
            if stopped.load(Ordering::Relaxed) {
                break;
            }
            file.write_all(&block).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"))
        .arg("--config")
        .arg(&config)
        .args([
            "--json",
            "writers",
            "--direct",
            "--interval",
            "2s",
            "--window",
            "1h",
            "--mount",
        ])
        .arg(mount.mount_point.as_path())
        .output()
        .unwrap();
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let row = doc["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["ino"].as_u64() == Some(ino))
        .unwrap();
    assert!(row["growth"].as_i64().unwrap() > 0);
    assert!((1..60).contains(&row["window_secs"].as_u64().unwrap()));
}
