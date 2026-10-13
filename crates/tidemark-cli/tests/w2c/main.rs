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
    assert_eq!(doc["data"][0]["row"]["mtime"], 100);
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
    assert_eq!(doc["data"][0]["newest_mtime"], 100);
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

fn fixture_command(state: &Path, json: bool) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::new(env!("CARGO_BIN_EXE_tidemark"));
    command
        .arg("--config")
        .arg(state.join("config.toml"))
        .arg("--state-dir")
        .arg(state);
    if json {
        command.arg("--json");
    }
    command
}

#[test]
fn non_xfs_modes_and_scan_lock_are_explicit() {
    let fixture = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(state.path().join("config.toml"), "[scan]\nthreads=2\n").unwrap();
    // No assumption about the host mount: the non-XFS assertion is conditional on its actual type.
    if !tidemark_fs::mount::read_mountinfo()
        .unwrap()
        .iter()
        .any(|m| m.fs_type == "xfs" && fixture.path().starts_with(m.mount_point.as_path()))
    {
        fixture_command(state.path(), false)
            .arg("scan")
            .arg(fixture.path())
            .args(["--mode", "bulk", "--throttle", "max"])
            .assert()
            .code(5);
        let output = fixture_command(state.path(), true)
            .arg("scan")
            .arg(fixture.path())
            .args(["--mode", "auto", "--throttle", "max"])
            .assert()
            .success()
            .get_output()
            .clone();
        let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(doc["data"]["mode"], "walk");
        assert!(doc["data"]["mode_reason"].as_str().unwrap().contains("XFS"));
    }
    let _lock = tidemark_scan::orchestrator::ScanLock::acquire(state.path()).unwrap();
    let output = fixture_command(state.path(), false)
        .arg("scan")
        .arg(fixture.path())
        .args(["--mode", "walk", "--throttle", "max"])
        .assert()
        .code(5)
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&output.stderr).contains("Busy"));
}

#[test]
fn prune_rebuilds_catalog_for_remaining_files() {
    let fixture = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let root = RawPath::from_path(fixture.path()).unwrap();
    let now = tidemark_core::time::unix_now();
    let old = publish(state.path(), &root, now - 86400, [4096; 4]);
    let retained = publish(state.path(), &root, now - 100, [6000; 4]);
    let current = publish(state.path(), &root, now, [8192; 4]);
    std::fs::write(
        state.path().join("config.toml"),
        "[retention.snapshots]\nrecent=2\ndaily=0\nweekly=0\n",
    )
    .unwrap();
    fixture_command(state.path(), true)
        .args(["snapshots", "prune"])
        .assert()
        .success();
    let catalog = tidemark_store::catalog::Catalog::open(state.path(), false).unwrap();
    assert!(catalog.get(&old).unwrap().is_none());
    assert!(catalog.get(&current).unwrap().is_some());
    assert_eq!(catalog.list(None).unwrap().len(), 2);
    for command in ["top", "files"] {
        fixture_command(state.path(), true)
            .arg(command)
            .arg(fixture.path())
            .assert()
            .success();
    }
    fixture_command(state.path(), true)
        .arg("diff")
        .arg(fixture.path())
        .args(["--from", &retained, "--to", &current])
        .assert()
        .success();
}

#[test]
fn scoped_rebuild_refuses_without_changing_other_root_rows() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let root = RawPath::from_path(second.path()).unwrap();
    let now = tidemark_core::time::unix_now();
    publish(state.path(), &root, now, [4096; 4]);
    let catalog = tidemark_store::catalog::Catalog::open(state.path(), true).unwrap();
    for status in ["failed", "running"] {
        let mut e = catalog.list(Some(&root)).unwrap().remove(0);
        e.scan_id = format!("historical-{status}");
        e.status = status.into();
        e.file = None;
        catalog.record_start(&e).unwrap();
    }
    let before = catalog.list(Some(&root)).unwrap();
    fixture_command(state.path(), true)
        .args(["snapshots", "rebuild-catalog", "--root"])
        .arg(first.path())
        .assert()
        .code(2);
    let after = catalog.list(Some(&root)).unwrap();
    assert_eq!(before.len(), after.len());
    for (before, after) in before.iter().zip(&after) {
        assert_eq!(before.scan_id, after.scan_id);
        assert_eq!(before.root, after.root);
        assert_eq!(before.mode, after.mode);
        assert_eq!(before.started_at, after.started_at);
        assert_eq!(before.finished_at, after.finished_at);
        assert_eq!(before.status, after.status);
        assert_eq!(before.degraded, after.degraded);
        assert_eq!(before.file, after.file);
        assert_eq!(before.t_disk, after.t_disk);
        assert_eq!(before.t_inodes, after.t_inodes);
        assert_eq!(before.t_dirs, after.t_dirs);
        assert_eq!(before.errors, after.errors);
        assert_eq!(before.duration_ms, after.duration_ms);
    }
}

#[test]
#[ignore = "scale"]
fn growth_reader_cap_refuses_instead_of_ranking_disk_prefix() {
    let fixture = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let root = RawPath::from_path(fixture.path()).unwrap();
    let now = tidemark_core::time::unix_now();
    publish(state.path(), &root, now - 86400, [4096; 4]);
    let mut m = meta(root.clone(), now);
    m.dirs = 1_000_001;
    let dir = state
        .path()
        .join("snapshots")
        .join(tidemark_store::catalog::root_slug(&root));
    let mut writer = tidemark_store::writer::SnapshotWriter::create(&dir, &m).unwrap();
    let mut root_row = row(0, None, root.as_bytes(), 4096);
    root_row.subtree_end = 1_000_000;
    writer.put_dirs(&[root_row]).unwrap();
    let mut batch = Vec::with_capacity(1000);
    for id in 1..=1_000_000 {
        let path = root.join(&RawName::new(format!("d{id:07}").into_bytes()).unwrap());
        batch.push(row(
            id,
            Some(0),
            path.as_bytes(),
            if id == 1_000_000 { 1 } else { 4096 },
        ));
        if batch.len() == 1000 {
            writer.put_dirs(&batch).unwrap();
            batch.clear();
        }
    }
    assert!(batch.is_empty());
    writer.finish(&m).unwrap();
    let catalog = tidemark_store::catalog::Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let rejected = fixture_command(state.path(), true)
        .arg("top")
        .arg(fixture.path())
        .args(["--sort", "growth", "--snapshot", &m.scan_id.to_string()])
        .assert()
        .code(1);
    assert!(
        String::from_utf8_lossy(&rejected.get_output().stderr).contains("1,000,000-row result cap")
    );
}
