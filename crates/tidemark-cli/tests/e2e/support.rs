//! Owned by: TEST-e2e-scan
//! Private state and unprivileged child commands, including on root test runners.
#[path = "../support/config.rs"]
mod config_support;
pub(super) use config_support::direct_config;
use serde_json::Value;
use std::{
    ffi::OsStr,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Output},
};
/// Owns isolated CLI state, configuration and an executable copy.
pub(super) struct Harness {
    /// Temporary owner of all command outputs.
    pub(super) temp: tempfile::TempDir,
    /// State path, initially absent so preflight mutations are observable.
    pub(super) state: PathBuf,
    config: PathBuf,
    binary: PathBuf,
    drop_uid: bool,
}
impl Harness {
    /// Prepare private storage and detect whether root needs to drop DAC privileges.
    pub(super) fn new() -> Self {
        let temp = ::test_support::private_tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o1777)).unwrap();
        let state = temp.path().join("state");
        let config = direct_config(
            temp.path(),
            "[scan]\nthreads=2\n[throttle]\nmin_ops_per_sec=50000\nmax_ops_per_sec=50000\n",
        );
        let binary = temp.path().join("tidemark");
        std::fs::copy(env!("CARGO_BIN_EXE_tidemark"), &binary).unwrap();
        let probe = temp.path().join("permission-probe");
        std::fs::create_dir(&probe).unwrap();
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o0)).unwrap();
        let drop_uid = rustix::process::geteuid().is_root() && std::fs::read_dir(&probe).is_ok();
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir(probe).unwrap();
        Self {
            temp,
            state,
            config,
            binary,
            drop_uid,
        }
    }
    /// Prepare state owned by the user of CLI children, including root test runners.
    pub(super) fn prepare_state(&self) {
        ::test_support::private_dir(&self.state).unwrap();
        if self.drop_uid {
            rustix::fs::chown(
                &self.state,
                Some(rustix::process::Uid::from_raw(65534)),
                Some(rustix::process::Gid::from_raw(65534)),
            )
            .unwrap();
        }
    }
    /// Run a fixture helper with the same uid as the CLI.
    pub(super) fn fixture_command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        if self.drop_uid {
            command.gid(65534).uid(65534);
        }
        command
    }
    /// Construct a real CLI child with explicit config and state paths.
    pub(super) fn command(&self, json: bool) -> Command {
        let mut c = Command::new(&self.binary);
        c.current_dir(self.temp.path());
        c.args([
            OsStr::new("--config"),
            self.config.as_os_str(),
            OsStr::new("--state-dir"),
            self.state.as_os_str(),
        ]);
        if json {
            c.arg("--json");
        }
        // Mode-000 must remain inaccessible even when CI itself runs as root.
        if self.drop_uid {
            c.gid(65534).uid(65534);
        }
        c
    }
    /// Run a command and require its documented exit code.
    pub(super) fn run(&self, json: bool, args: &[&OsStr], code: i32) -> Output {
        let out = self.command(json).args(args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(code),
            "args={args:?}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
    /// Validate the schema envelope and return its data payload.
    pub(super) fn doc(&self, args: &[&OsStr], code: i32, schema: &str) -> Value {
        let out = self.run(true, args, code);
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(doc["schema"], schema);
        doc["data"].clone()
    }
}
/// Permit a dropped-privilege child to traverse a fixture owner.
pub(super) fn accessible(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
/// Require bounded state traversal to find no leftover snapshot temporary files.
pub(super) fn no_temporaries(path: &Path) {
    let mut pending = vec![path.to_owned()];
    let mut visits = 0;
    while let Some(path) = pending.pop() {
        for e in std::fs::read_dir(path).unwrap() {
            let e = e.unwrap();
            visits += 1;
            assert!(visits < 1000);
            let name = e.file_name();
            let bytes = name.as_encoded_bytes();
            assert!(!bytes.ends_with(b".tmp"), "leftover {:?}", e.path());
            if e.file_type().unwrap().is_dir() {
                pending.push(e.path());
            }
        }
    }
}
