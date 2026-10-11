//! Owned by: W0-B2
//! Contract tests for all command help and executable behavior.

use clap::{CommandFactory, Parser};
use tidemark_cli::cli::Cli;

#[test]
fn complete_command_and_flag_tree() {
    let mut tree = Cli::command();
    tree.build();
    tree.clone().debug_assert();
    let expected = [
        ("probe", vec!["root"]),
        (
            "scan",
            vec![
                "root",
                "mode",
                "throttle",
                "threads",
                "exclude",
                "keep_partial",
                "cancel",
            ],
        ),
        ("snapshots", vec![]),
        (
            "top",
            vec![
                "path", "depth", "sort", "limit", "min_size", "snapshot", "wide", "live",
            ],
        ),
        (
            "files",
            vec![
                "path",
                "limit",
                "min_size",
                "older_than",
                "newer_than",
                "snapshot",
            ],
        ),
        ("wide", vec!["path", "min_entries", "limit"]),
        (
            "diff",
            vec![
                "path",
                "since",
                "from",
                "to",
                "depth",
                "metric",
                "explain",
                "explain_ratio",
                "explain_min",
                "limit",
            ],
        ),
        (
            "links",
            vec![
                "path",
                "boundary",
                "boundary_depth",
                "incomplete",
                "top",
                "show_paths",
            ],
        ),
        (
            "symlinks",
            vec!["path", "check_targets", "dangling", "escaping", "limit"],
        ),
        ("export", vec![]),
        ("now", vec!["mount"]),
        ("hot", vec!["window", "by", "mount", "limit"]),
        ("lvm", vec!["mount"]),
        ("phantom", vec!["mount", "direct"]),
        ("writers", vec!["mount", "window", "direct", "interval"]),
        ("pg", vec!["relations", "db"]),
        ("alerts", vec!["all"]),
        ("deploy", vec![]),
        ("watch", vec!["mount", "events"]),
        ("status", vec!["show_config"]),
        ("tui", vec!["path", "snapshot"]),
        ("completions", vec!["shell"]),
    ];
    assert_eq!(tree.get_subcommands().count(), expected.len());
    let help = tree.render_long_help().to_string();
    for (name, flags) in expected {
        assert!(help.contains(name));
        let command = tree.find_subcommand_mut(name).unwrap();
        let help = command.render_long_help().to_string();
        for flag in flags {
            let argument = command
                .get_arguments()
                .find(|a| a.get_id() == flag)
                .unwrap();
            if let Some(long) = argument.get_long() {
                assert!(help.contains(&format!("--{long}")), "{name}: {long}");
            }
        }
    }
    for (parent, children) in [
        (
            "snapshots",
            vec!["list", "show", "prune", "rebuild-catalog"],
        ),
        ("export", vec!["ncdu"]),
        ("deploy", vec!["stage", "verify", "unstage"]),
    ] {
        let parent = tree.find_subcommand_mut(parent).unwrap();
        assert_eq!(parent.get_subcommands().count(), children.len());
        for name in children {
            let child = parent.find_subcommand_mut(name).unwrap();
            let help = child.render_long_help().to_string();
            for argument in child.get_arguments() {
                if let Some(long) = argument.get_long() {
                    assert!(help.contains(&format!("--{long}")));
                }
            }
        }
    }
}

#[test]
fn nested_flags_and_short_options() {
    let mut tree = Cli::command();
    tree.build();
    for (parent, child, flags) in [
        ("snapshots", "list", vec!["root"]),
        ("snapshots", "show", vec!["id", "root"]),
        ("snapshots", "prune", vec!["root", "dry_run"]),
        ("snapshots", "rebuild-catalog", vec!["root"]),
        (
            "export",
            "ncdu",
            vec!["path", "output", "snapshot", "utf8_lossy", "max_depth"],
        ),
        ("deploy", "stage", vec!["from", "start_now", "skip_check"]),
        ("deploy", "verify", vec!["wait"]),
        ("deploy", "unstage", vec!["purge_state", "yes"]),
    ] {
        let command = tree
            .find_subcommand_mut(parent)
            .unwrap()
            .find_subcommand_mut(child)
            .unwrap();
        let help = command.render_long_help().to_string();
        for flag in flags {
            let arg = command
                .get_arguments()
                .find(|arg| arg.get_id() == flag)
                .unwrap();
            if let Some(long) = arg.get_long() {
                assert!(help.contains(&format!("--{long}")));
            }
            if let Some(short) = arg.get_short() {
                assert!(help.contains(&format!("-{short}")));
            }
        }
    }
    let root_help = tree.render_long_help().to_string();
    for flag in [
        "--config",
        "--state-dir",
        "--json",
        "--color",
        "-c",
        "-v",
        "-q",
    ] {
        assert!(root_help.contains(flag));
    }
}

