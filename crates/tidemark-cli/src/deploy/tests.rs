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
    activating: Cell<bool>,
    fail_enable: Cell<bool>,
    refuse_stop: Cell<bool>,
    fail_restart: Cell<bool>,
    restart_probe: RefCell<Option<Box<dyn FnOnce()>>>,
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
    fn is_activating(&self, _: &str) -> io::Result<bool> {
        Ok(self.activating.get())
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
        if let Some(probe) = self.restart_probe.borrow_mut().take() {
            probe();
        }
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
    move_config_parent: Cell<bool>,
    source_replacement: RefCell<Option<(PathBuf, Vec<u8>)>>,
    writes: std::rc::Rc<RefCell<Vec<String>>>,
}
impl FakeEnv {
    fn write_probe(&self, name: &std::ffi::OsStr, bytes: &[u8]) -> io::Result<()> {
        self.writes
            .borrow_mut()
            .push(name.to_string_lossy().into_owned());
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
    fn config_write(
        &self,
        parent: &std::os::fd::OwnedFd,
        name: &std::ffi::OsStr,
        bytes: &[u8],
    ) -> io::Result<()> {
        self.write_probe(name, bytes)?;
        if self.move_config_parent.replace(false) {
            let original = self.installed.parent().unwrap();
            let retained = original.with_file_name("retained-config");
            let target = original.with_file_name("config-target");
            fs::rename(original, retained)?;
            fs::create_dir(&target)?;
            symlink(&target, original)?;
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
            move_config_parent: Cell::new(false),
            source_replacement: RefCell::new(None),
            writes: Default::default(),
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
    fixture.sys.activating.set(true);
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
    assert!(fixture.sys.enabled.get());
    assert!(fixture.sys.active.get());
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
            assert!(error.contains(&format!("{:o}", mode)));
            assert!(
                error.contains("owner")
                    && error.contains(&rustix::process::geteuid().as_raw().to_string())
            );
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

#[test]
fn marker_never_waives_owner_or_write_permissions() {
    for foreign in [false, true] {
        let f = Fixture::new();
        f.stage();
        f.sys.calls.borrow_mut().clear();
        f.sys.queries.set(0);
        if foreign {
            FOREIGN_OWNER.with(|o| *o.borrow_mut() = Some(f.state.clone()));
        } else {
            fs::set_permissions(&f.state, fs::Permissions::from_mode(0o770)).unwrap();
        }
        let _probe = OwnerProbe;
        let before = fs::read(f.state.join(MARKER)).unwrap();
        assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
        assert!(unstage(&f.state, true, &f.sys).is_err());
        assert_eq!(fs::read(f.state.join(MARKER)).unwrap(), before);
        assert!(f.sys.calls.borrow().is_empty());
        assert_eq!(f.sys.queries.get(), 0);
        if !foreign {
            assert_eq!(
                fs::metadata(&f.state).unwrap().permissions().mode() & 0o777,
                0o770
            );
        }
    }
}

#[test]
fn unsafe_marker_and_state_content_are_refused_without_changes() {
    for name in [
        MARKER,
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-journal",
        "scan.lock",
        "deploy/staged.json",
        "deploy/firstboot.json",
    ] {
        for kind in ["link", "hardlink", "writable", "foreign", "fifo"] {
            let f = Fixture::new();
            f.stage();
            f.sys.calls.borrow_mut().clear();
            f.sys.queries.set(0);
            let target = f._dir.path().join("sentinel");
            fs::write(&target, b"sentinel").unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            let leaf = f.state.join(name);
            let _ = fs::remove_file(&leaf);
            match kind {
                "link" => std::os::unix::fs::symlink(&target, &leaf).unwrap(),
                "hardlink" => fs::hard_link(&target, &leaf).unwrap(),
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
                    fs::set_permissions(
                        &leaf,
                        fs::Permissions::from_mode(if kind == "writable" { 0o620 } else { 0o600 }),
                    )
                    .unwrap();
                }
            }
            let _probe = OwnerProbe;
            if kind == "foreign" {
                FOREIGN_OWNER.with(|o| *o.borrow_mut() = Some(leaf.clone()));
            }
            let error =
                match stage(&f.source, &StageOptions::default(), &f.sys, &f.env).unwrap_err() {
                    DeployError::Preflight(checks) => checks
                        .iter()
                        .map(|c| c.detail.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                    e => e.to_string(),
                };
            assert!(error.contains(name), "{kind} {name}: {error}");
            assert!(
                error.contains("owner") && error.contains("mode") && error.contains("links"),
                "{error}"
            );
            assert_eq!(fs::read(&target).unwrap(), b"sentinel");
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert!(f.sys.calls.borrow().is_empty());
            assert_eq!(f.sys.queries.get(), 0);
        }
    }
}

#[test]
fn stage_refuses_writable_ancestor() {
    let f = Fixture::new();
    fs::set_permissions(f._dir.path(), fs::Permissions::from_mode(0o770)).unwrap();
    assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
    assert!(!f.state.exists());
    assert!(!f.env.installed.exists());
    assert!(f.sys.calls.borrow().is_empty());
}

#[test]
fn config_publication_retains_verified_parent_after_path_replacement() {
    let f = Fixture::new();
    f.env.move_config_parent.set(true);
    f.stage();
    let retained = f._dir.path().join("retained-config/tidemark.toml");
    assert_eq!(fs::read(retained).unwrap(), fs::read(&f.source).unwrap());
    assert_eq!(
        fs::read_dir(f._dir.path().join("config-target"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(feature = "privileged")]
#[test]
fn real_foreign_owner_marker_cannot_authorize_root_stage_or_purge() {
    assert_eq!(
        std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(),
        Ok("1"),
        "disposable VM only"
    );
    assert!(rustix::process::geteuid().is_root());
    let f = Fixture::new();
    f.stage();
    f.sys.calls.borrow_mut().clear();
    let state = files::directory(&f.state, false).unwrap();
    let marker = rustix::fs::openat(
        &state,
        MARKER,
        rustix::fs::OFlags::RDONLY,
        rustix::fs::Mode::empty(),
    )
    .unwrap();
    let foreign = rustix::process::Uid::from_raw(65534);
    rustix::fs::fchown(&marker, Some(foreign), None).unwrap();
    rustix::fs::fchown(&state, Some(foreign), None).unwrap();
    let before = fs::read(f.state.join(MARKER)).unwrap();
    assert!(stage(&f.source, &StageOptions::default(), &f.sys, &f.env).is_err());
    assert!(unstage(&f.state, true, &f.sys).is_err());
    assert_eq!(fs::read(f.state.join(MARKER)).unwrap(), before);
    assert!(f.sys.calls.borrow().is_empty());
}

#[test]
fn eligibility_error_escapes_non_utf8_and_control_path_bytes() {
    use std::os::unix::ffi::OsStrExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp
        .path()
        .join(std::ffi::OsStr::from_bytes(b"state-\xff\n"));
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();
    let error = files::state_handles(&path, false).unwrap_err().to_string();
    assert!(error.contains(&tidemark_core::path::display_lossy_bytes(
        path.as_os_str().as_bytes()
    )));
    assert!(!error.contains('\n'));
    assert!(error.contains("owner") && error.contains("mode") && error.contains("links"));
}

#[test]
fn purge_checks_unknown_and_nested_directories_before_stop() {
    for (relative, mode, foreign) in [
        ("unknown", 0o770, false),
        ("unknown/nested", 0o707, false),
        ("unknown/nested", 0o700, true),
    ] {
        let f = Fixture::new();
        f.stage();
        f.sys.calls.borrow_mut().clear();
        let child = f.state.join(relative);
        fs::create_dir_all(&child).unwrap();
        fs::set_permissions(f.state.join("unknown"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&child, fs::Permissions::from_mode(mode)).unwrap();
        fs::write(child.join("sentinel"), b"unchanged").unwrap();
        let before = tree_bytes(&f.state);
        let _probe = OwnerProbe;
        if foreign {
            FOREIGN_OWNER.with(|o| *o.borrow_mut() = Some(child.clone()));
        }
        let error = unstage(&f.state, true, &f.sys).unwrap_err();
        assert!(error.to_string().contains(relative), "{error}");
        assert!(f.sys.calls.borrow().is_empty());
        assert_eq!(tree_bytes(&f.state), before);
    }
}
fn tree_bytes(path: &Path) -> Vec<(std::path::PathBuf, u32, Vec<u8>)> {
    let mut result = Vec::new();
    let mut pending = vec![path.to_owned()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        let bytes = if metadata.is_file() {
            fs::read(&path).unwrap()
        } else {
            Vec::new()
        };
        result.push((path.clone(), metadata.permissions().mode(), bytes));
        if metadata.is_dir() {
            for e in fs::read_dir(path).unwrap() {
                pending.push(e.unwrap().path());
            }
        }
    }
    result.sort();
    result
}
#[test]
fn purge_after_open_rename_preserves_moved_contents() {
    let f = Fixture::new();
    f.stage();
    f.sys.calls.borrow_mut().clear();
    let child = f.state.join("unknown");
    fs::create_dir(&child).unwrap();
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(child.join("sentinel"), b"outside sentinel").unwrap();
    let outside = f._dir.path().join("moved");
    let moved = outside.clone();
    super::purge::AFTER_OPEN.with(|h| {
        *h.borrow_mut() = Some((
            child.clone(),
            Box::new(move |opened| {
                fs::rename(opened, moved).unwrap();
            }),
        ))
    });
    assert!(unstage(&f.state, true, &f.sys).is_err());
    assert!(f.sys.calls.borrow().is_empty());
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"outside sentinel"
    );
    assert!(f.state.join(MARKER).exists());
}

struct PurgeChild(std::process::Child);
impl Drop for PurgeChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn purge_writer_child() {
    let Some(state) = std::env::var_os("TIDEMARK_PURGE_WRITER") else {
        return;
    };
    let state = PathBuf::from(state);
    let _catalog = tidemark_store::catalog::Catalog::open(&state, true).unwrap();
    let _series =
        tidemark_store::timeseries::TimeSeries::open(&state.join("timeseries.db"), true).unwrap();
    fs::write(state.join("ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(60));
}
#[test]
fn purge_refuses_live_writer_without_changing_bytes_or_inodes() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new();
    f.stage();
    let mut child = PurgeChild(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "deploy::tests::purge_writer_child"])
            .env("TIDEMARK_PURGE_WRITER", &f.state)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f.state.join("ready").exists() {
        assert!(Instant::now() < deadline, "writer never became ready");
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut paths = vec![f.state.clone()];
    let mut originals = Vec::new();
    while let Some(path) = paths.pop() {
        let meta = fs::symlink_metadata(&path).unwrap();
        if meta.is_dir() {
            paths.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
        }
        let bytes = meta.is_file().then(|| fs::read(&path).unwrap());
        originals.push((path, meta.ino(), bytes));
    }
    let error = unstage(&f.state, true, &f.sys).unwrap_err();
    assert!(matches!(error, DeployError::Guard(_)));
    assert!(error
        .to_string()
        .contains("state directory in use by another tidemark process; nothing was removed"));
    for (path, inode, bytes) in originals {
        assert_eq!(fs::symlink_metadata(&path).unwrap().ino(), inode);
        if let Some(bytes) = bytes {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
    child.0.kill().unwrap();
    child.0.wait().unwrap();
}
#[test]
fn idle_purge_removes_data_locks_and_directory() {
    let f = Fixture::new();
    f.stage();
    drop(tidemark_store::catalog::Catalog::open(&f.state, true).unwrap());
    drop(tidemark_scan::orchestrator::ScanLock::acquire(&f.state).unwrap());
    assert!(f.state.join("writer.lock").exists());
    assert!(f.state.join("scan.lock").exists());
    f.sys.active.set(true);
    f.sys.calls.borrow_mut().clear();
    unstage(&f.state, true, &f.sys).unwrap();
    assert!(!f.state.exists());
    assert!(!f.sys.enabled.get());
    assert!(!f.sys.active.get());
    assert_eq!(*f.sys.calls.borrow(), ["disable_now"]);
}
#[test]
fn purge_finalization_never_traverses_new_entries_or_replacement() {
    for replace in [false, true] {
        let f = Fixture::new();
        f.stage();
        let state = f.state.clone();
        let moved = f._dir.path().join("moved");
        super::purge::BEFORE_RMDIR.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                assert!(!state.join("scan.lock").exists());
                assert!(!state.join("writer.lock").exists());
                // A separate process recreates the namespace after the final unlink.
                let script = if replace {
                    "mv -- \"$1\" \"$2\" && mkdir -m 700 -- \"$1\" && echo new > \"$1/keep\""
                } else {
                    "echo new > \"$1/keep\""
                };
                let mut child = PurgeChild(
                    std::process::Command::new("bash")
                        .args(["-c", script, "purge-racer"])
                        .arg(&state)
                        .arg(&moved)
                        .spawn()
                        .unwrap(),
                );
                assert!(child.0.wait().unwrap().success());
            }))
        });
        assert!(unstage(&f.state, true, &f.sys).is_err());
        assert_eq!(fs::read(f.state.join("keep")).unwrap(), b"new\n");
    }
}

#[test]
fn purge_waiter_child() {
    let Some(state) = std::env::var_os("TIDEMARK_PURGE_WAITER") else {
        return;
    };
    let signal = PathBuf::from(std::env::var_os("TIDEMARK_PURGE_SIGNAL").unwrap());
    let state = PathBuf::from(state);
    let directory = tidemark_store::state_dir::StateDir::open(&state, false).unwrap();
    let file = directory
        .open_leaf(std::ffi::OsStr::new("writer.lock"), true, false)
        .unwrap();
    fs::write(signal.with_extension("ready"), b"ready").unwrap();
    rustix::fs::flock(&file, rustix::fs::FlockOperation::LockShared).unwrap();
    let error = tidemark_store::lease::verify_lock_identity(
        &directory,
        &state,
        &file,
        std::ffi::OsStr::new("writer.lock"),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    fs::write(&signal, b"busy").unwrap();
}
#[test]
fn purge_waiter_refuses_deleted_inode_after_complete_purge() {
    let f = Fixture::new();
    f.stage();
    let signal = f._dir.path().join("waiter");
    let state = f.state.clone();
    let child_slot = std::rc::Rc::new(RefCell::new(None));
    let slot = child_slot.clone();
    let child_signal = signal.clone();
    super::purge::BEFORE_LOCK_UNLINK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            *slot.borrow_mut() = Some(PurgeChild(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "deploy::tests::purge_waiter_child"])
                    .env("TIDEMARK_PURGE_WAITER", &state)
                    .env("TIDEMARK_PURGE_SIGNAL", &child_signal)
                    .spawn()
                    .unwrap(),
            ));
            let deadline = Instant::now() + Duration::from_secs(10);
            while !child_signal.with_extension("ready").exists() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
        }))
    });
    unstage(&f.state, true, &f.sys).unwrap();
    assert!(!f.state.exists());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !signal.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read(signal).unwrap(), b"busy");
    assert!(child_slot
        .borrow_mut()
        .as_mut()
        .unwrap()
        .0
        .wait()
        .unwrap()
        .success());
}

