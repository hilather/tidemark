//! Owned by: WSPACE-exit
//! Each command returns `StoreError::Space` for a guard refusal, and
//! `command_error` is the exit-5 mapping. Plain `top`'s floor skip stays Ok.

#![allow(clippy::unwrap_used)]

use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tidemark_core::{config::Config, path::RawPath, snapshot::DirRow, units::ByteSize};
use tidemark_scan::{
    mode::ModeRequest,
    orchestrator::{run_scan_with_clock, ScanError, ScanRecorder, ScanRequest},
};
use tidemark_store::{
    catalog::{Catalog, ScanEntry},
    space_guard::SpaceRefusal,
    StoreError,
};

use super::{diff, export, files, links, snapshots, top, tui, wide};

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

fn context(state: &Path, pct: f64, cap: u64) -> crate::Context {
    let mut config = Config::default();
    config.general.state_dir = state.to_path_buf();
    config.general.socket = state.join("absent.sock");
    config.retention.min_free_pct = pct;
    config.retention.max_state_size = ByteSize(cap);
    config.retention.snapshots.cold_after_recent = false;
    crate::Context {
        config,
        config_path: state.join("config.toml"),
        json: false,
        color: false,
    }
}

fn refusal(
    result: anyhow::Result<()>,
    what: &str,
    expect: impl Fn(Option<&SpaceRefusal>) -> bool,
    reason: &str,
) {
    let err = match result {
        Ok(()) => panic!("{what} succeeded; expected {reason}"),
        Err(err) => err,
    };
    let found = err.chain().any(|source| {
        matches!(
            source.downcast_ref::<StoreError>(),
            Some(StoreError::Space { kind, .. }) if expect(kind.as_ref())
        )
    });
    assert!(found, "{what} did not return the expected refusal: {err:#}");
    let cli = crate::command_error(err);
    assert_eq!(cli.exit_code(), 5, "{what}: {cli}");
    assert!(cli.to_string().contains(reason), "{what}: {cli}");
}

fn is_floor(kind: Option<&SpaceRefusal>) -> bool {
    matches!(kind, Some(SpaceRefusal::Floor { .. }))
}
fn is_cap(kind: Option<&SpaceRefusal>) -> bool {
    matches!(kind, Some(SpaceRefusal::MaxStateSize))
}
fn is_policy(kind: Option<&SpaceRefusal>) -> bool {
    matches!(kind, Some(SpaceRefusal::InvalidPolicy))
}

fn colden(state: &Path, id: &str) {
    let catalog = Catalog::open(state, true).unwrap();
    let hot = catalog.get(id).unwrap().unwrap().file.unwrap();
    let cold = tidemark_store::cold::compress_snapshot(&hot, 1, &|| false).unwrap();
    catalog.point_to_cold(id, &cold).unwrap();
    std::fs::remove_file(hot).unwrap();
}

fn diff_args(query: &Path, from: &str, to: &str, explain: bool) -> diff::Args {
    diff::Args {
        path: Some(query.to_path_buf()),
        since: None,
        from: Some(from.to_owned()),
        to: Some(to.to_owned()),
        depth: 1,
        metric: None,
        explain,
        explain_ratio: 0.5,
        explain_min: ByteSize(1 << 30),
        limit: Some(5),
    }
}

fn top_args(query: &Path, snapshot: Option<&str>, growth: bool) -> top::Args {
    top::Args {
        path: Some(query.to_path_buf()),
        depth: 1,
        sort: growth.then_some(top::Sort::Growth),
        limit: 25,
        min_size: None,
        snapshot: snapshot.map(str::to_owned),
        wide: false,
        live: false,
    }
}

