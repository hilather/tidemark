//! Owned by: W0-A
//! tidemark-live crate skeleton.

#![forbid(unsafe_code)]

/// alerts module skeleton.
pub mod alerts;
/// alerts/deliver module skeleton.
#[path = "alerts/deliver.rs"]
mod alerts_deliver;
/// alerts/rules module skeleton.
#[path = "alerts/rules.rs"]
mod alerts_rules;
/// now module skeleton.
pub mod now;
/// procscan module skeleton.
pub mod procscan;
/// rates module skeleton.
pub mod rates;
/// watch module skeleton.
pub mod watch;
/// watch/fdmode module skeleton.
#[path = "watch/fdmode.rs"]
mod watch_fdmode;
/// watch/fidmode module skeleton.
#[path = "watch/fidmode.rs"]
mod watch_fidmode;
/// watch/rings module skeleton.
#[path = "watch/rings.rs"]
mod watch_rings;
