//! Owned by: W6-A1
//! Daemon startup, signals, reload and bounded shutdown.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// api module skeleton.
pub mod api;
/// firstboot module skeleton.
pub mod firstboot;
/// metrics module skeleton.
pub mod metrics;
/// Configured mount presence and identity.
pub mod mounts;
/// scheduler module skeleton.
pub mod scheduler;
/// Daemon service modules.
pub mod services {
    /// alerts service skeleton.
    pub mod alerts;
    /// lvm service skeleton.
    pub mod lvm;
    /// procscan service skeleton.
    pub mod procscan;
    /// sampler service skeleton.
    pub mod sampler;
    /// tswriter service skeleton.
    pub mod tswriter;
    /// watcher service skeleton.
    pub mod watcher;
}
/// state module skeleton.
pub mod state;

use clap::Parser;
use std::io::Write;
use std::time::{Duration, Instant};

/// Daemon invocation options.
#[derive(Debug, Parser)]
#[command(name = "tidemarkd", version, long_version = concat!(env!("CARGO_PKG_VERSION"), "\ncommit: ", env!("TIDEMARK_BUILD_COMMIT"), " (", env!("TIDEMARK_BUILD_DATE"), ")\ntarget: ", env!("TIDEMARK_BUILD_TARGET"), ", profile ", env!("TIDEMARK_BUILD_PROFILE")), about = "Tidemark monitoring daemon")]
pub struct Args {
    /// Configuration file.
    #[arg(
        long,
        value_name = "FILE",
        default_value = "/etc/tidemark/tidemark.toml"
    )]
    pub config: std::path::PathBuf,
    /// Validate configuration and exit.
    #[arg(long)]
    pub check_config: bool,
}
/// Validate and report every issue; ordinary startup remains owned by W6-A.
pub fn run_args(args: &Args, output: &mut impl Write) -> u8 {
    if !args.check_config {
        let result = load_config(&args.config)
            .map(|(config, _)| config)
            .map_err(|error| error.to_string())
            .and_then(|config| {
                run(
                    config,
                    RunOptions {
                        config_path: args.config.clone(),
                        now: unix_now(),
                    },
                )
                .map_err(|error| error.to_string())
            });
        if let Err(error) = result {
            let _ = writeln!(output, "{error}");
            return 1;
        }
        return 0;
    }
    let config = match load_config(&args.config) {
        Ok((config, _)) => config,
        Err(error) => {
            let _ = writeln!(output, "{error}");
            if let DaemonError::Io { source: error, .. } = &error {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    let _ = writeln!(
                        output,
                        "Tidemark's files are root-only; run this command with sudo."
                    );
                }
            }
            return 2;
        }
    };
    match validate_config(&config) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(output, "{error}");
            2
        }
    }
}
/// Thin process entry point.
pub fn main_entry() -> std::process::ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let code = error.exit_code() as u8;
            let _ = error.print();
            return code.into();
        }
    };
    if !args.check_config {
        use tracing_subscriber::prelude::*;
        let configured_level = load_config(&args.config)
            .map(|(c, _)| c.general.log_level)
            .unwrap_or_else(|_| "info".into());
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(configured_level));
        if std::env::var_os("INVOCATION_ID").is_some() {
            match tracing_journald::layer() {
                Ok(layer) => {
                    let _ = tracing_subscriber::registry()
                        .with(filter)
                        .with(layer)
                        .try_init();
                }
                Err(_) => {
                    let _ = tracing_subscriber::registry()
                        .with(filter)
                        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                        .try_init();
                }
            }
        } else {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                .try_init();
        }
    }
    run_args(&args, &mut std::io::stderr()).into()
}

