//! Owned by: W5-D-fix
//! Rule-source regressions and named hysteresis transition cells.
use super::*;
use tidemark_core::{
    config::{AlertKind, AlertThreshold},
    live::AlertState,
};
#[derive(Default)]
struct Source {
    used: Option<u64>,
    pools: Option<Vec<LvmNow>>,
    growth: Option<Vec<(RawPath, i64)>>,
    cadence: Option<Duration>,
    missing: bool,
}
impl AlertSource for Source {
    fn fs_sample(&self, mount: &str) -> Option<FsSample> {
        self.used.map(|used| FsSample {
            ts: 0,
            mount: mount.into(),
            size: 100,
            used,
            avail: 100 - used,
            inodes: 1,
            inodes_used: 0,
        })
    }
    fn fs_rate(&self, _: &str, w: Duration) -> Option<f64> {
        (!self.missing).then_some(if w.as_secs() == 300 { 2.0 } else { 0.0 })
    }
    fn hot_dirs(&self, _: &str, w: Duration) -> Option<Vec<HotEntry>> {
        (!self.missing).then(|| {
            vec![HotEntry {
                key: b"/data/log".to_vec(),
                events: 1,
                files: 1,
                bytes_delta: if w.as_secs() == 300 { 0 } else { 3600 },
                top_unit: None,
                lossy: false,
            }]
        })
    }
    fn watch_overflows(&self, _: &str, w: Duration) -> Option<u64> {
        (!self.missing).then_some(if w.as_secs() == 300 { 11 } else { 0 })
    }
    fn phantom_bytes(&self, _: &str) -> Option<u64> {
        (!self.missing).then_some(100)
    }
    fn lvm(&self) -> Option<Vec<LvmNow>> {
        self.pools.clone()
    }
    fn dir_growth(&self, _: &str, _: u32) -> Option<Vec<(RawPath, i64)>> {
        self.growth.clone()
    }
    fn scan_age(&self, _: &str) -> Option<ScanAge> {
        (!self.missing).then_some(ScanAge {
            last_complete_at: Some(0),
            effective_cadence: self.cadence,
        })
    }
}
fn cfg(kind: AlertKind) -> AlertRule {
    use AlertKind::*;
    let mut r = AlertRule {
        name: format!("{kind:?}"),
        kind,
        for_: Duration::ZERO,
        resolve_for: Duration::from_secs(10),
        ..Default::default()
    };
    match kind {
        FsUsedPct | HotDirGrowth | PhantomBytes | WatchLossy => {
            r.mount = Some("/data".into());
            r.above = Some(AlertThreshold::Number(10.0));
        }
        TimeToFull => {
            r.mount = Some("/data".into());
            r.below = Some("1h".into());
        }
        ThinPoolPct => {
            r.data_above = Some(80.0);
            r.meta_above = Some(80.0);
        }
        VgFreeLow => {
            r.vg = Some("*".into());
            r.below = Some("1B".into());
        }
        DirGrowth => {
            r.root = Some("/data".into());
            r.depth = Some(1);
            r.above = Some(AlertThreshold::Number(10.0));
        }
        ScanStale => {
            r.root = Some("/data".into());
            r.older_than = Some("auto".into());
        }
    }
    if matches!(kind, TimeToFull | HotDirGrowth | WatchLossy) {
        r.window = Some("5m".into());
    }
    if kind == HotDirGrowth {
        r.path = Some("/data/*".into());
        r.above = Some(AlertThreshold::Number(0.5));
    }
    r
}
fn evaluate(e: &mut AlertEngine, now: i64, source: &Source) -> Vec<AlertEvent> {
    e.evaluate(
        now,
        &AlertInputs {
            source,
            state_space_refused: false,
        },
    )
}
fn engine(r: AlertRule) -> AlertEngine {
    AlertEngine::new(&[r], Duration::from_secs(10)).unwrap()
}
fn pool(data: Option<f64>, meta: Option<f64>) -> LvmNow {
    LvmNow {
        vg: "vg".into(),
        lv: "lv".into(),
        lv_size: 100,
        vg_free: 0,
        thin_pool: Some("pool".into()),
        data_pct: data,
        meta_pct: meta,
        sampled_at: 0,
    }
}
#[test]
fn missing_pool_percentages_never_resolve() {
    for (data, meta) in [
        (None, Some(10.0)),
        (Some(10.0), None),
        (None, None),
        (Some(f64::NAN), Some(10.0)),
    ] {
        let mut e = engine(cfg(AlertKind::ThinPoolPct));
        let mut s = Source {
            pools: Some(vec![pool(Some(90.0), Some(10.0))]),
            ..Default::default()
        };
        assert_eq!(evaluate(&mut e, 0, &s)[0].state, AlertState::Firing);
        s.pools = Some(vec![
            pool(data, meta),
            LvmNow {
                vg: "other".into(),
                ..pool(Some(0.0), Some(0.0))
            },
        ]);
        assert!(evaluate(&mut e, 1, &s).is_empty());
        assert!(evaluate(&mut e, 100, &s).is_empty());
        assert_eq!(e.active().len(), 1);
        assert_eq!(e.diagnostics().len(), 1);
        s.pools = Some(vec![]);
        evaluate(&mut e, 101, &s);
        assert_eq!(evaluate(&mut e, 111, &s)[0].state, AlertState::Resolved);
    }
}
#[test]
fn healthy_subjects_do_not_hide_breach() {
    let mut s = Source {
        growth: Some(
            (0..10_000)
                .map(|n| {
                    (
                        RawPath::new(format!("/data/{n:05}").into_bytes()).unwrap(),
                        0,
                    )
                })
                .collect(),
        ),
        ..Default::default()
    };
    s.growth
        .as_mut()
        .unwrap()
        .push((RawPath::new(b"/data/z".to_vec()).unwrap(), 11));
    let mut e = engine(cfg(AlertKind::DirGrowth));
    assert_eq!(evaluate(&mut e, 0, &s).len(), 1);
    assert_eq!(evaluate(&mut e, 10, &s)[0].state, AlertState::Renotify);
    assert_eq!(e.active().len(), 1);
    s.growth = Some(vec![]);
    evaluate(&mut e, 11, &s);
    assert_eq!(evaluate(&mut e, 21, &s)[0].state, AlertState::Resolved);
}
#[test]
fn capacity_is_reported() {
    let s = Source {
        growth: Some(
            (0..10_001)
                .map(|n| (RawPath::new(format!("/data/{n}").into_bytes()).unwrap(), 11))
                .collect(),
        ),
        ..Default::default()
    };
    let mut e = engine(cfg(AlertKind::DirGrowth));
    assert_eq!(evaluate(&mut e, 0, &s).len(), 10_000);
    assert_eq!(e.states.len(), 10_000);
    assert!(e.diagnostics()[0].reason.contains("capacity"));
}
#[test]
fn arbitrary_windows_and_auto_cadence() {
    let s = Source {
        used: Some(95),
        cadence: Some(Duration::from_secs(86_400)),
        ..Default::default()
    };
    for kind in [AlertKind::TimeToFull, AlertKind::WatchLossy] {
        let mut e = engine(cfg(kind));
        assert_eq!(evaluate(&mut e, 0, &s).len(), 1);
        assert!(e.diagnostics().is_empty());
    }
    let mut e = engine(cfg(AlertKind::ScanStale));
    assert!(evaluate(&mut e, 172_800, &s).is_empty());
    assert_eq!(evaluate(&mut e, 172_801, &s).len(), 1);
    let mut e = engine(cfg(AlertKind::ScanStale));
    evaluate(&mut e, 999_999, &Source::default());
    assert!(e.diagnostics()[0].reason.contains("cadence"));
}
#[test]
fn hot_windows_use_distinct_histories() {
    let short = cfg(AlertKind::HotDirGrowth);
    let mut long = short.clone();
    long.name = "hour".into();
    long.window = Some("1h".into());
    let mut e = AlertEngine::new(&[short, long], Duration::from_secs(10)).unwrap();
    let ev = evaluate(&mut e, 0, &Source::default());
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].rule, "hour");
    assert_eq!(ev[0].value, Some(1.0));
}
#[test]
fn all_nine_rules_and_unavailable_reasons() {
    use AlertKind::*;
    for kind in [
        FsUsedPct,
        TimeToFull,
        HotDirGrowth,
        ThinPoolPct,
        VgFreeLow,
        DirGrowth,
        PhantomBytes,
        ScanStale,
        WatchLossy,
    ] {
        let mut e = engine(cfg(kind));
        assert!(evaluate(
            &mut e,
            999_999,
            &Source {
                missing: true,
                ..Default::default()
            }
        )
        .is_empty());
        assert_eq!(e.diagnostics().len(), 1, "{kind:?}");
    }
}
fn state(pending: Option<i64>, firing: Option<i64>, clearing: Option<i64>) -> State {
    State {
        pending,
        firing,
        clearing,
        notified: 0,
        value: 1.0,
        paused: false,
    }
}
fn advance(s: &mut State, input: bool, now: i64) -> Option<AlertState> {
    step(
        s,
        input,
        now,
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(10),
    )
}
#[test]
fn inactive_false() {
    let mut s = state(None, None, None);
    assert_eq!(advance(&mut s, false, 0), None);
    assert!(s.pending.is_none());
}
#[test]
fn inactive_true() {
    let mut s = state(None, None, None);
    assert_eq!(advance(&mut s, true, 0), None);
    assert_eq!(s.pending, Some(0));
}
#[test]
fn pending_false() {
    let mut s = state(Some(0), None, None);
    advance(&mut s, false, 1);
    assert!(s.pending.is_none());
}
#[test]
fn pending_true() {
    let mut s = state(Some(0), None, None);
    assert_eq!(advance(&mut s, true, 4), None);
    assert_eq!(advance(&mut s, true, 5), Some(AlertState::Firing));
}
#[test]
fn firing_true() {
    let mut s = state(None, Some(0), None);
    assert_eq!(advance(&mut s, true, 9), None);
    assert_eq!(advance(&mut s, true, 10), Some(AlertState::Renotify));
}
#[test]
fn firing_false() {
    let mut s = state(None, Some(0), None);
    advance(&mut s, false, 1);
    assert_eq!(s.clearing, Some(1));
}
#[test]
fn clearing_true() {
    let mut s = state(None, Some(0), Some(1));
    advance(&mut s, true, 2);
    assert!(s.clearing.is_none());
    assert_eq!(s.firing, Some(0));
}
#[test]
fn clearing_false() {
    let mut s = state(None, Some(0), Some(1));
    assert_eq!(advance(&mut s, false, 10), Some(AlertState::Renotify));
    assert_eq!(advance(&mut s, false, 11), Some(AlertState::Resolved));
}
#[test]
fn resolution_precedes_renotify() {
    let mut s = state(None, Some(0), Some(0));
    assert_eq!(advance(&mut s, false, 10), Some(AlertState::Resolved));
}
fn unknown_cell(pending: bool, firing: bool, clearing: bool) {
    let mut r = cfg(AlertKind::FsUsedPct);
    r.for_ = Duration::from_secs(5);
    let mut e = engine(r);
    if pending || firing {
        evaluate(
            &mut e,
            0,
            &Source {
                used: Some(99),
                ..Default::default()
            },
        );
    }
    if firing {
        evaluate(
            &mut e,
            5,
            &Source {
                used: Some(99),
                ..Default::default()
            },
        );
    }
    if clearing {
        evaluate(
            &mut e,
            6,
            &Source {
                used: Some(0),
                ..Default::default()
            },
        );
    }
    let before = e.active();
    let timers: Vec<_> = e
        .states
        .values()
        .map(|s| (s.pending, s.firing, s.clearing, s.notified))
        .collect();
    assert!(evaluate(&mut e, 100, &Source::default()).is_empty());
    assert_eq!(e.active(), before);
    assert_eq!(
        e.states
            .values()
            .map(|s| (s.pending, s.firing, s.clearing, s.notified))
            .collect::<Vec<_>>(),
        timers
    );
    assert_eq!(e.diagnostics().len(), 1);
    if pending && !firing {
        assert!(e.states.values().next().unwrap().pending.is_some());
        assert!(evaluate(
            &mut e,
            101,
            &Source {
                used: Some(99),
                ..Default::default()
            }
        )
        .is_empty());
        assert_eq!(
            evaluate(
                &mut e,
                106,
                &Source {
                    used: Some(99),
                    ..Default::default()
                }
            )[0]
            .state,
            AlertState::Firing
        );
    }
    if clearing {
        evaluate(
            &mut e,
            101,
            &Source {
                used: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(
            evaluate(
                &mut e,
                111,
                &Source {
                    used: Some(0),
                    ..Default::default()
                }
            )[0]
            .state,
            AlertState::Resolved
        );
    }
}
#[test]
fn inactive_unknown() {
    unknown_cell(false, false, false);
}
#[test]
fn pending_unknown() {
    unknown_cell(true, false, false);
}
#[test]
fn firing_unknown() {
    unknown_cell(false, true, false);
}
#[test]
fn clearing_unknown() {
    unknown_cell(false, true, true);
}
#[test]
fn clock_regression_zero_durations_and_space_guard() {
    let mut e = engine(cfg(AlertKind::FsUsedPct));
    let s = Source {
        used: Some(99),
        ..Default::default()
    };
    evaluate(&mut e, 10, &s);
    assert!(evaluate(&mut e, 5, &s).is_empty());
    assert_eq!(e.active().len(), 1);
    let mut e = engine(cfg(AlertKind::FsUsedPct));
    assert_eq!(
        e.evaluate(
            0,
            &AlertInputs {
                source: &s,
                state_space_refused: true
            }
        )
        .len(),
        2
    );
    e.evaluate(
        1,
        &AlertInputs {
            source: &s,
            state_space_refused: false,
        },
    );
    assert!(e
        .evaluate(
            601,
            &AlertInputs {
                source: &s,
                state_space_refused: false
            }
        )
        .iter()
        .any(|v| v.rule == "state_space" && v.state == AlertState::Resolved));
    let mut r = cfg(AlertKind::FsUsedPct);
    r.for_ = Duration::from_secs(5);
    let mut pending = engine(r);
    evaluate(&mut pending, 10, &s);
    assert!(evaluate(&mut pending, 9, &Source::default()).is_empty());
    assert_eq!(pending.states.values().next().unwrap().pending, Some(10));
    assert!(evaluate(&mut pending, 10, &s).is_empty());
    assert_eq!(evaluate(&mut pending, 15, &s)[0].state, AlertState::Firing);
    assert!(!elapsed(-1, 0, Duration::ZERO));
    assert!(!elapsed(1, 0, Duration::from_millis(1500)));
    assert!(elapsed(2, 0, Duration::from_millis(1500)));
}
#[test]
fn all_nine_known_predicates_and_threshold_boundaries() {
    let s = Source {
        used: Some(95),
        pools: Some(vec![pool(Some(81.0), Some(10.0))]),
        growth: Some(vec![(RawPath::new(b"/data/log".to_vec()).unwrap(), 11)]),
        cadence: Some(Duration::from_secs(300)),
        ..Default::default()
    };
    use AlertKind::*;
    for kind in [
        FsUsedPct,
        TimeToFull,
        HotDirGrowth,
        ThinPoolPct,
        VgFreeLow,
        DirGrowth,
        PhantomBytes,
        ScanStale,
        WatchLossy,
    ] {
        let mut r = cfg(kind);
        if kind == HotDirGrowth {
            r.window = Some("1h".into());
        }
        let mut e = engine(r);
        assert_eq!(evaluate(&mut e, 1000, &s).len(), 1, "{kind:?}");
        assert!(e.diagnostics().is_empty());
    }
    let mut r = cfg(HotDirGrowth);
    r.window = Some("1h".into());
    r.above = Some(AlertThreshold::Text("1B/s".into()));
    assert!(evaluate(&mut engine(r.clone()), 0, &s).is_empty());
    r.above = Some(AlertThreshold::Text("0.5B/s".into()));
    assert_eq!(evaluate(&mut engine(r), 0, &s).len(), 1);
}
#[test]
fn disabled_invalid_and_zero_resolve() {
    let s = Source {
        used: Some(99),
        ..Default::default()
    };
    let mut r = cfg(AlertKind::FsUsedPct);
    r.enabled = false;
    let mut e = engine(r.clone());
    assert!(evaluate(&mut e, 0, &s).is_empty());
    assert!(e.diagnostics().is_empty());
    r.above = None;
    assert!(AlertEngine::new(&[r], Duration::from_secs(10)).is_err());
    assert!(AlertEngine::new(&[], Duration::ZERO).is_err());
    let mut r = cfg(AlertKind::FsUsedPct);
    r.resolve_for = Duration::ZERO;
    let mut e = engine(r);
    evaluate(&mut e, 0, &s);
    assert_eq!(
        evaluate(
            &mut e,
            1,
            &Source {
                used: Some(0),
                ..Default::default()
            }
        )[0]
        .state,
        AlertState::Resolved
    );
}
#[test]
fn duplicate_observations_and_healthy_eviction() {
    let mut s = Source {
        growth: Some(
            (0..10_000)
                .map(|n| (RawPath::new(format!("/data/{n}").into_bytes()).unwrap(), 11))
                .collect(),
        ),
        ..Default::default()
    };
    let mut e = engine(cfg(AlertKind::DirGrowth));
    evaluate(&mut e, 0, &s);
    s.growth.as_mut().unwrap()[0].1 = 0;
    s.growth
        .as_mut()
        .unwrap()
        .push((RawPath::new(b"/data/new".to_vec()).unwrap(), 11));
    s.growth
        .as_mut()
        .unwrap()
        .push((RawPath::new(b"/data/new".to_vec()).unwrap(), 11));
    let events = evaluate(&mut e, 1, &s);
    assert_eq!(
        events.iter().filter(|ev| ev.subject == "/data/new").count(),
        1
    );
    assert!(e.active().iter().any(|a| a.subject == "/data/new"));
    assert_eq!(e.active().len(), 10_000);
    assert!(e.diagnostics()[0].reason.contains("capacity"));
}

#[test]
fn raw_subject_bytes_preserve_identity() {
    let s = Source {
        growth: Some(vec![
            (RawPath::new(b"/data/a\xff".to_vec()).unwrap(), 11),
            (RawPath::new(b"/data/a\xfe".to_vec()).unwrap(), 11),
        ]),
        ..Default::default()
    };
    let mut e = engine(cfg(AlertKind::DirGrowth));
    let events = evaluate(&mut e, 0, &s);
    assert_eq!(events.len(), 2);
    assert_ne!(events[0].subject, events[1].subject);
    assert!(events.iter().any(|ev| ev.subject == "/data/a\\xFF"));
}
#[test]
fn capacity_never_evicts_unavailable_clearing_alert() {
    let mut s = Source {
        used: Some(99),
        growth: Some(
            (0..9_999)
                .map(|n| {
                    (
                        RawPath::new(format!("/data/{n:05}").into_bytes()).unwrap(),
                        11,
                    )
                })
                .collect(),
        ),
        ..Default::default()
    };
    let mut e = AlertEngine::new(
        &[cfg(AlertKind::DirGrowth), cfg(AlertKind::FsUsedPct)],
        Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(evaluate(&mut e, 0, &s).len(), 10_000);
    s.used = Some(0);
    evaluate(&mut e, 1, &s);
    s.used = None;
    s.growth
        .as_mut()
        .unwrap()
        .push((RawPath::new(b"/data/z".to_vec()).unwrap(), 11));
    assert!(evaluate(&mut e, 2, &s).is_empty());
    assert!(e.active().iter().any(|a| a.rule == "FsUsedPct"));
    assert_eq!(e.diagnostics().len(), 2);
}
