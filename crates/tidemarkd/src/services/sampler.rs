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
            while !shutdown.load(Ordering::Acquire) {
                let (config, generation) = state.config();
                let now = crate::unix_now();
                let mut rows = Vec::new();
                let mut errors = Vec::new();
                last_sample.retain(|path, _| config.sampler.mounts.contains(path));
                for path in &config.sampler.mounts {
                    // The frozen storage key has whole-second precision. Reloads and
                    // subsecond intervals must never insert the same mount/second twice.
                    if last_sample.get(path) == Some(&now) {
                        continue;
                    }
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    match sample(path, now) {
                        Ok(s) => {
                            last_sample.insert(path.clone(), now);
                            state.publish(
                                Publication::Fs(s.clone()),
                                crate::component("sampler", "ok", "statvfs sampled"),
                            )?;
                            rows.push(s);
                        }
                        Err(e) => errors.push(e.to_string()),
                    }
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
                crate::wait_service(&state, generation, &shutdown, config.sampler.interval);
            }
            Ok(())
        })
        .map_err(|source| DaemonError::Io {
            context: "spawn sampler".into(),
            source,
        })
}
