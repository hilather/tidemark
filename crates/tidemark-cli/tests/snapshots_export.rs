//! Owned by: WSNAP-export-sqlite
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
        .code(5)
        .get_output()
        .stderr
        .clone();
    let refused = String::from_utf8(refused).unwrap();
    assert_eq!(
        refused,
        format!("output path already exists: {}\n", target.display())
    );
    assert_eq!(std::fs::read(&target).unwrap(), before);
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 1);
}

#[test]
fn export_refuses_directory_and_dangling_symlink_without_staging() {
    use std::os::unix::fs::MetadataExt;
    let (_root, state, out, id) = fixture();
    let dir = out.path().join("existing-dir");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("keep"), b"untouched").unwrap();
    let dir_meta = std::fs::symlink_metadata(&dir).unwrap();
    let refused = command(state.path(), false)
        .args(["snapshots", "export", "--sqlite", &id])
        .arg(&dir)
        .assert()
        .failure()
        .code(5)
        .stdout("")
        .get_output()
        .stderr
        .clone();
    assert_eq!(
        String::from_utf8(refused).unwrap(),
        format!("output path already exists: {}\n", dir.display())
    );
    let after = std::fs::symlink_metadata(&dir).unwrap();
    assert!(after.is_dir());
    assert_eq!(after.ino(), dir_meta.ino());
    assert_eq!(after.mtime(), dir_meta.mtime());
    assert_eq!(std::fs::read(dir.join("keep")).unwrap(), b"untouched");
    let mut children: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    children.sort();
    assert_eq!(children, [std::ffi::OsString::from("keep")]);
    assert_no_staging(out.path());

    let link = out.path().join("dangling");
    std::os::unix::fs::symlink("missing-target", &link).unwrap();
    let link_meta = std::fs::symlink_metadata(&link).unwrap();
    let refused = command(state.path(), false)
        .args(["snapshots", "export", "--sqlite", &id])
        .arg(&link)
        .assert()
        .failure()
        .code(5)
        .stdout("")
        .get_output()
        .stderr
        .clone();
    assert_eq!(
        String::from_utf8(refused).unwrap(),
        format!("output path already exists: {}\n", link.display())
    );
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        std::path::Path::new("missing-target")
    );
    assert_eq!(
        std::fs::metadata(&link).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let after = std::fs::symlink_metadata(&link).unwrap();
    assert!(after.file_type().is_symlink());
    assert_eq!(after.ino(), link_meta.ino());
    assert_eq!(after.mtime(), link_meta.mtime());
    assert_no_staging(out.path());
}

#[test]
fn export_refuses_stdout_dash() {
    let (_root, state, out, id) = fixture();
    let refused = command(state.path(), false)
        .current_dir(out.path())
        .args(["snapshots", "export", "--sqlite", &id, "-"])
        .assert()
        .failure()
        .code(2)
        .stdout("")
        .get_output()
        .stderr
        .clone();
    let refused = String::from_utf8(refused).unwrap();
    assert!(
        refused.contains("output path '-' is not supported"),
        "{refused}"
    );
    assert!(!out.path().join("-").exists());
    assert_no_staging(out.path());
}

fn assert_no_staging(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(
            !name.to_string_lossy().starts_with(".tidemark-export-"),
            "staging leftover {name:?}"
        );
    }
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

#[test]
fn s1_json_output_after_publishing_a_relative_destination() {
    let (root, state, out, id) = fixture();
    let assert = command(state.path(), true)
        .current_dir(out.path())
        .args(["snapshots", "export", "--sqlite", &id, "relative.db"])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(value["data"]["exported"], id.as_str());
    check_export(&out.path().join("relative.db"), root.path());
}

#[test]
fn s6_space_floor_refusal_exits_5() {
    let (_root, state, out, id) = fixture();
    let catalog = Catalog::open(state.path(), true).unwrap();
    let hot = catalog.get(&id).unwrap().unwrap().file.unwrap();
    let cold = tidemark_store::cold::compress_snapshot(&hot, 1, &|| false).unwrap();
    catalog.point_to_cold(&id, &cold).unwrap();
    std::fs::remove_file(&hot).unwrap();
    drop(catalog);
    config_support::direct_config(state.path(), "[retention]\nmin_free_pct = 100.0\n");
    command(state.path(), false)
        .args(["snapshots", "export", "--sqlite", &id])
        .arg(out.path().join("never.db"))
        .assert()
        .code(5);
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn s2_termination_signals_leave_no_temporary_copies() {
    for signal in ["TERM", "INT", "HUP"] {
        let root = test_support::private_tempdir().unwrap();
        for i in 0..6000 {
            std::fs::create_dir_all(root.path().join(format!("d{i:05}/sub"))).unwrap();
        }
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
        let catalog = Catalog::open(state.path(), true).unwrap();
        let hot = catalog.get(&id).unwrap().unwrap().file.unwrap();
        let cold = tidemark_store::cold::compress_snapshot(&hot, 1, &|| false).unwrap();
        catalog.point_to_cold(&id, &cold).unwrap();
        std::fs::remove_file(&hot).unwrap();
        drop(catalog);

        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
            .arg("--config")
            .arg(state.path().join("direct-config.toml"))
            .arg("--state-dir")
            .arg(state.path())
            .args(["snapshots", "export", "--sqlite", &id])
            .arg(out.path().join("o.db"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Signal once the staging copy exists (or the export already finished).
        let started = std::time::Instant::now();
        while child.try_wait().unwrap().is_none() && started.elapsed().as_secs() < 20 {
            let staging = std::fs::read_dir(out.path()).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tidemark-export-")
            });
            if staging {
                let status = std::process::Command::new("kill")
                    .arg(format!("-{signal}"))
                    .arg(child.id().to_string())
                    .status()
                    .unwrap();
                assert!(status.success());
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let status = child.wait().unwrap();
        // Completed before the signal, or stopped cleanly with the interrupted status.
        assert!(
            status.success() || status.code() == Some(3),
            "{signal}: {status:?}"
        );
        for entry in std::fs::read_dir(out.path()).unwrap() {
            let name = entry.unwrap().file_name();
            assert_eq!(name, "o.db", "{signal}: leftover {name:?}");
        }
        let inflate = cold.parent().unwrap().join(".inflate");
        let leftovers = std::fs::read_dir(inflate).map(|d| d.count()).unwrap_or(0);
        assert_eq!(leftovers, 0, "{signal}: inflate leftovers");
    }
}
