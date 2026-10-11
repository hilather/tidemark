//! Owned by: W3-A
//! Snapshot selection and actual-age notices against a synthetic catalog.
#[path = "../../../tidemark-store/tests/w3a/support.rs"]
mod support;
use tidemark_store::{
    catalog::{Catalog, ScanEntry},
    Snapshot,
};
fn record(catalog: &Catalog, file: std::path::PathBuf, status: &str) -> String {
    let snapshot = Snapshot::open(&file).unwrap();
    let meta = snapshot.meta();
    let id = meta.scan_id.to_string();
    catalog
        .record_finish(&ScanEntry {
            scan_id: id.clone(),
            root: meta.root.clone(),
            mode: meta.mode,
            started_at: meta.started_at,
            finished_at: Some(meta.finished_at),
            status: status.into(),
            degraded: false,
            file: Some(file),
            t_disk: Some(snapshot.root().unwrap().t_disk),
            t_inodes: Some(0),
            t_dirs: Some(1),
            errors: Some(0),
            duration_ms: Some(1),
        })
        .unwrap();
    id
}
#[test]
fn since_age_notice_latest_complete_and_from_default_to() {
    use std::os::unix::ffi::OsStrExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let state = temp.path().join("state");
    let catalog = Catalog::open(&state, true).unwrap();
    let now = tidemark_core::time::unix_now();
    let mut row = support::row(0, None, root.as_os_str().as_bytes(), 100);
    row.subtree_end = 0;
    let a = support::snapshot(&state.join("snapshots"), &[row.clone()], now - 4 * 3600);
    row.t_disk = 300;
    let b = support::snapshot(&state.join("snapshots"), &[row.clone()], now);
    row.t_disk = 900;
    let partial = support::snapshot(&state.join("snapshots"), &[row], now + 1);
    let a = record(&catalog, a, "complete");
    record(&catalog, b, "complete");
    record(&catalog, partial, "partial");
    let invoke = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
            .arg("--config")
            .arg(temp.path().join("missing.toml"))
            .arg("--state-dir")
            .arg(&state)
            .arg("--json")
            .arg("diff")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };
    let out = invoke(&["--since", "1h"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["data"]["diff"][0]["delta"], 200);
    assert!(doc["data"]["notice"]
        .as_str()
        .unwrap()
        .contains("actual age"));
    let out = invoke(&["--from", &a]);
    assert!(out.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["data"]["diff"][0]["delta"], 200);
    assert!(doc["data"]["notice"].is_null());
    assert_eq!(invoke(&["--since", "10h"]).status.code(), Some(5));
    assert_eq!(invoke(&["--from", "missing"]).status.code(), Some(5));
    assert_eq!(invoke(&[]).status.code(), Some(2));
    assert_eq!(
        invoke(&["--from", &a, "--explain", "--explain-ratio", "1.5"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        invoke(&["--from", &a, "--limit", "1000001"]).status.code(),
        Some(2)
    );
}
