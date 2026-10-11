//! Owned by: W4-B
//! Fake-service and temporary-path deployment regression tests.

use super::*;
use std::{
    cell::{Cell, RefCell},
    os::unix::fs::{symlink, PermissionsExt},
};

#[derive(Default)]
struct FakeSystemctl {
    calls: RefCell<Vec<String>>,
    enabled: Cell<bool>,
    active: Cell<bool>,
    fail_enable: Cell<bool>,
    refuse_stop: Cell<bool>,
}
impl Systemctl for FakeSystemctl {
    fn is_enabled(&self, _: &str) -> io::Result<bool> {
        Ok(self.enabled.get())
    }
    fn is_active(&self, _: &str) -> io::Result<bool> {
        Ok(self.active.get())
    }
    fn enable(&self, _: &str) -> io::Result<()> {
        self.calls.borrow_mut().push("enable".into());
        if self.fail_enable.get() {
            Err(io::Error::other("injected enable failure"))
        } else {
            self.enabled.set(true);
            Ok(())
        }
    }
    fn disable_now(&self, _: &str) -> io::Result<()> {
        self.calls.borrow_mut().push("disable_now".into());
        self.enabled.set(false);
        if !self.refuse_stop.get() {
            self.active.set(false);
        }
        Ok(())
    }
    fn restart(&self, _: &str) -> io::Result<()> {
        self.calls.borrow_mut().push("restart".into());
        self.active.set(true);
        Ok(())
    }
}
struct FakeEnv {
    installed: PathBuf,
    failures: Vec<&'static str>,
    now: i64,
    capacity: (u64, u64),
    fail_record: Cell<bool>,
}
impl FakeEnv {
    fn probe(&self, name: &str) -> io::Result<()> {
        if self.failures.contains(&name) {
            Err(io::Error::other(format!("injected {name}")))
        } else {
            Ok(())
        }
    }
}
impl Env for FakeEnv {
    fn config_path(&self) -> &Path {
        &self.installed
    }
    fn now(&self) -> i64 {
        self.now
    }
    fn installation(&self, _: &Config) -> io::Result<()> {
        self.probe("installation")
    }
    fn local_directory(&self, _: &Path) -> io::Result<()> {
        self.probe("roots")
    }
    fn mount_point(&self, _: &Path) -> io::Result<()> {
        self.probe("mounts")
    }
    fn capacity(&self, _: &Path) -> io::Result<(u64, u64)> {
        self.probe("state_space")?;
        Ok(self.capacity)
    }
    fn fanotify(&self, _: &Config) -> io::Result<()> {
        self.probe("fanotify")
    }
    fn bulkstat(&self, _: &Config) -> io::Result<()> {
        self.probe("bulkstat")
    }
    fn lvm(&self, _: &Config) -> io::Result<()> {
        self.probe("lvm")
    }
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == "staged.json")
            && self.fail_record.replace(false)
        {
            return Err(io::Error::other("injected record write failure"));
        }
        atomic_write(path, bytes)
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    source: PathBuf,
    state: PathBuf,
    env: FakeEnv,
    sys: FakeSystemctl,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.toml");
        let state = dir.path().join("state");
        fs::write(&source, format!("[general]\nstate_dir = {:?}\n", state)).unwrap();
        let env = FakeEnv {
            installed: dir.path().join("etc/tidemark.toml"),
            failures: vec![],
            now: 100,
            capacity: (4 << 30, 10 << 30),
            fail_record: Cell::new(false),
        };
        Self {
            _dir: dir,
            source,
            state,
            env,
            sys: FakeSystemctl::default(),
        }
    }
    fn stage(&self) -> StagedRecord {
        stage(&self.source, &StageOptions::default(), &self.sys, &self.env).unwrap()
    }
    fn report(&self, hash: &str, healthy: bool) {
        let components: Vec<_> = ["sampler", "scheduler", "api", "tswriter"]
            .into_iter()
            .map(|name| serde_json::json!({"name":name,"state":"ok","detail":""}))
            .collect();
        fs::write(self.state.join("deploy/firstboot.json"), serde_json::to_vec(&serde_json::json!({"config_sha256":hash,"healthy":healthy,"components":components})).unwrap()).unwrap();
    }
}
struct Ping(bool);
impl ApiPing for Ping {
    fn ping(&self) -> io::Result<()> {
        if self.0 {
            Ok(())
        } else {
            Err(io::Error::other("API unavailable"))
        }
    }
}

