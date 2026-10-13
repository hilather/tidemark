//! Owned by: WSTATE-trust-A
//! Direct CLI scans refuse planted database links without creating state artifacts.
#![allow(clippy::unwrap_used)]
#[path = "support/config.rs"]
mod config_support;
use std::os::unix::fs::{symlink, PermissionsExt};
#[test]
fn direct_scan_refuses_planted_database_link_without_changes() {
    let work = test_support::private_tempdir().unwrap();
    let state = work.path().join("state");
    test_support::private_dir(&state).unwrap();
    let root = work.path().join("root");
    test_support::private_dir(&root).unwrap();
    let victim = work.path().join("victim");
    test_support::private_write(&victim, b"").unwrap();
    symlink(&victim, state.join("timeseries.db")).unwrap();
    let config = config_support::direct_config(work.path(), "");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
        .args(["--config"])
        .arg(config)
        .arg("--state-dir")
        .arg(&state)
        .arg("scan")
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reason = String::from_utf8_lossy(&output.stderr);
    assert!(
        reason.contains("timeseries.db") && reason.contains("regular file"),
        "{reason}"
    );
    assert_eq!(std::fs::read(&victim).unwrap(), b"");
    assert_eq!(std::fs::metadata(&victim).unwrap().len(), 0);
    assert_eq!(
        std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(std::fs::read_dir(&state).unwrap().count(), 1);
}
