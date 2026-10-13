//! Owned by: W7-D2
//! Manual generation, command coverage and packaged asset contracts.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "../src/man.rs"]
mod man;

use clap::CommandFactory;
use std::{collections::BTreeSet, path::Path};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn files(out: &Path) -> BTreeSet<String> {
    std::fs::read_dir(out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect()
}

#[test]
fn deterministic_pages_cover_real_command_tree_and_sections() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    man::run(
        root(),
        ["--out".into(), first.path().as_os_str().to_owned()].into_iter(),
    )
    .unwrap();
    man::generate(second.path()).unwrap();
    assert_eq!(files(first.path()), files(second.path()));
    for name in files(first.path()) {
        let page = std::fs::read_to_string(first.path().join(name)).unwrap();
        assert!(!page.contains("commit:"));
        assert!(!page.contains("target:"));
    }
    for name in files(first.path()) {
        assert_eq!(
            std::fs::read(first.path().join(&name)).unwrap(),
            std::fs::read(second.path().join(&name)).unwrap()
        );
    }
    let mut cli = tidemark_cli::cli::command();
    cli.build();
    let page = std::fs::read_to_string(first.path().join("tidemark.1")).unwrap();
    assert!(page.contains(cli.get_version().unwrap()));
    assert!(page.contains(".TH tidemark 1 \"\""), "empty date in title");
    let mut pending = vec![(cli, "tidemark".to_owned())];
    let mut expected = BTreeSet::from(["tidemarkd.8".to_owned()]);
    while let Some((command, name)) = pending.pop() {
        expected.insert(format!("{name}.1"));
        let page = std::fs::read_to_string(first.path().join(format!("{name}.1"))).unwrap();
        for child in command.get_subcommands().filter(|c| !c.is_hide_set()) {
            let child_name = format!("{name}-{}", child.get_name());
            assert!(
                page.contains(&child_name.replace('-', "\\-")),
                "missing {child_name}"
            );
            pending.push((child.clone(), child_name));
        }
        for section in ["EXIT STATUS", "FILES", "SEE ALSO"] {
            assert!(page.contains(section));
        }
    }
    assert_eq!(files(first.path()), expected);
    let daemon = std::fs::read_to_string(first.path().join("tidemarkd.8")).unwrap();
    let command = tidemarkd::Args::command();
    assert!(daemon.contains(command.get_version().unwrap()));
    for arg in command.get_arguments().filter_map(|arg| arg.get_long()) {
        assert!(
            daemon.contains(&arg.replace('-', "\\-")),
            "missing daemon argument {arg}"
        );
    }
    for text in [
        "SIGHUP",
        "SIGTERM",
        "SIGINT",
        "SIGNALS",
        "/run/tidemark/api.sock",
    ] {
        assert!(daemon.contains(text));
    }
}

#[test]
fn packaged_manuals_exactly_match_generated_files() {
    let workspace = tempfile::tempdir().unwrap();
    man::run(workspace.path(), std::iter::empty()).unwrap();
    let out = workspace.path().join("target/man");
    let metadata: toml::Value =
        toml::from_str(&std::fs::read_to_string(root().join("packaging/rpm/rpm.toml")).unwrap())
            .unwrap();
    let mut packaged = BTreeSet::new();
    for asset in metadata["assets"].as_array().unwrap() {
        let source = asset["source"].as_str().unwrap();
        if let Some(name) = source.strip_prefix("target/man/") {
            assert!(workspace.path().join(source).is_file());
            let section = if name.ends_with(".8") { "man8" } else { "man1" };
            assert_eq!(
                asset["dest"].as_str().unwrap(),
                format!("/usr/share/man/{section}/{name}")
            );
            assert_eq!(asset["mode"].as_str(), Some("0644"));
            assert_eq!(asset["user"].as_str(), Some("root"));
            assert_eq!(asset["group"].as_str(), Some("root"));
            assert_eq!(asset["doc"].as_bool(), Some(true));
            packaged.insert(name.to_owned());
        }
    }
    assert_eq!(packaged, files(&out));
}

#[test]
fn rejects_invalid_generation_arguments() {
    for args in [
        vec!["--out"],
        vec!["--bad"],
        vec!["--out", "/unused", "--out"],
    ] {
        assert!(man::run(root(), args.into_iter().map(Into::into)).is_err());
    }
}

#[test]
fn daemon_short_and_long_version_contract() {
    let short = tidemarkd::Args::command()
        .try_get_matches_from(["tidemarkd", "-V"])
        .unwrap_err()
        .to_string();
    assert_eq!(short, "tidemarkd 0.1.0\n");
    let long = tidemarkd::Args::command()
        .try_get_matches_from(["tidemarkd", "--version"])
        .unwrap_err()
        .to_string();
    let lines: Vec<_> = long.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], short.trim());
    assert!(lines[1].starts_with("commit: "));
    assert!(lines[2].starts_with("target: "));
    assert!(lines[2].contains(", profile "));
}
