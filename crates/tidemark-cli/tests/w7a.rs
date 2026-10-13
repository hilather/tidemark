//! Owned by: W7-A
//! Non-terminal usage is rejected before snapshot access.

#[test]
fn requires_terminal_with_usage_exit_code() {
    let mut cmd = assert_cmd::Command::cargo_bin("tidemark").unwrap();
    cmd.args(["--config", "/nonexistent/w7a-config", "tui"])
        .assert()
        .code(2)
        .stderr("tui requires terminal input and output\n");
}
