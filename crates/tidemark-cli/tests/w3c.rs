//! Owned by: W3-C
//! Scanned fixture exports and command output protection.
#![allow(clippy::unwrap_used)]
use std::sync::atomic::AtomicBool;
use tidemark_core::{config::Config, path::RawPath};
fn scan(
    fixture: &str,
) -> (
    test_support::Fixture,
    tempfile::TempDir,
    tidemark_store::Snapshot,
) {
    let fixture = test_support::materialize(fixture);
    let temp = tempfile::tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.general.state_dir = temp.path().join("state");
    let recorder = tidemark_cli::cmd::scan::DirectRecorder {
        state: cfg.general.state_dir.clone(),
    };
    let request = tidemark_scan::ScanRequest {
        root: RawPath::from_path(&fixture.root).unwrap(),
        mode: tidemark_scan::mode::ModeRequest::Walk,
        throttle: Some("max".into()),
        threads: Some(1),
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    };
    let result =
        tidemark_scan::run_scan(&cfg, &request, &recorder, &AtomicBool::new(false), &|_| {})
            .unwrap();
    let snap = tidemark_store::Snapshot::open(result.file.as_ref().unwrap()).unwrap();
    (fixture, temp, snap)
}
#[test]
fn export_mode_overwrite_state_protection_and_wide_json() {
    use std::os::unix::fs::PermissionsExt;
    let (fixture, temp, snap) = scan(include_str!("../../test-support/fixtures/nonutf8.toml"));
    let run = |args: &[&std::ffi::OsStr]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
            .arg("--config")
            .arg(temp.path().join("missing"))
            .arg("--state-dir")
            .arg(temp.path().join("state"))
            .args(args)
            .output()
            .unwrap()
    };
    let output = temp.path().join("export.json");
    let args = [
        std::ffi::OsStr::new("export"),
        std::ffi::OsStr::new("ncdu"),
        fixture.root.as_os_str(),
        std::ffi::OsStr::new("--utf8-lossy"),
        std::ffi::OsStr::new("-o"),
        output.as_os_str(),
    ];
    let result = run(&args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let original = std::fs::read(&output).unwrap();
    assert!(!run(&args).status.success());
    assert_eq!(std::fs::read(&output).unwrap(), original);
    let inside = temp.path().join("state/forbidden.json");
    let mut forbidden = args;
    forbidden[5] = inside.as_os_str();
    assert!(!run(&forbidden).status.success());
    assert!(!inside.exists());
    let alias = temp.path().join("state-alias");
    std::os::unix::fs::symlink(temp.path().join("state"), &alias).unwrap();
    let aliased = alias.join("forbidden.json");
    forbidden[5] = aliased.as_os_str();
    assert!(!run(&forbidden).status.success());
    assert!(!aliased.exists());
    let text = run(&[
        std::ffi::OsStr::new("wide"),
        fixture.root.as_os_str(),
        std::ffi::OsStr::new("--min-entries"),
        std::ffi::OsStr::new("0"),
        std::ffi::OsStr::new("--limit"),
        std::ffi::OsStr::new("1"),
    ]);
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("ENTRIES  DISK  PATH"));
    assert!(text.contains(tidemark_cli::render::wide::HINT));
    let stdout = run(&[
        std::ffi::OsStr::new("export"),
        std::ffi::OsStr::new("ncdu"),
        fixture.root.as_os_str(),
        std::ffi::OsStr::new("--utf8-lossy"),
        std::ffi::OsStr::new("--max-depth"),
        std::ffi::OsStr::new("0"),
    ]);
    assert!(stdout.status.success());
    let json: serde_json::Value = serde_json::from_slice(&stdout.stdout).unwrap();
    assert_eq!(json[0], 1);
    assert!(json[3]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| !entry.is_array()));
    let result = run(&[
        std::ffi::OsStr::new("--json"),
        std::ffi::OsStr::new("wide"),
        fixture.root.as_os_str(),
        std::ffi::OsStr::new("--min-entries"),
        std::ffi::OsStr::new("0"),
    ]);
    assert!(result.status.success());
    let json: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(json["schema"], "tidemark.wide.v1");
    assert!(json["data"]["directories"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r.get("path").is_some() || r.get("path_b64").is_some()));
    // A malformed immutable snapshot triggers an export error after output has
    // started. Publication must leave neither the target nor a temporary file.
    let malformed = tempfile::tempdir().unwrap();
    let mut row = snap.root().unwrap();
    row.dir_disk = 1;
    row.own_charged = 0;
    let mut writer =
        tidemark_store::writer::SnapshotWriter::create(malformed.path(), snap.meta()).unwrap();
    writer.put_dirs(&[row]).unwrap();
    let bad_snapshot = writer.finish(snap.meta()).unwrap();
    let snapshots: Vec<_> = std::fs::read_dir(
        temp.path()
            .join("state/snapshots")
            .join(tidemark_store::catalog::root_slug(&snap.meta().root)),
    )
    .unwrap()
    .map(|entry| entry.unwrap().path())
    .collect();
    let snapshot_file = snapshots
        .iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "db"))
        .unwrap();
    std::fs::copy(bad_snapshot, snapshot_file).unwrap();
    let failed = temp.path().join("failed.json");
    let mut failure_args = args;
    failure_args[5] = failed.as_os_str();
    let result = run(&failure_args);
    assert!(!result.status.success());
    assert!(!failed.exists());
    assert!(std::fs::read_dir(temp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .as_encoded_bytes()
            .starts_with(b".tidemark-export-")
    }));
}
#[cfg(feature = "privileged")]
#[test]
fn vm_ncdu_import_reexport() {
    // Optional VM package: no host mounts or privileged operations are needed.
    if std::process::Command::new("ncdu")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("pending VM: ncdu package missing");
        return;
    }
    let (_fixture, temp, snap) = scan(include_str!(
        "../../test-support/fixtures/link_semantics_basic.toml"
    ));
    let path = temp.path().join("export.json");
    let mut file = std::fs::File::create(&path).unwrap();
    tidemark_store::export_ncdu(
        &snap,
        &snap.meta().root,
        &mut file,
        &tidemark_store::NcduOptions {
            utf8_lossy: true,
            max_depth: None,
        },
    )
    .unwrap();
    assert!(std::process::Command::new("ncdu")
        .arg("-f")
        .arg(path)
        .args(["-o", "/dev/null"])
        .status()
        .unwrap()
        .success());
}

