//! Owned by: W2-C
//! Query and renderer coverage from deterministic hand-built snapshot rows.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{path::Path, time::Duration};
use tidemark_cli::render::tree::{self, Directory};
use tidemark_core::{
    config::LinkInstances,
    path::{RawName, RawPath},
    scan::{ScanMode, SnapshotStatus},
    snapshot::{DirRow, FileHit, FileRow, SnapshotMeta},
    time::ScanId,
};
fn row(id: u32, parent: Option<u32>, path: &[u8], disk: u64) -> DirRow {
    let path = RawPath::new(path.to_vec()).unwrap();
    DirRow {
        id,
        parent_id: parent,
        subtree_end: if id == 0 { 3 } else { id },
        depth: u32::from(parent.is_some()),
        name: path
            .file_name()
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_else(|| path.as_bytes().to_vec()),
        path_hash: path.hash64(),
        path,
        ino: u64::from(id) + 1,
        mtime: 100,
        dir_disk: 0,
        dir_apparent: 0,
        projid: None,
        entries: 1,
        errors: 0,
        own_files: 1,
        own_symlinks: 0,
        own_charged: disk,
        own_apparent: disk,
        t_dirs: 1,
        t_files: 1,
        t_inodes: 1,
        t_disk: disk,
        t_apparent: disk,
        t_charged: disk,
        t_reclaimable: disk / 2,
        t_errors: 0,
        age0: disk,
        age1: 0,
        age2: 0,
        age3: 0,
        age4: 0,
        age5: 0,
        newest_mtime: 100,
        max_entries: 1,
    }
}
fn meta(root: RawPath, now: i64) -> SnapshotMeta {
    SnapshotMeta {
        schema_version: 1,
        tool_version: "test".into(),
        scan_id: ScanId::new(now).unwrap(),
        root_display: root.display_lossy(),
        root,
        mode: ScanMode::Walk,
        mode_reason: "fixture".into(),
        status: SnapshotStatus::Complete,
        degraded: false,
        exclude: vec![],
        link_instances: LinkInstances::All,
        started_at: now,
        finished_at: now + 1,
        host: "test".into(),
        kernel: "test".into(),
        os_release: "test".into(),
        fs_uuid: String::new(),
        fs_dev: 0,
        fs_blocksize: 4096,
        threads: 1,
        throttle_profile: "max".into(),
        dirs: 4,
        files: 3,
        inodes: 3,
        errors: 0,
        races: 0,
        skipped: 0,
        noatime_fallbacks: 0,
        phase_ms: Default::default(),
        gate_stats: Default::default(),
    }
}
#[test]
fn tree_unicode_ascii_and_raw_paths() {
    let rows = vec![
        Directory {
            row: row(0, None, b"/fixture", 4096),
            delta: Some(1024),
            live_disk: Some(5120),
        },
        Directory {
            row: row(1, Some(0), b"/fixture/a\xff", 2048),
            delta: Some(-1024),
            live_disk: Some(1024),
        },
        Directory {
            row: row(2, Some(0), b"/fixture/z", 1024),
            delta: Some(0),
            live_disk: None,
        },
    ];
    for ascii in [false, true] {
        let mut buf = Vec::new();
        tree::directories(&rows, true, ascii, false, &mut buf).unwrap();
        insta::with_settings!({snapshot_path =>"snapshots"}, {
            insta::assert_snapshot!(if ascii { "tree_ascii" } else { "tree_unicode" },String::from_utf8(buf).unwrap());
        });
    }
    assert!(tree::ascii_for_lang(None));
    assert!(tree::ascii_for_lang(Some("C")));
    assert!(!tree::ascii_for_lang(Some("en_US.UTF-8")));
    assert!(!tree::ascii_for_lang(Some("C.utf8")));
}
#[test]
fn file_rendering_shared_and_unresolved() {
    let rows = vec![
        FileHit {
            row: FileRow {
                id: 0,
                dir_id: 0,
                name: Some(RawName::new(vec![b'a', 255]).unwrap()),
                ino: 12,
                disk: 1024,
                apparent: 8192,
                mtime: 100,
                nlink: 2,
            },
            path: Some(RawPath::new(b"/fixture/a\xff".to_vec()).unwrap()),
        },
        FileHit {
            row: FileRow {
                id: 1,
                dir_id: 0,
                name: None,
                ino: 13,
                disk: 512,
                apparent: 512,
                mtime: 101,
                nlink: 1,
            },
            path: None,
        },
    ];
    let mut buf = Vec::new();
    tree::files(&rows, false, &mut buf).unwrap();
    insta::with_settings!({snapshot_path =>"snapshots"}, {insta::assert_snapshot!("files",String::from_utf8(buf).unwrap());});
    let mut buf = Vec::new();
    tree::files(&rows, true, &mut buf).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    insta::with_settings!({snapshot_path => "snapshots"}, {insta::assert_json_snapshot!("files_json",doc);});
    assert_eq!(doc["schema"], "tidemark.files.v1");
    assert_eq!(doc["data"][0]["path_b64"], "L2ZpeHR1cmUvYf8=");
    assert!(doc["data"][0].get("path").is_none());
    assert!(doc["data"][1].get("path").is_none());
}
#[test]
fn top_json_schema_and_paths() {
    let rows = vec![Directory {
        row: row(0, None, b"/a\xff", 4096),
        delta: Some(-4),
        live_disk: None,
    }];
    let mut buf = Vec::new();
    tree::directories(&rows, false, true, true, &mut buf).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    insta::with_settings!({snapshot_path => "snapshots"}, {insta::assert_json_snapshot!("top_json",doc);});
    assert_eq!(doc["schema"], "tidemark.top.v1");
    assert!(doc["data"][0]["path_b64"].is_string());
    assert!(doc["data"][0].get("path").is_none());
    assert_eq!(doc["data"][0]["delta"], -4);
}
#[test]
fn outcome_json_schema_and_text() {
    let outcome = tidemark_scan::orchestrator::ScanOutcome {
        scan_id: "fixture".into(),
        file: Some("/state/a.db".into()),
        status: SnapshotStatus::Complete,
        degraded: false,
        mode: ScanMode::Walk,
        mode_reason: "test".into(),
        summary: tidemark_scan::agg::AggSummary {
            dirs: 4,
            files: 3,
            t_disk: 4096,
            t_reclaimable: 2048,
            ..Default::default()
        },
        duration: Duration::from_secs(2),
    };
    let mut buf = Vec::new();
    tidemark_cli::render::scan_progress::outcome(&outcome, true, &mut buf).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    insta::with_settings!({snapshot_path => "snapshots"}, {insta::assert_json_snapshot!("scan_json",doc);});
    assert_eq!(doc["schema"], "tidemark.scan.v1");
    let mut buf = Vec::new();
    tidemark_cli::render::scan_progress::outcome(&outcome, false, &mut buf).unwrap();
    insta::with_settings!({snapshot_path =>"snapshots"}, {insta::assert_snapshot!("scan_summary",String::from_utf8(buf).unwrap());});
}
fn publish(state: &Path, root: &RawPath, now: i64, disks: [u64; 4]) -> String {
    let m = meta(root.clone(), now);
    let dir = state
        .join("snapshots")
        .join(tidemark_store::catalog::root_slug(root));
    let mut writer = tidemark_store::writer::SnapshotWriter::create(&dir, &m).unwrap();
    let paths = [
        root.clone(),
        root.join(&RawName::new(b"a".to_vec()).unwrap()),
        root.join(&RawName::new(b"b".to_vec()).unwrap()),
        root.join(&RawName::new(b"c".to_vec()).unwrap()),
    ];
    let mut dirs = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        dirs.push(row(
            i as u32,
            if i == 0 { None } else { Some(0) },
            p.as_bytes(),
            disks[i],
        ));
    }
    writer.put_dirs(&dirs).unwrap();
    writer
        .put_files(&[FileRow {
            id: 0,
            dir_id: 1,
            name: Some(RawName::new(b"file".to_vec()).unwrap()),
            ino: 100,
            disk: 2048,
            apparent: 4096,
            mtime: now - 10,
            nlink: 2,
        }])
        .unwrap();
    let file = writer.finish(&m).unwrap();
    let catalog = tidemark_store::catalog::Catalog::open(state, true).unwrap();
    let entry = tidemark_store::catalog::ScanEntry {
        scan_id: m.scan_id.to_string(),
        root: root.clone(),
        mode: ScanMode::Walk,
        started_at: now,
        finished_at: Some(now + 1),
        status: "complete".into(),
        degraded: false,
        file: Some(file),
        t_disk: Some(disks[0]),
        t_inodes: Some(3),
        t_dirs: Some(4),
        errors: Some(0),
        duration_ms: Some(1000),
    };
    catalog.record_start(&entry).unwrap();
    m.scan_id.to_string()
}
fn command(state: &Path, args: &[&std::ffi::OsStr]) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"));
    command
        .arg("--config")
        .arg(state.join("absent.toml"))
        .arg("--state-dir")
        .arg(state)
        .arg("--json");
    command.args(args).output().unwrap()
}
#[test]
fn catalog_queries_growth_limits_filters_and_canonicalization() {
    use std::ffi::OsStr;
    let state = tempfile::tempdir().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let root = tidemark_fs::canon::canonicalize(fixture.path()).unwrap();
    let now = tidemark_core::time::unix_now();
    publish(state.path(), &root, now - 86400, [4096, 2048, 2048, 1024]);
    let id = publish(state.path(), &root, now, [5120, 1024, 3072, 2048]);
    let output = command(
        state.path(),
        &[
            OsStr::new("top"),
            fixture.path().as_os_str(),
            OsStr::new("--sort"),
            OsStr::new("growth"),
            OsStr::new("--limit"),
            OsStr::new("3"),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doc["schema"], "tidemark.top.v1");
    assert_eq!(doc["data"].as_array().unwrap().len(), 3);
    let paths: Vec<_> = doc["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        vec![
            root.display_lossy(),
            format!("{}/b", root.display_lossy()),
            format!("{}/c", root.display_lossy())
        ]
    );
    for r in doc["data"].as_array().unwrap() {
        assert_eq!(r["delta"], 1024);
    }
    let output = command(
        state.path(),
        &[
            OsStr::new("files"),
            fixture.path().as_os_str(),
            OsStr::new("--min-size"),
            OsStr::new("3KiB"),
        ],
    );
    assert!(output.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(doc["data"].as_array().unwrap().is_empty());
    let output = command(
        state.path(),
        &[OsStr::new("snapshots"), OsStr::new("show"), OsStr::new(&id)],
    );
    assert!(output.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doc["data"]["scan_id"], id);
    let link = state.path().join("query-link");
    std::os::unix::fs::symlink(fixture.path(), &link).unwrap();
    let output = command(
        state.path(),
        &[
            OsStr::new("top"),
            link.as_os_str(),
            OsStr::new("--depth"),
            OsStr::new("0"),
        ],
    );
    assert!(output.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doc["data"][0]["path"], root.display_lossy());
}
#[test]
fn missing_growth_is_exit_five() {
    use std::ffi::OsStr;
    let state = tempfile::tempdir().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let root = tidemark_fs::canon::canonicalize(fixture.path()).unwrap();
    publish(
        state.path(),
        &root,
        tidemark_core::time::unix_now(),
        [4096, 2048, 1024, 1024],
    );
    let output = command(
        state.path(),
        &[
            OsStr::new("top"),
            fixture.path().as_os_str(),
            OsStr::new("--sort"),
            OsStr::new("growth"),
        ],
    );
    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stderr).contains("older complete snapshot"));
}
#[test]
fn rebuild_preserves_corrupt_database_and_dry_prune_is_read_only() {
    use std::ffi::OsStr;
    let state = tempfile::tempdir().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let root = tidemark_fs::canon::canonicalize(fixture.path()).unwrap();
    publish(state.path(), &root, 100, [4096, 2048, 1024, 1024]);
    let file = state.path().join("timeseries.db");
    std::fs::write(&file, b"corrupt database").unwrap();
    let output = command(state.path(), &[OsStr::new("snapshots"), OsStr::new("list")]);
    assert!(!output.status.success());
    assert_eq!(std::fs::read(&file).unwrap(), b"corrupt database");
    let output = command(
        state.path(),
        &[OsStr::new("snapshots"), OsStr::new("rebuild-catalog")],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doc["data"]["rebuilt"], 1);
    assert_eq!(
        std::fs::read(state.path().join("timeseries-recovery-0/timeseries.db")).unwrap(),
        b"corrupt database"
    );
    let output = command(
        state.path(),
        &[
            OsStr::new("snapshots"),
            OsStr::new("prune"),
            OsStr::new("--dry-run"),
        ],
    );
    assert!(output.status.success());
    assert_eq!(
        tidemark_store::catalog::Catalog::open(state.path(), false)
            .unwrap()
            .list(None)
            .unwrap()
            .len(),
        1
    );
}