#[test]
fn preflight_all_environment_checks_and_config_validation() {
    let mut fixture = Fixture::new();
    let mut cfg = Config::load(&fixture.source).unwrap();
    assert!(preflight(&cfg, &fixture.env)
        .iter()
        .all(|c| c.status == CheckStatus::Pass));
    for name in [
        "installation",
        "roots",
        "mounts",
        "state_space",
        "fanotify",
        "bulkstat",
        "lvm",
    ] {
        fixture.env.failures = vec![name];
        let checks = preflight(&cfg, &fixture.env);
        assert_eq!(
            checks.iter().find(|c| c.name == name).unwrap().status,
            if ["fanotify", "bulkstat", "lvm"].contains(&name) {
                CheckStatus::Warn
            } else {
                CheckStatus::Fail
            }
        );
        assert_eq!(checks.len(), 8);
    }
    cfg.general.state_dir = PathBuf::from("relative");
    assert_eq!(
        preflight(&cfg, &fixture.env)
            .iter()
            .find(|c| c.name == "config")
            .unwrap()
            .status,
        CheckStatus::Fail
    );
}
#[test]
fn capacity_byte_and_percentage_floors() {
    let mut fixture = Fixture::new();
    let mut cfg = Config::load(&fixture.source).unwrap();
    for capacity in [
        (2 << 30, 4 << 30),
        ((2 << 30) - 1, 4 << 30),
        (2 << 30, 100 << 30),
        (2 << 30, 0),
    ] {
        fixture.env.capacity = capacity;
        let pass = preflight(&cfg, &fixture.env)
            .into_iter()
            .find(|c| c.name == "state_space")
            .unwrap()
            .status
            == CheckStatus::Pass;
        assert_eq!(pass, capacity == (2 << 30, 4 << 30));
    }
    cfg.retention.min_free_pct = 50.0;
    fixture.env.capacity = (2 << 30, 4 << 30);
    assert_eq!(preflight(&cfg, &fixture.env)[4].status, CheckStatus::Pass);
}
#[test]
fn blocking_failures_change_nothing() {
    for name in ["installation", "roots", "mounts", "state_space"] {
        let mut fixture = Fixture::new();
        fixture.env.failures = vec![name];
        fs::create_dir_all(fixture.env.installed.parent().unwrap()).unwrap();
        fs::write(&fixture.env.installed, b"original").unwrap();
        assert!(matches!(
            stage(
                &fixture.source,
                &StageOptions::default(),
                &fixture.sys,
                &fixture.env
            ),
            Err(DeployError::Preflight(_))
        ));
        assert_eq!(fs::read(&fixture.env.installed).unwrap(), b"original");
        assert!(fixture.sys.calls.borrow().is_empty());
        assert!(!fixture.state.exists());
    }
}
#[test]
fn invalid_config_changes_nothing() {
    let fixture = Fixture::new();
    fs::write(&fixture.source, "[general]\nunknown = true\n").unwrap();
    assert!(matches!(
        stage(
            &fixture.source,
            &StageOptions::default(),
            &fixture.sys,
            &fixture.env
        ),
        Err(DeployError::Preflight(_))
    ));
    assert!(!fixture.state.exists());
    assert!(!fixture.env.installed.exists());
    assert!(fixture.sys.calls.borrow().is_empty());
}
#[test]
fn private_install_backup_hash_and_idempotence() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.env.installed.parent().unwrap()).unwrap();
    fs::write(&fixture.env.installed, b"previous config").unwrap();
    let record = fixture.stage();
    assert_eq!(
        fs::read(
            fixture
                .env
                .installed
                .with_file_name("tidemark.toml.100.bak")
        )
        .unwrap(),
        b"previous config"
    );
    assert_eq!(
        record.config_sha256,
        format!("{:x}", Sha256::digest(fs::read(&fixture.source).unwrap()))
    );
    assert_eq!(
        fs::read(&fixture.env.installed).unwrap(),
        fs::read(&fixture.source).unwrap()
    );
    for path in [
        &fixture.env.installed,
        &fixture.state.join("deploy/staged.json"),
        &fixture.state.join(MARKER),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::metadata(&fixture.state).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(*fixture.sys.calls.borrow(), vec!["enable"]);
    let old = fs::read(fixture.state.join("deploy/staged.json")).unwrap();
    let second = fixture.stage();
    assert_eq!(record.staged_at, second.staged_at);
    assert_eq!(*fixture.sys.calls.borrow(), vec!["enable"]);
    assert_eq!(
        old,
        fs::read(fixture.state.join("deploy/staged.json")).unwrap()
    );
}
#[test]
fn warning_checks_allow_staging_and_start_now() {
    let mut fixture = Fixture::new();
    fixture.env.failures = vec!["fanotify", "bulkstat", "lvm"];
    let options = StageOptions {
        start_now: true,
        skip_check: vec![],
    };
    let report = stage(&fixture.source, &options, &fixture.sys, &fixture.env).unwrap();
    assert_eq!(
        report
            .checks
            .iter()
            .filter(|c| c.status == CheckStatus::Warn)
            .count(),
        3
    );
    assert_eq!(*fixture.sys.calls.borrow(), vec!["enable", "restart"]);
    assert!(matches!(
        stage(
            &fixture.source,
            &StageOptions {
                start_now: false,
                skip_check: vec!["roots".into()]
            },
            &fixture.sys,
            &fixture.env
        ),
        Err(DeployError::Guard(_))
    ));
}
#[test]
fn new_config_restage_requires_new_report() {
    let mut fixture = Fixture::new();
    let record = fixture.stage();
    fixture.report(&record.config_sha256, true);
    fixture.sys.active.set(true);
    assert!(
        verify(&fixture.state, Duration::ZERO, &fixture.sys, &Ping(true))
            .unwrap()
            .healthy
    );
    fixture.env.now = 101;
    let mut bytes = fs::read(&fixture.source).unwrap();
    bytes.extend_from_slice(b"# new configuration\n");
    fs::write(&fixture.source, bytes).unwrap();
    let second = fixture.stage();
    assert_ne!(record.config_sha256, second.config_sha256);
    assert!(
        !verify(&fixture.state, Duration::ZERO, &fixture.sys, &Ping(true))
            .unwrap()
            .healthy
    );
}
#[test]
fn failed_enable_and_record_publication_restore_previous_files() {
    for fail_enable in [true, false] {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.env.installed.parent().unwrap()).unwrap();
        fs::write(&fixture.env.installed, b"original").unwrap();
        fixture.sys.fail_enable.set(fail_enable);
        fixture.env.fail_record.set(!fail_enable);
        assert!(stage(
            &fixture.source,
            &StageOptions::default(),
            &fixture.sys,
            &fixture.env
        )
        .is_err());
        assert_eq!(fs::read(&fixture.env.installed).unwrap(), b"original");
        assert!(!fixture.state.join("deploy/staged.json").exists());
        assert!(!fixture.state.join(MARKER).exists());
        assert!(!fixture
            .env
            .installed
            .with_file_name("tidemark.toml.100.bak")
            .exists());
        assert!(!fixture.sys.enabled.get());
    }
}
#[test]
fn verify_missing_stale_unhealthy_healthy_and_api_failure() {
    let fixture = Fixture::new();
    let record = fixture.stage();
    let result = verify(&fixture.state, Duration::ZERO, &fixture.sys, &Ping(true)).unwrap();
    assert!(!result.healthy);
    assert!(result.items[0].detail.contains("waiting for start"));
    fixture.sys.active.set(true);
    fixture.report("stale", true);
    assert!(
        !verify(&fixture.state, Duration::ZERO, &fixture.sys, &Ping(true))
            .unwrap()
            .healthy
    );
    fixture.report(&record.config_sha256, false);
    assert!(
        !verify(&fixture.state, Duration::ZERO, &fixture.sys, &Ping(true))
            .unwrap()
            .healthy
    );
    fixture.report(&record.config_sha256, true);
    assert!(
        verify(&fixture.state, Duration::ZERO, &fixture.sys, &Ping(true))
            .unwrap()
            .healthy
    );
    assert!(
        !verify(&fixture.state, Duration::ZERO, &fixture.sys, &Ping(false))
            .unwrap()
            .healthy
    );
    let mut report = read_json(&fixture.state.join("deploy/firstboot.json")).unwrap();
    report["components"][0]["state"] = "degraded".into();
    fs::write(
        fixture.state.join("deploy/firstboot.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    assert!(
        !verify(&fixture.state, Duration::ZERO, &fixture.sys, &Ping(true))
            .unwrap()
            .healthy
    );
}
#[test]
fn verify_timeout_is_bounded() {
    let fixture = Fixture::new();
    fixture.stage();
    let start = Instant::now();
    let result = verify(
        &fixture.state,
        Duration::from_millis(20),
        &fixture.sys,
        &Ping(false),
    )
    .unwrap();
    assert!(!result.healthy);
    assert!(start.elapsed() >= Duration::from_millis(20));
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[test]
fn unstage_retains_data_and_purge_removes_only_state() {
    let fixture = Fixture::new();
    fixture.stage();
    fs::write(fixture.state.join("keep.db"), b"own data").unwrap();
    unstage(&fixture.state, false, &fixture.sys).unwrap();
    assert!(fixture.state.join("keep.db").exists());
    assert!(!fixture.state.join("deploy/staged.json").exists());
    fixture.stage();
    let outside = fixture._dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"untouched").unwrap();
    symlink(&outside, fixture.state.join("external-link")).unwrap();
    fs::create_dir(fixture.state.join("nested")).unwrap();
    fs::write(fixture.state.join("nested/file"), b"own data").unwrap();
    unstage(&fixture.state, true, &fixture.sys).unwrap();
    assert!(!fixture.state.exists());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"untouched");
    assert!(fixture.env.installed.exists());
}
#[test]
fn purge_missing_marker_config_mismatch_and_replaced_symlink_leave_state_untouched() {
    for case in ["marker", "config", "symlink"] {
        let fixture = Fixture::new();
        fixture.stage();
        fixture.sys.calls.borrow_mut().clear();
        let keep = fixture.state.join("keep");
        fs::write(&keep, b"data").unwrap();
        let moved = fixture._dir.path().join("moved");
        match case {
            "marker" => {
                fs::remove_file(fixture.state.join(MARKER)).unwrap();
            }
            "config" => {
                fs::write(
                    &fixture.env.installed,
                    format!("[general]\nstate_dir = {:?}\n", moved),
                )
                .unwrap();
            }
            _ => {
                fs::rename(&fixture.state, &moved).unwrap();
                symlink(&moved, &fixture.state).unwrap();
            }
        }
        assert!(unstage(&fixture.state, true, &fixture.sys).is_err());
        assert!(fixture.sys.calls.borrow().is_empty());
        assert_eq!(fs::read(&keep).unwrap(), b"data");
        assert!(fixture.state.join("deploy/staged.json").exists());
    }
}
#[test]
fn purge_waits_for_stop() {
    let fixture = Fixture::new();
    fixture.stage();
    fixture.sys.active.set(true);
    fixture.sys.refuse_stop.set(true);
    assert!(matches!(
        unstage(&fixture.state, true, &fixture.sys),
        Err(DeployError::Guard(_))
    ));
    assert!(fixture.state.join("deploy/staged.json").exists());
}

