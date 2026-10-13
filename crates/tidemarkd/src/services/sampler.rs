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
    crate::state::validate_bounds(&config)?;
    std::thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            let mut last_sample = std::collections::BTreeMap::new();
            let mut identities = std::collections::BTreeMap::new();
            let mut next_sample = std::time::Instant::now();
            while !shutdown.load(Ordering::Acquire) {
                let (config, generation) = state.config();
                let now = crate::unix_now();
                let mut rows = Vec::new();
                let mut errors = Vec::new();
                last_sample.retain(|path, _| config.sampler.mounts.contains(path));
                let mounts = match crate::mounts::read() {
                    Ok(mounts) => mounts,
                    Err(error) => {
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
                let changes =
                    crate::mounts::reconcile(&config.sampler.mounts, &mut identities, &mounts)?;
                crate::mounts::log(&changes, "sampler");
                for change in &changes {
                    last_sample.remove(&change.path);
                }
                if !changes.is_empty() {
                    next_sample = std::time::Instant::now();
                }
                let due = std::time::Instant::now() >= next_sample;
                // Include watch-only entries so the existing firstboot sampler
                // health input cannot claim healthy while any entry is absent.
                for path in config
                    .watch
                    .mounts
                    .iter()
                    .filter(|p| !config.sampler.mounts.contains(p))
                {
                    if crate::mounts::identity(&mounts, path)?.is_none() {
                        errors.push(format!("not mounted: {}", path.display()));
                    }
                }
                for path in &config.sampler.mounts {
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    if identities.get(path) == Some(&None) {
                        errors.push(format!("not mounted: {}", path.display()));
                        continue;
                    }
                    if !due {
                        continue;
                    }
                    match sample(path, now) {
                        Ok(s) => {
                            state.publish(
                                Publication::Fs(s.clone()),
                                crate::component("sampler", "ok", "statvfs sampled"),
                            )?;
                            // Measure every configured interval, but queue each whole-second
                            // storage key only once. Memory keeps the latest same-second fact.
                            if last_sample.get(path) != Some(&now) {
                                last_sample.insert(path.clone(), now);
                                rows.push(s);
                            }
                        }
                        Err(e) => errors.push(e.to_string()),
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