#[test]
fn defaults_and_rejected_values() {
    let cli = Cli::try_parse_from(["tidemark", "top"]).unwrap();
    if let tidemark_cli::cli::Command::Top(args) = cli.command {
        assert_eq!(args.depth, 1);
        assert_eq!(args.limit, 25);
    } else {
        panic!("wrong command");
    }
    for args in [
        vec!["tidemark", "scan", "--mode", "bad"],
        vec!["tidemark", "diff", "--since", "1h", "--from", "id"],
        vec!["tidemark", "diff", "--to", "id"],
        vec![
            "tidemark",
            "links",
            "--boundary",
            "*",
            "--boundary-depth",
            "1",
        ],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
    let cli =
        Cli::try_parse_from(["tidemark", "status", "--json", "--color", "never", "-v"]).unwrap();
    assert!(cli.json);
    assert_eq!(cli.verbose, 1);
}

#[test]
fn binaries_and_bash_completions() {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"));
    command.args(["completions", "bash"]).assert().success();
    let output = command.output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("complete"));
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"));
    command.args(["probe"]).assert().code(1);
}

#[test]
fn daemon_check_config_binary() {
    // This CLI-owned integration test explicitly builds the other workspace binary.
    // Unit-test harnesses alone do not produce target/debug/tidemarkd.
    assert_cmd::Command::new(env!("CARGO"))
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .args(["build", "--offline", "--bin", "tidemarkd"])
        .assert()
        .success();
    let mut binary = std::path::PathBuf::from(assert_cmd::cargo::cargo_bin!("tidemark"));
    binary.set_file_name("tidemarkd");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let doc = include_str!("../../../docs/plan/08-interfaces.md");
    let example = doc
        .split("```toml\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    std::fs::write(&path, example).unwrap();
    assert_cmd::Command::new(&binary)
        .arg("--config")
        .arg(&path)
        .arg("--check-config")
        .assert()
        .success();
    std::fs::write(&path, "[general]\nunknown_key = true\n").unwrap();
    let output = assert_cmd::Command::new(&binary)
        .arg("--config")
        .arg(&path)
        .arg("--check-config")
        .assert()
        .code(2)
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown field `unknown_key`"));
}

#[test]
fn documented_defaults_and_json_completion() {
    let cli = Cli::try_parse_from(["tidemark", "diff"]).unwrap();
    if let tidemark_cli::cli::Command::Diff(args) = cli.command {
        assert_eq!(args.depth, 3);
        assert_eq!(args.explain_ratio, 0.5);
        assert_eq!(args.explain_min.0, 1 << 30);
    } else {
        panic!("wrong command");
    }
    let cli = Cli::try_parse_from(["tidemark", "wide"]).unwrap();
    if let tidemark_cli::cli::Command::Wide(args) = cli.command {
        assert_eq!(args.min_entries, 10000);
    } else {
        panic!("wrong command");
    }
    let cli = Cli::try_parse_from(["tidemark", "writers"]).unwrap();
    if let tidemark_cli::cli::Command::Writers(args) = cli.command {
        assert_eq!(args.interval.as_secs(), 10);
        assert!(!args.direct);
    } else {
        panic!("wrong command");
    }
    assert!(Cli::try_parse_from(["tidemark", "writers", "--interval", "2s"]).is_err());
    assert!(Cli::try_parse_from(["tidemark", "writers", "--direct", "--interval", "2s"]).is_ok());
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tidemark"))
        .args(["--json", "completions", "bash"])
        .assert()
        .success()
        .get_output()
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "tidemark.completions.v1");
    assert!(value["data"].as_str().unwrap().contains("complete"));
}