#[cfg(test)]
mod log_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preflight_refuses_foreign_database_leaves_before_mutation() {
        use std::os::unix::fs::PermissionsExt;
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                state_files::FOREIGN_OWNER.with(|o| *o.borrow_mut() = None);
            }
        }
        for name in [
            "timeseries.db",
            "timeseries.db-wal",
            "timeseries.db-journal",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("state");
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            let leaf = path.join(name);
            std::fs::write(&leaf, b"sentinel").unwrap();
            let mut config = tidemark_core::config::Config::default();
            config.general.state_dir = path.clone();
            let _reset = Reset;
            state_files::FOREIGN_OWNER.with(|o| *o.borrow_mut() = Some(leaf.clone()));
            let error = prepare_state(&config).unwrap_err().to_string();
            assert!(
                error.contains(name) && error.contains("owner") && error.contains("links"),
                "{error}"
            );
            assert_eq!(std::fs::read(leaf).unwrap(), b"sentinel");
            assert!(!path.join("deploy").exists());
            assert!(!path.join("scan.lock").exists());
        }
    }

    #[cfg(feature = "privileged")]
    #[test]
    fn real_foreign_database_owner_refused_before_daemon_mutation() {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(),
            Ok("1"),
            "disposable VM only"
        );
        assert!(rustix::process::geteuid().is_root());
        for name in [
            "timeseries.db",
            "timeseries.db-wal",
            "timeseries.db-journal",
        ] {
            let temp = tempfile::tempdir().unwrap();
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let path = temp.path().join(name);
            std::fs::write(&path, b"sentinel").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            rustix::fs::chown(&path, Some(rustix::process::Uid::from_raw(65534)), None).unwrap();
            let mut config = tidemark_core::config::Config::default();
            config.general.state_dir = temp.path().to_owned();
            let error = prepare_state(&config).unwrap_err().to_string();
            assert!(
                error.contains(name) && error.contains("65534") && error.contains("links"),
                "{error}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"sentinel");
            assert!(!temp.path().join("deploy").exists());
        }
    }

    #[test]
    fn typed_memory_pressure_is_degraded() {
        assert!(resource_pressure(&std::io::Error::new(
            std::io::ErrorKind::OutOfMemory,
            "budget"
        )));
        assert!(!resource_pressure(&std::io::Error::other(
            "ordinary failure"
        )));
    }
    #[test]
    fn recovery_wait_preserves_busy_database_and_sidecars() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::recovery_scan_lock_child"])
            .env("TIDEMARK_RECOVERY_LOCK_CHILD", dir.path())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.path().join("ready").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut originals = Vec::new();
        for name in ["timeseries.db", "timeseries.db-wal", "timeseries.db-shm"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"corrupt fixture").unwrap();
            originals.push((path.clone(), std::fs::metadata(&path).unwrap().ino()));
        }
        let start = Instant::now();
        let error = startup_catalog(dir.path(), 100, Duration::from_millis(60))
            .err()
            .unwrap();
        assert!(error
            .to_string()
            .contains("state directory in use by another tidemark process"));
        assert!(start.elapsed() < Duration::from_secs(1));
        for (path, inode) in originals {
            assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
            assert_eq!(std::fs::read(path).unwrap(), b"corrupt fixture");
        }
        assert!(!dir.path().join("timeseries-recovery-0").exists());
        child.kill().unwrap();
        child.wait().unwrap();
        let catalog = startup_catalog(dir.path(), 100, Duration::ZERO).unwrap();
        assert_eq!(catalog.recovery_files().len(), 3);
    }
    #[test]
    fn startup_refuses_pre_api_writer() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::pre_api_writer_child"])
            .env("TIDEMARK_PRE_API_WRITER", dir.path())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.path().join("ready").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut originals = Vec::new();
        for name in ["timeseries.db", "timeseries.db-wal", "timeseries.db-shm"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"corrupt fixture").unwrap();
            originals.push((path.clone(), std::fs::metadata(path).unwrap().ino()));
        }
        let start = Instant::now();
        let error = startup_catalog(dir.path(), 100, Duration::from_millis(60))
            .err()
            .unwrap();
        assert!(error
            .to_string()
            .contains("state directory in use by another tidemark process"));
        assert!(start.elapsed() < Duration::from_secs(1));
        for (path, inode) in originals {
            assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
            assert_eq!(std::fs::read(path).unwrap(), b"corrupt fixture");
        }
        assert!(!dir.path().join("timeseries-recovery-0").exists());
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(
            startup_catalog(dir.path(), 100, Duration::ZERO)
                .unwrap()
                .recovery_files()
                .len(),
            3
        );
    }
    #[test]
    fn racing_corrupt_starts_recover_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("timeseries.db"), b"corruption").unwrap();
        let mut children = Vec::new();
        for id in ["one", "two"] {
            children.push(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "tests::racing_start_child"])
                    .env("TIDEMARK_RACING_START", dir.path())
                    .env("TIDEMARK_RACING_ID", id)
                    .spawn()
                    .unwrap(),
            );
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.path().join("one").exists() || !dir.path().join("two").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::write(dir.path().join("go"), b"go").unwrap();
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        assert!(dir.path().join("timeseries-recovery-0").exists());
        assert!(!dir.path().join("timeseries-recovery-1").exists());
        assert!(tidemark_store::catalog::Catalog::open(dir.path(), false).is_ok());
    }
    #[test]
    fn racing_start_child() {
        let Some(path) = std::env::var_os("TIDEMARK_RACING_START") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        std::fs::write(
            path.join(std::env::var_os("TIDEMARK_RACING_ID").unwrap()),
            b"ready",
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.join("go").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let catalog = startup_catalog(&path, 100, Duration::from_secs(2)).unwrap();
        assert!(catalog.list(None).unwrap().is_empty());
    }
    #[test]
    fn pre_api_writer_child() {
        let Some(path) = std::env::var_os("TIDEMARK_PRE_API_WRITER") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        let _catalog = tidemark_store::catalog::Catalog::open(&path, true).unwrap();
        let _series =
            tidemark_store::timeseries::TimeSeries::open(&path.join("timeseries.db"), true)
                .unwrap();
        std::fs::write(path.join("ready"), b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }
    #[test]
    fn recovery_scan_lock_child() {
        let Some(path) = std::env::var_os("TIDEMARK_RECOVERY_LOCK_CHILD") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        let _lock = tidemark_scan::orchestrator::ScanLock::acquire(&path).unwrap();
        std::fs::write(path.join("ready"), b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }
    #[test]
    fn healthy_catalog_does_not_wait_for_scan_lock() {
        let dir = tempfile::tempdir().unwrap();
        drop(tidemark_store::catalog::Catalog::open(dir.path(), true).unwrap());
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::pre_api_writer_child"])
            .env("TIDEMARK_PRE_API_WRITER", dir.path())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.path().join("ready").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let _lock = tidemark_scan::orchestrator::ScanLock::acquire(dir.path()).unwrap();
        let start = Instant::now();
        drop(startup_catalog(dir.path(), 100, Duration::from_secs(30)).unwrap());
        assert!(start.elapsed() < Duration::from_secs(1));
        child.kill().unwrap();
        child.wait().unwrap();
    }
    #[test]
    fn startup_pressure_types_retry_at_lock_and_store_seams() {
        use tidemark_scan::orchestrator::ScanError;
        for code in [24, 12] {
            for seam in 0..4 {
                let state = state::LiveState::new(&tidemark_core::config::Config::default(), 0);
                let shutdown = Shutdown::default();
                let mut attempts = 0;
                let result = retry_start("catalog", &state, &shutdown, || {
                    attempts += 1;
                    if attempts == 1 {
                        let io = std::io::Error::from_raw_os_error(code);
                        let error = match seam {
                            0 => startup_scan_error(ScanError::Io(io)),
                            1 => startup_scan_error(ScanError::Store(
                                tidemark_store::StoreError::Io(io),
                            )),
                            2 => startup_lease_error(io),
                            _ => startup_scan_error(ScanError::Sys(
                                rustix::io::Errno::from_raw_os_error(code),
                            )),
                        };
                        assert!(daemon_pressure(&error));
                        Err(error)
                    } else {
                        Ok(42)
                    }
                })
                .unwrap();
                assert_eq!(result, 42);
                assert_eq!(attempts, 2);
            }
        }
        let state = state::LiveState::new(&tidemark_core::config::Config::default(), 0);
        let mut attempts = 0;
        let result = retry_start::<()>("catalog", &state, &Shutdown::default(), || {
            attempts += 1;
            Err(startup_scan_error(ScanError::Busy))
        });
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("state directory in use"));
        assert_eq!(attempts, 1);
    }
    #[test]
    fn requested_shutdown_preserves_first_startup_attempt() {
        let state = state::LiveState::new(&tidemark_core::config::Config::default(), 0);
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let mut calls = 0;
        assert_eq!(
            retry_start("catalog", &state, &shutdown, || {
                calls += 1;
                Ok(42)
            })
            .unwrap(),
            42
        );
        assert_eq!(calls, 1);
        assert!(matches!(
            retry_start::<()>("catalog", &state, &shutdown, || Err(DaemonError::Io {
                context: "open".into(),
                source: std::io::Error::from_raw_os_error(24)
            })),
            Err(DaemonError::Shutdown)
        ));
    }
    #[test]
    fn resource_exit_does_not_fail_lifecycle_join() {
        let handle = std::thread::spawn(|| {
            Err(DaemonError::Io {
                context: "final pass".into(),
                source: std::io::Error::from_raw_os_error(24),
            })
        });
        assert!(join_until(
            "sampler",
            handle,
            std::time::Instant::now() + std::time::Duration::from_secs(5)
        )
        .is_ok());
    }
    #[test]
    fn sqlite_extended_pressure_codes_exclude_corruption() {
        for code in [7, 10, 14, 10 | (3 << 8), 14 | (2 << 8)] {
            assert!(sqlite_pressure_code(code));
        }
        for code in [0, 11, 26, 19, 8] {
            assert!(!sqlite_pressure_code(code));
        }
    }
    #[test]
    fn service_start_resource_pressure_retries_without_shutdown() {
        let state = state::LiveState::new(&tidemark_core::config::Config::default(), 0);
        let shutdown = Shutdown::default();
        let mut calls = 0;
        let result = retry_start("sampler", &state, &shutdown, || {
            calls += 1;
            if calls <= 2 {
                Err(DaemonError::Io {
                    context: "spawn".into(),
                    source: std::io::Error::from_raw_os_error(12),
                })
            } else {
                Ok(42)
            }
        })
        .unwrap();
        assert_eq!((calls, result), (3, 42));
        assert!(!shutdown.load(std::sync::atomic::Ordering::Acquire));
        assert!(state
            .view()
            .status(0)
            .unwrap()
            .components
            .iter()
            .any(|c| c.name == "sampler" && c.state == "degraded"));
    }
    #[test]
    fn per_mount_descriptor_budget_reserves_headroom() {
        assert_eq!(fd_budget(Some(1024), 2), 256);
        assert_eq!(fd_budget(Some(1024), 4), 128);
        assert_eq!(fd_budget(Some(640), 4), 32);
        assert_eq!(fd_budget(Some(64), 1), 16);
        assert_eq!(fd_budget(None, 100), 256);
        assert_eq!(fd_budget(Some(64), 0), 256);
    }
    #[test]
    fn descriptor_limit_raise_equal_and_refused() {
        use rustix::process::Rlimit;
        use std::cell::Cell;
        let limit = Cell::new(Rlimit {
            current: Some(64),
            maximum: Some(1024),
        });
        let calls = Cell::new(0);
        let raised = raise_fd_limit(
            || limit.get(),
            |next| {
                calls.set(calls.get() + 1);
                limit.set(next);
                Ok(())
            },
        );
        assert_eq!(raised.current, Some(1024));
        assert_eq!(calls.get(), 1);
        raise_fd_limit(|| limit.get(), |_| panic!("equal limits must not be set"));
        limit.set(Rlimit {
            current: Some(64),
            maximum: Some(1024),
        });
        let refused = raise_fd_limit(|| limit.get(), |_| Err(rustix::io::Errno::PERM));
        assert_eq!(refused.current, Some(64));
    }

    #[test]
    fn operational_intervals_have_inclusive_366_day_cap() {
        let max = std::time::Duration::from_secs(366 * 86400);
        let setters: [fn(&mut tidemark_core::config::Config, std::time::Duration); 7] = [
            |c, d| c.throttle.sample_interval = d,
            |c, d| c.sampler.interval = d,
            |c, d| c.lvm.interval = d,
            |c, d| c.lvm.timeout = d,
            |c, d| c.procscan.interval = d,
            |c, d| c.metrics.interval = d,
            |c, d| c.alerts.renotify = d,
        ];
        for set in setters {
            let mut config = tidemark_core::config::Config::default();
            set(&mut config, max);
            validate_config(&config).unwrap();
            set(&mut config, max + std::time::Duration::from_nanos(1));
            assert!(validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("366 days"));
        }
        let mut config = tidemark_core::config::Config::default();
        config.roots[0].schedule = "every 31622400s".into();
        validate_config(&config).unwrap();
        config.roots[0].schedule = "every 31622401s".into();
        assert!(validate_config(&config)
            .unwrap_err()
            .to_string()
            .contains("366 days"));
    }
    #[test]
    fn captured_config_bytes_determine_both_parse_and_hash() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = b"[sampler]\ninterval='1h'\n";
        std::fs::write(&path, original).unwrap();
        // Mutate the same inode at the real loader's capture/parse boundary.
        let (config, hash) = load_config_captured(&path, |captured| {
            assert_eq!(captured, original);
            std::fs::write(&path, b"[sampler]\ninterval='2h'\n").unwrap();
        })
        .unwrap();
        assert_eq!(
            config.sampler.interval,
            std::time::Duration::from_secs(3600)
        );
        assert_eq!(hash, format!("{:x}", Sha256::digest(original)));
        let (later, later_hash) = load_config(&path).unwrap();
        assert_eq!(later.sampler.interval, std::time::Duration::from_secs(7200));
        assert_ne!(hash, later_hash);
    }
    #[test]
    fn expired_shutdown_deadline_still_joins_worker() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let completed = Arc::new(AtomicBool::new(false));
        let flag = completed.clone();
        let (ready, started) = crossbeam_channel::bounded(1);
        let handle = std::thread::spawn(move || {
            ready.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
            flag.store(true, Ordering::Release);
            Ok(())
        });
        started.recv().unwrap();
        assert!(join_until("test", handle, std::time::Instant::now()).is_err());
        assert!(completed.load(Ordering::Acquire));
    }
    #[test]
    fn pending_scan_reservation_survives_reload_and_stale_schedule_cannot_reappear() {
        use tidemark_core::{
            config::{Config, RootConfig},
            live::ScanStatus,
        };
        let config = Config::default();
        let state = state::LiveState::new(&config, 0);
        let old_root = config.roots[0].path.to_string_lossy().into_owned();
        state.reserve_scan(old_root.clone()).unwrap();
        let before = state.config();
        let mut next = config;
        next.roots = (0..128)
            .map(|n| RootConfig {
                path: format!("/replacement/{n}").into(),
                schedule: "off".into(),
                ..RootConfig::default()
            })
            .collect();
        assert!(state.replace_config(next.clone()).is_err());
        assert_eq!(state.config(), before);
        state.release_scan();
        state.replace_config(next).unwrap();
        state
            .publish_scheduled(
                ScanStatus {
                    root: old_root,
                    running: None,
                    last_complete: None,
                    next_due: None,
                },
                before.1,
                component("scheduler", "ok", "stale"),
            )
            .unwrap();
        assert_eq!(state.view().scans().unwrap().len(), 128);
        assert!(!state
            .reserve_scheduled("/retired".into(), before.1)
            .unwrap());
        assert_eq!(state.view().scans().unwrap().len(), 128);
    }
    #[test]
    fn example_and_unknown_key() {
        let doc = include_str!("../../../docs/plan/08-interfaces.md");
        let example = doc
            .split("```toml\n")
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, example).unwrap();
        let args = Args {
            config: path.clone(),
            check_config: true,
        };
        let mut output = Vec::new();
        assert_eq!(
            run_args(&args, &mut output),
            0,
            "{}",
            String::from_utf8_lossy(&output)
        );
        std::fs::write(&path, "[general]\nunknown_key = true\n").unwrap();
        output.clear();
        assert_eq!(run_args(&args, &mut output), 2);
        assert!(String::from_utf8_lossy(&output).contains("unknown field `unknown_key`"));
    }
    #[test]
    fn reports_all_validation_issues() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(
            &path,
            "[general]\nstate_dir = 'relative'\nsocket = 'relative'\n",
        )
        .unwrap();
        let mut output = Vec::new();
        assert_eq!(
            run_args(
                &Args {
                    config: path,
                    check_config: true
                },
                &mut output
            ),
            2
        );
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("general.state_dir"));
        assert!(text.contains("general.socket"));
    }
}

