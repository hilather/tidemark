//! Owned by: W2-C
//! End-to-end walk oracles and VM-only bulk auto selection checks.
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
        tidemark_scan::orchestrator::record_dir_series(&self.0, root, id, ts, rows)
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
#[test]
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
        let spec = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../test-support/fixtures/{name}.toml")),
        )
        .unwrap();
        let fixture = match test_support::fixture::try_materialize(&spec) {
            Ok(fixture) => fixture,
            Err(e)
                if name == "mixed_special" && e.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                eprintln!(
                    "mixed_special pending unrestricted sandbox: Unix socket creation denied ({e})"
                );
                continue;
            }
            Err(e) => panic!("{name}: {e}"),
        };
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
        let catalog = Catalog::open(state.path(), false).unwrap();
        let entry = catalog.get(&result.scan_id).unwrap().unwrap();
        assert_eq!(entry.t_disk, Some(result.summary.t_disk));
        assert_eq!(entry.t_dirs, Some(result.summary.dirs));
        assert_eq!(entry.t_inodes, Some(result.summary.inodes));
        assert_eq!(entry.errors, Some(0));
        assert_eq!(entry.status, "complete");
        let snapshot = Snapshot::open(result.file.as_ref().unwrap()).unwrap();
        assert_eq!(snapshot.meta().dirs, result.summary.dirs);
        assert_eq!(snapshot.meta().files, result.summary.files);
        assert_eq!(snapshot.meta().errors, result.summary.errors);
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
                test_support::oracle::du_apparent_normalized(row.path.as_path()).unwrap(),
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

