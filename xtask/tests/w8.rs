//! Owned by: WRC-fix-product
//! Build identity fallback and rootless VM runner regression tests.
#![allow(dead_code, clippy::unwrap_used)]
#[path = "../../crates/tidemark-cli/build.rs"]
mod cli_build;
#[path = "../../crates/tidemarkd/build.rs"]
mod daemon_build;
#[path = "../src/rpm.rs"]
mod rpm;
#[path = "../src/vm_test.rs"]
mod vm_test;

#[test]
fn remote_commands_pass_host_identity_to_every_build() {
    let identity = vec![
        "TIDEMARK_BUILD_COMMIT=abc-dirty".into(),
        "TIDEMARK_BUILD_DATE=2026-10-13".into(),
    ];
    for suite in ["cargo", "fault", "install"] {
        let args = ["--host", "vm", "--suite", suite]
            .map(str::to_owned)
            .to_vec();
        let command = vm_test::test_command(args, &identity).unwrap();
        for pair in &identity {
            assert_eq!(
                command.matches(pair).count(),
                if suite == "cargo" { 2 } else { 1 },
                "{command}"
            );
        }
    }
}

#[test]
fn scripts_classify_pass_fail_and_skip_with_reason_and_counts() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("tests/fault")).unwrap();
    std::fs::create_dir(temp.path().join("bin")).unwrap();
    let sudo = temp.path().join("bin/sudo");
    std::fs::write(&sudo, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    std::fs::set_permissions(sudo, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (name, text) in [
        ("01", "echo pass; exit 0"),
        ("02", "echo 'SKIP: own reason'; exit 0"),
        ("03", "echo 'SKIP: failed anyway'; exit 7"),
    ] {
        std::fs::write(temp.path().join(format!("tests/fault/{name}.sh")), text).unwrap();
    }
    let command = vm_test::test_command(
        vec![
            "--host".into(),
            "vm".into(),
            "--suite".into(),
            "fault".into(),
            "--remote-dir".into(),
            temp.path().display().to_string(),
        ],
        &[],
    )
    .unwrap();
    let run = || {
        std::process::Command::new("sh")
            .args(["-c", &command])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    temp.path().join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap()
    };
    let output = run();
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("PASS tests/fault/01.sh exit=0"), "{text}");
    assert!(text.contains("SKIP tests/fault/02.sh exit=0"), "{text}");
    assert!(text.contains("SKIP: own reason"), "{text}");
    assert!(text.contains("FAIL tests/fault/03.sh exit=7"), "{text}");
    assert!(text.contains("passed=1 skipped=1"), "{text}");
    std::fs::remove_file(temp.path().join("tests/fault/03.sh")).unwrap();
    assert!(run().status.success());
}
