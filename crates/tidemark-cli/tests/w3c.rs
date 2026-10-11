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
fn sum(v: &serde_json::Value) -> u64 {
    if let Some(a) = v.as_array() {
        a.iter().map(sum).sum()
    } else {
        v.get("dsize").and_then(|v| v.as_u64()).unwrap_or(0)
    }
}
#[test]
fn scanned_link_fixture_additive_export() {
    let (_fixture, _temp, snap) = scan(include_str!(
        "../../test-support/fixtures/link_semantics_basic.toml"
    ));
    let mut out = vec![];
    let stats = tidemark_store::export_ncdu(
        &snap,
        &snap.meta().root,
        &mut out,
        &tidemark_store::NcduOptions {
            utf8_lossy: true,
            max_depth: None,
        },
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(sum(&json[3]), snap.root().unwrap().t_charged);
    assert_eq!(stats.disk_bytes, snap.root().unwrap().t_charged);
    assert!(stats.files > 0);
}
#[test]
fn scanned_nonutf8_preserved_or_lossy_and_escaped() {
    let (_fixture, _temp, snap) = scan(include_str!("../../test-support/fixtures/nonutf8.toml"));
    for lossy in [false, true] {
        let mut out = vec![];
        tidemark_store::export_ncdu(
            &snap,
            &snap.meta().root,
            &mut out,
            &tidemark_store::NcduOptions {
                utf8_lossy: lossy,
                max_depth: None,
            },
        )
        .unwrap();
        if lossy {
            assert!(serde_json::from_slice::<serde_json::Value>(&out).is_ok());
            assert!(out.windows(6).any(|s| s == "��".as_bytes()));
        } else {
            assert!(out.windows(2).any(|s| s == [0xff, 0xfe]));
        }
        assert!(out.windows(6).any(|s| s == b"\\u000a"));
        assert!(out.windows(2).any(|s| s == b"\\\\"));
        assert!(out.windows(2).any(|s| s == b"\\\""));
    }
}
#[test]
fn export_mode_overwrite_state_protection_and_wide_json() {
    use std::os::unix::fs::PermissionsExt;
    let (fixture, temp, _) = scan(include_str!("../../test-support/fixtures/nonutf8.toml"));
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
