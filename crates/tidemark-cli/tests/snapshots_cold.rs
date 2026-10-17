//! Owned by: WSNAP-cold-daemon
//! CLI cold listing and prune publication with fixed-clock scan fixtures.
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
#[test]
fn list_sizes_prune_compression_dry_run_and_cold_deletion() {
    let root = test_support::private_tempdir().unwrap();
    let state = test_support::private_tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.general.state_dir = state.path().to_owned();
    cfg.retention.snapshots.cold_after_recent = false;
    cfg.retention.snapshots.recent = 5;
    for day in 0..5 {
        scan(&cfg, root.path(), 1_700_000_000 + day * 86400);
    }
    config_support::direct_config(
        state.path(),
        "[retention.snapshots]\nrecent=2\ndaily=100000\nweekly=0\ncold_after_recent=false\n",
    );
    let disabled = command(state.path(), true)
        .args(["snapshots", "prune"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let disabled: serde_json::Value = serde_json::from_slice(&disabled).unwrap();
    assert_eq!(disabled["data"]["compressed"].as_array().unwrap().len(), 0);
    config_support::direct_config(
        state.path(),
        "[retention.snapshots]\nrecent=2\ndaily=100000\nweekly=0\ncold_level=1\n",
    );
    let dry = command(state.path(), true)
        .args(["snapshots", "prune", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let dry: serde_json::Value = serde_json::from_slice(&dry).unwrap();
    assert_eq!(dry["data"]["compressed"].as_array().unwrap().len(), 3);
    assert_eq!(dry["data"]["pruned"].as_array().unwrap().len(), 0);
    let cat = Catalog::open(state.path(), false).unwrap();
    assert!(cat
        .list(None)
        .unwrap()
        .iter()
        .all(|e| e.file.as_ref().unwrap().extension().unwrap() == "db"));
    let out = command(state.path(), false)
        .args(["snapshots", "prune"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(out)
            .unwrap()
            .matches("compressed ")
            .count(),
        3
    );
    let text = command(state.path(), false)
        .args(["snapshots", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(text).unwrap();
    assert!(text.contains("cold"));
    assert!(text.contains("FILE BYTES"));
    assert!(text.contains("UNCOMPRESSED BYTES"));
    let list = command(state.path(), true)
        .args(["snapshots", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list: serde_json::Value = serde_json::from_slice(&list).unwrap();
    let rows = list["data"].as_array().unwrap();
    assert_eq!(rows.iter().filter(|e| e["cold"] == true).count(), 3);
    for e in rows {
        let entry = cat.get(e["scan_id"].as_str().unwrap()).unwrap().unwrap();
        let file = entry.file.unwrap();
        assert_eq!(e["file_bytes"], std::fs::metadata(&file).unwrap().len());
        let size = if e["cold"] == true {
            tidemark_store::cold::read_cold_header(&file)
                .unwrap()
                .uncompressed_len
        } else {
            std::fs::metadata(&file).unwrap().len()
        };
        assert_eq!(e["uncompressed_bytes"], size);
        let text_row = text
            .lines()
            .find(|line| line.starts_with(e["scan_id"].as_str().unwrap()))
            .unwrap();
        let mut columns = text_row.split_whitespace().rev();
        assert_eq!(columns.next().unwrap().parse::<u64>().unwrap(), size);
        assert_eq!(
            columns.next().unwrap().parse::<u64>().unwrap(),
            std::fs::metadata(&file).unwrap().len()
        );
        assert_eq!(
            columns.next(),
            Some(if e["cold"] == true { "cold" } else { "hot" })
        );
        assert!(e.get("t_disk").is_some());
        assert!(Snapshot::open(&file).is_ok());
    }
    let cold_id = rows.iter().find(|e| e["cold"] == true).unwrap()["scan_id"]
        .as_str()
        .unwrap();
    command(state.path(), true)
        .args(["snapshots", "show", cold_id])
        .assert()
        .success();
    config_support::direct_config(state.path(), "[retention]\nmax_state_size='1B'\n");
    command(state.path(), true)
        .args(["snapshots", "show", cold_id])
        .assert()
        .failure();
    let old: Vec<_> = cat
        .list(None)
        .unwrap()
        .into_iter()
        .filter_map(|e| e.file)
        .filter(|p| p.extension().unwrap() == "zst")
        .collect();
    for cold in &old {
        let hot = cold.with_extension("");
        test_support::private_write(hot.with_extension("db.bad"), b"evidence").unwrap();
        test_support::private_write(hot.with_extension("db.zst.bad"), b"evidence").unwrap();
    }
    config_support::direct_config(
        state.path(),
        "[retention.snapshots]\nrecent=1\ndaily=0\nweekly=0\n",
    );
    command(state.path(), false)
        .args(["snapshots", "prune", "--dry-run"])
        .assert()
        .success();
    assert!(old.iter().all(|p| p.exists()));
    command(state.path(), true)
        .args(["snapshots", "prune"])
        .assert()
        .success();
    for cold in &old {
        assert!(!cold.exists());
        assert!(!cold.with_extension("").with_extension("db.bad").exists());
        assert!(!cold
            .with_extension("")
            .with_extension("db.zst.bad")
            .exists());
    }
    assert_eq!(cat.list(None).unwrap().len(), 1);
}

fn two_scans(state: &Path, root: &Path) -> Vec<ScanEntry> {
    let mut cfg = Config::default();
    cfg.general.state_dir = state.to_owned();
    cfg.retention.snapshots.cold_after_recent = false;
    cfg.retention.snapshots.recent = 5;
    for day in 0..2 {
        scan(&cfg, root, 1_700_000_000 + day * 86400);
    }
    let mut entries = Catalog::open(state, false).unwrap().list(None).unwrap();
    entries.sort_by_key(|e| e.started_at);
    entries
}

#[test]
fn prune_removes_both_published_forms_in_both_pointer_directions() {
    for point_to_cold in [false, true] {
        let root = test_support::private_tempdir().unwrap();
        let state = test_support::private_tempdir().unwrap();
        let entries = two_scans(state.path(), root.path());
        let hot = entries[0].file.clone().unwrap();
        let cold = tidemark_store::cold::compress_snapshot(&hot, 1, &|| false).unwrap();
        if point_to_cold {
            Catalog::open(state.path(), true)
                .unwrap()
                .point_to_cold(&entries[0].scan_id, &cold)
                .unwrap();
        }
        let bad = hot.with_extension("db.bad");
        test_support::private_write(&bad, b"evidence").unwrap();
        config_support::direct_config(
            state.path(),
            "[retention.snapshots]\nrecent=1\ndaily=0\nweekly=0\ncold_after_recent=false\n",
        );
        command(state.path(), true)
            .args(["snapshots", "prune"])
            .assert()
            .success();
        assert!(!hot.exists() && !cold.exists() && !bad.exists());
        let rows = Catalog::open(state.path(), false)
            .unwrap()
            .list(None)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_ne!(rows[0].scan_id, entries[0].scan_id);
    }
}

#[test]
fn list_survives_missing_hot_file_and_corrupt_cold_header() {
    let root = test_support::private_tempdir().unwrap();
    let state = test_support::private_tempdir().unwrap();
    let mut entries = two_scans(state.path(), root.path());
    let healthy = entries.pop().unwrap();
    let broken = entries.pop().unwrap();
    // Case 1: the catalog row's hot file is gone.
    let file = broken.file.clone().unwrap();
    std::fs::remove_file(&file).unwrap();
    for json in [false, true] {
        let out = command(state.path(), json)
            .args(["snapshots", "list"])
            .assert()
            .success();
        let out = out.get_output();
        assert!(!out.stderr.is_empty());
        let stdout = String::from_utf8(out.stdout.clone()).unwrap();
        if json {
            let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
            let rows = v["data"].as_array().unwrap();
            assert_eq!(rows.len(), 2);
            let row = rows
                .iter()
                .find(|r| r["scan_id"] == broken.scan_id)
                .unwrap();
            assert!(row["file_bytes"].is_null() && row["uncompressed_bytes"].is_null());
            let ok = rows
                .iter()
                .find(|r| r["scan_id"] == healthy.scan_id)
                .unwrap();
            assert!(ok["file_bytes"].is_number());
        } else {
            let line = stdout
                .lines()
                .find(|l| l.starts_with(&broken.scan_id))
                .unwrap();
            assert!(line.ends_with('-'));
            assert!(stdout.lines().any(|l| l.starts_with(&healthy.scan_id)));
        }
    }
    // Case 2: a cold file with a corrupt header.
    let hot = healthy.file.clone().unwrap();
    let cold = tidemark_store::cold::compress_snapshot(&hot, 1, &|| false).unwrap();
    Catalog::open(state.path(), true)
        .unwrap()
        .point_to_cold(&healthy.scan_id, &cold)
        .unwrap();
    let mut bytes = std::fs::read(&cold).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&cold, bytes).unwrap();
    for json in [false, true] {
        let out = command(state.path(), json)
            .args(["snapshots", "list"])
            .assert()
            .success();
        let out = out.get_output();
        assert!(!out.stderr.is_empty());
        let stdout = String::from_utf8(out.stdout.clone()).unwrap();
        if json {
            let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
            let row = v["data"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["scan_id"] == healthy.scan_id)
                .unwrap();
            assert!(row["cold"] == true && row["uncompressed_bytes"].is_null());
            assert!(row["file_bytes"].is_number());
        } else {
            let line = stdout
                .lines()
                .find(|l| l.starts_with(&healthy.scan_id))
                .unwrap();
            assert!(line.ends_with('-') && line.contains("cold"));
        }
    }
}