#[test]
#[ignore = "needs W2-A"]
fn scan_then_top_files_and_snapshots_end_to_end() {
    use std::ffi::OsStr;
    let fixture = test_support::materialize_file(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-support/fixtures/nonutf8.toml"),
    );
    let state = tempfile::tempdir().unwrap();
    let scan = command(
        state.path(),
        &[
            OsStr::new("scan"),
            fixture.root.as_os_str(),
            OsStr::new("--mode"),
            OsStr::new("walk"),
            OsStr::new("--throttle"),
            OsStr::new("max"),
        ],
    );
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&scan.stdout).unwrap();
    assert_eq!(doc["schema"], "tidemark.scan.v1");
    for cmd in ["top", "files"] {
        let output = command(state.path(), &[OsStr::new(cmd), fixture.root.as_os_str()]);
        assert!(output.status.success());
        let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(doc["schema"], format!("tidemark.{cmd}.v1"));
    }
    let output = command(state.path(), &[OsStr::new("snapshots"), OsStr::new("list")]);
    assert!(output.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doc["schema"], "tidemark.snapshots.v1");
    let live = command(
        state.path(),
        &[
            OsStr::new("top"),
            fixture.root.as_os_str(),
            OsStr::new("--live"),
        ],
    );
    assert!(
        live.status.success(),
        "{}",
        String::from_utf8_lossy(&live.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&live.stdout).unwrap();
    assert!(doc["data"][0]["live_disk"].is_number());
    assert_eq!(
        tidemark_store::catalog::Catalog::open(state.path(), false)
            .unwrap()
            .list(None)
            .unwrap()
            .len(),
        1
    );
}