/// Commands that must open `older` (cold) while `newer` is still hot.
fn hard_while_newer_is_hot(
    ctx: &crate::Context,
    query: &Path,
    older: &str,
    newer: &str,
    out: &Path,
) -> Vec<(&'static str, anyhow::Result<()>)> {
    vec![
        (
            "diff",
            diff::run(&diff_args(query, older, newer, false), ctx),
        ),
        (
            "diff --explain",
            diff::run(&diff_args(query, older, newer, true), ctx),
        ),
        (
            "top --snapshot",
            top::run(&top_args(query, Some(older), false), ctx),
        ),
        (
            "top --sort growth",
            top::run(&top_args(query, None, true), ctx),
        ),
        (
            "snapshots show",
            snapshots::run(
                &snapshots::Args {
                    command: snapshots::Command::Show {
                        id: older.to_owned(),
                        root: None,
                    },
                },
                ctx,
            ),
        ),
        (
            "snapshots export",
            snapshots::run(
                &snapshots::Args {
                    command: snapshots::Command::Export {
                        sqlite: true,
                        id: older.to_owned(),
                        out: out.to_path_buf(),
                        root: None,
                    },
                },
                ctx,
            ),
        ),
        (
            "files --snapshot",
            files::run(
                &files::Args {
                    path: Some(query.to_path_buf()),
                    direct: false,
                    limit: Some(5),
                    min_size: None,
                    older_than: None,
                    newer_than: None,
                    snapshot: Some(older.to_owned()),
                },
                ctx,
            ),
        ),
        (
            "export ncdu --snapshot",
            export::run(
                &export::Args {
                    command: export::Command::Ncdu {
                        path: Some(query.to_path_buf()),
                        output: None,
                        snapshot: Some(older.to_owned()),
                        utf8_lossy: false,
                        max_depth: Some(1),
                    },
                },
                ctx,
            ),
        ),
        (
            "tui startup select",
            snapshots::select(ctx, Some(query), Some(older)).map(|_| ()),
        ),
    ]
}

fn latest_cold(ctx: &crate::Context, query: &Path) -> Vec<(&'static str, anyhow::Result<()>)> {
    vec![
        (
            "wide",
            wide::run(
                &wide::Args {
                    path: Some(query.to_path_buf()),
                    min_entries: 1,
                    limit: Some(5),
                },
                ctx,
            ),
        ),
        (
            "links",
            links::run(
                &links::Args {
                    path: Some(query.to_path_buf()),
                    boundary: Vec::new(),
                    boundary_depth: None,
                    incomplete: false,
                    top: Some(5),
                    show_paths: false,
                },
                ctx,
            ),
        ),
        (
            "top selected cold snapshot",
            top::run(&top_args(query, None, false), ctx),
        ),
    ]
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

#[test]
fn each_command_preserves_the_refusal_kind_for_command_error() {
    let root = test_support::private_tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("child")).unwrap();
    let state = test_support::private_tempdir().unwrap();
    let out = test_support::private_tempdir().unwrap();
    let query = tidemark_fs::canon::canonicalize(root.path()).unwrap();
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
    let query = query.as_path();
    let export_out = out.path().join("export.db");

    let floor = context(state.path(), 100.0, u64::MAX);
    let floor_reason = "state filesystem free-space floor would be crossed";
    top::run(&top_args(query, None, false), &floor).expect("plain top floor skip stays success");
    for (name, result) in hard_while_newer_is_hot(&floor, query, &older, &newer, &export_out) {
        refusal(result, name, is_floor, floor_reason);
    }

    let cap = context(state.path(), 5.0, 0);
    let cap_reason = "maximum state size would be exceeded";
    refusal(
        top::run(&top_args(query, None, false), &cap),
        "plain top",
        is_cap,
        cap_reason,
    );
    for (name, result) in hard_while_newer_is_hot(&cap, query, &older, &newer, &export_out) {
        refusal(result, name, is_cap, cap_reason);
    }

    // Config-file validation would reject this percentage with exit 2. The
    // space guard still classifies it, and every command that reaches the
    // guard must exit 5 rather than treating it as a floor skip.
    let invalid = context(state.path(), f64::NAN, u64::MAX);
    let policy_reason = "invalid minimum free percentage";
    refusal(
        top::run(&top_args(query, None, false), &invalid),
        "plain top",
        is_policy,
        policy_reason,
    );
    for (name, result) in hard_while_newer_is_hot(&invalid, query, &older, &newer, &export_out) {
        refusal(result, name, is_policy, policy_reason);
    }
    // Without a terminal, tui reports usage before select. The select path
    // above is what startup uses once stdin and stdout are terminals.
    let usage = tui::run(
        &tui::Args {
            path: Some(query.to_path_buf()),
            snapshot: Some(older.clone()),
        },
        &invalid,
    )
    .expect_err("tui without a terminal");
    assert_eq!(crate::command_error(usage).exit_code(), 2);

    colden(state.path(), &newer);
    for (name, result) in latest_cold(&floor, query) {
        refusal(result, name, is_floor, floor_reason);
    }
    for (name, result) in latest_cold(&cap, query) {
        refusal(result, name, is_cap, cap_reason);
    }
    for (name, result) in latest_cold(&invalid, query) {
        refusal(result, name, is_policy, policy_reason);
    }
}
