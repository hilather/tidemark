//! Owned by: W5-D
//! Bounded alert hysteresis and rule-scoped measurement diagnostics.

use std::time::Duration;
use tidemark_core::{
    config::AlertRule,
    live::{AlertEvent, AlertSummary, FsSample, HotEntry, LvmNow},
    path::RawPath,
};

/// Measurements queried for each rule. None means unavailable, never zero.
/// Returned collections must be complete; callers bound their collector storage.
pub trait AlertSource {
    /// Current filesystem sample.
    fn fs_sample(&self, mount: &str) -> Option<FsSample>;
    /// Growth in bytes per second for precisely this window.
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64>;
    /// Directory attribution for precisely this window.
    fn hot_dirs(&self, mount: &str, window: Duration) -> Option<Vec<HotEntry>>;
    /// Overflow count in precisely this window.
    fn watch_overflows(&self, mount: &str, window: Duration) -> Option<u64>;
    /// Current deleted-but-open bytes.
    fn phantom_bytes(&self, mount: &str) -> Option<u64>;
    /// Complete tracked LV inventory, including measured empty inventory.
    fn lvm(&self) -> Option<Vec<LvmNow>>;
    /// Complete snapshot comparison at the requested depth.
    fn dir_growth(&self, root: &str, depth: u32) -> Option<Vec<(RawPath, i64)>>;
    /// Last complete scan and effective schedule cadence.
    fn scan_age(&self, root: &str) -> Option<ScanAge>;
}
/// Scan timing; windowed schedules must supply a cadence of at least 24 hours.
pub struct ScanAge {
    /// Last complete scan time; None means no scan has completed.
    pub last_complete_at: Option<i64>,
    /// Effective cadence, including the 24-hour windowed schedule minimum.
    pub effective_cadence: Option<Duration>,
}
/// Borrowed source and internal space guard input.
pub struct AlertInputs<'a> {
    /// Rule-specific measurements.
    pub source: &'a dyn AlertSource,
    /// Snapshot space guard refused a write.
    pub state_space_refused: bool,
}
/// An enabled rule that could not be evaluated, or whose subject cap was hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertDiagnostic {
    /// Configured rule name.
    pub rule: String,
    /// Unavailable measurement or capacity reason.
    pub reason: String,
}
/// Bounded per-rule, per-subject hysteresis engine.
pub struct AlertEngine {
    rules: Vec<rules::Rule>,
    states: std::collections::BTreeMap<(usize, String), State>,
    renotify: Duration,
    last_now: Option<i64>,
    space: State,
    diagnostics: Vec<AlertDiagnostic>,
}
struct State {
    pending: Option<i64>,
    clearing: Option<i64>,
    firing: Option<i64>,
    notified: i64,
    value: f64,
    paused: bool,
}
/// Maximum simultaneously tracked rule/subject pairs.
const MAX_ALERT_SUBJECTS: usize = 10_000;
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
            diagnostics: Vec::new(),
            space: State {
                pending: None,
                clearing: None,
                firing: None,
                notified: 0,
                value: 0.0,
                paused: false,
            },
        })
    }
    /// Evaluate at injected Unix seconds. Backward clocks reset continuity timers.
    pub fn evaluate(&mut self, now: i64, inputs: &AlertInputs) -> Vec<AlertEvent> {
        if self.last_now.is_some_and(|previous| now < previous) {
            for state in self.states.values_mut() {
                state.paused = true;
                state.notified = now;
            }
        }
        if self.last_now.is_some_and(|previous| now < previous) {
            self.space.clearing = None;
            self.space.notified = now;
        }
        self.last_now = Some(now);
        let mut events = Vec::new();
        self.diagnostics.clear();
        // Only false observations from this evaluation are known healthy.
        // Previous clearing evidence must not evict an unavailable alert.
        let mut healthy = std::collections::BTreeSet::new();
        for (idx, rule) in self.rules.iter().enumerate().filter(|(_, r)| r.cfg.enabled) {
            let mut seen = std::collections::BTreeSet::new();
            let mut reason = None;
            let mut capacity_hit = false;
            let complete = rule.visit(now, inputs.source, |o| {
                let key = (idx, o.subject.clone());
                // Healthy unseen subjects need no state and consume no budget.
                if !self.states.contains_key(&key) && o.condition == Some(false) {
                    return;
                }
                if !self.states.contains_key(&key) && o.condition.is_none() {
                    reason = Some("subject measurement unavailable".to_owned());
                    return;
                }
                if seen.contains(&key) {
                    return;
                }
                if !self.states.contains_key(&key) && self.states.len() >= MAX_ALERT_SUBJECTS {
                    // Prefer evicting known healthy/clearing subjects over hiding a breach.
                    if let Some(k) = healthy.pop_first() {
                        self.states.remove(&k);
                        seen.remove(&k);
                    }
                    capacity_hit = true;
                    if self.states.len() >= MAX_ALERT_SUBJECTS {
                        return;
                    }
                }
                seen.insert(key.clone());
                let state = self.states.entry(key.clone()).or_insert(State {
                    pending: None,
                    clearing: None,
                    firing: None,
                    notified: now,
                    value: o.value,
                    paused: false,
                });
                let Some(condition) = o.condition else {
                    state.paused = true;
                    healthy.remove(&key);
                    reason = Some("subject measurement unavailable".to_owned());
                    return;
                };
                state.value = o.value;
                if let Some(kind) = step(
                    state,
                    condition,
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
                            if condition { "met" } else { "cleared" }
                        ),
                    });
                }
                if state.clearing.is_some() {
                    healthy.insert(key.clone());
                } else {
                    healthy.remove(&key);
                }
                if state.pending.is_none() && state.firing.is_none() {
                    self.states.remove(&key);
                    seen.remove(&key);
                    healthy.remove(&key);
                }
            });
            if let Err(why) = &complete {
                reason = Some(why.to_string());
            }
            for (key, state) in self
                .states
                .iter_mut()
                .filter(|(k, _)| k.0 == idx && !seen.contains(*k))
            {
                if complete.is_err() {
                    state.paused = true;
                    healthy.remove(key);
                    continue;
                }
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
                if state.clearing.is_some() {
                    healthy.insert(key.clone());
                } else {
                    healthy.remove(key);
                }
            }
            self.states
                .retain(|_, s| s.firing.is_some() || s.pending.is_some());
            if capacity_hit {
                let capacity = "subject capacity reached (10000); observations dropped or healthy state evicted";
                reason = Some(match reason {
                    Some(reason) => format!("{reason}; {capacity}"),
                    None => capacity.to_owned(),
                });
            }
            if let Some(reason) = reason {
                self.diagnostics.push(AlertDiagnostic {
                    rule: rule.cfg.name.clone(),
                    reason,
                });
            }
        }
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
    /// Diagnostics for the latest evaluation, for alert listings and now reports.
    pub fn diagnostics(&self) -> &[AlertDiagnostic] {
        &self.diagnostics
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
    if s.paused {
        if s.pending.is_some() {
            s.pending = Some(now);
        }
        s.clearing = None;
        s.paused = false;
    }
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
    if s.firing.is_some() && elapsed(now, s.notified, renotify) {
        s.notified = now;
        return Some(Renotify);
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
mod tests;
