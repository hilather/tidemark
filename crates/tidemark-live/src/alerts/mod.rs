//! Owned by: W5-D
//! Cross-task interface stubs.

use crate::rates::{DirGrowth, MountRates};
use std::time::Duration;
use tidemark_core::{
    config::AlertRule,
    live::{AlertEvent, AlertSummary, HotEntry, LvmNow, PgNow, PhantomSummary, ScanStatus},
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
    pub pg: Option<&'a PgNow>,
    /// Contract field.
    pub scans: &'a [ScanStatus],
    /// Contract field.
    pub dir_growth: &'a [DirGrowth],
    /// Contract field.
    pub watch_overflows_15m: &'a [(String, u64)],
    /// Contract field.
    pub state_space_refused: bool,
}
/// Reserved for W5-D.
pub struct AlertEngine;
impl AlertEngine {
    /// Reserved for W5-D.
    pub fn new(rules: &[AlertRule], renotify: Duration) -> Result<Self, AlertError> {
        let _ = rules;
        let _ = renotify;
        Err(AlertError::NotYetImplemented)
    }
    /// Reserved for W5-D.
    pub fn evaluate(&mut self, now: i64, inputs: &AlertInputs) -> Vec<AlertEvent> {
        let _ = now;
        let _ = inputs;
        Vec::new()
    }
    /// Reserved for W5-D.
    pub fn active(&self) -> Vec<AlertSummary> {
        Vec::new()
    }
}

mod deliver;
mod rules;
pub use tidemark_core::config::default_rules;

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum AlertError {
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}

pub use deliver::{Deliver, DeliverError, JournalDeliver, WebhookDeliver};
