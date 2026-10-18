//! Owned by: SNAP-export-sqlite
//! End-to-end `snapshots export --sqlite` from hot and cold snapshots.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tidemark_core::{config::Config, path::RawPath, snapshot::DirRow};
use tidemark_scan::{
    mode::ModeRequest,
    orchestrator::{run_scan_with_clock, ScanError, ScanRecorder, ScanRequest},
};
use tidemark_store::{
    catalog::{Catalog, ScanEntry},
    reader::Snapshot,
};
struct Recorder(PathBuf);
impl ScanRecorder for Recorder {
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
        Ok(Catalog::open(&self.0, true)?.record_start(e)?)
    }
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
        Ok(Catalog::open(&self.0, true)?.record_finish(e)?)
    }
    fn dir_series(&self, _: &RawPath, _: &str, _: i64, _: &[DirRow]) -> Result<(), ScanError> {
        Ok(())
    }
}
fn scan(cfg: &Config, root: &Path, now: i64) {
    let req = ScanRequest {
        root: RawPath::from_path(root).unwrap(),
        mode: ModeRequest::Walk,
        throttle: Some("max".into()),
        threads: Some(1),
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    };
    let out = run_scan_with_clock(
        cfg,
        &req,
        &Recorder(cfg.general.state_dir.clone()),
        &AtomicBool::new(false),
        &|_| {},
        &|| now,
    )
    .unwrap();
    assert!(out.file.unwrap().exists());
}

#[path = "support/config.rs"]
mod config_support;
fn command(state: &Path, json: bool) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::new(env!("CARGO_BIN_EXE_tidemark"));
    cmd.arg("--config")
        .arg(state.join("direct-config.toml"))
        .arg("--state-dir")
        .arg(state);
    if json {
        cmd.arg("--json");
    }
    cmd
}
fn fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    tempfile::TempDir,
    String,
) {
    let root = test_support::private_tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("alpha/beta")).unwrap();
    std::fs::write(root.path().join("alpha/beta/f"), vec![1u8; 5000]).unwrap();
    let state = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.general.state_dir = state.path().to_owned();
    scan(&cfg, root.path(), 1_700_000_000);
    config_support::direct_config(state.path(), "");
    let list = command(state.path(), true)
        .args(["snapshots", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list: serde_json::Value = serde_json::from_slice(&list).unwrap();
    let id = list["data"][0]["scan_id"].as_str().unwrap().to_owned();
    (root, state, out, id)
}
fn check_export(out: &Path, root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(out).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(std::fs::read(out)
        .unwrap()
        .starts_with(b"SQLite format 3\0"));
    // The v1 reader opens it only if user_version, meta and the dirs table agree.
    let snap = Snapshot::open(out).unwrap();
    assert_eq!(snap.meta().schema_version, 1);
    let canonical = RawPath::from_path(&std::fs::canonicalize(root).unwrap()).unwrap();
    let alpha_beta = canonical.join(&tidemark_core::path::RawName::new(b"alpha".to_vec()).unwrap());
    let alpha_beta = alpha_beta.join(&tidemark_core::path::RawName::new(b"beta".to_vec()).unwrap());
    // `dir_by_path` is `SELECT * FROM dirs WHERE path_hash=? AND path=?` on a v1 database.
    let row = snap.dir_by_path(&alpha_beta).unwrap().unwrap();
    assert_eq!(row.path, alpha_beta);
    assert!(snap.dir_by_path(&canonical).unwrap().is_some());
}

#[test]
fn export_writes_a_queryable_v1_database_and_refuses_to_overwrite() {
    let (root, state, out, id) = fixture();
    let target = out.path().join("export.db");
    let text = command(state.path(), false)
        .args(["snapshots", "export", "--sqlite", &id])
        .arg(&target)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(text)
        .unwrap()
        .contains(&format!("Exported {id}")));
    check_export(&target, root.path());
    let before = std::fs::read(&target).unwrap();
    let refused = command(state.path(), false)
        .args(["snapshots", "export", "--sqlite", &id])
        .arg(&target)
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let refused = String::from_utf8(refused).unwrap();
    assert!(refused.contains("already exists"), "{refused}");
    assert_eq!(std::fs::read(&target).unwrap(), before);
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 1);
}

#[test]
fn export_json_unknown_id_and_state_dir_are_reported() {
    let (_root, state, out, id) = fixture();
    let target = out.path().join("j.db");
    let json = command(state.path(), true)
        .args(["snapshots", "export", "--sqlite", &id])
        .arg(&target)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(json["data"]["exported"], id);
    assert!(json["data"]["dirs"].as_u64().unwrap() >= 3);
    command(state.path(), false)
        .args(["snapshots", "export", "--sqlite", "no-such-scan"])
        .arg(out.path().join("x.db"))
        .assert()
        .failure()
        .code(5);
    assert!(!out.path().join("x.db").exists());
    command(state.path(), false)
        .args(["snapshots", "export", "--sqlite", &id])
        .arg(state.path().join("inside.db"))
        .assert()
        .failure()
        .code(5);
    // `--sqlite` is required.
    command(state.path(), false)
        .args(["snapshots", "export", &id])
        .arg(out.path().join("y.db"))
        .assert()
        .failure()
        .code(2);
}

#[test]
fn export_of_a_cold_snapshot_cleans_up_its_inflated_copy() {
    let (root, state, out, id) = fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    let hot = catalog.get(&id).unwrap().unwrap().file.unwrap();
    let cold = tidemark_store::cold::compress_snapshot(&hot, 1, &|| false).unwrap();
    catalog.point_to_cold(&id, &cold).unwrap();
    std::fs::remove_file(&hot).unwrap();
    drop(catalog);
    let target = out.path().join("cold.db");
    command(state.path(), false)
        .args(["snapshots", "export", "--sqlite", &id])
        .arg(&target)
        .assert()
        .success();
    check_export(&target, root.path());
    let leftovers = std::fs::read_dir(cold.parent().unwrap().join(".inflate"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0);
    assert!(cold.exists());
}
