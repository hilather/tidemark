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
    queries: Cell<usize>,
    enabled: Cell<bool>,
    active: Cell<bool>,
    fail_enable: Cell<bool>,
    refuse_stop: Cell<bool>,
    fail_restart: Cell<bool>,
}
impl Systemctl for FakeSystemctl {
    fn is_enabled(&self, _: &str) -> io::Result<bool> {
        self.queries.set(self.queries.get() + 1);
        Ok(self.enabled.get())
    }
    fn is_active(&self, _: &str) -> io::Result<bool> {
        self.queries.set(self.queries.get() + 1);
        Ok(self.active.get())
    }
    fn enable(&self, _: &str) -> io::Result<()> {
        self.calls.borrow_mut().push("enable".into());
        if self.fail_enable.replace(false) {
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
        if self.fail_restart.replace(false) {
            return Err(io::Error::other("injected restart failure"));
        }
        self.active.set(true);
        Ok(())
    }
}
struct FakeEnv {
    installed: PathBuf,
    required: Vec<PathBuf>,
    filesystem: &'static str,
    filesystem_path: Option<PathBuf>,
    failures: Vec<&'static str>,
    now: i64,
    capacity: (u64, u64),
    fail_record: Cell<bool>,
    fail_name: RefCell<Option<String>>,
    fail_restore: Cell<bool>,
    mutations: Cell<usize>,
    source_replacement: RefCell<Option<(PathBuf, Vec<u8>)>>,
}
impl FakeEnv {
    fn write_probe(&self, name: &std::ffi::OsStr, bytes: &[u8]) -> io::Result<()> {
        self.mutations.set(self.mutations.get() + 1);
        if self.fail_restore.get() && name == "tidemark.toml" && bytes == b"original" {
            return Err(io::Error::other("injected compensation failure"));
        }
        if self
            .fail_name
            .borrow()
            .as_deref()
            .is_some_and(|n| name == n)
        {
            self.fail_name.borrow_mut().take();
            return Err(io::Error::other("injected file failure"));
        }
        Ok(())
    }
    fn probe(&self, name: &str) -> io::Result<()> {
        if self.failures.contains(&name) {
            Err(io::Error::other(format!("injected {name}")))
        } else {
            Ok(())
        }
    }
}
impl Env for FakeEnv {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let bytes = read_bytes(path)?;
        if self
            .source_replacement
            .borrow()
            .as_ref()
            .is_some_and(|(source, _)| source == path)
        {
            if let Some((_, replacement)) = self.source_replacement.borrow_mut().take() {
                fs::write(path, replacement)?;
            }
        }
        Ok(bytes)
    }
    fn config_path(&self) -> &Path {
        &self.installed
    }
    fn now(&self) -> i64 {
        self.now
    }
    fn installation(&self, cfg: &Config) -> io::Result<()> {
        self.probe("installation")?;
        real::installation(
            cfg,
            &self.installed,
            &self
                .required
                .iter()
                .map(PathBuf::as_path)
                .collect::<Vec<_>>(),
        )
    }
    fn local_directory(&self, path: &Path) -> io::Result<()> {
        self.probe("roots")?;
        real::local_filesystem(
            if self
                .filesystem_path
                .as_deref()
                .is_none_or(|p| real::existing_ancestor(p).is_ok_and(|ancestor| ancestor == path))
            {
                self.filesystem
            } else {
                "xfs"
            },
        )
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
    fn state_write(
        &self,
        parent: &std::os::fd::OwnedFd,
        name: &std::ffi::OsStr,
        bytes: &[u8],
    ) -> io::Result<()> {
        self.write_probe(name, bytes)?;
        if name == "staged.json" && self.fail_record.replace(false) {
            return Err(io::Error::other("injected record write failure"));
        }
        files::write_at(parent, name, bytes, false)
    }
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.write_probe(path.file_name().unwrap(), bytes)?;
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
        let required: Vec<_> = ["tidemark", "tidemarkd", "tidemarkd.service", "systemctl"]
            .into_iter()
            .map(|name| dir.path().join(name))
            .collect();
        for path in &required {
            fs::write(path, b"fake installed artifact").unwrap();
        }
        let env = FakeEnv {
            installed: dir.path().join("etc/tidemark.toml"),
            required,
            filesystem: "xfs",
            filesystem_path: None,
            failures: vec![],
            now: 100,
            capacity: (4 << 30, 10 << 30),
            fail_record: Cell::new(false),
            fail_name: RefCell::new(None),
            fail_restore: Cell::new(false),
            mutations: Cell::new(0),
            source_replacement: RefCell::new(None),
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
        assert_eq!(checks.len(), 9);
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

#[test]
fn unsafe_state_values_never_mutate() {
    let f = Fixture::new();
    let roots = Config::default().roots;
    let mut values: Vec<PathBuf> = [
        "/",
        "relative",
        "/tmp/../danger",
        "/etc",
        "/usr",
        "/var",
        "/var/lib",
        "/run",
        "/home",
        "/root",
        "/boot",
        "/dev",
        "/proc",
        "/sys",
        "/tmp",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    values.extend(roots.into_iter().map(|r| r.path));
    let linked = f._dir.path().join("linked");
    symlink(f._dir.path(), &linked).unwrap();
    values.push(linked.join("state"));
    for value in values {
        fs::write(&f.source, format!("[general]\nstate_dir = {:?}\n", value)).unwrap();
        assert!(
            matches!(
                stage(&f.source, &StageOptions::default(), &f.sys, &f.env),
                Err(DeployError::Preflight(_))
            ),
            "{value:?}"
        );
        assert!(unstage(&value, false, &f.sys).is_err(), "{value:?}");
        assert!(f.sys.calls.borrow().is_empty());
        assert!(!f.env.installed.exists());
        assert_eq!(f.env.mutations.get(), 0);
        assert!(!f.state.exists());
    }
}

#[test]
fn state_symlinks_refused_before_service_for_all_operations() {
    for name in [
        "deploy",
        MARKER,
        "deploy/staged.json",
        "deploy/firstboot.json",
    ] {
        for purge in [false, true] {
            let f = Fixture::new();
            f.stage();
            f.sys.calls.borrow_mut().clear();
            let outside = f._dir.path().join("outside");
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("staged.json"), b"outside").unwrap();
            let target = f.state.join(name);
            if target.is_dir() {
                fs::remove_dir_all(&target).unwrap();
            } else if target.exists() {
                fs::remove_file(&target).unwrap();
            }
            symlink(
                if name == "deploy" {
                    outside.clone()
                } else {
                    outside.join("staged.json")
                },
                &target,
            )
            .unwrap();
            assert!(unstage(&f.state, purge, &f.sys).is_err());
            assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
            assert!(verify(&f.state, Duration::ZERO, &f.sys, &Ping(true)).is_err());
            assert!(f.sys.calls.borrow().is_empty());
            assert_eq!(fs::read(outside.join("staged.json")).unwrap(), b"outside");
        }
    }
}

#[test]
fn identical_restage_honors_start_and_repairs_enablement() {
    let f = Fixture::new();
    f.stage();
    assert!(!f.sys.active.get());
    let record = stage(
        &f.source,
        &StageOptions {
            start_now: true,
            ..Default::default()
        },
        &f.sys,
        &f.env,
    )
    .unwrap();
    assert!(record.start_now);
    assert!(f.sys.active.get());
    f.sys.enabled.set(false);
    stage(&f.source, &StageOptions::default(), &f.sys, &f.env).unwrap();
    assert!(f.sys.enabled.get());
}

#[test]
fn state_cannot_contain_tracked_root() {
    let f = Fixture::new();
    fs::write(
        &f.source,
        format!(
            "[general]\nstate_dir = {:?}\n[[roots]]\npath = {:?}\n",
            f.state,
            f.state.join("tracked")
        ),
    )
    .unwrap();
    assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
    assert!(f.sys.calls.borrow().is_empty());
    assert!(!f.state.exists());
}

#[test]
fn every_publication_and_service_failure_compensates_previous_state() {
    for point in [
        "tidemark.toml.100.bak",
        "tidemark.toml",
        MARKER,
        "staged.json",
        "enable",
        "restart",
    ] {
        for (enabled, active) in [(false, false), (false, true), (true, false), (true, true)] {
            let f = Fixture::new();
            fs::create_dir_all(f.env.installed.parent().unwrap()).unwrap();
            fs::write(&f.env.installed, b"original").unwrap();
            f.sys.enabled.set(enabled);
            f.sys.active.set(active);
            match point {
                "enable" => f.sys.fail_enable.set(true),
                "restart" => f.sys.fail_restart.set(true),
                name => *f.env.fail_name.borrow_mut() = Some(name.into()),
            }
            // An enable failure is consumed once so compensation itself can succeed.
            let error = stage(
                &f.source,
                &StageOptions {
                    start_now: true,
                    ..Default::default()
                },
                &f.sys,
                &f.env,
            )
            .unwrap_err();
            assert!(error
                .to_string()
                .contains("re-run deploy stage or deploy unstage"));
            assert_eq!(fs::read(&f.env.installed).unwrap(), b"original", "{point}");
            assert_eq!(f.sys.enabled.get(), enabled, "{point}");
            assert_eq!(f.sys.active.get(), active, "{point}");
            assert!(!f.state.join("deploy/staged.json").exists());
        }
    }
}
#[test]
fn compensation_errors_are_reported_with_original_failure() {
    let f = Fixture::new();
    fs::create_dir_all(f.env.installed.parent().unwrap()).unwrap();
    fs::write(&f.env.installed, b"original").unwrap();
    f.env.fail_record.set(true);
    f.env.fail_restore.set(true);
    let message = stage(&f.source, &StageOptions::default(), &f.sys, &f.env)
        .unwrap_err()
        .to_string();
    assert!(message.contains("injected record write failure"));
    assert!(message.contains("injected compensation failure"));
    assert!(message.contains("re-run deploy stage or deploy unstage"));
}
#[test]
fn existing_unmarked_directory_permissions_are_untouched() {
    let f = Fixture::new();
    fs::create_dir(&f.state).unwrap();
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
    assert!(f.sys.calls.borrow().is_empty());
    assert_eq!(
        fs::metadata(&f.state).unwrap().permissions().mode() & 0o777,
        0o750
    );
}

#[test]
fn previous_staged_and_marker_bytes_survive_publication_failure() {
    let f = Fixture::new();
    f.stage();
    let old_config = fs::read(&f.env.installed).unwrap();
    let old_record = fs::read(f.state.join("deploy/staged.json")).unwrap();
    let old_marker = fs::read(f.state.join(MARKER)).unwrap();
    let mut source = fs::read(&f.source).unwrap();
    source.extend_from_slice(b"# replacement\n");
    fs::write(&f.source, source).unwrap();
    f.env.fail_record.set(true);
    assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
    assert_eq!(fs::read(&f.env.installed).unwrap(), old_config);
    assert_eq!(
        fs::read(f.state.join("deploy/staged.json")).unwrap(),
        old_record
    );
    assert_eq!(fs::read(f.state.join(MARKER)).unwrap(), old_marker);
}
#[test]
fn service_compensation_failure_is_reported_and_verified() {
    let f = Fixture::new();
    f.sys.active.set(true);
    f.sys.fail_restart.set(true);
    f.env.fail_record.set(true);
    let message = stage(&f.source, &StageOptions::default(), &f.sys, &f.env)
        .unwrap_err()
        .to_string();
    assert!(message.contains("injected record write failure"));
    assert!(message.contains("injected restart failure"));
    assert!(message.contains("service compensation active=false, expected true"));
}

#[test]
fn unsafe_existing_directories_refuse_stage_and_unstage() {
    for mode in [0o777, 0o750] {
        for child in [false, true] {
            let f = Fixture::new();
            fs::create_dir(&f.state).unwrap();
            fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();
            fs::create_dir(f.state.join("deploy")).unwrap();
            let bad = if child {
                f.state.join("deploy")
            } else {
                f.state.clone()
            };
            fs::set_permissions(&bad, fs::Permissions::from_mode(mode)).unwrap();
            fs::write(f.state.join("deploy/staged.json"), b"original").unwrap();
            let outside = f._dir.path().join("outside");
            fs::write(&outside, b"outside").unwrap();
            assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
            assert!(unstage(&f.state, false, &f.sys).is_err());
            assert!(f.sys.calls.borrow().is_empty());
            assert_eq!(f.sys.queries.get(), 0);
            assert_eq!(
                fs::metadata(&bad).unwrap().permissions().mode() & 0o777,
                mode
            );
            assert_eq!(
                fs::read(f.state.join("deploy/staged.json")).unwrap(),
                b"original"
            );
            assert!(!f.env.installed.exists());
            assert!(!f.state.join("deploy/firstboot.json").exists());
            assert_eq!(fs::read(outside).unwrap(), b"outside");
        }
    }
}

#[test]
fn source_replacement_does_not_change_parsed_or_installed_bytes() {
    let f = Fixture::new();
    let original = fs::read(&f.source).unwrap();
    let (cfg, bytes) = source_from_read(&f.source, |path| {
        let bytes = files::read(path)?;
        fs::write(path, b"[general]\nstate_dir = '/different'\n")?;
        Ok(bytes)
    })
    .unwrap();
    assert_eq!(cfg.general.state_dir, f.state);
    assert_eq!(bytes, original);
    f.env.atomic_write(&f.env.installed, &bytes).unwrap();
    assert_eq!(fs::read(&f.env.installed).unwrap(), original);
    assert_eq!(
        Config::from_toml_bytes(&bytes).unwrap().general.state_dir,
        cfg.general.state_dir
    );
}
#[test]
fn source_fifo_and_symlink_are_refused() {
    let f = Fixture::new();
    let original = fs::read(&f.source).unwrap();
    let outside = f._dir.path().join("outside");
    fs::write(&outside, &original).unwrap();
    fs::remove_file(&f.source).unwrap();
    symlink(&outside, &f.source).unwrap();
    assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
    fs::remove_file(&f.source).unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &f.source,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
    assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
    assert!(f.sys.calls.borrow().is_empty());
    assert_eq!(fs::read(outside).unwrap(), original);
}

#[cfg(feature = "privileged")]
#[test]
fn foreign_owned_directories_refuse_stage_and_unstage() {
    assert_eq!(
        std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(),
        Ok("1"),
        "disposable VM only"
    );
    for child in [false, true] {
        let f = Fixture::new();
        fs::create_dir(&f.state).unwrap();
        fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(f.state.join("deploy")).unwrap();
        fs::set_permissions(f.state.join("deploy"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(f.state.join("deploy/staged.json"), b"original").unwrap();
        let outside = f._dir.path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        let bad = if child {
            f.state.join("deploy")
        } else {
            f.state.clone()
        };
        let fd = files::directory(&bad, false).unwrap();
        rustix::fs::fchown(&fd, Some(rustix::process::Uid::from_raw(65534)), None).unwrap();
        assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
        let error = unstage(&f.state, false, &f.sys).unwrap_err().to_string();
        assert!(error.contains("65534"));
        assert!(error.contains("0700"));
        assert!(error.contains(&bad.display().to_string()));
        assert!(f.sys.calls.borrow().is_empty());
        assert_eq!(f.sys.queries.get(), 0);
        assert_eq!(
            fs::read(f.state.join("deploy/staged.json")).unwrap(),
            b"original"
        );
        assert_eq!(fs::read(outside).unwrap(), b"outside");
        assert!(!f.state.join("deploy/firstboot.json").exists());
    }
}

#[test]
fn marker_and_purge_config_fifo_or_symlink_are_refused() {
    for fifo in [false, true] {
        let f = Fixture::new();
        f.stage();
        f.sys.calls.borrow_mut().clear();
        let outside = f._dir.path().join("outside");
        fs::write(&outside, fs::read(&f.env.installed).unwrap()).unwrap();
        fs::remove_file(&f.env.installed).unwrap();
        if fifo {
            rustix::fs::mknodat(
                rustix::fs::CWD,
                &f.env.installed,
                rustix::fs::FileType::Fifo,
                rustix::fs::Mode::RUSR,
                0,
            )
            .unwrap();
        } else {
            symlink(&outside, &f.env.installed).unwrap();
        }
        let record = fs::read(f.state.join("deploy/staged.json")).unwrap();
        assert!(unstage(&f.state, false, &f.sys).is_err());
        assert!(purge_guard(&f.state).is_err());
        assert!(f.sys.calls.borrow().is_empty());
        assert_eq!(
            fs::read(f.state.join("deploy/staged.json")).unwrap(),
            record
        );
        assert_eq!(Config::load(&outside).unwrap().general.state_dir, f.state);
    }
}

#[test]
fn unsafe_existing_directories_refuse_unstage_without_marker() {
    for mode in [0o777, 0o750] {
        for child in [false, true] {
            let f = Fixture::new();
            fs::create_dir(&f.state).unwrap();
            fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();
            fs::create_dir(f.state.join("deploy")).unwrap();
            fs::set_permissions(f.state.join("deploy"), fs::Permissions::from_mode(0o700)).unwrap();
            let bad = if child {
                f.state.join("deploy")
            } else {
                f.state.clone()
            };
            fs::set_permissions(&bad, fs::Permissions::from_mode(mode)).unwrap();
            fs::write(f.state.join("deploy/staged.json"), b"original").unwrap();
            let outside = f._dir.path().join("outside");
            fs::write(&outside, b"outside").unwrap();
            let error = unstage(&f.state, false, &f.sys).unwrap_err().to_string();
            assert!(error.contains(&bad.display().to_string()));
            assert!(error.contains(&format!("{:04o}", mode)));
            assert!(error.contains(&format!("owner {}", rustix::process::geteuid().as_raw())));
            assert!(f.sys.calls.borrow().is_empty());
            assert_eq!(f.sys.queries.get(), 0);
            assert_eq!(
                fs::metadata(&bad).unwrap().permissions().mode() & 0o777,
                mode
            );
            assert_eq!(
                fs::read(f.state.join("deploy/staged.json")).unwrap(),
                b"original"
            );
            assert_eq!(fs::read(outside).unwrap(), b"outside");
            assert!(!f.state.join("deploy/firstboot.json").exists());
        }
    }
}

#[test]
fn stage_installs_and_hashes_the_config_parsed_before_replacement() {
    let f = Fixture::new();
    let original = fs::read(&f.source).unwrap();
    let other = f._dir.path().join("other-state");
    *f.env.source_replacement.borrow_mut() = Some((
        f.source.clone(),
        format!("[general]\nstate_dir = {:?}\n", other).into_bytes(),
    ));
    let record = f.stage();
    assert_eq!(fs::read(&f.env.installed).unwrap(), original);
    assert_eq!(
        record.config_sha256,
        format!("{:x}", Sha256::digest(&original))
    );
    assert_eq!(
        Config::load(&f.env.installed).unwrap().general.state_dir,
        f.state
    );
    assert!(f.state.join("deploy/staged.json").exists());
    assert!(!other.exists());
    assert_ne!(fs::read(&f.source).unwrap(), original);
}

#[test]
fn marker_authorizes_state_normalization_only_after_deploy_is_eligible() {
    let f = Fixture::new();
    f.stage();
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o750)).unwrap();
    fs::write(
        &f.source,
        format!(
            "{}\n# changed\n",
            String::from_utf8(fs::read(&f.source).unwrap()).unwrap()
        ),
    )
    .unwrap();
    f.stage();
    assert_eq!(
        fs::metadata(&f.state).unwrap().permissions().mode() & 0o777,
        0o700
    );
    f.sys.calls.borrow_mut().clear();
    f.sys.queries.set(0);
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o750)).unwrap();
    fs::set_permissions(f.state.join("deploy"), fs::Permissions::from_mode(0o750)).unwrap();
    assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
    assert_eq!(
        fs::metadata(&f.state).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert!(f.sys.calls.borrow().is_empty());
    assert_eq!(f.sys.queries.get(), 0);
}

use super::files::FOREIGN_OWNER;
struct OwnerProbe;
impl Drop for OwnerProbe {
    fn drop(&mut self) {
        FOREIGN_OWNER.with(|owner| *owner.borrow_mut() = None);
    }
}

#[path = "scenarios.rs"]
mod scenarios;