#[test]
fn scanned_symlink_other_count_through_cli() {
    let (fixture, temp, snap) = scan(
        r#"
[[entry]]
path = "regular"
kind = "file"
size = 4096
[[entry]]
path = "symlink"
kind = "symlink"
target = "regular"
"#,
    );
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
        .arg("--config")
        .arg(temp.path().join("missing"))
        .arg("--state-dir")
        .arg(temp.path().join("state"))
        .args(["export", "ncdu"])
        .arg(&fixture.root)
        .arg("--utf8-lossy")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let entries = doc[3].as_array().unwrap();
    let individual = entries.len() as u64 - 2;
    let expected = snap.root().unwrap().own_files - individual;
    assert_eq!(
        entries.last().unwrap()["name"],
        format!("[other files: {expected}]")
    );
}

#[test]
fn scanned_outside_subroot_hardlink_apparent_matches_at_all_depths() {
    let (_fixture, _temp, snap) = scan(
        r#"
[[entry]]
path = "a/d/e/shared"
kind = "file"
size = 4096
[[entry]]
path = "b/alias"
kind = "hardlink"
target = "a/d/e/shared"
"#,
    );
    fn apparent(v: &serde_json::Value) -> u64 {
        if let Some(a) = v.as_array() {
            a.iter().map(apparent).sum()
        } else {
            v["asize"].as_u64().unwrap_or(0)
        }
    }
    let under = snap
        .meta()
        .root
        .join(&tidemark_core::path::RawName::new(b"a".to_vec()).unwrap());
    let mut full = None;
    for depth in [None, Some(0), Some(1), Some(2), Some(3)] {
        let mut out = vec![];
        tidemark_store::export_ncdu(
            &snap,
            &under,
            &mut out,
            &tidemark_store::NcduOptions {
                utf8_lossy: true,
                max_depth: depth,
            },
        )
        .unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let size = apparent(&doc[3]);
        if let Some(full) = full {
            assert_eq!(size, full, "depth {depth:?}");
        } else {
            full = Some(size);
        }
    }
}
