//! Owned by: WPKG-build-id
//! Short-version compatibility and build identity provenance.
#![allow(dead_code)]

#[path = "../build.rs"]
mod cli_build;
#[path = "../../tidemarkd/build.rs"]
mod daemon_build;

#[test]
fn short_and_long_versions() {
    let run = |arg| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"))
            .arg(arg)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };
    let short = run("-V");
    assert_eq!(short, format!("tidemark {}\n", env!("CARGO_PKG_VERSION")));
    let long = run("--version");
    let lines: Vec<_> = long.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], short.trim());
    let commit = lines[1]
        .strip_prefix("commit: ")
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .trim_end_matches("-dirty");
    assert!(
        commit == "unknown"
            || (commit.len() == 12 && commit.bytes().all(|b| b.is_ascii_hexdigit()))
    );
    assert!(lines[2].starts_with("target: "));
    assert!(lines[2].contains(", profile "));
}
