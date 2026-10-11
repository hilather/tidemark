//! Owned by: W3-A
//! Walk scan, mutate, scan and executable diff explanation.
use std::sync::atomic::AtomicBool;
use tidemark_core::{config::Config, path::RawPath};
use tidemark_scan::{mode::ModeRequest, ScanRequest};
#[test]
fn scan_mutate_scan_diff_names_changed_directory() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = test_support::materialize(
        "[[entry]]\npath = 'changed'\nkind = 'dir'\n[[entry]]\npath = 'unchanged'\nkind = 'dir'\n",
    );
    let root = fixture.root.clone();
    story(&root, &temp.path().join("state"));
}
#[cfg(feature = "privileged")]
#[test]
fn xfs_scan_mutate_scan_diff_names_changed_directory() {
    let temp = tempfile::tempdir().unwrap();
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let root = fs.mountpoint.join("fixture");
    std::fs::create_dir_all(root.join("changed")).unwrap();
    std::fs::create_dir(root.join("unchanged")).unwrap();
    story(&root, &temp.path().join("state"));
}
fn story(root: &std::path::Path, state: &std::path::Path) {
    let temp = tempfile::tempdir().unwrap();
    let changed = root.join("changed");
    let state = state.to_path_buf();
    let mut cfg = Config::default();
    cfg.general.state_dir = state.clone();
    let request = ScanRequest {
        root: RawPath::from_path(root).unwrap(),
        mode: ModeRequest::Walk,
        throttle: Some("max".into()),
        threads: Some(1),
        exclude_extra: vec![],
        keep_partial: false,
        annotations: vec![],
    };
    let recorder = tidemark_cli::cmd::scan::DirectRecorder {
        state: state.clone(),
    };
    let a = tidemark_scan::run_scan(&cfg, &request, &recorder, &AtomicBool::new(false), &|_| {})
        .unwrap();
    // Allocate actual data, then measure through the scanner rather than asserting
    // filesystem-dependent block sizes.
    std::fs::write(changed.join("growth"), vec![1u8; 2 * 1024 * 1024]).unwrap();
    let b = tidemark_scan::run_scan(&cfg, &request, &recorder, &AtomicBool::new(false), &|_| {})
        .unwrap();
    let old = tidemark_store::Snapshot::open(a.file.as_ref().unwrap()).unwrap();
    let new = tidemark_store::Snapshot::open(b.file.as_ref().unwrap()).unwrap();
    let changed_path = RawPath::from_path(&changed).unwrap();
    let expected = new.dir_by_path(&changed_path).unwrap().unwrap().t_disk as i128
        - old.dir_by_path(&changed_path).unwrap().unwrap().t_disk as i128;
    assert!(expected > 0);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
        .arg("--config")
        .arg(temp.path().join("missing.toml"))
        .arg("--state-dir")
        .arg(&state)
        .arg("diff")
        .arg(root)
        .arg("--from")
        .arg(&a.scan_id)
        .arg("--to")
        .arg(&b.scan_id)
        .args(["--explain", "--explain-min", "1B"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains(&format!("└─ {}", changed_path.display_lossy())),
        "{text}"
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
        .arg("--config")
        .arg(temp.path().join("missing.toml"))
        .arg("--state-dir")
        .arg(&state)
        .arg("--json")
        .arg("diff")
        .arg(root)
        .arg("--from")
        .arg(&a.scan_id)
        .arg("--to")
        .arg(&b.scan_id)
        .args(["--explain", "--explain-min", "1B"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        doc["data"]["explanation"]["steps"][0]["delta"]
            .as_i64()
            .unwrap() as i128,
        expected
    );
    // A removed query target remains queryable from snapshot byte paths.
    std::fs::remove_dir_all(&changed).unwrap();
    let c = tidemark_scan::run_scan(&cfg, &request, &recorder, &AtomicBool::new(false), &|_| {})
        .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
        .arg("--config")
        .arg(temp.path().join("missing.toml"))
        .arg("--state-dir")
        .arg(&state)
        .arg("diff")
        .arg(&changed)
        .arg("--from")
        .arg(&b.scan_id)
        .arg("--to")
        .arg(&c.scan_id)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("Target removed"));
}
