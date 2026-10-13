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
    crate::state::validate_bounds(&config)?;
    std::thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            let mut last_sample = std::collections::BTreeMap::new();
            let mut identities = std::collections::BTreeMap::new();
            let mut next_sample = std::time::Instant::now();
            let mut previous_generation = None;
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
                let mounts = match mount_source() {
                    Ok(mounts) => mounts,
                    Err(error) => {
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
                crate::mounts::log(&changes, "sampler");
                for change in &changes {
                    last_sample.remove(&change.path);
                    state.reset_fs_mount(generation, &change.path.to_string_lossy())?;
                }
                if !changes.is_empty() {
                    next_sample = std::time::Instant::now();
                }
                let due = std::time::Instant::now() >= next_sample;
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
                        break;
                    }
                    if identities.get(path) == Some(&None) {
                        continue;
                    }
                    if !due {
                        continue;
                    }
                    match sample_checked(path, now, identities.get(path).copied().flatten()) {
                        Ok(s) => {
                            state.publish(
                                Publication::FsAttempt {
                                    generation,
                                    mount: s.mount.clone(),
                                    sample: Some(s.clone()),
                                },
                                if errors.is_empty() {
                                    crate::component("sampler", "ok", "statvfs sampled")
                                } else {
                                    crate::component("sampler", "degraded", errors.join("; "))
                                },
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
                            errors.push(e.to_string());
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
                let health = if errors.is_empty() {
                    crate::component("sampler", "ok", "statvfs sampled")
                } else {
                    crate::component("sampler", "degraded", errors.join("; "))
                };
                state.publish_component(health)?;
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
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
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
        match rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
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