/// Daemon library failures; owners may extend this enum within W6-A1's file.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    /// Preparation stub has no runtime implementation.
    #[error("NotYetImplemented (Wave 6)")]
    NotYetImplemented,
    /// Filesystem or operating-system operation failed.
    #[error("{context}: {source}")]
    Io {
        /// Operation being attempted.
        context: String,
        /// Underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// Configuration could not be loaded or validated.
    #[error("configuration: {0}")]
    Config(String),
    /// Persistent storage failed.
    #[error("storage: {0}")]
    Store(String),
    /// Scan failed.
    #[error("scan: {0}")]
    Scan(String),
    /// Component is disabled or has no measurement.
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// Request or argument is invalid.
    #[error("invalid: {0}")]
    Invalid(String),
    /// Daemon is shutting down.
    #[error("daemon shutting down")]
    Shutdown,
    /// Internal invariant or worker failed.
    #[error("internal: {0}")]
    Internal(String),
}
fn limit_text(limit: Option<u64>) -> String {
    limit.map_or_else(|| "unlimited".into(), |value| value.to_string())
}
fn log_descriptor_limit(limit: rustix::process::Rlimit) {
    tracing::info!(soft = %limit_text(limit.current), hard = %limit_text(limit.maximum), "effective descriptor limit: soft {}, hard {}", limit_text(limit.current), limit_text(limit.maximum));
}
fn raise_fd_limit(
    get: impl Fn() -> rustix::process::Rlimit,
    set: impl Fn(rustix::process::Rlimit) -> Result<(), rustix::io::Errno>,
) -> rustix::process::Rlimit {
    let limit = get();
    if limit.current != limit.maximum {
        if let Err(error) = set(rustix::process::Rlimit {
            current: limit.maximum,
            ..limit
        }) {
            tracing::warn!(%error, "could not raise descriptor soft limit: {error}");
        }
    }
    get()
}

