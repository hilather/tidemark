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

