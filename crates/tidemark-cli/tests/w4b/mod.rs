//! Owned by: W4-B
//! Command failures use temporary configs and never invoke the service manager.

#[test]
fn malformed_stage_config_prints_check_table_and_exits_five() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("invalid.toml");
    std::fs::write(&source, "[general]\nunknown = true\n").unwrap();
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
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        "[general]\nunknown = true\n"
    );
}
#[test]
fn json_preflight_failure_and_purge_confirmation_exit_five() {
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let state = dir.path().join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("keep"), b"untouched").unwrap();
    std::fs::write(&config, format!("[general]\nstate_dir = {:?}\n", state)).unwrap();
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
