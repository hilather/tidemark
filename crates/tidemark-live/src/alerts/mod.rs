//! Owned by: W5-D
//! Bounded alert hysteresis and deterministic metrics over core inputs.

use crate::rates::{DirGrowth, MountRates};
use std::time::Duration;
use tidemark_core::{
    config::AlertRule,
    live::{AlertEvent, AlertSummary, HotEntry, LvmNow, PhantomSummary, ScanStatus},
};

/// Reserved for W5-D.
pub struct AlertInputs<'a> {
    /// Contract field.
    pub mounts: &'a [MountRates],
    /// Contract field.
    pub lvm: &'a [(String, LvmNow)],
    /// Contract field.
    pub phantom: &'a [PhantomSummary],
    /// Contract field.
    pub hot_dirs_window: &'a [(String, Vec<HotEntry>)],
    /// Contract field.
    pub scans: &'a [ScanStatus],
    /// Contract field.
    pub dir_growth: &'a [DirGrowth],
    /// Contract field.
    pub watch_overflows_15m: &'a [(String, u64)],
    /// Contract field.
    pub state_space_refused: bool,
}
/// Bounded per-rule, per-subject hysteresis engine.
pub struct AlertEngine {
    rules: Vec<rules::Rule>,
    states: std::collections::BTreeMap<(usize, String), State>,
    renotify: Duration,
    last_now: Option<i64>,
    space: State,
}
struct State {
    pending: Option<i64>,
    clearing: Option<i64>,
    firing: Option<i64>,
    notified: i64,
    value: f64,
}
/// Maximum simultaneously tracked rule/subject pairs.
pub const MAX_ALERT_SUBJECTS: usize = 10_000;
impl AlertEngine {
    /// Validate rules against core configuration and compile their predicates.
    pub fn new(rules: &[AlertRule], renotify: Duration) -> Result<Self, AlertError> {
        if rules.len() > MAX_ALERT_SUBJECTS || renotify.is_zero() {
            return Err(AlertError::Invalid(
                "rule cap exceeded or zero renotify".into(),
            ));
        }
        let mut cfg = tidemark_core::config::Config::default();
        cfg.alerts.rule = rules.to_vec();
        let issues: Vec<_> = cfg
            .validate()
            .into_iter()
            .filter(|e| e.field.starts_with("alerts.rule"))
            .collect();
        if !issues.is_empty() {
            return Err(AlertError::Invalid(format!("{issues:?}")));
        }
        Ok(Self {
            rules: rules
                .iter()
                .map(rules::Rule::compile)
                .collect::<Result<_, _>>()?,
            states: Default::default(),
            renotify,
            last_now: None,
            space: State {
                pending: None,
                clearing: None,
                firing: None,
                notified: 0,
                value: 0.0,
            },
        })
    }
    /// Evaluate at injected Unix seconds. Backward clocks reset continuity timers.
    pub fn evaluate(&mut self, now: i64, inputs: &AlertInputs) -> Vec<AlertEvent> {
        if self.last_now.is_some_and(|previous| now < previous) {
            for state in self.states.values_mut() {
                state.pending = None;
                state.clearing = None;
                state.notified = now;
            }
        }
        if self.last_now.is_some_and(|previous| now < previous) {
            self.space.clearing = None;
            self.space.notified = now;
        }
        self.last_now = Some(now);
        let mut events = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (idx, rule) in self.rules.iter().enumerate().filter(|(_, r)| r.cfg.enabled) {
            rule.visit(now, inputs, |o| {
                let key = (idx, o.subject.clone());
                if seen.contains(&key) {
                    return;
                }
                if !self.states.contains_key(&key) && self.states.len() >= MAX_ALERT_SUBJECTS {
                    return;
                }
                seen.insert(key.clone());
                let state = self.states.entry(key).or_insert(State {
                    pending: None,
                    clearing: None,
                    firing: None,
                    notified: now,
                    value: o.value,
                });
                state.value = o.value;
                if let Some(kind) = step(
                    state,
                    o.condition,
                    now,
                    rule.cfg.for_,
                    rule.cfg.resolve_for,
                    self.renotify,
                ) {
                    events.push(AlertEvent {
                        ts: now,
                        rule: rule.cfg.name.clone(),
                        subject: o.subject,
                        state: kind,
                        value: Some(o.value),
                        message: format!(
                            "{} condition {}",
                            rule.cfg.name,
                            if o.condition { "met" } else { "cleared" }
                        ),
                    });
                }
            });
            // A subject disappearing from a complete collector result means zero growth.
            // An unavailable collector must not be mistaken for a healthy observation.
            if rule.available(inputs) {
                for (key, state) in self
                    .states
                    .iter_mut()
                    .filter(|(key, _)| key.0 == idx && !seen.contains(*key))
                {
                    if let Some(kind) = step(
                        state,
                        false,
                        now,
                        rule.cfg.for_,
                        rule.cfg.resolve_for,
                        self.renotify,
                    ) {
                        events.push(AlertEvent {
                            ts: now,
                            rule: rule.cfg.name.clone(),
                            subject: key.1.clone(),
                            state: kind,
                            value: Some(0.0),
                            message: format!("{} subject absent", rule.cfg.name),
                        });
                    }
                    state.value = 0.0;
                }
                for key in self.states.keys().filter(|key| key.0 == idx) {
                    seen.insert(key.clone());
                }
            }
        }
        // Missing observations break continuity and cannot resolve or re-notify alerts.
        self.states.retain(|key, s| {
            if !seen.contains(key) {
                s.pending = None;
                s.clearing = None;
            }
            s.firing.is_some() || s.pending.is_some()
        });
        self.space.value = if inputs.state_space_refused { 1.0 } else { 0.0 };
        if let Some(state) = step(
            &mut self.space,
            inputs.state_space_refused,
            now,
            Duration::ZERO,
            Duration::from_secs(600),
            self.renotify,
        ) {
            events.push(AlertEvent {
                ts: now,
                rule: "state_space".into(),
                subject: "state".into(),
                state,
                value: Some(self.space.value),
                message: "snapshot space guard".into(),
            });
        }
        events.sort_by(|a, b| (&a.rule, &a.subject).cmp(&(&b.rule, &b.subject)));
        events
    }
    /// Active alerts, deterministically ordered by rule and subject.
    pub fn active(&self) -> Vec<AlertSummary> {
        let mut result: Vec<_> = self
            .states
            .iter()
            .filter_map(|((idx, subject), s)| {
                s.firing.map(|since| AlertSummary {
                    rule: self.rules[*idx].cfg.name.clone(),
                    subject: subject.clone(),
                    since,
                    value: Some(s.value),
                    message: format!("{} condition met", self.rules[*idx].cfg.name),
                })
            })
            .collect();
        if let Some(since) = self.space.firing {
            result.push(AlertSummary {
                rule: "state_space".into(),
                subject: "state".into(),
                since,
                value: Some(self.space.value),
                message: "snapshot space guard".into(),
            });
        }
        result.sort_by(|a, b| (&a.rule, &a.subject).cmp(&(&b.rule, &b.subject)));
        result
    }
    /// Render the frozen Prometheus alert gauge with escaped labels.
    pub fn render_metrics(&self) -> String {
        fn label(s: &str) -> String {
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        }
        self.active()
            .iter()
            .map(|a| {
                format!(
                    "tidemark_alert_firing{{rule=\"{}\",subject=\"{}\"}} 1\n",
                    label(&a.rule),
                    label(&a.subject)
                )
            })
            .collect()
    }
}
fn elapsed(now: i64, since: i64, duration: Duration) -> bool {
    let delta = i128::from(now) - i128::from(since);
    delta >= 0 && delta as f64 >= duration.as_secs_f64()
}
fn step(
    s: &mut State,
    condition: bool,
    now: i64,
    for_: Duration,
    resolve: Duration,
    renotify: Duration,
) -> Option<tidemark_core::live::AlertState> {
    use tidemark_core::live::AlertState::*;
    if condition {
        s.clearing = None;
        if s.firing.is_none() {
            let since = *s.pending.get_or_insert(now);
            if elapsed(now, since, for_) {
                s.firing = Some(now);
                s.pending = None;
                s.notified = now;
                return Some(Firing);
            }
        } else if elapsed(now, s.notified, renotify) {
            s.notified = now;
            return Some(Renotify);
        }
    } else {
        s.pending = None;
        if s.firing.is_some() {
            let since = *s.clearing.get_or_insert(now);
            if elapsed(now, since, resolve) {
                s.firing = None;
                s.clearing = None;
                return Some(Resolved);
            }
        }
    }
    None
}

