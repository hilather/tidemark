//! Owned by: W2W5-prep
//! Compile and inert-result coverage of live contracts.
#![allow(clippy::unwrap_used)] // Test fixture construction only.
use std::{sync::Arc, time::Duration};
use tidemark_core::{
    live::{AlertEvent, AlertState},
    path::RawPath,
};
use tidemark_live::{alerts::*, now::*, procscan::*, rates::*, watch::*};
#[test]
fn every_stub_has_an_inert_result() {
    let root = RawPath::new(b"/".to_vec()).unwrap();
    let cache = Arc::new(tidemark_fs::attribution::AttributionCache::new(
        Duration::ZERO,
        0,
    ));
    let opts = WatcherOptions {
        mount: root.clone(),
        events: vec![],
        hot_depth: 0,
        exclude_prefixes: vec![],
        lru_entries: 0,
    };
    assert!(matches!(
        Watcher::start(opts, cache.clone()),
        Err(WatchError::NotYetImplemented)
    ));
    assert_eq!(
        namespace_ok().err().map(|e| e.kind()),
        Some(std::io::ErrorKind::Unsupported)
    );
    let watcher = Watcher;
    assert!(matches!(watcher.mode(), WatchMode::FdMount));
    for scope in [HotScope::Dir, HotScope::Unit, HotScope::Comm] {
        assert!(watcher.hot(0, scope, 0).is_empty());
    }
    assert!(watcher.drain_completed_minutes(0).is_empty());
    assert!(watcher.take_dirty(&root).is_empty());
    let stats = watcher.stats();
    assert!(matches!(stats.mode, WatchMode::FdMount));
    assert_eq!(
        (
            stats.events_total,
            stats.overflows_total,
            stats.lossy_minutes_15m,
            stats.dropped_keys
        ),
        (0, 0, 0, 0)
    );
    watcher.stop();
    let mut proc = ProcScanner::new(
        ProcScannerOptions {
            mounts: vec![],
            open_writers: false,
            exclude_prefixes: vec![],
        },
        cache,
    );
    assert_eq!(
        proc.scan_once(0).err().map(|e| e.kind()),
        Some(std::io::ErrorKind::Unsupported)
    );
    assert!(proc.phantom(None).is_empty());
    assert!(proc.writers(None, Duration::ZERO, 0).is_empty());
    let (phantom, writers) = proc.take_samples();
    assert!(phantom.is_empty() && writers.is_empty());
    assert!(slope_bytes_per_sec(&[]).is_none());
    assert!(window_rate(&[], 0, Duration::ZERO).is_none());
    assert!(time_to_full(0, 1.0).is_none());
    assert!(matches!(
        AlertEngine::new(&[], Duration::ZERO),
        Err(AlertError::NotYetImplemented)
    ));
    let mut engine = AlertEngine;
    let inputs = AlertInputs {
        mounts: &[],
        lvm: &[],
        phantom: &[],
        hot_dirs_window: &[],
        pg: None,
        scans: &[],
        dir_growth: &[],
        watch_overflows_15m: &[],
        state_space_refused: false,
    };
    assert!(engine.evaluate(0, &inputs).is_empty());
    assert!(engine.active().is_empty());
    let ev = AlertEvent {
        ts: 0,
        rule: String::new(),
        subject: String::new(),
        state: AlertState::Firing,
        value: None,
        message: String::new(),
    };
    assert!(matches!(
        JournalDeliver.deliver(&ev),
        Err(DeliverError::NotYetImplemented)
    ));
    assert!(matches!(
        WebhookDeliver.deliver(&ev),
        Err(DeliverError::NotYetImplemented)
    ));
    let _ = default_rules(&tidemark_core::config::Config::default());
    let inputs = NowInputs {
        host: "",
        mounts: &[],
        lvm: &[],
        hot_dirs_5m: &[],
        writers_5m: &[],
        phantom: &[],
        open_writers: &[],
        pg: None,
        alerts: &[],
        scans: &[],
        watch_modes: &[],
        degraded: &[],
    };
    let now = build_now(&inputs, 123);
    assert_eq!(now.generated_at, 0);
    assert!(
        now.host.is_empty()
            && now.mounts.is_empty()
            && now.pg.is_none()
            && now.alerts_firing.is_empty()
            && now.scans.is_empty()
            && now.degraded.is_empty()
    );
    assert!(render_now_text(&now, 80, false).is_empty());
}