#[test]
fn purge_refuses_busy_scan_without_removing_state() {
    let f = Fixture::new();
    f.stage();
    drop(tidemark_store::catalog::Catalog::open(&f.state, true).unwrap());
    let _scan = tidemark_scan::orchestrator::ScanLock::acquire(&f.state).unwrap();
    let keep = f.state.join("keep");
    fs::write(&keep, b"data").unwrap();
    let error = unstage(&f.state, true, &f.sys).unwrap_err();
    assert!(error
        .to_string()
        .contains("state directory in use by another tidemark process; nothing was removed"));
    assert_eq!(fs::read(keep).unwrap(), b"data");
    assert!(f.state.join(MARKER).exists());
    assert!(f.state.join("writer.lock").exists());
    assert!(f.state.join("scan.lock").exists());
}

#[test]
fn start_now_publishes_new_records_before_restart_and_restores_on_failure() {
    for (previous, fail) in [(false, false), (true, false), (false, true), (true, true)] {
        let f = Fixture::new();
        if previous {
            f.stage();
            fs::write(
                &f.source,
                format!(
                    "[general]\nstate_dir = {:?}\n[retention]\nmin_free_pct = 11.0\n",
                    f.state
                ),
            )
            .unwrap();
        }
        let old_config = fs::read(&f.env.installed).ok();
        let old_marker = fs::read(f.state.join(MARKER)).ok();
        let old_record = fs::read(f.state.join("deploy/staged.json")).ok();
        let state = f.state.clone();
        let installed = f.env.installed.clone();
        let new_bytes = fs::read(&f.source).unwrap();
        f.env.writes.borrow_mut().clear();
        let writes = f.env.writes.clone();
        *f.sys.restart_probe.borrow_mut() = Some(Box::new(move || {
            writes.borrow_mut().push("restart".into());
            let order = writes.borrow();
            assert_eq!(
                &order[order.len() - 3..],
                &[MARKER, "staged.json", "restart"]
            );
            let marker: Marker =
                serde_json::from_slice(&fs::read(state.join(MARKER)).unwrap()).unwrap();
            assert_eq!(marker.state_dir.as_path(), state);
            assert_eq!(marker.config_path.as_path(), installed);
            let record: StagedRecord =
                serde_json::from_slice(&fs::read(state.join("deploy/staged.json")).unwrap())
                    .unwrap();
            assert!(record.start_now);
            assert_eq!(
                record.config_sha256,
                format!("{:x}", Sha256::digest(&new_bytes))
            );
        }));
        f.sys.fail_restart.set(fail);
        let result = stage(
            &f.source,
            &StageOptions {
                start_now: true,
                ..Default::default()
            },
            &f.sys,
            &f.env,
        );
        if !fail {
            assert!(result.is_ok());
            assert!(f.sys.active.get());
            continue;
        }
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("injected restart failure"));
        assert_eq!(fs::read(&f.env.installed).ok(), old_config);
        assert_eq!(fs::read(f.state.join(MARKER)).ok(), old_marker);
        assert_eq!(
            fs::read(f.state.join("deploy/staged.json")).ok(),
            old_record
        );
    }
}

