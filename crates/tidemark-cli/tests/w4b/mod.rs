//! Owned by: W4-B
//! Command failures use temporary configs and never invoke the service manager.

#[test]
fn malformed_stage_config_prints_check_table_and_exits_five() {
    let dir = ::test_support::private_tempdir().unwrap();
    let source = dir.path().join("invalid.toml");
    let invalid = format!(
        "[general]\nsocket = {:?}\nunknown = true\n",
        dir.path().join("absent-api.sock")
    );
    std::fs::write(&source, &invalid).unwrap();
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"))
        .arg("-c")
        .arg(dir.path().join("missing.toml"))
        .args(["deploy", "stage", "--from"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("FAIL"));
    assert!(text.contains("config"));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), invalid);
}
#[test]
fn json_preflight_failure_and_purge_confirmation_exit_five() {
    let dir = ::test_support::private_tempdir().unwrap();
    let source = dir.path().join("invalid.toml");
    std::fs::write(&source, "bad TOML\n").unwrap();
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"))
        .arg("-c")
        .arg(dir.path().join("missing.toml"))
        .args(["--json", "deploy", "stage", "--from"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["staged"], false);
    assert_eq!(result["checks"][0]["status"], "fail");
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"))
        .arg("-c")
        .arg(dir.path().join("missing.toml"))
        .args(["deploy", "unstage", "--purge-state"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires --yes"));
}
#[cfg(feature = "privileged")]
mod privileged;

#[test]
fn missing_purge_marker_is_precondition_exit_five_without_service_calls() {
    let dir = ::test_support::private_tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let state = dir.path().join("state");
    ::test_support::private_dir(&state).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(state.join("keep"), b"untouched").unwrap();
    std::fs::write(
        &config,
        format!(
            "[general]\nsocket = {:?}\nstate_dir = {:?}\n",
            dir.path().join("absent-api.sock"),
            state
        ),
    )
    .unwrap();
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"))
        .arg("-c")
        .arg(config)
        .args(["deploy", "unstage", "--purge-state", "--yes"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stderr).contains("state marker"));
    assert_eq!(std::fs::read(state.join("keep")).unwrap(), b"untouched");
}

#[test]
fn valid_source_bypasses_invalid_installed_config_and_exits_five_for_unsafe_state() {
    let dir = ::test_support::private_tempdir().unwrap();
    let installed = dir.path().join("installed.toml");
    let source = dir.path().join("source.toml");
    std::fs::write(&installed, "invalid TOML").unwrap();
    std::fs::write(
        &source,
        format!(
            "[general]\nsocket = {:?}\nstate_dir = '/'\n",
            dir.path().join("absent-api.sock")
        ),
    )
    .unwrap();
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"))
        .arg("-c")
        .arg(&installed)
        .args(["deploy", "stage", "--from"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stdout).contains("unsafe state_dir"));
    assert_eq!(std::fs::read(&installed).unwrap(), b"invalid TOML");
}

#[test]
fn unsafe_directory_permissions_exit_five_before_service_access() {
    use std::os::unix::fs::PermissionsExt;
    for mode in [0o777, 0o750] {
        for child in [false, true] {
            let dir = ::test_support::private_tempdir().unwrap();
            let state = dir.path().join("state");
            ::test_support::private_dir(state.join("deploy")).unwrap();
            for path in [&state, &state.join("deploy")] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let bad = if child {
                state.join("deploy")
            } else {
                state.clone()
            };
            std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(mode)).unwrap();
            let config = dir.path().join("config.toml");
            std::fs::write(
                &config,
                format!(
                    "[general]\nsocket = {:?}\nstate_dir = {:?}\n",
                    dir.path().join("absent-api.sock"),
                    state
                ),
            )
            .unwrap();
            std::fs::write(state.join("deploy/staged.json"), b"original").unwrap();
            let outside = dir.path().join("outside");
            std::fs::write(&outside, b"outside").unwrap();
            for operation in ["stage", "unstage"] {
                let mut command =
                    assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"));
                command.arg("-c").arg(&config).args(["deploy", operation]);
                if operation == "stage" {
                    command.arg("--from").arg(&config);
                }
                let output = command.output().unwrap();
                assert_eq!(output.status.code(), Some(5));
                let message = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(message.contains(&bad.display().to_string()), "{message}");
                assert!(message.contains(&format!("{:04o}", mode)), "{message}");
                assert!(message.contains("owner"), "{message}");
                assert_eq!(
                    std::fs::read(state.join("deploy/staged.json")).unwrap(),
                    b"original"
                );
                assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
                assert!(!state.join("deploy/firstboot.json").exists());
            }
        }
    }
}
