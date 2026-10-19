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

fn unlimited(pct: f64) -> tidemark_core::config::RetentionConfig {
    tidemark_core::config::RetentionConfig {
        min_free_pct: pct,
        max_state_size: tidemark_core::units::ByteSize(u64::MAX),
        ..tidemark_core::config::RetentionConfig::default()
    }
}

fn floor_reason(state: &Path, length: u64, pct: f64) -> Option<String> {
    match tidemark_store::space_guard::space_check(state, length, &unlimited(pct)).unwrap() {
        tidemark_store::space_guard::SpaceVerdict::Ok => None,
        tidemark_store::space_guard::SpaceVerdict::Refuse { reason }
            if reason.contains("free-space floor") =>
        {
            Some(reason)
        }
        tidemark_store::space_guard::SpaceVerdict::Refuse { reason } => {
            panic!("space check at {pct}% refused {length} bytes for another reason: {reason}")
        }
    }
}

fn filesystem_totals(state: &Path) -> tidemark_store::space_guard::FsTotals {
    let file = std::fs::File::open(state).unwrap();
    let stat = rustix::fs::fstatvfs(&file).unwrap();
    tidemark_store::space_guard::FsTotals {
        size: stat.f_blocks.saturating_mul(stat.f_frsize),
        avail: stat.f_bavail.saturating_mul(stat.f_frsize),
    }
}

fn refuses_frozen(totals: tidemark_store::space_guard::FsTotals, length: u64, pct: f64) -> bool {
    match tidemark_store::space_guard::space_check_with(|| Ok(totals), 0, length, &unlimited(pct)) {
        tidemark_store::space_guard::SpaceVerdict::Ok => false,
        tidemark_store::space_guard::SpaceVerdict::Refuse { reason }
            if reason.contains("free-space floor") =>
        {
            true
        }
        tidemark_store::space_guard::SpaceVerdict::Refuse { reason } => {
            panic!(
                "frozen space check at {pct}% refused {length} bytes for another reason: {reason}"
            )
        }
    }
}

fn first_floor_refusal(totals: tidemark_store::space_guard::FsTotals, pct: f64) -> Option<u64> {
    let max = i64::MAX as u64 - 1;
    if !refuses_frozen(totals, max, pct) {
        return None;
    }
    let mut lo = 1u64;
    let mut hi = max;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if refuses_frozen(totals, mid, pct) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    refuses_frozen(totals, lo, pct).then_some(lo)
}

/// A declared inflate that `pct` refuses and the 5% default still allows.
///
/// The boundary comes from one filesystem snapshot. Searching the live free
/// space lets a parallel test move the floor and collapse the two policies.
fn length_between(state: &Path, pct: f64) -> Option<u64> {
    for _ in 0..4 {
        let totals = filesystem_totals(state);
        let strict = first_floor_refusal(totals, pct)?;
        let loose = first_floor_refusal(totals, 5.0)?;
        if strict >= loose {
            return None;
        }
        let length = strict + (loose - strict) / 2;
        if floor_reason(state, length, pct).is_some() && floor_reason(state, length, 5.0).is_none()
        {
            return Some(length);
        }
    }
    None
}

/// `pct` is no stricter than the mandatory 2 GiB floor, so no declared length
/// can separate it from the default 5% floor.
fn absolute_floor_hides_percentage(
    totals: tidemark_store::space_guard::FsTotals,
    pct: f64,
) -> bool {
    match (
        first_floor_refusal(totals, pct),
        first_floor_refusal(totals, 0.0),
    ) {
        (Some(configured), Some(absolute)) => configured >= absolute,
        _ => false,
    }
}

#[test]
fn absolute_floor_hides_fifty_percent_only_on_a_small_filesystem() {
    let small = tidemark_store::space_guard::FsTotals {
        size: 1 << 30,
        avail: 1 << 30,
    };
    assert!(absolute_floor_hides_percentage(small, 50.0));
    let large = tidemark_store::space_guard::FsTotals {
        size: 100 << 30,
        avail: 80 << 30,
    };
    assert!(!absolute_floor_hides_percentage(large, 50.0));
}

