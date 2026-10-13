//! Owned by: WSTATE-trust-B
//! Daemon admission rejects planted state before opening services or changing referents.
#![forbid(unsafe_code)]
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    process::Command,
};

#[test]
fn binary_refuses_hostile_database_families_before_service_start() {
    for name in [
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-journal",
        "timeseries.db.bak-v0-journal",
        "scan.lock",
    ] {
        for kind in ["hardlink", "writable", "symlink", "fifo"] {
            let temp = test_support::private_tempdir().unwrap();
            let state = temp.path().join("state");
            fs::create_dir(&state).unwrap();
            fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
            let sentinel = temp.path().join("sentinel");
            fs::write(&sentinel, b"untouched").unwrap();
            fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o600)).unwrap();
            let leaf = state.join(name);
            match kind {
                "hardlink" => fs::hard_link(&sentinel, &leaf).unwrap(),
                "symlink" => std::os::unix::fs::symlink(&sentinel, &leaf).unwrap(),
                "fifo" => rustix::fs::mknodat(
                    rustix::fs::CWD,
                    &leaf,
                    rustix::fs::FileType::Fifo,
                    rustix::fs::Mode::from_raw_mode(0o600),
                    0,
                )
                .unwrap(),
                _ => {
                    fs::write(&leaf, b"planted").unwrap();
                    fs::set_permissions(&leaf, fs::Permissions::from_mode(0o620)).unwrap();
                }
            }
            let config = temp.path().join("config.toml");
            fs::write(&config, format!("[general]\nstate_dir={state:?}\nsocket={:?}\n[sampler]\nmounts=[]\n[watch]\nenabled=false\nmounts=[]\n[lvm]\nenabled=false\n[procscan]\nenabled=false\n[metrics]\ntextfile=''\n[[roots]]\npath={:?}\nschedule='off'\n", temp.path().join("api.sock"), temp.path())).unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_tidemarkd"))
                .args(["--config"])
                .arg(&config)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1));
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains(name)
                    && error.contains("owner")
                    && error.contains("mode")
                    && error.contains("links"),
                "{kind} {name}: {error}"
            );
            assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
            assert_eq!(fs::metadata(&sentinel).unwrap().mode() & 0o777, 0o600);
            assert!(!state.join("deploy").exists());
            assert_eq!(fs::read_dir(&state).unwrap().count(), 1);
        }
    }
}

#[test]
fn binary_refuses_nonsticky_writable_state_ancestor() {
    let temp = test_support::private_tempdir().unwrap();
    let ancestor = temp.path().join("unsafe");
    fs::create_dir(&ancestor).unwrap();
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o770)).unwrap();
    let state = ancestor.join("state");
    fs::create_dir(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    let config = temp.path().join("config.toml");
    fs::write(&config, format!("[general]\nstate_dir={state:?}\nsocket={:?}\n[sampler]\nmounts=[]\n[watch]\nenabled=false\nmounts=[]\n[lvm]\nenabled=false\n[procscan]\nenabled=false\n[metrics]\ntextfile=''\n[[roots]]\npath={:?}\nschedule='off'\n", temp.path().join("api.sock"), temp.path())).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tidemarkd"))
        .arg("--config")
        .arg(config)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("writable") && error.contains("unsafe"),
        "{error}"
    );
    assert_eq!(fs::read_dir(state).unwrap().count(), 0);
    assert_eq!(fs::metadata(ancestor).unwrap().mode() & 0o777, 0o770);
}