fn fd_budget(limit: Option<u64>, mounts: usize) -> usize {
    limit.map_or(256, |limit| {
        if mounts == 0 {
            256
        } else {
            (limit.saturating_sub(512) / mounts as u64).clamp(16, 256) as usize
        }
    })
}

pub(crate) fn watch_fd_budget(mounts: usize) {
    let limit = rustix::process::getrlimit(rustix::process::Resource::Nofile).current;
    let needed = (mounts as u64).saturating_mul(256).saturating_add(512);
    let batch = fd_budget(limit, mounts);
    tidemark_live::watch::set_fd_batch(batch);
    if limit.is_some_and(|limit| limit < needed) {
        tracing::warn!(
            limit = %limit_text(limit),
            needed,
            mounts,
            batch,
            "descriptor budget reduced; minimum batch may exceed available reserve: soft {}, needed {needed}, mounts {mounts}, batch {batch}", limit_text(limit)
        );
    }
}

fn daemon_pressure(error: &DaemonError) -> bool {
    matches!(error, DaemonError::Io { source, .. } if resource_pressure(source))
        || matches!(error, DaemonError::Unavailable(detail) if detail.starts_with("resource pressure:"))
}
fn startup_store_error(error: tidemark_store::StoreError) -> DaemonError {
    if store_pressure(&error) {
        DaemonError::Unavailable(format!("resource pressure: {error}"))
    } else {
        DaemonError::Store(error.to_string())
    }
}
fn retry_start<T>(
    name: &str,
    state: &state::LiveState,
    shutdown: &Shutdown,
    mut start: impl FnMut() -> Result<T, DaemonError>,
) -> Result<T, DaemonError> {
    use std::{
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    let mut failures = 0u32;
    let mut last_log: Option<Instant> = None;
    loop {
        match start() {
            Err(error) if daemon_pressure(&error) => {
                // Preserve the initial startup attempt: recovery tests and callers
                // may request shutdown before entry but still require catalog repair.
                if shutdown.load(Ordering::Acquire) {
                    return Err(DaemonError::Shutdown);
                }
                state.publish_component(component(name, "degraded", error.to_string()))?;
                if last_log.is_none_or(|at| at.elapsed() >= Duration::from_secs(60)) {
                    tracing::warn!(component=%name, %error, "startup resource pressure; retrying {name}: {error}");
                    last_log = Some(Instant::now());
                }
                let end = Instant::now() + Duration::from_millis(50u64 << failures.min(4));
                failures = failures.saturating_add(1);
                while !shutdown.load(Ordering::Acquire) && Instant::now() < end {
                    std::thread::park_timeout(Duration::from_millis(20));
                }
            }
            result => return result,
        }
    }
}

// Resource pressure must be contained by the component owning the retry loop.
pub(crate) fn resource_pressure(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::OutOfMemory
        || error.raw_os_error().is_some_and(|code| {
            [
                rustix::io::Errno::MFILE,
                rustix::io::Errno::NFILE,
                rustix::io::Errno::NOMEM,
                rustix::io::Errno::NOBUFS,
            ]
            .iter()
            .any(|errno| errno.raw_os_error() == code)
        })
}

fn sqlite_pressure_code(code: i32) -> bool {
    matches!(code & 0xff, 7 | 10 | 14)
}

pub(crate) fn store_pressure(error: &tidemark_store::StoreError) -> bool {
    match error {
        tidemark_store::StoreError::Io(error) => resource_pressure(error),
        // SQLite preserves its extended result code, but not the underlying errno.
        // CANTOPEN, NOMEM and IOERR are retried by read-only scheduling passes.
        tidemark_store::StoreError::Sql(error) => error
            .sqlite_error()
            .is_some_and(|error| sqlite_pressure_code(error.extended_code)),
        _ => false,
    }
}

/// Shared shutdown flag; lifecycle owner broadcasts by setting it true.
pub type Shutdown = std::sync::Arc<std::sync::atomic::AtomicBool>;
/// Service thread joined during bounded shutdown. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub type ServiceHandle = std::thread::JoinHandle<Result<(), DaemonError>>;
/// Startup options with injected clock. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub struct RunOptions {
    /// Source file for reload.
    pub config_path: std::path::PathBuf,
    /// Unix startup timestamp.
    pub now: i64,
}
/// Start the daemon with OS signals and systemd notification.
pub fn run(config: tidemark_core::config::Config, options: RunOptions) -> Result<(), DaemonError> {
    let state = state::LiveState::new(&config, options.now);
    let shutdown = Shutdown::default();
    let reload = Shutdown::default();
    let ids = [
        signal_hook::flag::register(signal_hook::consts::SIGTERM, shutdown.clone()),
        signal_hook::flag::register(signal_hook::consts::SIGINT, shutdown.clone()),
        signal_hook::flag::register(signal_hook::consts::SIGHUP, reload.clone()),
    ];
    let mut registered = Vec::new();
    for id in ids {
        match id {
            Ok(id) => registered.push(id),
            Err(source) => {
                for id in registered {
                    signal_hook::low_level::unregister(id);
                }
                return Err(DaemonError::Io {
                    context: "register daemon signals".into(),
                    source,
                });
            }
        }
    }
    let result = run_controlled(config, options, state, shutdown, reload);
    for id in registered {
        signal_hook::low_level::unregister(id);
    }
    result
}
/// In-process lifecycle entry point. Caller controls shutdown/reload flags and observes state.
/// No process-global signal handlers are installed. All flags use Acquire/Release ordering.
// Default remains compatible when the scheduler owner replaces its unit stub.
#[allow(clippy::default_constructed_unit_structs)]
pub fn run_controlled(
    mut config: tidemark_core::config::Config,
    options: RunOptions,
    state: state::LiveState,
    shutdown: Shutdown,
    reload_flag: Shutdown,
) -> Result<(), DaemonError> {
    use std::{
        sync::{atomic::Ordering, Arc},
        time::{Duration, Instant},
    };
    tracing::info!(
        "tidemarkd {} starting: commit {} ({}), target {}, profile {}",
        env!("CARGO_PKG_VERSION"),
        env!("TIDEMARK_BUILD_COMMIT"),
        env!("TIDEMARK_BUILD_DATE"),
        env!("TIDEMARK_BUILD_TARGET"),
        env!("TIDEMARK_BUILD_PROFILE")
    );
    validate_config(&config)?;
    let limit = raise_fd_limit(
        || rustix::process::getrlimit(rustix::process::Resource::Nofile),
        |limit| rustix::process::setrlimit(rustix::process::Resource::Nofile, limit),
    );
    log_descriptor_limit(limit);
    let (loaded, hash) = retry_start("config", &state, &shutdown, || {
        load_config(&options.config_path)
    })?;
    if loaded != config {
        return Err(DaemonError::Config(
            "supplied configuration differs from loaded bytes".into(),
        ));
    }
    // Validate all unsafe paths before creating any artifact; retain the directory handle.
    let fd = retry_start("daemon", &state, &shutdown, || prepare_state(&config))?;
    protect_config(&mut config)?;
    state.replace_config(config.clone())?;
    state.bind_storage(fd);
    state.publish(
        state::Publication::Deployment {
            config_sha256: hash,
            started_at: options.now,
        },
        component("daemon", "starting", "initializing"),
    )?;
    // Both storage opens perform header preflight before SQLite. Recovery is corruption-only.
    let storage = state.storage_path();
    let catalog = retry_start("catalog", &state, &shutdown, || {
        startup_catalog(&storage, options.now, Duration::from_secs(30))
    })?;
    // Read catalog history before treating any configured root as measured never-scanned.
    for root in &config.roots {
        let raw = tidemark_core::path::RawPath::from_path(&root.path)
            .map_err(|e| DaemonError::Config(e.to_string()))?;
        let entry = retry_start("catalog", &state, &shutdown, || {
            catalog.latest(&raw).map_err(startup_store_error)
        })?;
        let last_complete = entry.and_then(|e| {
            Some(tidemark_core::live::LastScan {
                scan_id: e.scan_id,
                finished_at: e.finished_at?,
                mode: format!("{:?}", e.mode).to_lowercase(),
                duration_ms: e.duration_ms.unwrap_or(0),
            })
        });
        state.publish(
            state::Publication::Scan(tidemark_core::live::ScanStatus {
                root: root.path.to_string_lossy().into_owned(),
                running: None,
                last_complete,
                next_due: None,
            }),
            component("catalog", "ok", "scan history read"),
        )?;
    }
    drop(catalog);
    let ts = retry_start("tswriter", &state, &shutdown, || {
        tidemark_store::timeseries::TimeSeries::open(&storage.join("timeseries.db"), true)
            .map_err(startup_store_error)
    })?;
    drop(ts);
    if config.watch.enabled {
        match tidemark_live::watch::namespace_ok() {
            Ok(true) => {}
            Err(error) if resource_pressure(&error) => {
                state.publish_component(component("watcher", "degraded", error.to_string()))?;
            }
            result => {
                let detail = match result {
                    Ok(false) => "daemon is outside the host mount namespace".to_owned(),
                    Err(error) => error.to_string(),
                    Ok(true) => "host namespace check changed".to_owned(),
                };
                let reason =
                    format!("host mount namespace unavailable: {detail}; watcher disabled");
                tracing::error!(%reason, "{reason}");
                config.watch.enabled = false;
                state.publish(
                    state::Publication::Degraded(reason.clone()),
                    component("watcher", "disabled", reason),
                )?;
                state.replace_config(config.clone())?;
            }
        }
    }
    match tidemark_store::space_guard::space_check(&storage, 0, &config.retention) {
        Ok(verdict) => state.publish(
            state::Publication::StateSpaceRefused(matches!(
                verdict,
                tidemark_store::space_guard::SpaceVerdict::Refuse { .. }
            )),
            component("state_space", "ok", "space guard measured"),
        )?,
        Err(error) => {
            state.publish_component(component("state_space", "error", error.to_string()))?
        }
    }
    let config = Arc::new(config);
    let writer_shutdown = Shutdown::default();
    let (writer, writer_handle) = retry_start("tswriter", &state, &shutdown, || {
        services::tswriter::start_supervised(config.clone(), state.clone(), writer_shutdown.clone())
    })?;
    // Any startup error stops already-started services before returning.
    let mut handles = Vec::new();
    let recorder_deadline = Arc::new(parking_lot::Mutex::new(None));
    let started = (|| -> Result<_, DaemonError> {
        add_service(
            "sampler",
            retry_start("sampler", &state, &shutdown, || {
                services::sampler::start(
                    config.clone(),
                    state.clone(),
                    writer.clone(),
                    shutdown.clone(),
                )
            }),
            &state,
            &mut handles,
        )?;
        add_service(
            "lvm",
            retry_start("lvm", &state, &shutdown, || {
                services::lvm::start(
                    config.clone(),
                    state.clone(),
                    writer.clone(),
                    shutdown.clone(),
                )
            }),
            &state,
            &mut handles,
        )?;
        add_service(
            "watcher",
            retry_start("watcher", &state, &shutdown, || {
                services::watcher::start(
                    config.clone(),
                    state.clone(),
                    writer.clone(),
                    shutdown.clone(),
                )
            }),
            &state,
            &mut handles,
        )?;
        add_service(
            "procscan",
            retry_start("procscan", &state, &shutdown, || {
                services::procscan::start(
                    config.clone(),
                    state.clone(),
                    writer.clone(),
                    shutdown.clone(),
                )
            }),
            &state,
            &mut handles,
        )?;
        add_service(
            "alerts",
            retry_start("alerts", &state, &shutdown, || {
                services::alerts::start(
                    config.clone(),
                    state.clone(),
                    writer.clone(),
                    shutdown.clone(),
                )
            }),
            &state,
            &mut handles,
        )?;
        let scans = match retry_start("scheduler", &state, &shutdown, || {
            scheduler::start_with_deadline(
                config.clone(),
                state.clone(),
                writer.clone(),
                shutdown.clone(),
                recorder_deadline.clone(),
            )
        }) {
            Ok((scans, h)) => {
                handles.push(("scheduler", h));
                scans
            }
            Err(DaemonError::NotYetImplemented) => {
                state.publish_component(component(
                    "scheduler",
                    "disabled",
                    "implementation pending",
                ))?;
                scheduler::ScanControl::default()
            }
            Err(e) => return Err(e),
        };
        add_service(
            "metrics",
            retry_start("metrics", &state, &shutdown, || {
                metrics::start(config.clone(), state.clone(), shutdown.clone())
            }),
            &state,
            &mut handles,
        )?;
        let reload = cooperative_reload_callback(options.config_path.clone(), state.clone());
        let context = api::Context {
            state: state.clone(),
            scans: scans.clone(),
            reload: reload.clone(),
        };
        add_service(
            "api",
            retry_start("api", &state, &shutdown, || {
                api::start(
                    config.clone(),
                    api::Context {
                        state: context.state.clone(),
                        scans: context.scans.clone(),
                        reload: context.reload.clone(),
                    },
                    shutdown.clone(),
                )
            }),
            &state,
            &mut handles,
        )?;
        Ok((scans, reload))
    })();
    let mut shutdown_deadline = None;
    let outcome = match started {
        Ok((scans, reload)) => {
            state.publish_component(component("daemon", "ok", "ready"))?;
            let _ = sd_notify::notify(
                false,
                &[
                    sd_notify::NotifyState::Ready,
                    sd_notify::NotifyState::Status("Tidemark ready"),
                ],
            );
            tracing::info!("daemon ready");
            let mut firstboot_followup = firstboot::Followup::default();
            let firstboot_elapsed = monotonic_clock();
            let mut firstboot_due = Instant::now();
            while !shutdown.load(Ordering::Acquire) {
                if reload_flag.swap(false, Ordering::AcqRel) {
                    match reload.call(&api::Cancellation::new(
                        shutdown.clone(),
                        std::time::Duration::from_secs(10),
                    )) {
                        Ok(r) if r.reloaded => {
                            tracing::info!(reloaded = true, "config reload: accepted")
                        }
                        Ok(_) => {}
                        Err(e) => tracing::error!(error=%e,"config reload rejected: {e}"),
                    }
                }
                if Instant::now() >= firstboot_due {
                    firstboot_due = Instant::now() + Duration::from_secs(10);
                    if let Some(inputs) = state.view().firstboot_inputs() {
                        if let Err(error) = firstboot_followup.poll(
                            &state.config().0.general.state_dir,
                            &inputs,
                            unix_now(),
                            firstboot_elapsed(),
                        ) {
                            tracing::warn!(%error, "firstboot followup failed: {error}");
                        }
                    }
                }
                for (name, h) in &handles {
                    if h.is_finished() {
                        let _ = state.publish_component(component(
                            name,
                            "error",
                            "service thread exited",
                        ));
                    }
                }
                std::thread::park_timeout(Duration::from_millis(50));
            }
            let deadline = Instant::now() + Duration::from_secs(9);
            shutdown_deadline = Some(deadline);
            *recorder_deadline.lock() = Some(deadline);
            scans.begin_shutdown(deadline);
            // API observes shutdown first; cancellation precedes producer joins and writer drain.
            for scan in state
                .view()
                .scans()
                .unwrap_or_default()
                .iter()
                .filter(|s| s.running.is_some())
            {
                if let Ok(root) = tidemark_core::path::RawPath::new(scan.root.as_bytes().to_vec()) {
                    let _ = scans.cancel(&tidemark_core::api::ScanCancelParams { root });
                }
            }
            Ok(())
        }
        Err(e) => Err(e),
    };
    let _ = sd_notify::notify(
        false,
        &[
            sd_notify::NotifyState::Stopping,
            sd_notify::NotifyState::Status("Tidemark stopping"),
        ],
    );
    tracing::info!("daemon stopping");
    shutdown.store(true, Ordering::Release);
    let deadline = shutdown_deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(9));
    *recorder_deadline.lock() = Some(deadline);
    // API first, then scheduler/watchers and the other producers.
    handles.sort_by_key(|(name, _)| match *name {
        "api" => 0,
        "scheduler" => 1,
        "watcher" => 2,
        _ => 3,
    });
    let mut cleanup_error = None;
    for (name, h) in handles {
        if let Err(e) = join_until(name, h, deadline) {
            cleanup_error = Some(e);
        }
    }
    state.clear_watchers();
    let (ack, done) = crossbeam_channel::bounded(1);
    let remaining = deadline.saturating_duration_since(Instant::now());
    if writer
        .send_timeout(services::tswriter::Message::Flush(ack), remaining)
        .is_ok()
    {
        let _ = done.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    }
    drop(writer);
    writer_shutdown.store(true, Ordering::Release);
    if let Err(e) = join_until("tswriter", writer_handle, deadline) {
        cleanup_error = Some(e);
    }
    state.publish_component(component("daemon", "stopped", "shutdown completed"))?;
    outcome.and(cleanup_error.map_or(Ok(()), Err))
}
fn add_service(
    name: &'static str,
    result: Result<ServiceHandle, DaemonError>,
    state: &state::LiveState,
    handles: &mut Vec<(&'static str, ServiceHandle)>,
) -> Result<(), DaemonError> {
    match result {
        Ok(h) => {
            handles.push((name, h));
            Ok(())
        }
        Err(DaemonError::NotYetImplemented) => {
            tracing::warn!(component = %name, "service implementation pending: {name}");
            state.publish_component(component(name, "disabled", "implementation pending"))
        }
        Err(e) => Err(e),
    }
}
fn join_until(
    name: &str,
    handle: ServiceHandle,
    deadline: std::time::Instant,
) -> Result<(), DaemonError> {
    let mut expired = false;
    while !handle.is_finished() {
        if std::time::Instant::now() >= deadline {
            expired = true;
            break;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(10));
    }
    // Never detach a producer: resources and the writer remain alive until it exits.
    let result = handle
        .join()
        .map_err(|_| DaemonError::Internal(format!("{name} thread panicked")))?;
    if expired {
        return Err(DaemonError::Internal(format!(
            "{name} did not stop before shutdown deadline"
        )));
    }
    if let Err(error) = &result {
        if daemon_pressure(error) {
            tracing::warn!(component=%name, %error, "service degraded at shutdown: {name}: {error}");
            return Ok(());
        }
    }
    result
}
fn startup_catalog(
    storage: &std::path::Path,
    now: i64,
    wait: Duration,
) -> Result<tidemark_store::catalog::Catalog, DaemonError> {
    // Probe without migrations or writes before deciding whether to wait.
    let needs_recovery = match tidemark_store::catalog::Catalog::open(storage, false) {
        Ok(catalog) => {
            drop(catalog);
            false
        }
        Err(error) if corrupt(&error) => true,
        Err(_) => !storage.join("timeseries.db").exists(),
    };
    let recovery_lease = if needs_recovery {
        Some(
            tidemark_store::lease::RecoveryLease::acquire(storage, wait)
                .map_err(startup_lease_error)?,
        )
    } else {
        None
    };
    let writer_lease = if needs_recovery {
        None
    } else {
        Some(tidemark_store::lease::WriterLease::acquire(storage).map_err(startup_lease_error)?)
    };
    let deadline = Instant::now() + wait;
    let lock = loop {
        match tidemark_scan::orchestrator::ScanLock::acquire(storage) {
            Ok(lock) => break Some(lock),
            Err(tidemark_scan::orchestrator::ScanError::Busy) if needs_recovery => {
                if Instant::now() >= deadline {
                    return Err(DaemonError::Store(
                        "state directory in use by another tidemark process".into(),
                    ));
                }
                std::thread::sleep(
                    Duration::from_millis(25)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(tidemark_scan::orchestrator::ScanError::Busy) => break None,
            Err(error) => return Err(startup_scan_error(error)),
        }
    };
    if let Some(lease) = recovery_lease {
        let held = lock
            .as_ref()
            .ok_or_else(|| DaemonError::Store("recovery requires scan ownership".into()))?;
        // A racing start may already have recovered while we waited for ownership.
        let files = match tidemark_store::catalog::Catalog::open(storage, false) {
            Ok(_) => Vec::new(),
            Err(error) if corrupt(&error) => held
                .recover_catalog(storage, &lease)
                .map_err(startup_scan_error)?,
            Err(_) if !storage.join("timeseries.db").exists() => held
                .recover_catalog(storage, &lease)
                .map_err(startup_scan_error)?,
            Err(error) => return Err(startup_store_error(error)),
        };
        drop(lock);
        drop(lease);
        let catalog = tidemark_store::catalog::Catalog::open_after_recovery(storage, files)
            .map_err(startup_store_error)?;
        // Cleanup requires scan exclusion after normal writer ownership is retained.
        if let Ok(_lock) = tidemark_scan::orchestrator::ScanLock::acquire(storage) {
            tidemark_store::catalog::cleanup_abandoned_scans(storage, now);
        }
        return Ok(catalog);
    }
    let _writer_lease = writer_lease;
    let catalog = match &lock {
        Some(lock) => {
            let catalog = lock.open_catalog(storage).map_err(startup_scan_error)?;
            tidemark_store::catalog::cleanup_abandoned_scans(storage, now);
            catalog
        }
        None => {
            tracing::info!("startup scan cleanup skipped: host scan lock is held");
            tidemark_store::catalog::Catalog::open(storage, true).map_err(startup_store_error)?
        }
    };
    Ok(catalog)
}

fn startup_lease_error(source: std::io::Error) -> DaemonError {
    if source.kind() == std::io::ErrorKind::WouldBlock {
        DaemonError::Store(source.to_string())
    } else {
        DaemonError::Io {
            context: "writer lease".into(),
            source,
        }
    }
}

fn startup_scan_error(error: tidemark_scan::orchestrator::ScanError) -> DaemonError {
    use tidemark_scan::orchestrator::ScanError;
    match error {
        ScanError::Io(source) => DaemonError::Io {
            context: "startup scan ownership".into(),
            source,
        },
        ScanError::Sys(source) => DaemonError::Io {
            context: "startup scan ownership".into(),
            source: source.into(),
        },
        ScanError::Store(error) => startup_store_error(error),
        ScanError::Busy => {
            DaemonError::Store("state directory in use by another tidemark process".into())
        }
        error => DaemonError::Store(error.to_string()),
    }
}

fn corrupt(e: &tidemark_store::StoreError) -> bool {
    // Match the exposed SQLite error code without adding a direct rusqlite dependency.
    matches!(e,tidemark_store::StoreError::Sql(e) if e.sqlite_error_code().is_some_and(|code|matches!(format!("{code:?}").as_str(),"DatabaseCorrupt"|"NotADatabase")))
}
fn validate_config(c: &tidemark_core::config::Config) -> Result<(), DaemonError> {
    let issues = c.validate();
    if !issues.is_empty() {
        return Err(DaemonError::Config(
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    validate_output_paths(c)?;
    state::validate_bounds(c)?;
    for (index, root) in c.roots.iter().enumerate() {
        scheduler::parse_schedule(&root.schedule)
            .map_err(|e| DaemonError::Config(format!("roots[{index}].schedule: {e}")))?;
        if let Some(window) = &root.window {
            scheduler::parse_window(window)
                .map_err(|e| DaemonError::Config(format!("roots[{index}].window: {e}")))?;
        }
    }
    // Retention horizons are wall-clock history, not runtime wait intervals.
    for (field, duration) in [
        ("throttle.sample_interval", c.throttle.sample_interval),
        ("sampler.interval", c.sampler.interval),
        ("lvm.interval", c.lvm.interval),
        ("lvm.timeout", c.lvm.timeout),
        ("procscan.interval", c.procscan.interval),
        ("metrics.interval", c.metrics.interval),
        ("alerts.renotify", c.alerts.renotify),
    ] {
        validate_interval(field, duration)?;
    }
    for (index, root) in c.roots.iter().enumerate() {
        if let Some(duration) = root.schedule.strip_prefix("every ") {
            let duration = humantime::parse_duration(duration)
                .map_err(|e| DaemonError::Config(e.to_string()))?;
            validate_interval(&format!("roots[{index}].schedule"), duration)?;
        }
    }
    for (index, rule) in c.alerts.rule.iter().enumerate() {
        validate_interval(&format!("alerts.rule[{index}].for"), rule.for_)?;
        validate_interval(
            &format!("alerts.rule[{index}].resolve_for"),
            rule.resolve_for,
        )?;
    }
    tidemark_live::alerts::AlertEngine::new(&c.alerts.rule, c.alerts.renotify)
        .map_err(|e| DaemonError::Config(format!("alerts: {e}")))?;
    Ok(())
}
fn validate_output_paths(c: &tidemark_core::config::Config) -> Result<(), DaemonError> {
    use std::os::unix::fs::MetadataExt;
    use std::path::{Component, Path, PathBuf};
    fn suffix(path: &Path, suffix: &str) -> PathBuf {
        let mut name = path.file_name().unwrap_or(path.as_os_str()).to_os_string();
        name.push(suffix);
        path.with_file_name(name)
    }
    fn normalize(path: &Path) -> PathBuf {
        let mut result = PathBuf::new();
        for part in path.components() {
            match part {
                Component::CurDir => {}
                Component::ParentDir => {
                    result.pop();
                }
                part => result.push(part.as_os_str()),
            }
        }
        result
    }
    fn identity(path: &Path) -> Result<Option<(u64, u64, std::ffi::OsString)>, DaemonError> {
        let Some(parent) = path.parent() else {
            return Ok(None);
        };
        let Some(name) = path.file_name() else {
            return Ok(None);
        };
        match std::fs::metadata(parent) {
            Ok(md) => Ok(Some((md.dev(), md.ino(), name.to_os_string()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(DaemonError::Config(format!(
                "output parent {}: {e}",
                state_files::display(parent)
            ))),
        }
    }
    let textfile = &c.metrics.textfile;
    if textfile.as_os_str().is_empty() {
        return Ok(());
    }
    for api in [&c.general.socket, &suffix(&c.general.socket, ".lock")] {
        for metrics in [textfile, &suffix(textfile, ".tmp")] {
            let api_identity = identity(api)?;
            let metrics_identity = identity(metrics)?;
            if normalize(api) == normalize(metrics)
                || api_identity.is_some() && api_identity == metrics_identity
            {
                return Err(DaemonError::Config(format!(
                    "metrics.textfile collides with API output: {} and {}",
                    state_files::display(metrics),
                    state_files::display(api)
                )));
            }
        }
    }
    Ok(())
}
fn validate_interval(field: &str, duration: std::time::Duration) -> Result<(), DaemonError> {
    if duration > std::time::Duration::from_secs(366 * 24 * 60 * 60) {
        return Err(DaemonError::Config(format!(
            "{field}: interval must not exceed 366 days"
        )));
    }
    Ok(())
}
/// Add state_dir to all collector and scan exclusions before publishing config.
pub fn protect_config(c: &mut tidemark_core::config::Config) -> Result<(), DaemonError> {
    let path = c.general.state_dir.clone();
    for excludes in [
        &mut c.watch.exclude_prefixes,
        &mut c.procscan.exclude_prefixes,
    ] {
        if !excludes.contains(&path) {
            excludes.push(path.clone());
        }
    }
    let text = path
        .to_str()
        .ok_or_else(|| DaemonError::Config("state_dir from TOML must be UTF-8".into()))?;
    // Glob metacharacters are escaped so the directory is always excluded literally.
    let mut literal = String::new();
    for ch in text.chars() {
        if matches!(ch, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
            literal.push('\\');
        }
        literal.push(ch);
    }
    for root in &mut c.roots {
        if !root.exclude.contains(&literal) {
            root.exclude.push(literal.clone());
        }
    }
    Ok(())
}
/// API/SIGHUP reload callback. Invalid config returns reloaded=false with all issues.
/// Services observe LiveState::config generation; state_dir/socket require restart.
pub fn reload_callback(
    path: std::path::PathBuf,
    state: state::LiveState,
) -> std::sync::Arc<
    dyn Fn() -> Result<tidemark_core::api::ConfigReloadResult, DaemonError> + Send + Sync,
> {
    let callback = cooperative_reload_callback(path, state);
    std::sync::Arc::new(move || {
        callback.call(&api::Cancellation::new(
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            std::time::Duration::from_secs(10),
        ))
    })
}
fn cooperative_reload_callback(path: std::path::PathBuf, state: state::LiveState) -> api::Reload {
    std::sync::Arc::new(api::CooperativeReload(
        move |cancellation: &api::Cancellation| {
            let result = (|| {
                cancellation.check()?;
                let (mut config, hash) = load_config(&path)?;
                cancellation.check()?;
                validate_config(&config)?;
                validate_state_paths(&config)?;
                protect_config(&mut config)?;
                cancellation.check()?;
                scheduler::preflight_reload(&state, &config)?;
                // Atomic request cancellation wins before this commit boundary, or
                // publication completes afterward. In-flight kernel I/O cannot be
                // interrupted; a timeout after commit begins has uncertain outcome.
                cancellation.begin_commit()?;
                state.replace_config(config)?;
                let started = state
                    .view()
                    .firstboot_inputs()
                    .map_or(0, |i| i.daemon_started_at);
                state.publish(
                    state::Publication::Deployment {
                        config_sha256: hash,
                        started_at: started,
                    },
                    component("config", "ok", "reloaded"),
                )?;
                Ok::<_, DaemonError>(())
            })();
            match result {
                Ok(()) => {
                    let _ = sd_notify::notify(
                        false,
                        &[sd_notify::NotifyState::Status("Configuration reloaded")],
                    );
                    Ok(tidemark_core::api::ConfigReloadResult {
                        reloaded: true,
                        warnings: Vec::new(),
                    })
                }
                Err(e) => {
                    tracing::error!(reloaded=false,error=%e,"keeping previous config: reload rejected: {e}");
                    let _ = sd_notify::notify(
                        false,
                        &[sd_notify::NotifyState::Status(
                            "Configuration invalid; retaining previous configuration",
                        )],
                    );
                    state.publish_component(component("config", "error", e.to_string()))?;
                    Ok(tidemark_core::api::ConfigReloadResult {
                        reloaded: false,
                        warnings: vec![e.to_string()],
                    })
                }
            }
        },
    ))
}
#[path = "../../tidemark-cli/src/deploy/files.rs"]
// firstboot::files is private; both inclusions share the CLI-owned policy source.
#[allow(dead_code, clippy::duplicate_mod)]
mod state_files;
fn validate_state_paths(
    c: &tidemark_core::config::Config,
) -> Result<(Option<std::os::fd::OwnedFd>, Option<std::os::fd::OwnedFd>), DaemonError> {
    state_files::validate_state(
        &c.general.state_dir,
        c.roots
            .iter()
            .map(|r| &r.path)
            .chain(c.sampler.mounts.iter())
            .chain(c.watch.mounts.iter()),
    )
    .map_err(|source| DaemonError::Io {
        context: "unsafe state_dir refused".into(),
        source,
    })?;
    // Preserve existing child mount-boundary rejection alongside integrity checks.
    for name in ["deploy", "snapshots", "tmp"] {
        let path = c.general.state_dir.join(name);
        if std::fs::symlink_metadata(&path).is_ok() {
            state_files::validate_state(&path, std::iter::empty::<&std::path::Path>()).map_err(
                |source| DaemonError::Io {
                    context: format!("unsafe {name} directory"),
                    source,
                },
            )?;
        }
    }
    // Check eligibility without creating anything, and retain the checked handles.
    let (state, deploy) =
        state_files::state_handles(&c.general.state_dir, false).map_err(|source| {
            DaemonError::Io {
                context: "open eligible state directories".into(),
                source,
            }
        })?;
    state_files::validate_marker_roots_at(&c.general.state_dir, state.as_ref()).map_err(
        |source| DaemonError::Io {
            context: "validate state marker".into(),
            source,
        },
    )?;
    Ok((state, deploy))
}
fn prepare_state(c: &tidemark_core::config::Config) -> Result<std::os::fd::OwnedFd, DaemonError> {
    let (state, deploy) = validate_state_paths(c)?;
    let (fd, _deploy) = state_files::complete_handles(&c.general.state_dir, state, deploy)
        .map_err(|source| DaemonError::Io {
            context: "complete private state directories".into(),
            source,
        })?;
    Ok(fd)
}
fn load_config(
    path: &std::path::Path,
) -> Result<(tidemark_core::config::Config, String), DaemonError> {
    load_config_captured(path, |_| {})
}
fn load_config_captured(
    path: &std::path::Path,
    after_capture: impl FnOnce(&[u8]),
) -> Result<(tidemark_core::config::Config, String), DaemonError> {
    use rustix::fs::{self, FileType, Mode, OFlags};
    use std::io::Read;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| DaemonError::Io {
                context: "config cwd".into(),
                source,
            })?
            .join(path)
    };
    let parent = state_files::directory(
        absolute
            .parent()
            .ok_or_else(|| DaemonError::Config("config parent missing".into()))?,
        false,
    )
    .map_err(|source| DaemonError::Io {
        context: "open config parent".into(),
        source,
    })?;
    let fd = fs::openat(
        &parent,
        absolute
            .file_name()
            .ok_or_else(|| DaemonError::Config("config name missing".into()))?,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|e| DaemonError::Io {
        context: "open config without following links".into(),
        source: e.into(),
    })?;
    if FileType::from_raw_mode(
        fs::fstat(&fd)
            .map_err(|e| DaemonError::Config(e.to_string()))?
            .st_mode,
    ) != FileType::RegularFile
    {
        return Err(DaemonError::Config("config must be a regular file".into()));
    }
    let mut file = std::fs::File::from(fd);
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| DaemonError::Config(e.to_string()))?;
    if bytes.len() > 1024 * 1024 {
        return Err(DaemonError::Config("config exceeds 1 MiB".into()));
    }
    after_capture(&bytes);
    parse_config_bytes(&bytes)
}
fn parse_config_bytes(
    bytes: &[u8],
) -> Result<(tidemark_core::config::Config, String), DaemonError> {
    use sha2::{Digest, Sha256};
    let config = tidemark_core::config::Config::from_toml_bytes(bytes)
        .map_err(|e| DaemonError::Config(e.to_string()))?;
    Ok((config, format!("{:x}", Sha256::digest(bytes))))
}

/// Injectable seconds clock; wall and monotonic instances are wired separately.
pub type Clock = std::sync::Arc<dyn Fn() -> i64 + Send + Sync>;
/// Injectable elapsed-seconds clock, independent of wall-clock corrections.
pub fn monotonic_clock() -> Clock {
    let origin = std::time::Instant::now();
    std::sync::Arc::new(move || i64::try_from(origin.elapsed().as_secs()).unwrap_or(i64::MAX))
}
/// Wall clock adapter used only at process/service boundaries.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
/// Construct component health using the shared frozen DTO.
pub fn component(
    name: &str,
    state: &str,
    detail: impl Into<String>,
) -> tidemark_core::api::ComponentState {
    tidemark_core::api::ComponentState {
        name: name.into(),
        state: state.into(),
        detail: detail.into(),
    }
}
/// Wait interruptibly for a service interval; reload generation changes wake services.
pub fn wait_service(
    state: &state::LiveState,
    generation: u64,
    shutdown: &Shutdown,
    interval: std::time::Duration,
) {
    let start = std::time::Instant::now();
    while !shutdown.load(std::sync::atomic::Ordering::Acquire) && state.config().1 == generation {
        let remaining = interval.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            break;
        }
        std::thread::park_timeout(remaining.min(std::time::Duration::from_millis(50)));
    }
}