#[test]
fn cold_commands_enforce_configured_min_free_pct() {
    let root = test_support::private_tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("child")).unwrap();
    let state = test_support::private_tempdir().unwrap();
    let canon = tidemark_fs::canon::canonicalize(root.path()).unwrap();
    let query = canon.as_path();
    let mut cfg = Config::default();
    cfg.general.state_dir = state.path().to_owned();
    cfg.retention.snapshots.cold_after_recent = false;
    let now = tidemark_core::time::unix_now();
    scan(&cfg, query, now - 86_400);
    scan(&cfg, query, now);
    let catalog = Catalog::open(state.path(), true).unwrap();
    let mut entries = catalog.list(None).unwrap();
    entries.sort_by_key(|entry| entry.started_at);
    assert_eq!(entries.len(), 2);
    let older_id = entries[0].scan_id.clone();
    let newer_id = entries[1].scan_id.clone();
    let hot = entries[0].file.clone().unwrap();
    let cold = tidemark_store::cold::compress_snapshot(&hot, 1, &|| false).unwrap();
    catalog.point_to_cold(&older_id, &cold).unwrap();
    std::fs::remove_file(&hot).unwrap();
    drop(catalog);
    let Some(length) = length_between(state.path(), 50.0) else {
        if absolute_floor_hides_percentage(filesystem_totals(state.path()), 50.0) {
            eprintln!(
                "skipped: filesystem is too small to host a declared inflate length that 50% refuses and 5% allows"
            );
            return;
        }
        panic!("could not separate a 50% free-space floor from the default 5%");
    };
    let mut bytes = std::fs::read(&cold).unwrap();
    bytes[8..16].copy_from_slice(&length.to_le_bytes());
    std::fs::write(&cold, bytes).unwrap();
    config_support::direct_config(
        state.path(),
        "[retention]\nmin_free_pct = 50.0\nmax_state_size = \"1000TiB\"\n",
    );
    let floor = "state filesystem free-space floor would be crossed";
    let cold_id = older_id.clone();
    for args in [
        vec![
            "diff".to_string(),
            "--from".into(),
            older_id.clone(),
            "--to".into(),
            newer_id,
            "--depth".into(),
            "1".into(),
            query.display().to_string(),
        ],
        vec![
            "top".into(),
            "--snapshot".into(),
            older_id,
            "--depth".into(),
            "1".into(),
            query.display().to_string(),
        ],
        vec![
            "top".into(),
            "--sort".into(),
            "growth".into(),
            "--depth".into(),
            "1".into(),
            query.display().to_string(),
        ],
    ] {
        let stderr = String::from_utf8(
            command(state.path(), false)
                .args(&args)
                .assert()
                .code(1)
                .get_output()
                .stderr
                .clone(),
        )
        .unwrap();
        assert!(
            stderr.contains(floor),
            "configured 50% was not enforced for {args:?}: {stderr}"
        );
        assert!(
            !stderr.contains("maximum state size"),
            "cap masked the percentage for {args:?}: {stderr}"
        );
        if args.first().is_some_and(|arg| arg == "diff") {
            assert!(
                stderr.contains(&cold_id),
                "diff floor refusal did not name the snapshot: {stderr}"
            );
        }
    }
    let plain = command(state.path(), false)
        .args(["top", "--depth", "1", &query.display().to_string()])
        .assert()
        .success();
    let plain = plain.get_output();
    let stdout = String::from_utf8(plain.stdout.clone()).unwrap();
    let stderr = String::from_utf8(plain.stderr.clone()).unwrap();
    assert!(
        stdout.contains("DELTA24h") || stdout.contains("Δ24h"),
        "plain top omitted Δ24h when the predecessor exists: {stdout}"
    );
    assert!(
        stdout.split_whitespace().any(|cell| cell == "-"),
        "plain top did not show '-' for an unavailable Δ24h: {stdout}"
    );
    assert!(
        !stderr.contains(floor),
        "plain top reported the floor refusal: {stderr}"
    );
    let json = command(state.path(), true)
        .args(["top", "--depth", "1", &query.display().to_string()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let rows = doc["data"].as_array().expect("top json data");
    assert!(!rows.is_empty(), "{doc}");
    assert!(
        rows.iter().all(|row| row["delta"].is_null()),
        "plain top json kept a delta without the predecessor: {doc}"
    );
}
