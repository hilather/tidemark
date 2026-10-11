//! Owned by: W0-A
//! Bounded scan pipelines, aggregation, progress and snapshot lifecycle.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// agg scan module.
pub mod agg;
/// bulk scan module.
pub mod bulk;
/// mode scan module.
pub mod mode;
/// orchestrator scan module.
pub mod orchestrator;
/// progress scan module.
pub mod progress;

pub use agg::SinkError;
pub use mode::{choose_mode, ModeFacts, ModeRequest};
pub use orchestrator::{run_scan, ScanError, ScanOutcome, ScanRecorder, ScanRequest};
pub use progress::Progress;
