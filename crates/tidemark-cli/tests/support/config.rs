//! Owned by: TEST-cancel-deadline
//! Test-owned configuration paths that cannot connect to the system daemon.
#![allow(clippy::unwrap_used)]
/// Write a direct-mode config with an absent socket under the fixture owner.
pub fn direct_config(owner: &std::path::Path, sections: &str) -> std::path::PathBuf {
    let config = owner.join("direct-config.toml");
    std::fs::write(
        &config,
        format!(
            "[general]\nsocket = {:?}\n{sections}",
            owner.join("absent-api.sock")
        ),
    )
    .unwrap();
    config
}
