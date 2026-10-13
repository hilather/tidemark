//! Owned by: W1-A
//! CLI probe reports the remaining sections without an installed configuration.
#[test]
fn probe_json_has_remaining_sections() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
        .args([
            "--config",
            "/tidemark-w1a-missing-config",
            "--json",
            "probe",
            "--root",
            "/",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "tidemark.probe.v1");
    assert_eq!(value["data"].as_object().unwrap().len(), 3);
    for key in ["host", "roots", "recommendations"] {
        assert!(value["data"].get(key).is_some());
    }
}

#[test]
fn executable_cli_and_config_paths_use_realpath() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("target/child")).unwrap();
    std::fs::create_dir(dir.path().join("existing")).unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("target/child"),
        dir.path().join("local_link"),
    )
    .unwrap();
    std::os::unix::fs::symlink("/proc/sys/kernel", dir.path().join("proc_link")).unwrap();
    for suffix in ["local_link/..", "proc_link/..", "missing/../existing"] {
        let requested = dir.path().join(suffix);
        for configured in [false, true] {
            let config = dir.path().join("config.toml");
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"));
            if configured {
                std::fs::write(
                    &config,
                    format!(
                        "[general]\nsocket = {:?}\n[[roots]]\npath = {:?}\n",
                        dir.path().join("absent-api.sock"),
                        requested
                    ),
                )
                .unwrap();
                command.arg("--config").arg(&config);
            } else {
                command.args(["--config", "/tidemark-w1a-missing-config"]);
            }
            command.args(["--json", "probe"]);
            if !configured {
                command.arg("--root").arg(&requested);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let roots = value["data"]["roots"].as_array().unwrap();
            let root = roots.last().unwrap();
            if suffix.starts_with("missing") {
                assert!(root["mount_point"].is_null());
                assert!(root["other_mounts"].is_null());
                assert!(root["facts"]
                    .as_object()
                    .unwrap()
                    .values()
                    .all(|fact| fact["value"].is_null() && fact["why"].is_string()));
            } else {
                let expected = std::fs::canonicalize(&requested).unwrap();
                assert_eq!(
                    root["root"],
                    tidemark_core::path::RawPath::from_path(&expected)
                        .unwrap()
                        .to_json()
                );
                assert!(root["facts"]["total_bytes"]["value"].is_string());
                if suffix.starts_with("proc") {
                    assert_eq!(root["facts"]["fs_type"]["value"], "proc");
                }
            }
        }
    }
}