#[test]
fn random_disk_snapshot_matches_independent_reference() {
    use std::os::unix::fs::MetadataExt;
    use test_support::random::{model_tree, random_tree, RandomTreeParams, SizeDist};
    let params = RandomTreeParams {
        dirs: 100,
        files: 3000,
        link_sets: 350,
        max_depth: 8,
        incomplete_ratio: 0.3,
        size_dist: SizeDist::Fixed(64),
        ..RandomTreeParams::default()
    };
    let fixture = random_tree(8128, params.clone());
    let mut model = model_tree(8128, params);
    model.root = RawPath::from_path(&fixture.root).unwrap();
    let paths = model.paths();
    // Replace abstract sizes with measured metadata, keeping the independently generated links.
    for (dir, path) in model.dirs.iter_mut().zip(&paths) {
        let m = std::fs::symlink_metadata(path.as_path()).unwrap();
        dir.disk = m.blocks() * 512;
        dir.apparent = m.size();
        dir.mtime = m.mtime();
    }
    for (ino, inode) in &mut model.inodes {
        let entry = model.entries.iter().find(|e| e.ino == *ino).unwrap();
        let path = paths[entry.dir].join(&entry.name);
        let m = std::fs::symlink_metadata(path.as_path()).unwrap();
        inode.disk = m.blocks() * 512;
        inode.apparent = m.size();
        inode.mtime = m.mtime();
        assert_eq!(inode.nlink, m.nlink() as u32);
    }
    let state = tempfile::tempdir().unwrap();
    let result = run_scan(
        &config(state.path()),
        &request(&fixture.root, ModeRequest::Walk),
        &Recorder(state.path().to_owned()),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let snapshot = Snapshot::open(result.file.as_ref().unwrap()).unwrap();
    model.now = snapshot.meta().started_at;
    let reference = test_support::reference::compute(&model);
    let rows = snapshot
        .subtree_dirs(0, u32::MAX, tidemark_core::snapshot::SortKey::Name, 1000)
        .unwrap();
    assert_eq!(rows.len(), paths.len());
    for row in rows {
        let id = paths.iter().position(|p| p == &row.path).unwrap();
        let expected = &reference[&tidemark_core::scan::DirId(id as u32)];
        assert_eq!(
            (
                row.t_disk,
                row.t_apparent,
                row.t_charged,
                row.t_reclaimable,
                row.t_disk - row.t_reclaimable
            ),
            (
                expected.t_disk,
                expected.t_apparent,
                expected.t_charged,
                expected.t_reclaimable,
                expected.shared
            ),
            "{}",
            row.path.display_lossy()
        );
        assert_eq!(
            (
                row.t_files,
                row.t_inodes,
                row.t_dirs,
                row.t_errors,
                u64::from(row.max_entries),
                row.newest_mtime
            ),
            (
                expected.t_files,
                expected.t_inodes,
                expected.t_dirs,
                expected.t_errors,
                expected.max_entries,
                expected.newest_mtime
            )
        );
        assert_eq!(
            [row.age0, row.age1, row.age2, row.age3, row.age4, row.age5],
            expected.age
        );
    }
    assert!(result.summary.incomplete_link_sets > 0);
}

#[test]
fn failed_walk_keeps_valid_collected_records_only_when_requested() {
    let fixture = tempfile::tempdir().unwrap();
    let child = fixture.path().join("child");
    std::fs::create_dir(&child).unwrap();
    // The documented threshold-candidate cap deterministically fails the child
    // after its parent record has been sent; no allocation-size oracle is needed.
    for id in 0..4097 {
        std::fs::File::create(child.join(format!("f{id}"))).unwrap();
    }
    for keep in [false, true] {
        let state = tempfile::tempdir().unwrap();
        let mut cfg = config(state.path());
        cfg.roots = vec![tidemark_core::config::RootConfig {
            path: fixture.path().to_owned(),
            big_file_threshold: tidemark_core::units::ByteSize(0),
            top_files: 0,
            ..Default::default()
        }];
        let mut req = request(fixture.path(), ModeRequest::Walk);
        req.threads = Some(1);
        req.keep_partial = keep;
        let result = run_scan(
            &cfg,
            &req,
            &Recorder(state.path().to_owned()),
            &AtomicBool::new(false),
            &|_| {},
        );
        let catalog = Catalog::open(state.path(), false).unwrap();
        if keep {
            let outcome = result.unwrap();
            assert_eq!(outcome.status, tidemark_core::scan::SnapshotStatus::Partial);
            let snapshot = Snapshot::open(outcome.file.as_ref().unwrap()).unwrap();
            assert_eq!(
                snapshot.meta().status,
                tidemark_core::scan::SnapshotStatus::Partial
            );
            assert!(snapshot.dir_by_path(&req.root).unwrap().is_some());
            assert!(catalog.latest(&req.root).unwrap().is_none());
        } else {
            assert!(matches!(
                result,
                Err(ScanError::Walk(tidemark_fs::walk::WalkError::Limit(_)))
            ));
            assert!(catalog
                .list(Some(&req.root))
                .unwrap()
                .iter()
                .all(|e| e.status == "failed" && e.file.is_none()));
        }
    }
}

#[test]
fn automatic_retention_rebuilds_catalog_after_deleting_old_files() {
    let fixture = test_support::materialize("[[entry]]\npath='file'\nkind='file'\nsize=8\n");
    let state = tempfile::tempdir().unwrap();
    let mut cfg = config(state.path());
    cfg.retention.snapshots.recent = 1;
    cfg.retention.snapshots.daily = 0;
    cfg.retention.snapshots.weekly = 0;
    let req = request(&fixture.root, ModeRequest::Walk);
    let recorder = Recorder(state.path().to_owned());
    let first = run_scan(&cfg, &req, &recorder, &AtomicBool::new(false), &|_| {}).unwrap();
    let catalog = Catalog::open(state.path(), true).unwrap();
    let mut entry = catalog.get(&first.scan_id).unwrap().unwrap();
    entry.started_at -= 172800;
    catalog.record_finish(&entry).unwrap();
    drop(catalog);
    let second = run_scan(&cfg, &req, &recorder, &AtomicBool::new(false), &|_| {}).unwrap();
    assert!(!first.file.as_ref().unwrap().exists());
    let catalog = Catalog::open(state.path(), false).unwrap();
    assert!(catalog.get(&first.scan_id).unwrap().is_none());
    assert!(catalog.get(&second.scan_id).unwrap().is_some());
    let remaining = Snapshot::open(second.file.as_ref().unwrap()).unwrap();
    assert!(remaining.dir_by_path(&req.root).unwrap().is_some());
}
