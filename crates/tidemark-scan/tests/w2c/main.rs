//! Owned by: W2-C
//! Full walk/oracle checks waiting on W2-A and VM-only auto selection checks.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tidemark_core::{config::Config, path::RawPath, snapshot::DirRow};
use tidemark_scan::{
    mode::ModeRequest,
    orchestrator::{run_scan, ScanError, ScanRecorder, ScanRequest},
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
    fn dir_series(
        &self,
        root: &RawPath,
        id: &str,
        ts: i64,
        rows: &[DirRow],
    ) -> Result<(), ScanError> {
        Ok(
            tidemark_store::timeseries::TimeSeries::open(&self.0.join("timeseries.db"), true)?
                .insert_dir_series(root, id, ts, rows)?,
        )
    }
}
fn request(path: &Path, mode: ModeRequest) -> ScanRequest {
    ScanRequest {
        root: tidemark_fs::canon::canonicalize(path).unwrap(),
        mode,
        throttle: Some("max".into()),
        threads: Some(2),
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    }
}
fn config(state: &Path) -> Config {
    let mut cfg = Config::default();
    cfg.general.state_dir = state.to_owned();
    cfg
}
fn normalized_apparent(path: &Path) -> u64 {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let mut total = test_support::oracle::du_apparent(path);
    if test_support::oracle::du_apparent_counts_directories().unwrap() {
        return total;
    }
    let mut stack = vec![path.to_owned()];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(path) = stack.pop() {
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        if !seen.insert((metadata.dev(), metadata.ino())) {
            continue;
        }
        if metadata.is_dir() {
            total += metadata.len();
            for entry in std::fs::read_dir(&path).unwrap() {
                stack.push(entry.unwrap().path());
            }
        } else if metadata.file_type().is_fifo()
            || metadata.file_type().is_socket()
            || metadata.file_type().is_char_device()
            || metadata.file_type().is_block_device()
        {
            total += metadata.len();
        }
    }
    total
}
#[test]
#[ignore = "needs W2-A"]
fn mandatory_walk_fixtures_match_du_find_for_every_directory() {
    assert!(test_support::oracle::gnu_available("du"));
    assert!(test_support::oracle::gnu_available("find"));
    for name in [
        "link_semantics_basic",
        "wide_dir",
        "deep",
        "symlink_loops",
        "nonutf8",
        "hardlink_web",
        "sparse",
        "mixed_special",
    ] {
        let fixture = test_support::materialize_file(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../test-support/fixtures/{name}.toml")),
        );
        let state = tempfile::tempdir().unwrap();
        let cfg = config(state.path());
        let result = run_scan(
            &cfg,
            &request(&fixture.root, ModeRequest::Walk),
            &Recorder(state.path().to_owned()),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        let snapshot = Snapshot::open(&result.file.unwrap()).unwrap();
        let rows = snapshot
            .subtree_dirs(
                0,
                u32::MAX,
                tidemark_core::snapshot::SortKey::Name,
                1_000_000,
            )
            .unwrap();
        for row in rows {
            assert_eq!(
                row.t_disk,
                test_support::oracle::du_disk(row.path.as_path()),
                "{name}: {}",
                row.path.display_lossy()
            );
            assert_eq!(
                row.t_apparent,
                normalized_apparent(row.path.as_path()),
                "{name}: {}",
                row.path.display_lossy()
            );
            let counts = test_support::oracle::find_counts(row.path.as_path());
            assert_eq!(
                (row.t_files, row.t_dirs, row.t_inodes),
                (counts.files, counts.dirs, counts.inodes),
                "{name}: {}",
                row.path.display_lossy()
            );
        }
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(snapshot_path(
                &state,
                &result.scan_id,
                &snapshot.meta().root
            ))
            .unwrap()
            .permissions()
            .mode()
                & 0o777,
            0o600
        );
    }
}
fn snapshot_path(state: &tempfile::TempDir, id: &str, root: &RawPath) -> PathBuf {
    state
        .path()
        .join("snapshots")
        .join(tidemark_store::catalog::root_slug(root))
        .join(format!("{id}.db"))
}
#[cfg(feature = "privileged")]
#[test]
fn auto_xfs_mount_root_and_small_subdir_record_reasons() {
    let fs =
        test_support::xfs::LoopXfs::new(512 << 20, &["-m", "crc=1", "-n", "ftype=1"], "noatime");
    let small = fs.mountpoint.join("small");
    std::fs::create_dir(&small).unwrap();
    for i in 0..32 {
        std::fs::File::create(fs.mountpoint.join(format!("file{i}")))
            .unwrap()
            .set_len(8192)
            .unwrap();
    }
    let state = tempfile::tempdir().unwrap();
    let cfg = config(state.path());
    let rec = Recorder(state.path().to_owned());
    let root = run_scan(
        &cfg,
        &request(&fs.mountpoint, ModeRequest::Auto),
        &rec,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert_eq!(root.mode, tidemark_core::scan::ScanMode::Bulk);
    assert!(root.mode_reason.contains("v5 bulkstat"));
    assert_eq!(
        Snapshot::open(&root.file.unwrap())
            .unwrap()
            .meta()
            .mode_reason,
        root.mode_reason
    );
    let child = run_scan(
        &cfg,
        &request(&small, ModeRequest::Auto),
        &rec,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert_eq!(child.mode, tidemark_core::scan::ScanMode::Walk);
    assert!(child.mode_reason.contains("50%"));
    assert_eq!(
        Snapshot::open(&child.file.unwrap())
            .unwrap()
            .meta()
            .mode_reason,
        child.mode_reason
    );
}