#[test]
fn refused_purge_restores_each_previous_service_state_and_reports_failures() {
    for (enabled, active, fail) in [
        (true, true, false),
        (false, true, false),
        (true, false, false),
        (false, false, false),
        (true, true, true),
    ] {
        let f = Fixture::new();
        f.stage();
        f.sys.enabled.set(enabled);
        f.sys.active.set(active);
        f.sys.fail_enable.set(fail);
        f.sys.calls.borrow_mut().clear();
        let _lease = tidemark_store::lease::WriterLease::acquire(&f.state).unwrap();
        let marker = fs::read(f.state.join(MARKER)).unwrap();
        let record = fs::read(f.state.join("deploy/staged.json")).unwrap();
        let error = unstage(&f.state, true, &f.sys).unwrap_err();
        assert!(matches!(error, DeployError::Guard(_)));
        let message = error.to_string();
        assert!(message.contains("nothing was removed"));
        let mut expected = vec!["disable_now"];
        if enabled {
            expected.push("enable");
        }
        if active {
            expected.push("restart");
        }
        assert_eq!(*f.sys.calls.borrow(), expected);
        if fail {
            assert!(
                f.sys.active.get(),
                "restart must still be attempted after failed enable"
            );
            assert!(message.contains("compensation failures"));
            assert!(message.contains("injected enable failure"));
            assert!(message.contains("enabled=false, expected true"));
        } else {
            assert!(message.contains("as before"));
            assert_eq!(f.sys.enabled.get(), enabled);
            assert_eq!(f.sys.active.get(), active);
        }
        assert_eq!(fs::read(f.state.join(MARKER)).unwrap(), marker);
        assert_eq!(
            fs::read(f.state.join("deploy/staged.json")).unwrap(),
            record
        );
    }
}

