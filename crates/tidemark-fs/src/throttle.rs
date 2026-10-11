//! Owned by: W1-D
//! Cross-task signatures reserved for W1-D.

use std::{io, sync::Arc, time::Duration};
/// Shared core statistics.
pub use tidemark_core::scan::GateStats;
use tidemark_core::{
    config::{ConfigIssue, ThrottleConfig},
    dev::{BlockDevInfo, DiskStat},
    scan::Gate,
};
/// Reserved for W1-D.
pub struct AdaptiveParams {
    #[doc = "Field specified by the owning task."]
    pub max_ops: u32,
    #[doc = "Field specified by the owning task."]
    pub min_ops: u32,
    #[doc = "Field specified by the owning task."]
    pub max_util_pct: f64,
    #[doc = "Field specified by the owning task."]
    pub target_latency_ms: f64,
    #[doc = "Field specified by the owning task."]
    pub interval: Duration,
}
/// Reserved for W1-D.
pub type StatSampler = Box<dyn FnMut() -> io::Result<Vec<DiskStat>> + Send>;
/// Reserved for W1-D.
pub struct FixedGate;
/// Reserved for W1-D.
pub struct AdaptiveGate;
impl AdaptiveGate {
    /// Reserved for W1-D.
    pub fn start(p: AdaptiveParams, sampler: StatSampler) -> Arc<AdaptiveGate> {
        let _ = p;
        let _ = sampler;
        Arc::new(AdaptiveGate)
    }
    /// Reserved for W1-D.
    pub fn stats(&self) -> GateStats {
        GateStats::default()
    }
    /// Reserved for W1-D.
    pub fn stop(&self) {}
}

/// Reserved for W1-D.
pub fn controller_step(
    rate: f64,
    prev: &[DiskStat],
    cur: &[DiskStat],
    dt_ms: f64,
    p: &AdaptiveParams,
) -> f64 {
    let _ = rate;
    let _ = prev;
    let _ = cur;
    let _ = dt_ms;
    let _ = p;
    f64::NAN
}

/// Reserved for W1-D.
pub fn gate_from_config(
    profile: &str,
    cfg: &ThrottleConfig,
    leaves: &[BlockDevInfo],
    sampler: StatSampler,
) -> Result<Arc<dyn Gate>, ConfigIssue> {
    let _ = profile;
    let _ = cfg;
    let _ = leaves;
    let _ = sampler;
    Err(ConfigIssue {
        field: "throttle".into(),
        message: "NotYetImplemented".into(),
    })
}

impl Gate for FixedGate {
    fn acquire(&self, _cost: u32) {}
    fn current_rate(&self) -> f64 {
        0.0
    }
}
impl Gate for AdaptiveGate {
    fn acquire(&self, _cost: u32) {}
    fn current_rate(&self) -> f64 {
        0.0
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        FixedGate.acquire(0);
        assert_eq!(FixedGate.current_rate(), 0.0);
        AdaptiveGate.acquire(0);
        assert_eq!(AdaptiveGate.current_rate(), 0.0);

        assert_eq!(
            AdaptiveGate::start(
                AdaptiveParams {
                    max_ops: 1,
                    min_ops: 1,
                    max_util_pct: 1.0,
                    target_latency_ms: 1.0,
                    interval: std::time::Duration::ZERO
                },
                Box::new(|| Ok(Vec::new()))
            )
            .current_rate(),
            0.0
        );
        assert_eq!((AdaptiveGate).stats(), GateStats::default());
        (AdaptiveGate).stop();
        assert!(controller_step(
            0.0,
            &[],
            &[],
            0.0,
            &AdaptiveParams {
                max_ops: 1,
                min_ops: 1,
                max_util_pct: 1.0,
                target_latency_ms: 1.0,
                interval: std::time::Duration::ZERO
            }
        )
        .is_nan());
        assert_eq!(
            gate_from_config(
                "",
                &ThrottleConfig::default(),
                &[],
                Box::new(|| Ok(Vec::new()))
            )
            .err()
            .unwrap()
            .message,
            "NotYetImplemented"
        );
    }
}
