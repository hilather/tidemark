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
