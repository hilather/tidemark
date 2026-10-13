//! Owned by: W6-A1
//! Read-only LVM reporting with bounded retry backoff.

use crate::{
    state::{LiveState, Publication},
    DaemonError, ServiceHandle, Shutdown,
};
use std::{sync::atomic::Ordering, sync::Arc, time::Duration};
use tidemark_core::{api::LvmResult, config::Config};
/// Retry from max(interval, five minutes), doubling to max(interval, one hour).
pub fn retry_delay(failures: u32, interval: Duration) -> Duration {
    interval
        .max(Duration::from_secs(300))
        .saturating_mul(1u32 << failures.saturating_sub(1).min(31))
        .min(interval.max(Duration::from_secs(3600)))
}
fn collection_delay(success: bool, failures: &mut u32, interval: Duration) -> Duration {
    if success {
        *failures = 0;
        interval
    } else {
        *failures = failures.saturating_add(1);
        retry_delay(*failures, interval)
    }
}
/// Start LVM reporting. Disabled LVM performs no reporting command.
/// Failed collections preserve latest successful data and report component errors.
pub fn start(
    _config: Arc<Config>,
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
) -> Result<ServiceHandle, DaemonError> {
    start_worker(
        state,
        writer,
        shutdown,
        |config, now| tidemark_fs::lvm::collect(&config.lvm, now).map_err(|e| e.to_string()),
        tidemark_fs::mount::read_mountinfo,
    )
}
fn start_worker<C, M>(
    state: LiveState,
    writer: super::tswriter::Sender,
    shutdown: Shutdown,
    collect: C,
    mounts: M,
) -> Result<ServiceHandle, DaemonError>
where
    C: Fn(&Config, i64) -> Result<tidemark_fs::lvm::LvmReport, String> + Send + 'static,
    M: Fn() -> std::io::Result<Vec<tidemark_fs::mount::MountInfo>> + Send + 'static,
{
    std::thread::Builder::new()
        .name("lvm".into())
        .spawn(move || {
            let mut failures = 0u32;
            while !shutdown.load(Ordering::Acquire) {
                let (config, generation) = state.config();
                if !config.lvm.enabled {
                    state.publish_component(crate::component(
                        "lvm",
                        "disabled",
                        "configuration",
                    ))?;
                    crate::wait_service(&state, generation, &shutdown, config.lvm.interval);
                    continue;
                }
                let result = collect(&config, crate::unix_now()).and_then(|report| {
                    let mounts = mounts().map_err(|e| e.to_string())?;
                    let mounts: Vec<_> = mounts
                        .into_iter()
                        .filter(|m| {
                            config
                                .sampler
                                .mounts
                                .iter()
                                .any(|p| p == m.mount_point.as_path())
                        })
                        .collect();
                    let tracked = tidemark_fs::lvm::map_mounts(&report, &mounts);
                    let samples = tidemark_fs::lvm::to_samples(&report, &tracked);
                    state
                        .publish(
                            Publication::Lvm(LvmResult {
                                mounts: tracked
                                    .iter()
                                    .map(|m| {
                                        (
                                            m.mount.as_path().to_string_lossy().into_owned(),
                                            tidemark_fs::lvm::lvm_now(&report, m),
                                        )
                                    })
                                    .collect(),
                                vgs: report.vgs,
                                sampled_at: report.sampled_at,
                            }),
                            crate::component("lvm", "ok", "report collected"),
                        )
                        .map_err(|e| e.to_string())?;
                    if writer
                        .try_send(super::tswriter::Message::Lvm(samples))
                        .is_err()
                    {
                        state
                            .publish_component(crate::component(
                                "lvm",
                                "degraded",
                                "writer queue full or disconnected; persistence dropped",
                            ))
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(())
                });
                let delay = match result {
                    Ok(()) => collection_delay(true, &mut failures, config.lvm.interval),
                    Err(error) => {
                        state.publish_component(crate::component("lvm", "error", error))?;
                        collection_delay(false, &mut failures, config.lvm.interval)
                    }
                };
                crate::wait_service(&state, generation, &shutdown, delay);
            }
            Ok(())
        })
        .map_err(|source| DaemonError::Io {
            context: "spawn lvm".into(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    fn wait(mut predicate: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !predicate() {
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
    }
    #[test]
    fn retry_sequence_custom_intervals_and_success_reset() {
        let interval = Duration::from_secs(300);
        let mut failures = 0;
        for seconds in [300, 600, 1200, 2400, 3600, 3600] {
            assert_eq!(
                collection_delay(false, &mut failures, interval).as_secs(),
                seconds
            );
        }
        assert_eq!(collection_delay(true, &mut failures, interval), interval);
        assert_eq!(failures, 0);
        assert_eq!(collection_delay(false, &mut failures, interval), interval);
        for (interval, expected) in [
            (1, vec![300, 600, 1200, 2400, 3600]),
            (900, vec![900, 1800, 3600, 3600, 3600]),
            (7200, vec![7200; 5]),
        ] {
            for (n, seconds) in expected.into_iter().enumerate() {
                assert_eq!(
                    retry_delay(n as u32 + 1, Duration::from_secs(interval)).as_secs(),
                    seconds
                );
            }
        }
    }
    #[test]
    fn full_queue_does_not_recollect() {
        let mut cfg = Config::default();
        cfg.lvm.enabled = true;
        cfg.lvm.interval = Duration::from_secs(3600);
        let state = LiveState::new(&cfg, 0);
        let (tx, _rx) = crossbeam_channel::bounded(1);
        tx.send(super::super::tswriter::Message::Lvm(vec![]))
            .unwrap();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = count.clone();
        let stop = Shutdown::default();
        let handle = start_worker(
            state.clone(),
            tx,
            stop.clone(),
            move |_, now| {
                calls.fetch_add(1, Ordering::AcqRel);
                Ok(tidemark_fs::lvm::LvmReport {
                    lvs: vec![],
                    vgs: vec![],
                    sampled_at: now,
                })
            },
            || Ok(vec![]),
        )
        .unwrap();
        wait(|| {
            state
                .view()
                .status(0)
                .unwrap()
                .components
                .iter()
                .any(|c| c.name == "lvm" && c.state == "degraded")
        });
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(count.load(Ordering::Acquire), 1);
        stop.store(true, Ordering::Release);
        handle.join().unwrap().unwrap();
    }
    #[test]
    fn fixture_reports_and_failures() {
        let mut cfg = Config::default();
        cfg.sampler.mounts = vec!["/fixture".into()];
        cfg.lvm.interval = Duration::from_secs(3600);
        let state = LiveState::new(&cfg, 0);
        let (tx, rx) = crossbeam_channel::bounded(1);
        let shutdown = Shutdown::default();
        let handle = start_worker(
            state.clone(),
            tx,
            shutdown.clone(),
            |_, now| {
                Ok(tidemark_fs::lvm::LvmReport {
                    lvs: tidemark_fs::lvm::parse_lvs_json(include_bytes!(
                        "../../../test-support/fixtures/lvm/lvs.json"
                    ))
                    .unwrap(),
                    vgs: tidemark_fs::lvm::parse_vgs_json(include_bytes!(
                        "../../../test-support/fixtures/lvm/vgs.json"
                    ))
                    .unwrap(),
                    sampled_at: now,
                })
            },
            || {
                Ok(tidemark_fs::mount::parse_mountinfo(
                    b"10 1 253:0 / /fixture rw - xfs /dev/mapper/fixture rw\n",
                )
                .unwrap())
            },
        )
        .unwrap();
        wait(|| {
            state
                .view()
                .lvm_report(&tidemark_core::api::LvmParams { mount: None })
                .is_some()
        });
        let report = state
            .view()
            .lvm_report(&tidemark_core::api::LvmParams { mount: None })
            .unwrap();
        assert_eq!(report.mounts[0].1.lv, "root");
        assert_eq!(report.vgs[0].vg, "vg-with-hyphens");
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            super::super::tswriter::Message::Lvm(_)
        ));
        shutdown.store(true, Ordering::Release);
        handle.join().unwrap().unwrap();
        let (tx, _) = crossbeam_channel::bounded(1);
        let shutdown = Shutdown::default();
        let handle = start_worker(
            state.clone(),
            tx,
            shutdown.clone(),
            |_, _| Err("fixture collection failed".into()),
            || Ok(vec![]),
        )
        .unwrap();
        wait(|| {
            state
                .view()
                .status(1)
                .unwrap()
                .components
                .iter()
                .any(|c| c.name == "lvm" && c.state == "error")
        });
        assert!(state
            .view()
            .lvm_report(&tidemark_core::api::LvmParams { mount: None })
            .is_some());
        shutdown.store(true, Ordering::Release);
        handle.join().unwrap().unwrap();
    }
}
