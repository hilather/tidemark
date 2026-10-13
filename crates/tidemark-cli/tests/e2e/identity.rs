//! Owned by: WGUEST-fix3
//! Run state-reading test bodies under the same uid that creates their state.
use std::os::unix::{fs::PermissionsExt, process::CommandExt};

/// Root runners delegate the entire body, preserving private state ownership.
/// Ordinary runners execute the original body directly.
pub(crate) fn delegate(test: &str) -> bool {
    if !rustix::process::geteuid().is_root() {
        return false;
    }
    let temp = test_support::private_tempdir().unwrap();
    // Match the harness: uid 0 without DAC bypass already respects mode-000.
    // Such sandboxes may also have no mapping for uid 65534.
    let probe = temp.path().join("permission-probe");
    std::fs::create_dir(&probe).unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o0)).unwrap();
    let bypass = std::fs::read_dir(&probe).is_ok();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_dir(probe).unwrap();
    if !bypass {
        return false;
    }
    let binary = temp.path().join("test");
    std::fs::copy(std::env::current_exe().unwrap(), &binary).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    rustix::fs::chown(
        temp.path(),
        Some(rustix::process::Uid::from_raw(65534)),
        Some(rustix::process::Gid::from_raw(65534)),
    )
    .unwrap();
    let cli = temp.path().join("tidemark");
    std::fs::copy(env!("CARGO_BIN_EXE_tidemark"), &cli).unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = std::process::Command::new(binary)
        .args(["--exact", test, "--nocapture"])
        .current_dir(temp.path())
        .env("TIDEMARK_TEST_BINARY", &cli)
        .gid(65534)
        .uid(65534)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "delegated {test}: {}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "delegation must execute exactly one test: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    true
}
