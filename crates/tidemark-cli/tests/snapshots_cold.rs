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