struct ChildGuard(Option<std::process::Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn interrupt_scan(keep_partial: bool) -> (std::process::Output, tempfile::TempDir) {
    let fixture = tempfile::tempdir().unwrap();
    for i in 0..100 {
        let dir = fixture.path().join(format!("dir{i}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::File::create(dir.join("file")).unwrap();
    }
    let state = tempfile::tempdir().unwrap();
    let config = state.path().join("config.toml");
    std::fs::write(
        &config,
        "[throttle]\nmin_ops_per_sec=10\nmax_ops_per_sec=10\n",
    )
    .unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"));
    command
        .arg("--config")
        .arg(&config)
        .arg("--state-dir")
        .arg(state.path())
        .arg("--json")
        .arg("scan")
        .arg(fixture.path())
        .args(["--mode", "walk", "--threads", "1", "--throttle", "gentle"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if keep_partial {
        command.arg("--keep-partial");
    }
    let child = command.spawn().unwrap();
    let pid = child.id();
    let mut guard = ChildGuard(Some(child));
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(catalog) = tidemark_store::catalog::Catalog::open(state.path(), false) {
            if catalog
                .list(None)
                .unwrap()
                .iter()
                .any(|e| e.status == "running")
            {
                break;
            }
        }
        assert!(std::time::Instant::now() < deadline, "scan never started");
        assert!(
            guard.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "scan exited before interruption"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Let the walker publish the root record before stopping it.
    std::thread::sleep(Duration::from_millis(400));
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid as i32).unwrap(),
        rustix::process::Signal::INT,
    )
    .unwrap();
    loop {
        if guard.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "cancelled scan did not exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = guard.0.take().unwrap().wait_with_output().unwrap();
    (output, state)
}
#[test]
fn sigint_records_failed_and_leaves_no_snapshot() {
    let (output, state) = interrupt_scan(false);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = tidemark_store::catalog::Catalog::open(state.path(), false)
        .unwrap()
        .list(None)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "failed");
    assert!(rows[0].file.is_none());
    assert!(!state.path().join("snapshots").exists());
}
#[test]
#[ignore = "needs W2-A"]
fn sigint_keep_partial_publishes_partial_and_is_not_latest() {
    let (output, state) = interrupt_scan(true);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let catalog = tidemark_store::catalog::Catalog::open(state.path(), false).unwrap();
    let rows = catalog.list(None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "partial");
    assert!(rows[0].file.as_ref().unwrap().is_file());
    assert!(catalog.latest(&rows[0].root).unwrap().is_none());
    let snapshot = tidemark_store::reader::Snapshot::open(rows[0].file.as_ref().unwrap()).unwrap();
    assert_eq!(snapshot.meta().status, SnapshotStatus::Partial);
}
