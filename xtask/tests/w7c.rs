//! Owned by: W7-C2-fault
//! Static validation of disposable-guest fault entrypoints.

use std::{io::Write, path::Path, process::Command};

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
            // Follow the same sources for advisory diagnostics and the warning gate.
            let notes = Command::new("shellcheck")
                .current_dir(root)
                .arg("-x")
                .arg(&path)
                .output()
                .unwrap();
            let mut stderr = std::io::stderr().lock();
            stderr
                .write_all(
                    format!(
                        "shellcheck notes for {}:\n{}\n{}",
                        path.display(),
                        String::from_utf8_lossy(&notes.stdout),
                        String::from_utf8_lossy(&notes.stderr),
                    )
                    .as_bytes(),
                )
                .unwrap();
            stderr.write_all(b"\n").unwrap();
            drop(stderr);
            let output = Command::new("shellcheck")
                .current_dir(root)
                .arg("--severity=warning")
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

// Deliberately conservative: reject forbidden command words even in quoted
// subprocess argv, absolute paths, and inline shell/Python snippets. This is a
// source guard, not shell evaluation or dynamic command/data-flow analysis.
fn forbidden_guest_tool(source: &str) -> Option<&str> {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(|line| line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-'))
        .find(|word| matches!(*word, "rg" | "jq" | "fd" | "bc"))
}

#[test]
fn fault_scripts_use_available_guest_tools() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut pending = vec![root.join("tests/fault")];
    let mut checked = 0;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "sh" || ext == "bash" || ext == "py")
            {
                let source = std::fs::read_to_string(&path).unwrap();
                assert_eq!(
                    forbidden_guest_tool(&source),
                    None,
                    "unsupported guest tool in {}",
                    path.display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 9, "must check entrypoints and shared helpers");
    let common = std::fs::read_to_string(root.join("tests/fault/common.bash")).unwrap();
    assert!(common.find("command -v mkfs.xfs").unwrap() < common.find("work=$(mktemp").unwrap());
    assert!(common.contains("install xfsprogs in the disposable guest"));
}

#[test]
fn guest_tool_guard_covers_shell_and_python_commands() {
    for tool in ["rg", "jq", "fd", "bc"] {
        for source in [
            format!("{tool} -q pattern file"),
            format!("if /usr/bin/{tool} file; then :; fi"),
            format!("subprocess.run(['{tool}', 'file'])"),
            format!("os.system(\"{tool} file\")"),
            format!("run_helper 20 {tool} file"),
        ] {
            assert_eq!(forbidden_guest_tool(&source), Some(tool), "{source}");
        }
    }
    assert_eq!(
        forbidden_guest_tool("# rg is unavailable\ngrep -Fq NoSpace file\nchild_fd = 1"),
        None
    );
}

#[test]
fn shellcheck_advisories_survive_passing_harness_and_warnings_fail() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("shellcheck");
    let path = std::env::join_paths(std::iter::once(dir.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    for fail_warning in [false, true] {
        std::fs::write(&stub, format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then exit 0; fi\nfor arg in \"$@\"; do\nif [ \"$arg\" = --severity=warning ]; then echo WARNING_GATE >&2; exit {}; fi\ndone\necho ADVISORY_STDOUT\necho ADVISORY_STDERR >&2\nexit 1\n",
            u8::from(fail_warning),
        )).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Exact filtering avoids recursively invoking this regression. No nocapture.
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fault_scripts_are_guarded_and_valid_shell",
                "--test-threads=1",
            ])
            .env("PATH", &path)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !fail_warning,
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("ADVISORY_STDOUT"), "{stderr}");
        assert!(stderr.contains("ADVISORY_STDERR"), "{stderr}");
        if fail_warning {
            assert!(String::from_utf8_lossy(&output.stdout).contains("WARNING_GATE"));
        }
    }
}
