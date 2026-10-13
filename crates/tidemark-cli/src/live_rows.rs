//! Owned by: W7-A2-live
//! Shared gated metadata-only live rescans for top and the browser.

/// Rescan a directory without publication, using the gentle gate and caller cancellation.
pub(crate) fn live_rows(
    path: &tidemark_core::path::RawPath,
    config: &tidemark_core::config::Config,
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<Vec<tidemark_core::snapshot::DirRow>> {
    check_cancel(cancel)?;
    use tidemark_scan::agg::{AggConfig, Aggregator, VecSink};
    let _lock = tidemark_scan::orchestrator::ScanLock::acquire(&config.general.state_dir)
        .map_err(crate::cmd::scan::scan_error)?;
    let mounts = tidemark_fs::mount::read_mountinfo()?;
    let mount = tidemark_fs::mount::mount_for_path(&mounts, path)
        .ok_or_else(|| crate::CliError::Precondition("live root mount unknown".into()))?;
    let leaves = tidemark_fs::blockdev::resolve_leaves(mount.dev).unwrap_or_default();
    let sample_leaves = leaves.clone();
    let scan_gate = tidemark_fs::throttle::gate_from_config(
        "gentle",
        &config.throttle,
        &leaves,
        Box::new(move || tidemark_fs::blockdev::read_disk_stats(&sample_leaves)),
    )?;
    let gate = scan_gate.gate();
    let root = config
        .roots
        .iter()
        .find(|r| r.path == path.as_path())
        .cloned()
        .unwrap_or_default();
    let now = tidemark_core::time::unix_now();
    let threads = config.scan.threads as usize;
    let capacity = tidemark_scan::orchestrator::channel_capacity(
        config.scan.memory_budget.0,
        threads,
        root.top_files,
        config.scan.getdents_buf.0,
    )?;
    let mut globs = globset::GlobSetBuilder::new();
    for g in &root.exclude {
        globs.add(globset::Glob::new(g)?);
    }
    let mut skips = tidemark_fs::mount::submounts_under(&mounts, path);
    let state = tidemark_fs::canon::canonicalize(&config.general.state_dir)?;
    if &state == path {
        return Err(crate::CliError::Precondition("cannot scan state directory".into()).into());
    }
    if state.starts_with(path) {
        skips.push(state);
    }
    let cfg = tidemark_fs::walk::WalkConfig {
        root: path.clone(),
        threads,
        stat_entries: true,
        exclude: globs.build()?,
        skip_mounts: skips,
        big_file_threshold: root.big_file_threshold.0,
        top_files: root.top_files,
        scan_start: now,
        getdents_buf: usize::try_from(config.scan.getdents_buf.0)?,
        io_idle: true,
        max_record_bytes: tidemark_scan::orchestrator::record_limits(
            config.scan.memory_budget.0,
            threads,
        )?
        .1,
    };
    let mut agg = Aggregator::new(AggConfig {
        root: path.clone(),
        scan_start: now,
        top_files: root.top_files,
        big_file_threshold: root.big_file_threshold.0,
        max_notable: config.scan.max_notable,
        link_instances: false,
        max_stored_errors: config.scan.max_stored_errors,
    });
    let walked = std::thread::scope(|scope| {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        let worker = scope.spawn(|| tidemark_fs::walk::walk(&cfg, gate.as_ref(), cancel, tx));
        loop {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                scan_gate.cancel();
            }
            let record = match rx.recv_timeout(std::time::Duration::from_millis(20)) {
                Ok(record) => record,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            };
            match record {
                tidemark_core::scan::WalkOutput::Dir(d) => agg.add_dir_record(d),
                tidemark_core::scan::WalkOutput::Gone(g) => agg.add_gone(&g),
                _ => {}
            }
        }
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("live walker panicked"))?
            .map_err(anyhow::Error::from)
    });
    scan_gate.stop();
    walked?;
    check_cancel(cancel)?;
    let mut sink = VecSink::default();
    let prepared = agg.prepare()?;
    check_cancel(cancel)?;
    prepared.write(&mut sink)?;
    check_cancel(cancel)?;
    Ok(sink.dirs)
}

fn check_cancel(cancel: &std::sync::atomic::AtomicBool) -> anyhow::Result<()> {
    if cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err(tidemark_fs::walk::WalkError::Cancelled.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tidemark_core::{config::Config, path::RawPath};

    fn config(state: &std::path::Path) -> Config {
        let mut config = Config::default();
        config.general.state_dir = state.to_owned();
        config.general.socket = state.join("absent.sock");
        config.throttle.sample_interval = std::time::Duration::from_millis(20);
        config
    }

    #[test]
    fn fixture_rows_match_top_disk_oracle() {
        let fixture = test_support::materialize(
            r#"
[[entry]]
path = "a/file"
kind = "file"
size = 8192
[[entry]]
path = "b/empty"
kind = "file"
size = 0
"#,
        );
        let state = tempfile::tempdir().unwrap();
        let path = RawPath::from_path(&fixture.root).unwrap();
        let rows = live_rows(&path, &config(state.path()), &AtomicBool::new(false)).unwrap();
        assert_eq!(rows.len(), 3);
        for row in rows {
            assert_eq!(
                row.t_disk,
                test_support::oracle::du_disk(row.path.as_path())
            );
        }
    }

    #[test]
    fn symlink_to_outside_directory_is_not_followed() {
        let fixture = test_support::materialize(
            r#"
[[entry]]
path = "link"
kind = "symlink"
target = "../outside"
[[outside]]
path = "large"
kind = "file"
size = 1048576
"#,
        );
        let state = tempfile::tempdir().unwrap();
        let path = RawPath::from_path(&fixture.root).unwrap();
        let rows = live_rows(&path, &config(state.path()), &AtomicBool::new(false)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].t_files, 1); // The link itself is a non-directory entry.
        assert_eq!(rows[0].t_disk, test_support::oracle::du_disk(&fixture.root));
    }

    #[test]
    fn preset_cancellation_returns_cancelled_before_state_io() {
        let cancel = AtomicBool::new(true);
        let error = live_rows(
            &RawPath::from_path(std::path::Path::new("/absent")).unwrap(),
            &Config::default(),
            &cancel,
        )
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<tidemark_fs::walk::WalkError>(),
            Some(tidemark_fs::walk::WalkError::Cancelled)
        ));
    }

    #[test]
    fn cancellation_wakes_throttled_scan_and_releases_lock() {
        let fixture = test_support::materialize(
            r#"
[[entry]]
path = "a/file"
kind = "file"
size = 8192
"#,
        );
        let state = tempfile::tempdir().unwrap();
        let mut config = config(state.path());
        config.throttle.min_ops_per_sec = 1;
        config.throttle.max_ops_per_sec = 1;
        let cancel = AtomicBool::new(false);
        let path = RawPath::from_path(&fixture.root).unwrap();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| live_rows(&path, &config, &cancel));
            std::thread::sleep(std::time::Duration::from_millis(100));
            let start = std::time::Instant::now();
            cancel.store(true, Ordering::Release);
            let error = worker.join().unwrap().unwrap_err();
            assert!(matches!(
                error.downcast_ref::<tidemark_fs::walk::WalkError>(),
                Some(tidemark_fs::walk::WalkError::Cancelled)
            ));
            assert!(start.elapsed() < std::time::Duration::from_secs(2));
        });
        let _lock = tidemark_scan::orchestrator::ScanLock::acquire(state.path()).unwrap();
    }
}