#[test]
fn staged_inactive_verify_returns_with_start_hint_without_waiting() {
    let fixture = Fixture::new();
    fixture.stage();
    let start = Instant::now();
    let result = verify(
        &fixture.state,
        Duration::from_secs(300),
        &fixture.sys,
        &Ping(true),
    )
    .unwrap();
    assert!(start.elapsed() < Duration::from_secs(10));
    assert!(!result.healthy);
    assert_eq!(result.items[0].status, CheckStatus::Fail);
    assert!(result
        .items
        .iter()
        .any(|item| item.detail == SERVICE_START_HINT));
}

#[test]
fn purge_absent_state_still_stops_service_and_preserves_stop_refusal() {
    for refuse in [false, true] {
        let f = Fixture::new();
        assert!(!f.state.exists());
        f.sys.active.set(true);
        f.sys.enabled.set(true);
        f.sys.refuse_stop.set(refuse);
        let result = unstage(&f.state, true, &f.sys);
        assert_eq!(result.is_ok(), !refuse);
        assert!(!f.state.exists());
        assert!(f
            .sys
            .calls
            .borrow()
            .iter()
            .any(|call| call == "disable_now"));
        if refuse {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("service is still active"));
        } else {
            assert!(!f.sys.active.get());
            assert!(!f.sys.enabled.get());
        }
    }
}
