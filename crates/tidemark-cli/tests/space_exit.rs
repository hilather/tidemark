//! Owned by: issue-12
//! Process exit codes for a refused cold inflate.
//!
//! Hard refusals exit 5. Plain `top`'s free-space floor skip stays 0. I/O stays 1.
//! An invalid `min_free_pct` in the config file stays 2 (it never reaches the guard).
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]

use std::{
    path::{Path, PathBuf},
    process::Output,
    sync::atomic::AtomicBool,
};
use tidemark_core::{config::Config, path::RawPath, snapshot::DirRow};
use tidemark_scan::{
    mode::ModeRequest,
    orchestrator::{run_scan_with_clock, ScanError, ScanRecorder, ScanRequest},
};
use tidemark_store::catalog::{Catalog, ScanEntry};

#[path = "support/config.rs"]
mod config_support;

struct Recorder(PathBuf);
impl ScanRecorder for Recorder {
    fn scan_started(&self, entry: &ScanEntry) -> Result<(), ScanError> {
        Ok(Catalog::open(&self.0, true)?.record_start(entry)?)
    }
    fn scan_finished(&self, entry: &ScanEntry) -> Result<(), ScanError> {
        Ok(Catalog::open(&self.0, true)?.record_finish(entry)?)
    }
    fn dir_series(&self, _: &RawPath, _: &str, _: i64, _: &[DirRow]) -> Result<(), ScanError> {
        Ok(())
    }
}

fn run_scan(state: &Path, root: &Path, now: i64) -> Result<(), ScanError> {
    let mut cfg = Config::default();
    cfg.general.state_dir = state.to_path_buf();
    cfg.retention.snapshots.cold_after_recent = false;
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
        &cfg,
        &req,
        &Recorder(state.to_path_buf()),
        &AtomicBool::new(false),
        &|_| {},
        &|| now,
    )?;
    assert!(out.file.unwrap().exists());
    Ok(())
}

fn colden(state: &Path, id: &str) {
    let catalog = Catalog::open(state, true).unwrap();
    let hot = catalog.get(id).unwrap().unwrap().file.unwrap();
    let cold = tidemark_store::cold::compress_snapshot(&hot, 1, &|| false).unwrap();
    catalog.point_to_cold(id, &cold).unwrap();
    std::fs::remove_file(hot).unwrap();
}

fn tidemark(state: &Path, json: bool, pty: bool, args: &[String]) -> Output {
    let bin = env!("CARGO_BIN_EXE_tidemark");
    let mut argv = vec![
        bin.to_owned(),
        "-q".to_owned(),
        "--config".to_owned(),
        state.join("direct-config.toml").display().to_string(),
        "--state-dir".to_owned(),
        state.display().to_string(),
    ];
    if json {
        argv.push("--json".to_owned());
    }
    argv.extend(args.iter().cloned());
    if pty {
        std::process::Command::new("python3")
            .arg("-c")
            .arg(PTY)
            .args(&argv)
            .output()
            .unwrap()
    } else {
        std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .unwrap()
    }
}

const PTY: &str = r#"
import os, pty, select, sys
argv = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.execvp(argv[0], argv)
    os._exit(127)
chunks = []
while True:
    ready, _, _ = select.select([fd], [], [], 20)
    if not ready:
        os.kill(pid, 9)
        sys.stderr.buffer.write(b"tui pty timeout\n")
        sys.exit(124)
    try:
        data = os.read(fd, 4096)
    except OSError:
        break
    if not data:
        break
    chunks.append(data)
_, status = os.waitpid(pid, 0)
sys.stderr.buffer.write(b"".join(chunks))
if os.WIFEXITED(status):
    sys.exit(os.WEXITSTATUS(status))
sys.exit(1)
"#;

