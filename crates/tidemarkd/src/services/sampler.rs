//! Owned by: W6-A1
//! Interruptible filesystem sampling into bounded memory and persistence.

use crate::{
    state::{LiveState, Publication},
    DaemonError, ServiceHandle, Shutdown,
};
use std::{sync::atomic::Ordering, sync::Arc};
use tidemark_core::{config::Config, live::FsSample};

/// Sample filesystem gauges through an O_NOFOLLOW directory descriptor.
/// This is L0 sampling, not a scan, and does not acquire the scan Gate.
pub fn sample(path: &std::path::Path, now: i64) -> Result<FsSample, DaemonError> {
    sample_checked(path, now, None)
}

fn sample_checked(
    path: &std::path::Path,
    now: i64,
    intended: Option<crate::mounts::Identity>,
) -> Result<FsSample, DaemonError> {
    use rustix::fs::{self, Mode, OFlags};
    let base = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = match fs::open(path, base | OFlags::NOATIME, Mode::empty()) {
        Err(rustix::io::Errno::PERM) => fs::open(path, base, Mode::empty()),
        result => result,
    }
    .map_err(|e| DaemonError::Io {
        context: "open sampler mount".into(),
        source: e.into(),
    })?;
    if let Some(intended) = intended {
        use std::{io::Read, os::fd::AsRawFd};
        let mut info = String::new();
        std::fs::File::open(format!("/proc/self/fdinfo/{}", fd.as_raw_fd()))
            .and_then(|file| file.take(4097).read_to_string(&mut info))
            .map_err(|source| DaemonError::Io {
                context: "read sampler mount descriptor identity".into(),
                source,
            })?;
        let id = info
            .lines()
            .find_map(|line| line.strip_prefix("mnt_id:")?.trim().parse::<u32>().ok());
        let stat = fs::fstat(&fd).map_err(|e| DaemonError::Io {
            context: "stat sampler mount descriptor".into(),
            source: e.into(),
        })?;
        if info.len() > 4096
            || id != Some(intended.id)
            || fs::major(stat.st_dev) != intended.major
            || fs::minor(stat.st_dev) != intended.minor
        {
            return Err(DaemonError::Invalid(
                "sampler mount identity changed during open".into(),
            ));
        }
    }
    let s = fs::fstatvfs(&fd).map_err(|e| DaemonError::Io {
        context: "sample statvfs".into(),
        source: e.into(),
    })?;
    let bytes = |blocks: u64| {
        blocks
            .checked_mul(s.f_frsize)
            .ok_or_else(|| DaemonError::Invalid("statvfs byte count overflow".into()))
    };
    Ok(FsSample {
        ts: now,
        mount: path.to_string_lossy().into_owned(),
        size: bytes(s.f_blocks)?,
        used: bytes(s.f_blocks.saturating_sub(s.f_bfree))?,
        avail: bytes(s.f_bavail)?,
        inodes: s.f_files,
        inodes_used: s.f_files.saturating_sub(s.f_ffree),
    })
}
/// Start sampling immediately. Full writer queues drop persistence with degraded health;
/// the measurement remains in memory. Reload is observed through LiveState::config.
pub fn start(
    config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
) -> Result<ServiceHandle, DaemonError> {
    start_with(
        config,
        state,
        writer,
        shutdown,
        Arc::new(crate::unix_now),
        Arc::new(crate::mounts::read),
    )
}

type MountSource =
    Arc<dyn Fn() -> Result<Vec<tidemark_fs::mount::MountInfo>, DaemonError> + Send + Sync>;
fn start_with(
    config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
    clock: crate::Clock,
    mount_source: MountSource,
) -> Result<ServiceHandle, DaemonError> {
    start_with_sample(
        config,
        state,
        writer,
        shutdown,
        clock,
        mount_source,
        Arc::new(sample_checked),
    )
}
type SampleSource = Arc<
    dyn Fn(&std::path::Path, i64, Option<crate::mounts::Identity>) -> Result<FsSample, DaemonError>
        + Send
        + Sync,
