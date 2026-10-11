//! Owned by: W0-A
//! tidemark-pg crate skeleton.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// collector module skeleton.
pub mod collector;
/// labels module skeleton.
pub mod labels;
/// queries module skeleton.
pub mod queries;

/// Stub error reserved for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum PgError {
    /// Cross-task implementation is not available yet.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}

/// Postgres probe signatures.
pub use collector::{probe_from_config, PgProbe};
