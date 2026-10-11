//! Owned by: W1-A
//! CLI probe runs without dependency implementations or an installed configuration.
#[test]
fn probe_json_has_all_sections_and_unknown_postgres() {
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
    for key in ["host", "roots", "recommendations", "postgres"] {
        assert!(value["data"].get(key).is_some());
    }
    assert!(value["data"]["postgres"]["reachable"]["value"].is_null());
    assert_eq!(
        value["data"]["postgres"]["reachable"]["why"],
        "NotYetImplemented"
    );
}