>;
fn start_with_sample(
    config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
    clock: crate::Clock,
    mount_source: MountSource,
    collect: SampleSource,
) -> Result<ServiceHandle, DaemonError> {
    start_with_pass(
        config,
        state,
        writer,
        shutdown,
        clock,
        mount_source,
        collect,
        Arc::new(|| {}),
    )
}
#[allow(clippy::too_many_arguments)]
fn start_with_pass(
    config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
    clock: crate::Clock,
    mount_source: MountSource,
    collect: SampleSource,
    pass_finished: Arc<dyn Fn() + Send + Sync>,
) -> Result<ServiceHandle, DaemonError> {
    crate::state::validate_bounds(&config)?;
    std::thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            let mut last_sample = std::collections::BTreeMap::new();
            let mut identities = std::collections::BTreeMap::new();
            let mut next_sample = std::time::Instant::now();
            let mut previous_generation = None;
            let mut measurement_errors = std::collections::BTreeMap::new();
            while !shutdown.load(Ordering::Acquire) {
                let (config, generation) = state.config();
                if previous_generation != Some(generation) {
                    next_sample = std::time::Instant::now();
                    previous_generation = Some(generation);
                }
                let now = clock();
                let mut rows = Vec::new();
                let mut errors = Vec::new();
                last_sample.retain(|path, _| config.sampler.mounts.contains(path));
                measurement_errors.retain(|path, _| config.sampler.mounts.contains(path));
                let mounts = match mount_source() {
                    Ok(mounts) => mounts,
                    Err(error) => {
                        for path in &config.sampler.mounts {
                            measurement_errors.insert(path.clone(), error.to_string());
                        }
                        for path in &config.sampler.mounts {
                            state.publish(
                                Publication::FsAttempt {
                                    generation,
                                    mount: path.to_string_lossy().into_owned(),
                                    sample: None,
                                },
                                crate::component("sampler", "degraded", error.to_string()),
                            )?;
                        }
                        state.publish_component(crate::component(
                            "sampler",
                            "degraded",
                            error.to_string(),
                        ))?;
                        crate::wait_service(
                            &state,
                            generation,
                            &shutdown,
                            config
                                .sampler
                                .interval
                                .min(std::time::Duration::from_secs(10)),
                        );
                        continue;
                    }
                };
                let changes = crate::mounts::reconcile_live(
                    &config.sampler.mounts,
                    &mut identities,
                    &mounts,
                )?;
                state.publish_mount_presence(generation, &mounts)?;
                for change in &changes {
                    last_sample.remove(&change.path);
                }
                if !changes.is_empty() {
                    next_sample = std::time::Instant::now();
                }
                let due = std::time::Instant::now() >= next_sample;
                if due {
                    // No incomplete pass may certify firstboot, even after a healthy pass.
                    state.publish_component(crate::component(
                        "sampler",
                        "degraded",
                        "measurement pass in progress",
                    ))?;
                }
                // Include watch-only entries so the existing firstboot sampler
                // health input cannot claim healthy while any entry is absent.
                for path in config.sampler.mounts.iter().chain(&config.watch.mounts) {
                    if crate::mounts::live_identity(&mounts, path)?.is_none() {
                        let reason = format!(
                            "not mounted: {}",
                            tidemark_core::path::RawPath::from_path(path)
                                .map_err(|e| DaemonError::Config(e.to_string()))?
                                .display_lossy()
                        );
                        if !errors.contains(&reason) {
                            errors.push(reason);
                        }
                    }
                }
                for path in &config.sampler.mounts {
                    if shutdown.load(Ordering::Acquire) {
                        errors.push("measurement pass interrupted by shutdown".into());
                        break;
                    }
                    if identities.get(path) == Some(&None) {
                        continue;
                    }
                    if !due {
                        continue;
                    }
                    match collect(path, now, identities.get(path).copied().flatten()) {
                        Ok(s) => {
                            measurement_errors.remove(path);
                            state.publish(
                                Publication::FsAttempt {
                                    generation,
                                    mount: s.mount.clone(),
                                    sample: Some(s.clone()),
                                },
                                crate::component(
                                    "sampler",
                                    "degraded",
                                    "measurement pass in progress",
                                ),
                            )?;
                            // Measure every configured interval, but queue each whole-second
                            // storage key only once. Memory keeps the latest same-second fact.
                            if last_sample.get(path) != Some(&now) {
                                last_sample.insert(path.clone(), now);
                                rows.push(s);
                            }
                        }
                        Err(e) => {
                            state.publish(
                                Publication::FsAttempt {
                                    generation,
                                    mount: path.to_string_lossy().into_owned(),
                                    sample: None,
                                },
                                crate::component("sampler", "degraded", e.to_string()),
                            )?;
                            measurement_errors.insert(path.clone(), e.to_string());
                        }
                    }
                }
                if due {
                    next_sample = std::time::Instant::now() + config.sampler.interval;
                }
                if !rows.is_empty() && writer.try_send(super::tswriter::Message::Fs(rows)).is_err()
                {
                    errors.push("writer queue full or disconnected; persistence dropped".into());
                }
                errors.extend(measurement_errors.values().cloned());
                let health = if errors.is_empty() {
                    crate::component("sampler", "ok", "statvfs sampled")
                } else {
                    crate::component("sampler", "degraded", errors.join("; "))
                };
                state.publish_component(health)?;
                pass_finished();
                crate::wait_service(
                    &state,
                    generation,
                    &shutdown,
                    next_sample
                        .saturating_duration_since(std::time::Instant::now())
                        .min(std::time::Duration::from_secs(10)),
                );
            }
            Ok(())
        })
        .map_err(|source| DaemonError::Io {
            context: "spawn sampler".into(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_mount_recovery_waits_for_complete_pass_and_persisted_firstboot() {
        use std::time::Duration;
        use tidemark_live::alerts::AlertSource;
        let dir = test_support::private_tempdir().unwrap();
        let paths = vec![dir.path().join("a"), dir.path().join("b")];
        let mut cfg = Config::default();
        cfg.sampler.mounts = paths.clone();
        cfg.watch.mounts.clear();
        cfg.sampler.interval = Duration::from_millis(20);
        let state = LiveState::new(&cfg, 100);
        state
            .publish(
                Publication::Deployment {
                    config_sha256: "a".repeat(64),
                    started_at: 100,
                },
                crate::component("scheduler", "ok", "fixture"),
            )
            .unwrap();
        for name in ["api", "tswriter"] {
            state
                .publish_component(crate::component(name, "ok", "fixture"))
                .unwrap();
        }
        let table = paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                format!(
                    "{} 0 8:{} / {} rw - xfs /dev/fixture rw\n",
                    i + 1,
                    i,
                    path.display()
                )
            })
            .collect::<String>()
            .into_bytes();
        let (commands, responses) = crossbeam_channel::bounded(1);
        let (entered, waiting) = crossbeam_channel::bounded(1);
        let collect = Arc::new(move |path: &std::path::Path, now, _| {
            entered.send(path.to_path_buf()).unwrap();
            if responses.recv_timeout(Duration::from_secs(10)).unwrap() {
                Ok(FsSample {
                    ts: now,
                    mount: path.to_string_lossy().into_owned(),
                    size: 100,
                    used: 10,
                    avail: 90,
                    inodes: 10,
                    inodes_used: 1,
                })
            } else {
                Err(DaemonError::Invalid("injected B failure".into()))
            }
        });
        let (finished, passes) = crossbeam_channel::bounded(1);
        let (resume, resumed) = crossbeam_channel::bounded(1);
        let (tx, _rx) = crossbeam_channel::bounded(16);
        let stop = Shutdown::default();
        let worker = start_with_pass(
            Arc::new(cfg),
            state.clone(),
            tx,
            stop.clone(),
            Arc::new(|| 100),
            Arc::new(move || crate::mounts::parse(&table)),
            collect,
            Arc::new(move || {
                finished.send(()).unwrap();
                let _ = resumed.recv();
            }),
        )
        .unwrap();
        let health = || {
            state
                .view_at(100)
                .status(100)
                .unwrap()
                .components
                .into_iter()
                .find(|c| c.name == "sampler")
                .unwrap()
                .state
        };
        let mut followup = crate::firstboot::Followup::default();
        let mut elapsed = 0;
        let mut persisted = |healthy| {
            let inputs = state.view_at(100).firstboot_inputs().unwrap();
            followup.poll(dir.path(), &inputs, 100, elapsed).unwrap();
            elapsed += 10;
            let report: crate::firstboot::FirstbootReport = serde_json::from_slice(
                &std::fs::read(dir.path().join("deploy/firstboot.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(report.healthy, healthy);
        };
        for b_success in [false, false, true] {
            assert_eq!(
                waiting.recv_timeout(Duration::from_secs(10)).unwrap(),
                paths[0]
            );
            commands.send(true).unwrap();
            assert_eq!(
                waiting.recv_timeout(Duration::from_secs(10)).unwrap(),
                paths[1]
            );
            assert_ne!(health(), "ok");
            assert!(state
                .view_at(100)
                .fs_sample(&paths[0].to_string_lossy())
                .is_some());
            if !dir.path().join("deploy/firstboot.json").exists() {
                let inputs = state.view_at(100).firstboot_inputs().unwrap();
                crate::firstboot::write(dir.path(), &crate::firstboot::build(&inputs, 100))
                    .unwrap();
                std::fs::write(dir.path().join("deploy/staged.json"), b"{}").unwrap();
            }
            persisted(false);
            commands.send(b_success).unwrap();
            passes.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(health() == "ok", b_success);
            persisted(b_success);
            if b_success {
                stop.store(true, Ordering::Release);
            }
            resume.send(()).unwrap();
        }
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn emfile_collection_invalidates_firing_input_and_retains_history() {
        use tidemark_core::config::{AlertRule, AlertThreshold};
        use tidemark_live::alerts::{AlertEngine, AlertSource};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let mut cfg = Config::default();
        cfg.sampler.mounts = vec![path.clone()];
        cfg.watch.mounts.clear();
        cfg.sampler.interval = std::time::Duration::from_millis(20);
        cfg.alerts.rule = vec![AlertRule {
            name: "usage".into(),
            mount: Some(path.clone()),
            above: Some(AlertThreshold::Number(50.)),
            for_: std::time::Duration::ZERO,
            ..Default::default()
        }];
        let state = LiveState::new(&cfg, 100);
        let (tx, _rx) = crossbeam_channel::bounded(64);
        let (commands, collect_commands) = crossbeam_channel::bounded(1);
        let stop = Shutdown::default();
        let collect = Arc::new(move |path: &std::path::Path, _, _| {
            let success = collect_commands
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            if success {
                Ok(FsSample {
                    ts: 100,
                    mount: path.to_string_lossy().into_owned(),
                    size: 100,
                    used: 95,
                    avail: 5,
                    inodes: 1,
                    inodes_used: 0,
                })
            } else {
                Err(DaemonError::Io {
                    context: "injected collection".into(),
                    source: std::io::Error::from_raw_os_error(24),
                })
            }
        });
        let table = format!("1 0 8:0 / {} rw - xfs /dev/fixture rw\n", path.display()).into_bytes();
        let worker = start_with_sample(
            Arc::new(cfg.clone()),
            state.clone(),
            tx.clone(),
            stop.clone(),
            Arc::new(|| 100),
            Arc::new(move || crate::mounts::parse(&table)),
            collect,
        )
        .unwrap();
        let wait = |predicate: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !predicate() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
        };
        commands.send(true).unwrap();
        wait(&|| {
            state
                .view_at(100)
                .fs_sample(&path.to_string_lossy())
                .is_some()
        });
        let mut engine = AlertEngine::new(&cfg.alerts.rule, cfg.alerts.renotify).unwrap();
        let mut failures = Default::default();
        super::super::alerts::tick_current(&mut engine, 100, &state, &tx, &mut [], &mut failures)
            .unwrap();
        assert_eq!(engine.active().len(), 1);
        commands.send(false).unwrap();
        wait(&|| {
            state
                .view_at(100)
                .fs_sample(&path.to_string_lossy())
                .is_none()
        });
        assert!(state.view_at(100).fs_samples().is_some());
        super::super::alerts::tick_current(&mut engine, 100, &state, &tx, &mut [], &mut failures)
            .unwrap();
        assert!(!state.view_at(100).alert_diagnostics().unwrap().is_empty());
        commands.send(true).unwrap();
        wait(&|| {
            state
                .view_at(100)
                .fs_sample(&path.to_string_lossy())
                .is_some()
        });
        stop.store(true, Ordering::Release);
        // The service may already have dropped its receiver after observing shutdown.
        let _ = commands.send(true);
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn failed_measurement_survives_presence_poll_until_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mount");
        std::os::unix::fs::symlink("/", &path).unwrap();
        let mut cfg = Config::default();
        cfg.sampler.mounts = vec![path];
        cfg.watch.mounts.clear();
        cfg.sampler.interval = std::time::Duration::from_secs(60);
        let state = LiveState::new(&cfg, 100);
        state
            .publish(
                Publication::Deployment {
                    config_sha256: "a".repeat(64),
                    started_at: 100,
                },
                crate::component("scheduler", "ok", "fixture"),
            )
            .unwrap();
        for name in ["api", "tswriter"] {
            state
                .publish_component(crate::component(name, "ok", "fixture"))
                .unwrap();
        }
        let report =
            || crate::firstboot::build(&state.view_at(100).firstboot_inputs().unwrap(), 100);
        let (tx, _rx) = crossbeam_channel::bounded(16);
        let stop = Shutdown::default();
        let (finished, passes) = crossbeam_channel::bounded(1);
        let (resume, resumed) = crossbeam_channel::bounded(1);
        let table = Arc::new(parking_lot::Mutex::new(
            std::fs::read("/proc/self/mountinfo").unwrap(),
        ));
        let source = {
            let table = table.clone();
            Arc::new(move || crate::mounts::parse(&table.lock()))
        };
        let worker = start_with_pass(
            Arc::new(cfg.clone()),
            state.clone(),
            tx,
            stop.clone(),
            Arc::new(|| 100),
            source,
            Arc::new(sample_checked),
            Arc::new(move || {
                finished.send(()).unwrap();
                let _ = resumed.recv();
            }),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let health = || {
            state
                .view_at(100)
                .status(100)
                .unwrap()
                .components
                .into_iter()
                .find(|c| c.name == "sampler")
        };
        while !health().is_some_and(|c| c.state == "degraded") {
            assert!(
                std::time::Instant::now() < deadline,
                "health={:?}",
                health()
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(state.view_at(100).fs_samples().is_none());
        assert!(!report().healthy);
        passes
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        resume.send(()).unwrap();
        passes
            .recv_timeout(std::time::Duration::from_secs(15))
            .unwrap();
        assert_eq!(health().unwrap().state, "degraded"); // firstboot's sampler input
        assert!(!report().healthy);
        assert!(state.view_at(100).fs_samples().is_none());
        let raw = tidemark_core::path::RawPath::from_path(dir.path()).unwrap();
        let parsed = crate::mounts::parse(&table.lock()).unwrap();
        let actual = tidemark_fs::mount::mount_for_path(&parsed, &raw).unwrap();
        *table.lock() = format!(
            "{} 0 {}:{} / {} rw - xfs /dev/fixture rw\n",
            actual.id,
            actual.major,
            actual.minor,
            dir.path().display()
        )
        .into_bytes();
        cfg.sampler.mounts = vec![dir.path().to_path_buf()];
        state.replace_config(cfg).unwrap();
        resume.send(()).unwrap();
        passes
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        while !health().is_some_and(|c| c.state == "ok") {
            assert!(
                std::time::Instant::now() < deadline,
                "health={:?}",
                health()
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(state.view_at(100).fs_samples().is_some());
        assert!(report().healthy);
        stop.store(true, Ordering::Release);
        resume.send(()).unwrap();
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn injected_mount_appears_then_disappears_without_sampling_parent() {
        use std::sync::atomic::AtomicI64;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data");
        std::fs::create_dir(&path).unwrap();
        let actual = crate::mounts::read().unwrap();
        let raw = tidemark_core::path::RawPath::from_path(&path).unwrap();
        let actual = tidemark_fs::mount::mount_for_path(&actual, &raw).unwrap();
        let present = format!(
            "{} 0 {}:{} / {} rw - xfs /dev/fixture rw\n",
            actual.id,
            actual.major,
            actual.minor,
            path.display()
        )
        .into_bytes();
        let bytes = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let source = {
            let bytes = bytes.clone();
            Arc::new(move || crate::mounts::parse(&bytes.lock()))
        };
        let now = Arc::new(AtomicI64::new(100));
        let clock = {
            let now = now.clone();
            Arc::new(move || now.load(Ordering::Acquire))
        };
        let mut config = Config::default();
        config.sampler.mounts = vec![path.clone(), dir.path().join("absent")];
        config.sampler.interval = std::time::Duration::from_millis(20);
        config.watch.mounts.clear();
        let state = LiveState::new(&config, 100);
        let (tx, rx) = crossbeam_channel::bounded(16);
        let stop = Shutdown::default();
        let worker = start_with(
            Arc::new(config),
            state.clone(),
            tx,
            stop.clone(),
            clock,
            source,
        )
        .unwrap();
        let wait = |condition: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !condition() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
        };
        let missing_path = format!("not mounted: {}", path.display());
        let degraded = || {
            state
                .view_at(100)
                .status(100)
                .unwrap()
                .components
                .iter()
                .any(|c| {
                    c.name == "sampler" && c.state == "degraded" && c.detail.contains(&missing_path)
                })
        };
        wait(&degraded);
        assert!(state.view_at(100).fs_samples().is_none());
        assert!(rx.try_recv().is_err());
        now.store(101, Ordering::Release);
        *bytes.lock() = present;
        wait(&|| state.view_at(101).fs_samples().is_some());
        match rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap() {
            super::super::tswriter::Message::Fs(rows) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].ts, 101);
            }
            _ => panic!("unexpected persistence"),
        }
        bytes.lock().clear();
        now.store(102, Ordering::Release);
        wait(&degraded);
        assert!(state.view_at(102).fs_samples().is_none());
        assert!(rx.try_recv().is_err());
        stop.store(true, Ordering::Release);
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn descriptor_identity_is_checked_before_sampling_with_injected_clock() {
        let dir = tempfile::tempdir().unwrap();
        let table = crate::mounts::read().unwrap();
        let raw = tidemark_core::path::RawPath::from_path(dir.path()).unwrap();
        let mount = tidemark_fs::mount::mount_for_path(&table, &raw).unwrap();
        let intended = crate::mounts::Identity {
            id: mount.id,
            major: mount.major,
            minor: mount.minor,
        };
        let sample = sample_checked(dir.path(), 123, Some(intended)).unwrap();
        assert_eq!(sample.ts, 123);
        assert!(sample_checked(
            dir.path(),
            124,
            Some(crate::mounts::Identity {
                id: intended.id.wrapping_add(1),
                ..intended
            })
        )
        .is_err());
    }
}
