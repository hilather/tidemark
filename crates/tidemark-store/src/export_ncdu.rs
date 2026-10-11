//! Owned by: W3-C
//! Compiling cross-task interfaces; runtime behavior belongs to W3-C.

use crate::{reader::Snapshot, StoreError};
use std::io::Write;
use tidemark_core::path::RawPath;
/// Cross-task contract reserved for the owning task.
pub struct NcduOptions {
    #[doc = "Contract field."]
    pub utf8_lossy: bool,
    #[doc = "Contract field."]
    pub max_depth: Option<u32>,
}
/// Cross-task contract reserved for the owning task.
pub fn export_ncdu(
    snap: &Snapshot,
    under: &RawPath,
    out: &mut dyn Write,
    opts: &NcduOptions,
) -> Result<ExportStats, StoreError> {
    let _ = snap;
    let _ = under;
    let _ = out;
    let _ = opts;
    Err(StoreError::NotYetImplemented)
}

/// Cross-task contract reserved for the owning task.
pub struct ExportStats;
