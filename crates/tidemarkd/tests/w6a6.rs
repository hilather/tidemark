//! Owned by: WRECHECK-life
//! Output alias validation and exporter destination preservation.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]

use std::{
    os::unix::fs::{symlink, MetadataExt},
    path::Path,
};
use tidemark_core::config::Config;

fn config(path: &Path, socket: &Path, textfile: &Path, state: &Path) {
    std::fs::write(
        path,
        format!(
            "[general]\nstate_dir='{}'\nsocket='{}'\n[metrics]\ntextfile='{}'\n",
            state.display(),
            socket.display(),
            textfile.display()
        ),
    )
    .unwrap();
}

#[test]
fn collisions_rejected_by_check_config_and_reload() {
    let dir = ::test_support::private_tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let alias = dir.path().join("alias");
    symlink(&real, &alias).unwrap();
    let state_dir = dir.path().join("state");
    let path = dir.path().join("config.toml");
    let socket = real.join("api.sock");
    config(&path, &socket, &real.join("metrics.prom"), &state_dir);
    let old = Config::load(&path).unwrap();
    let state = tidemarkd::state::LiveState::new(&old, 100);
    let reload = tidemarkd::reload_callback(path.clone(), state.clone());
    let before = state.config();
    for (socket, textfile) in [
        (socket.clone(), socket.clone()),
        (socket.clone(), real.join("api.sock.lock")),
        (real.join("metrics.prom.tmp"), real.join("metrics.prom")),
        (socket.clone(), alias.join("api.sock")),
        (socket.clone(), real.join("child/../api.sock")),
        (real.join("api.sock/"), real.join("api.sock.lock")),
        (real.join("metrics.prom.tmp"), real.join("metrics.prom/")),
    ] {
        config(&path, &socket, &textfile, &state_dir);
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidemarkd"))
            .args(["--check-config", "--config"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("collides"),
            "{:?}",
            out
        );
        let result = reload().unwrap();
        assert!(!result.reloaded);
        assert_eq!(state.config(), before);
    }
    config(&path, &socket, &real.join("metrics.prom"), &state_dir);
    let mut output = Vec::new();
    assert_eq!(
        tidemarkd::run_args(
            &tidemarkd::Args {
                config: path,
                check_config: true
            },
            &mut output
        ),
        0
    );
}

#[test]
fn exporter_preserves_nonregular_destination() {
    let dir = ::test_support::private_tempdir().unwrap();
    let destination = dir.path().join("metrics.prom");
    let outside = dir.path().join("outside");
    std::fs::write(&outside, b"unchanged").unwrap();
    symlink(&outside, &destination).unwrap();
    let inode = std::fs::symlink_metadata(&destination).unwrap().ino();
    assert!(tidemarkd::metrics::write_textfile(&destination, "metric 1\n").is_err());
    assert_eq!(
        std::fs::symlink_metadata(&destination).unwrap().ino(),
        inode
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"unchanged");
    std::fs::remove_file(&destination).unwrap();
    std::fs::create_dir(&destination).unwrap();
    assert!(tidemarkd::metrics::write_textfile(&destination, "metric 1\n").is_err());
    assert!(destination.is_dir());
}

#[test]
fn exporter_preserves_live_socket() {
    let dir = ::test_support::private_tempdir().unwrap();
    let path = dir.path().join("api.sock");
    let _listener = match std::os::unix::net::UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("pending unrestricted host: Unix socket bind denied");
            return;
        }
        Err(error) => panic!("socket bind failed: {error}"),
    };
    let inode = std::fs::symlink_metadata(&path).unwrap().ino();
    assert!(tidemarkd::metrics::write_textfile(&path, "metric 1\n").is_err());
    assert_eq!(std::fs::symlink_metadata(&path).unwrap().ino(), inode);
}

#[test]
fn check_config_accepts_metrics_disabled_by_default() {
    let dir = ::test_support::private_tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for metrics in [
        "",
        "[metrics]\ninterval='30s'\n",
        "[metrics]\ntextfile=''\n",
    ] {
        std::fs::write(
            &path,
            format!(
                "[general]\nstate_dir='{}'\nsocket='{}'\n{metrics}",
                dir.path().join("state").display(),
                dir.path().join("api.sock").display()
            ),
        )
        .unwrap();
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_tidemarkd"))
            .args(["--check-config", "--config"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }
}
