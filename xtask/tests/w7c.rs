//! Owned by: W7-C2-fault
//! Static validation of disposable-guest fault entrypoints.

use std::{path::Path, process::Command};

#[test]
fn fault_scripts_are_guarded_and_valid_shell() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut scripts: Vec<_> = std::fs::read_dir(root.join("tests/fault"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sh"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "fault suite must have entrypoints");
    let shellcheck = Command::new("shellcheck").arg("--version").output();
    let has_shellcheck = match shellcheck {
        Ok(output) => {
            assert!(
                output.status.success(),
                "shellcheck --version failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => panic!("cannot execute shellcheck: {error}"),
    };
    for path in scripts {
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("#!/bin/bash\n"), "{}", path.display());
        assert!(text.contains("set -euo pipefail"), "{}", path.display());
        assert!(
            text.contains("TIDEMARK_DEPLOY_TEST_VM"),
            "{}",
            path.display()
        );
        assert!(text.contains("$EUID == 0"), "{}", path.display());
        // The common harness owns the executable trap; require the delegation too.
        assert!(text.contains("trap") && text.contains("source tests/fault/common.bash"));
        for line in text
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
        {
            if line.contains("timeout ") {
                assert!(line.contains("--foreground"), "untracked timeout: {line}");
            }
        }
        if path.file_name().unwrap() == "04-compaction-crash.sh" {
            assert!(text.contains("${TIDEMARK_FAULT_SLOW:-0} != 1"));
            assert!(text.contains(
                "SKIP: 04-compaction-crash.sh needs TIDEMARK_FAULT_SLOW=1 (about 70 minutes)"
            ));
            assert!(text.find("TIDEMARK_FAULT_SLOW").unwrap() < text.find("setup\n").unwrap());
        }
        let output = Command::new("bash").arg("-n").arg(&path).output().unwrap();
        assert!(
            output.status.success(),
            "bash -n {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        if has_shellcheck {
            let output = Command::new("shellcheck")
                .current_dir(root)
                .arg("-x")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "shellcheck {}:\n{}\n{}",
                path.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let common = std::fs::read_to_string(root.join("tests/fault/common.bash")).unwrap();
    assert!(common.contains("trap cleanup EXIT"));
    assert!(common.contains("mkdir -m 0755 \"$work/metrics\""));
    assert!(common.contains("$work/metrics/metrics.prom"));
    for line in common
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
    {
        if line.contains("timeout ") {
            assert!(line.contains("--foreground"), "untracked timeout: {line}");
        }
    }
    let output = Command::new("bash")
        .arg("-n")
        .arg(root.join("tests/fault/common.bash"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bash -n common.bash: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