mod deliver;
mod rules;
pub use tidemark_core::config::default_rules;

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum AlertError {
    /// Invalid configuration or unsupported construction.
    #[error("invalid alert rules: {0}")]
    Invalid(String),
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}

pub use deliver::{Deliver, DeliverError, JournalDeliver, WebhookDeliver};

#[cfg(test)]
mod tests {
    use super::*;
    use tidemark_core::{
        config::{AlertKind, AlertThreshold},
        live::AlertState,
    };
    fn inputs(mounts: &[MountRates]) -> AlertInputs<'_> {
        AlertInputs {
            mounts,
            lvm: &[],
            phantom: &[],
            hot_dirs_window: &[],
            scans: &[],
            dir_growth: &[],
            watch_overflows_15m: &[],
            state_space_refused: false,
        }
    }
    fn mount(name: &str, used: u64) -> MountRates {
        MountRates {
            mount: name.into(),
            size: 100,
            used,
            avail: 100 - used,
            rate_15m: None,
            rate_1h: None,
            ttf: None,
        }
    }
    fn rule(name: &str, mount: &str) -> AlertRule {
        AlertRule {
            name: name.into(),
            kind: AlertKind::FsUsedPct,
            mount: Some(mount.into()),
            above: Some(AlertThreshold::Number(90.0)),
            for_: Duration::from_secs(5),
            resolve_for: Duration::from_secs(10),
            ..Default::default()
        }
    }
    #[test]
    fn hysteresis_transition_table() {
        let mut e = AlertEngine::new(&[rule("full", "/data")], Duration::from_secs(20)).unwrap();
        let timeline = [
            (0, false, None),
            (1, true, None),
            (4, false, None),
            (5, true, None),
            (9, true, None),
            (10, true, Some(AlertState::Firing)),
            (11, true, None),
            (12, false, None),
            (21, false, None),
            (22, true, None),
            (30, true, Some(AlertState::Renotify)),
            (31, false, None),
            (40, false, None),
            (41, false, Some(AlertState::Resolved)),
            (42, false, None),
        ];
        for (now, condition, event) in timeline {
            let mounts = [mount("/data", if condition { 91 } else { 90 })];
            let events = e.evaluate(now, &inputs(&mounts));
            assert_eq!(events.first().map(|e| e.state.clone()), event, "at {now}");
        }
        assert!(e.active().is_empty());
    }
    #[test]
    fn subjects_missing_and_backward_clock() {
        let mut e =
            AlertEngine::new(&[rule("a", "/a"), rule("b", "/b")], Duration::from_secs(20)).unwrap();
        let m = [mount("/a", 91), mount("/b", 90)];
        e.evaluate(10, &inputs(&m));
        e.evaluate(12, &inputs(&[]));
        assert!(e.evaluate(15, &inputs(&m)).is_empty());
        assert_eq!(e.evaluate(20, &inputs(&m)).len(), 1);
        assert_eq!(e.active().len(), 1);
        assert!(e.evaluate(9, &inputs(&m)).is_empty());
        assert!(e.evaluate(10, &inputs(&[])).is_empty());
        assert_eq!(e.active().len(), 1);
        let m = [mount("/a", 90), mount("/b", 91)];
        e.evaluate(11, &inputs(&m));
        assert_eq!(e.evaluate(16, &inputs(&m)).len(), 1);
        assert_eq!(e.active().len(), 2);
        assert_eq!(e.evaluate(21, &inputs(&m)).len(), 1);
        assert_eq!(e.active()[0].rule, "b");
        assert!(e.render_metrics().contains("tidemark_alert_firing"));
    }
    #[test]
    fn zero_durations_disabled_and_invalid() {
        let mut r = rule("x", "/data");
        r.for_ = Duration::ZERO;
        r.resolve_for = Duration::ZERO;
        let mut e = AlertEngine::new(&[r.clone()], Duration::from_secs(1)).unwrap();
        assert_eq!(
            e.evaluate(0, &inputs(&[mount("/data", 91)]))[0].state,
            AlertState::Firing
        );
        assert_eq!(
            e.evaluate(1, &inputs(&[mount("/data", 90)]))[0].state,
            AlertState::Resolved
        );
        r.enabled = false;
        let mut e = AlertEngine::new(&[r.clone()], Duration::from_secs(1)).unwrap();
        assert!(e.evaluate(0, &inputs(&[mount("/data", 91)])).is_empty());
        r.above = None;
        assert!(AlertEngine::new(&[r], Duration::from_secs(1)).is_err());
        assert!(AlertEngine::new(&[], Duration::ZERO).is_err());
    }
    #[test]
    fn subject_cap_and_duplicate_observations() {
        let r = AlertRule {
            name: "dirs".into(),
            kind: AlertKind::DirGrowth,
            root: Some("/data".into()),
            depth: Some(1),
            above: Some(AlertThreshold::Number(0.0)),
            for_: Duration::ZERO,
            ..Default::default()
        };
        let mut e = AlertEngine::new(&[r], Duration::from_secs(20)).unwrap();
        let mut growth: Vec<_> = (0..MAX_ALERT_SUBJECTS + 1)
            .map(|n| DirGrowth {
                root: "/data".into(),
                path: tidemark_core::path::RawPath::new(format!("/data/{n}").into_bytes()).unwrap(),
                depth: 1,
                delta: 1,
                between: ("a".into(), "b".into()),
            })
            .collect();
        growth.push(DirGrowth {
            root: "/data".into(),
            path: tidemark_core::path::RawPath::new(b"/data/0".to_vec()).unwrap(),
            depth: 1,
            delta: 1,
            between: ("a".into(), "b".into()),
        });
        let mut i = inputs(&[]);
        i.dir_growth = &growth;
        assert_eq!(e.evaluate(0, &i).len(), MAX_ALERT_SUBJECTS);
        assert_eq!(e.active().len(), MAX_ALERT_SUBJECTS);
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    use tidemark_core::live::AlertState;
    fn state() -> State {
        State {
            pending: None,
            clearing: None,
            firing: None,
            notified: 0,
            value: 1.0,
        }
    }
    #[test]
    fn step_guards_and_transitions() {
        let mut s = state();
        let d = Duration::from_secs(5);
        let r = Duration::from_secs(10);
        assert!(step(&mut s, false, 0, d, d, r).is_none()); // inactive false
        assert!(step(&mut s, true, 0, d, d, r).is_none()); // starts pending
        assert!(step(&mut s, true, 4, d, d, r).is_none()); // pending guard false
        assert!(step(&mut s, false, 4, d, d, r).is_none());
        assert!(s.pending.is_none()); // flap resets
        assert!(step(&mut s, true, 5, d, d, r).is_none());
        assert_eq!(step(&mut s, true, 10, d, d, r), Some(AlertState::Firing)); // pending guard true
        assert!(step(&mut s, true, 19, d, d, r).is_none()); // renotify guard false
        assert_eq!(step(&mut s, true, 20, d, d, r), Some(AlertState::Renotify)); // true
        assert!(step(&mut s, false, 21, d, d, r).is_none()); // clearing starts
        assert!(step(&mut s, false, 25, d, d, r).is_none()); // resolve guard false
        assert!(step(&mut s, true, 25, d, d, r).is_none());
        assert!(s.clearing.is_none()); // clear flap
        assert!(step(&mut s, false, 26, d, d, r).is_none());
        assert_eq!(step(&mut s, false, 31, d, d, r), Some(AlertState::Resolved)); // resolve guard true
        assert!(!elapsed(-1, 0, Duration::ZERO));
        assert!(elapsed(0, 0, Duration::ZERO));
        assert!(!elapsed(1, 0, Duration::from_millis(1500)));
        assert!(elapsed(2, 0, Duration::from_millis(1500)));
    }
    #[test]
    fn internal_space_guard() {
        let mut e = AlertEngine::new(&[], Duration::from_secs(1000)).unwrap();
        let mut i = AlertInputs {
            mounts: &[],
            lvm: &[],
            phantom: &[],
            hot_dirs_window: &[],
            scans: &[],
            dir_growth: &[],
            watch_overflows_15m: &[],
            state_space_refused: true,
        };
        assert_eq!(e.evaluate(0, &i)[0].rule, "state_space");
        assert_eq!(e.active().len(), 1);
        i.state_space_refused = false;
        assert!(e.evaluate(1, &i).is_empty());
        assert_eq!(e.evaluate(601, &i)[0].state, AlertState::Resolved);
        assert!(e.active().is_empty());
    }
}
