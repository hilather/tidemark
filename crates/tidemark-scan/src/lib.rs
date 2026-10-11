//! Owned by: W0-A
//! tidemark-scan crate skeleton.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// agg module skeleton.
pub mod agg;
/// bulk module skeleton.
pub mod bulk;
/// mode module skeleton.
pub mod mode;
/// orchestrator module skeleton.
pub mod orchestrator;
/// progress module skeleton.
pub mod progress;

pub use agg::SinkError;
pub use mode::{choose_mode, ModeFacts, ModeRequest};
pub use orchestrator::{run_scan, ScanError, ScanOutcome, ScanRecorder, ScanRequest};
pub use progress::Progress;
