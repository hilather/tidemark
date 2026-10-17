//! Owned by: WSNAP-cold-daemon
//! Serialized scans apply cold retention using an injected clock.
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
fn config(state: &Path, enabled: bool) -> Config {
    let mut cfg = Config::default();
    cfg.general.state_dir = state.to_owned();
    cfg.retention.snapshots.recent = 2;
    cfg.retention.snapshots.cold_after_recent = enabled;
    cfg.retention.snapshots.cold_level = 1;
    cfg
}
#[test]
fn completed_scans_compress_calendar_representatives_and_preserve_totals() {
    for enabled in [true, false] {
        let state = test_support::private_tempdir().unwrap();
        let root = test_support::private_tempdir().unwrap();
        let other = test_support::private_tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        test_support::private_write(root.path().join("child/file"), b"fixture").unwrap();
        let cfg = config(state.path(), enabled);
        let epoch = 1_800_000_000;
        scan(&cfg, other.path(), epoch - 86400);
        for day in 0..5 {
            scan(&cfg, root.path(), epoch + day * 86400);
        }
        let catalog = Catalog::open(state.path(), false).unwrap();
        let entries = catalog
            .list(Some(&RawPath::from_path(root.path()).unwrap()))
            .unwrap();
        assert_eq!(entries.len(), 5);
        let mut entries = entries;
        entries.sort_by_key(|e| e.started_at);
        let mut total = None;
        for (i, e) in entries.iter().enumerate() {
            let file = e.file.as_ref().unwrap();
            assert_eq!(
                file.as_os_str().as_encoded_bytes().ends_with(b".db.zst"),
                enabled && i < 3
            );
            let snapshot = Snapshot::open(file).unwrap();
            let row = snapshot.root().unwrap();
            let totals = (row.t_disk, row.t_files, row.t_dirs, row.t_inodes);
            assert_eq!(*total.get_or_insert(totals), totals);
            assert_eq!(snapshot.meta().scan_id.to_string(), e.scan_id);
        }
        let newest_other = catalog
            .latest(&RawPath::from_path(other.path()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(newest_other.file.unwrap().extension().unwrap(), "db");
        // Explicit policy deletion must remove cold and both quarantine siblings.
        let old = entries[0].file.as_ref().unwrap();
        let hot = if enabled {
            old.with_extension("")
        } else {
            old.clone()
        };
        let bad_hot = hot.with_extension("db.bad");
        let bad_cold = hot.with_extension("db.zst.bad");
        test_support::private_write(&bad_hot, b"quarantine").unwrap();
        test_support::private_write(&bad_cold, b"quarantine").unwrap();
        let mut deletion = cfg.clone();
        deletion.retention.snapshots.daily = 0;
        deletion.retention.snapshots.weekly = 0;
        scan(&deletion, root.path(), epoch + 5 * 86400);
        assert!(!old.exists());
        assert!(!bad_hot.exists());
        assert!(!bad_cold.exists());
        assert_eq!(
            catalog
                .list(Some(&RawPath::from_path(root.path()).unwrap()))
                .unwrap()
                .len(),
            2
        );
    }
}

struct FillingRecorder(Recorder);
impl ScanRecorder for FillingRecorder {
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError> {
        self.0.scan_started(e)
    }
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError> {
        self.0.scan_finished(e)?;
        test_support::private_write(self.0 .0.join("ballast"), vec![0; 32 << 20])?;
        Ok(())
    }
    fn dir_series(&self, _: &RawPath, _: &str, _: i64, _: &[DirRow]) -> Result<(), ScanError> {
        Ok(())
    }
}
#[test]
fn compression_space_refusal_keeps_hot_and_published_scan_succeeds() {
    let state = test_support::private_tempdir().unwrap();
    let root = test_support::private_tempdir().unwrap();
    let mut cfg = config(state.path(), true);
    scan(&cfg, root.path(), 1_800_000_000);
    scan(&cfg, root.path(), 1_800_086_400);
    cfg.retention.max_state_size = tidemark_core::units::ByteSize(16 << 20);
    let req = ScanRequest {
        root: RawPath::from_path(root.path()).unwrap(),
        mode: ModeRequest::Walk,
        throttle: Some("max".into()),
        threads: Some(1),
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    };
    let result = run_scan_with_clock(
        &cfg,
        &req,
        &FillingRecorder(Recorder(state.path().to_owned())),
        &AtomicBool::new(false),
        &|_| {},
        &|| 1_800_172_800,
    )
    .unwrap();
    assert!(result.file.unwrap().exists());
    let entries = Catalog::open(state.path(), false)
        .unwrap()
        .list(None)
        .unwrap();
    assert_eq!(entries.len(), 3);
    assert!(entries
        .iter()
        .all(|e| e.file.as_ref().unwrap().extension().unwrap() == "db"));
}

#[test]
fn following_scan_estimates_cold_predecessor_from_uncompressed_header() {
    use tidemark_store::{
        cold,
        space_guard::{space_check, SpaceVerdict},
    };
    let state = test_support::private_tempdir().unwrap();
    let root = test_support::private_tempdir().unwrap();
    let mut cfg = config(state.path(), false);
    scan(&cfg, root.path(), 1_800_000_000);
    let catalog = Catalog::open(state.path(), true).unwrap();
    let entry = catalog.list(None).unwrap().remove(0);
    let hot = entry.file.unwrap();
    let cold = cold::compress_snapshot(&hot, 1, &|| false).unwrap();
    catalog.point_to_cold(&entry.scan_id, &cold).unwrap();
    std::fs::remove_file(hot).unwrap();
    let compressed_estimate = std::fs::metadata(&cold)
        .unwrap()
        .len()
        .saturating_mul(5)
        .div_ceil(4);
    let actual_estimate = cold::read_cold_header(&cold)
        .unwrap()
        .uncompressed_len
        .saturating_mul(5)
        .div_ceil(4);
    assert!(actual_estimate > compressed_estimate);
    // Find a cap that admits an estimate based on compressed bytes but refuses decoded bytes.
    cfg.retention.max_state_size.0 = 4096;
    while !matches!(
        space_check(state.path(), compressed_estimate, &cfg.retention).unwrap(),
        SpaceVerdict::Ok
    ) {
        cfg.retention.max_state_size.0 += 4096;
        assert!(cfg.retention.max_state_size.0 < 4 << 20);
    }
    assert!(matches!(
        space_check(state.path(), actual_estimate, &cfg.retention).unwrap(),
        SpaceVerdict::Refuse { .. }
    ));
    let req = ScanRequest {
        root: RawPath::from_path(root.path()).unwrap(),
        mode: ModeRequest::Walk,
        throttle: Some("max".into()),
        threads: Some(1),
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    };
    let result = run_scan_with_clock(
        &cfg,
        &req,
        &Recorder(state.path().to_owned()),
        &AtomicBool::new(false),
        &|_| {},
        &|| 1_800_086_400,
    );
    assert!(matches!(result, Err(ScanError::NoSpace)));
    assert_eq!(catalog.list(None).unwrap().len(), 1);
    assert!(cold.exists());
}
