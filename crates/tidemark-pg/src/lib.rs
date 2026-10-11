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

/// Postgres probe signatures.
pub use collector::{probe_from_config, PgCollector, PgConn, PgConnector, PgError, PgProbe, PgRow};