fn assert_refusal(
    state: &Path,
    json: bool,
    pty: bool,
    args: &[String],
    needle: &str,
    absent: &str,
) {
    let out = tidemark(state, json, pty, args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(5),
        "{args:?} json={json} pty={pty}\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        stdout.is_empty(),
        "{args:?} json={json} wrote stdout: {stdout}"
    );
    assert!(
        stderr.contains(needle) && stderr.contains("state space exhausted"),
        "{args:?} json={json}: {stderr}"
    );
    if !absent.is_empty() {
        assert!(!stderr.contains(absent), "{args:?} json={json}: {stderr}");
    }
    assert!(
        !stderr.contains("delta_skipped") && !stderr.contains("Δ24h skipped"),
        "{args:?} was reported as a soft skip: {stderr}"
    );
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn hard_args(query: &str, older: &str, newer: &str, out: &str) -> Vec<Vec<String>> {
    vec![
        owned(&[
            "diff", "--from", older, "--to", newer, "--depth", "1", query,
        ]),
        owned(&[
            "diff",
            "--explain",
            "--from",
            older,
            "--to",
            newer,
            "--depth",
            "1",
            query,
        ]),
        owned(&["top", "--snapshot", older, "--depth", "1", query]),
        owned(&["top", "--sort", "growth", "--depth", "1", query]),
        owned(&["snapshots", "show", older]),
        owned(&["snapshots", "export", "--sqlite", older, out]),
        owned(&["files", "--snapshot", older, query]),
        owned(&["export", "ncdu", query, "--snapshot", older]),
    ]
}

#[test]
fn refused_cold_inflate_exits_5_for_every_command() {
    let root = test_support::private_tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("child")).unwrap();
    let state = test_support::private_tempdir().unwrap();
    let export_dir = test_support::private_tempdir().unwrap();
    let query = tidemark_fs::canon::canonicalize(root.path()).unwrap();
    let query_s = query.as_path().display().to_string();
    let now = tidemark_core::time::unix_now();
    for stamp in [now - 86_400, now] {
        match run_scan(state.path(), query.as_path(), stamp) {
            Ok(()) => {}
            Err(ScanError::NoSpace) => {
                eprintln!("skipped: fixture scan was refused by the free-space floor");
                return;
            }
            Err(error) => panic!("fixture scan failed: {error}"),
        }
    }
    let catalog = Catalog::open(state.path(), false).unwrap();
    let mut entries = catalog.list(None).unwrap();
    drop(catalog);
    entries.sort_by_key(|entry| entry.started_at);
    assert_eq!(entries.len(), 2);
    let older = entries[0].scan_id.clone();
    let newer = entries[1].scan_id.clone();
    colden(state.path(), &older);
    let export_out = export_dir.path().join("out.db");
    let export_s = export_out.display().to_string();

    config_support::direct_config(
        state.path(),
        "[retention]\nmin_free_pct = 100.0\nmax_state_size = \"1000TiB\"\n",
    );
    let floor = "state filesystem free-space floor would be crossed";
    for json in [false, true] {
        let plain = tidemark(
            state.path(),
            json,
            false,
            &["top", "--depth", "1", &query_s]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        );
        let stderr = String::from_utf8_lossy(&plain.stderr);
        let stdout = String::from_utf8_lossy(&plain.stdout);
        assert_eq!(plain.status.code(), Some(0), "plain top floor: {stderr}");
        assert!(
            stderr.contains("Δ24h skipped"),
            "plain top floor warning missing: {stderr}"
        );
        if json {
            let doc: serde_json::Value = serde_json::from_str(&stdout).unwrap();
            let rows = doc["data"].as_array().expect("top json");
            assert!(
                rows.iter().all(|row| {
                    row["delta"].is_null() && row["delta_skipped"] == "free_space_floor"
                }),
                "{doc}"
            );
        } else {
            assert!(
                stdout.split_whitespace().any(|cell| cell == "-"),
                "plain top did not blank Δ24h: {stdout}"
            );
        }
        for args in hard_args(&query_s, &older, &newer, &export_s) {
            assert_refusal(state.path(), json, false, &args, floor, "");
        }
        assert_refusal(
            state.path(),
            json,
            true,
            &owned(&["tui", "--snapshot", &older, &query_s]),
            floor,
            "",
        );
    }
    assert!(!export_out.exists(), "floor refusal created an export");

    config_support::direct_config(
        state.path(),
        "[retention]\nmin_free_pct = 5.0\nmax_state_size = 0\n",
    );
    let cap = "maximum state size would be exceeded";
    for json in [false, true] {
        assert_refusal(
            state.path(),
            json,
            false,
            &owned(&["top", "--depth", "1", query_s.as_str()]),
            cap,
            "free-space floor would be crossed",
        );
        for args in hard_args(&query_s, &older, &newer, &export_s) {
            assert_refusal(
                state.path(),
                json,
                false,
                &args,
                cap,
                "free-space floor would be crossed",
            );
        }
        assert_refusal(
            state.path(),
            json,
            true,
            &owned(&["tui", "--snapshot", &older, &query_s]),
            cap,
            "free-space floor would be crossed",
        );
    }

    colden(state.path(), &newer);
    config_support::direct_config(
        state.path(),
        "[retention]\nmin_free_pct = 100.0\nmax_state_size = \"1000TiB\"\n",
    );
    for json in [false, true] {
        for args in [
            owned(&["wide", query_s.as_str()]),
            owned(&["links", query_s.as_str()]),
            owned(&["top", "--depth", "1", query_s.as_str()]),
            owned(&["files", query_s.as_str()]),
            owned(&["export", "ncdu", query_s.as_str()]),
            owned(&["snapshots", "show", newer.as_str()]),
            owned(&["tui", query_s.as_str()]),
        ] {
            let pty = args[0] == "tui";
            assert_refusal(state.path(), json, pty, &args, floor, "");
        }
    }
    config_support::direct_config(
        state.path(),
        "[retention]\nmin_free_pct = 5.0\nmax_state_size = 0\n",
    );
    for json in [false, true] {
        for args in [
            owned(&["wide", query_s.as_str()]),
            owned(&["links", query_s.as_str()]),
            owned(&["top", "--depth", "1", query_s.as_str()]),
        ] {
            assert_refusal(
                state.path(),
                json,
                false,
                &args,
                cap,
                "free-space floor would be crossed",
            );
        }
    }

    let missing = Catalog::open(state.path(), false)
        .unwrap()
        .get(&older)
        .unwrap()
        .unwrap()
        .file
        .unwrap();
    std::fs::remove_file(&missing).unwrap();
    config_support::direct_config(state.path(), "[retention]\nmin_free_pct = 5.0\n");
    for args in [
        vec!["snapshots".to_owned(), "show".to_owned(), older.clone()],
        vec![
            "top".to_owned(),
            "--snapshot".to_owned(),
            older.clone(),
            "--depth".to_owned(),
            "1".to_owned(),
            query_s.clone(),
        ],
    ] {
        let out = tidemark(state.path(), false, false, &args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "I/O became {out:?}: {stderr}");
        assert!(
            !stderr.contains("state space exhausted"),
            "I/O was classified as a space refusal: {stderr}"
        );
    }

    config_support::direct_config(state.path(), "[retention]\nmin_free_pct = 150\n");
    let invalid = tidemark(state.path(), false, false, &["top".to_owned(), query_s]);
    let stderr = String::from_utf8_lossy(&invalid.stderr);
    assert_eq!(
        invalid.status.code(),
        Some(2),
        "invalid min_free_pct reached the space guard: {stderr}"
    );
    assert!(
        stderr.contains("retention.min_free_pct"),
        "config error did not name the key: {stderr}"
    );
}
