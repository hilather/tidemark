//! Owned by: TEST-e2e-scan
//! Operator scenarios through the real CLI and independent filesystem oracles.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "e2e/scan_lifecycle.rs"]
mod scan_lifecycle;
#[path = "e2e/scan_reports.rs"]
mod scan_reports;
#[path = "e2e/support.rs"]
mod support;

#[path = "e2e/identity.rs"]
mod identity;
