//! Owned by: W0-A
//! tidemark-live crate skeleton.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// alerts module skeleton.
pub mod alerts;
/// now module skeleton.
pub mod now;
/// procscan module skeleton.
pub mod procscan;
/// rates module skeleton.
pub mod rates;
/// watch module skeleton.
pub mod watch;

/// Hidden parser seams for adversarial tests and fuzz targets.
#[doc(hidden)]
pub mod fuzzing {
    pub use crate::watch::parse_mount_id;
}