#[test]
fn skipped_warnings_do_not_invoke_probes() {
    struct Skipped<'a>(&'a FakeEnv);
    impl Env for Skipped<'_> {
        fn config_path(&self) -> &Path {
            self.0.config_path()
        }
        fn now(&self) -> i64 {
            self.0.now()
        }
        fn installation(&self, cfg: &Config) -> io::Result<()> {
            self.0.installation(cfg)
        }
        fn local_directory(&self, path: &Path) -> io::Result<()> {
            self.0.local_directory(path)
        }
        fn mount_point(&self, path: &Path) -> io::Result<()> {
            self.0.mount_point(path)
        }
        fn capacity(&self, path: &Path) -> io::Result<(u64, u64)> {
            self.0.capacity(path)
        }
        fn fanotify(&self, _: &Config) -> io::Result<()> {
            panic!("skipped fanotify invoked")
        }
        fn bulkstat(&self, _: &Config) -> io::Result<()> {
            panic!("skipped bulkstat invoked")
        }
        fn lvm(&self, _: &Config) -> io::Result<()> {
            panic!("skipped lvm invoked")
        }
    }
    let fixture = Fixture::new();
    let opts = StageOptions {
        start_now: false,
        skip_check: vec!["fanotify".into(), "bulkstat".into(), "lvm".into()],
    };
    let report = stage(&fixture.source, &opts, &fixture.sys, &Skipped(&fixture.env)).unwrap();
    assert_eq!(
        report
            .checks
            .iter()
            .filter(|c| c.detail == "skipped by operator")
            .count(),
        3
    );
}
#[test]
fn marker_symlink_and_corrupt_marker_are_rejected_before_stop() {
    for symlink_marker in [true, false] {
        let fixture = Fixture::new();
        fixture.stage();
        fixture.sys.calls.borrow_mut().clear();
        let marker = fixture.state.join(MARKER);
        if symlink_marker {
            let moved = fixture._dir.path().join("marker");
            fs::rename(&marker, &moved).unwrap();
            symlink(&moved, &marker).unwrap();
        } else {
            fs::write(&marker, b"broken").unwrap();
        }
        assert!(unstage(&fixture.state, true, &fixture.sys).is_err());
        assert!(fixture.sys.calls.borrow().is_empty());
        assert!(fixture.state.join("deploy/staged.json").exists());
    }
}
#[test]
fn replacing_state_during_stop_does_not_delete_either_directory() {
    struct Replacing {
        state: PathBuf,
        moved: PathBuf,
        other: PathBuf,
    }
    impl Systemctl for Replacing {
        fn is_enabled(&self, _: &str) -> io::Result<bool> {
            Ok(true)
        }
        fn is_active(&self, _: &str) -> io::Result<bool> {
            Ok(false)
        }
        fn enable(&self, _: &str) -> io::Result<()> {
            unreachable!()
        }
        fn restart(&self, _: &str) -> io::Result<()> {
            unreachable!()
        }
        fn disable_now(&self, _: &str) -> io::Result<()> {
            fs::rename(&self.state, &self.moved)?;
            symlink(&self.other, &self.state)
        }
    }
    let fixture = Fixture::new();
    fixture.stage();
    let moved = fixture._dir.path().join("moved");
    let other = fixture._dir.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("keep"), b"untouched").unwrap();
    let replacing = Replacing {
        state: fixture.state.clone(),
        moved: moved.clone(),
        other: other.clone(),
    };
    assert!(unstage(&fixture.state, true, &replacing).is_err());
    assert!(moved.join("deploy/staged.json").exists());
    assert_eq!(fs::read(other.join("keep")).unwrap(), b"untouched");
}
#[test]
fn raw_config_filename_backup_preserves_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let mut fixture = Fixture::new();
    fixture.env.installed = fixture
        ._dir
        .path()
        .join(std::ffi::OsString::from_vec(b"config-\xff.toml".to_vec()));
    fs::write(&fixture.env.installed, b"original").unwrap();
    fixture.stage();
    let backup = fixture._dir.path().join(std::ffi::OsString::from_vec(
        b"config-\xff.toml.100.bak".to_vec(),
    ));
    assert_eq!(fs::read(backup).unwrap(), b"original");
    unstage(&fixture.state, true, &fixture.sys).unwrap();
    assert!(!fixture.state.exists());
}
#[test]
fn subprocess_wait_is_bounded_without_systemctl() {
    let started = Instant::now();
    let error = RealSystemctl::wait(
        std::process::Command::new("/bin/sh").args(["-c", "exec sleep 10"]),
        Duration::from_millis(20),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(1));
}
