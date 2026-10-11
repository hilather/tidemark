//! Owned by: W3-B
//! Compiling cross-task interfaces; runtime behavior belongs to W3-B.

use std::{io, sync::atomic::AtomicBool};
use tidemark_core::{path::RawPath, scan::Gate};

/// Cross-task contract reserved for the owning task.
pub struct SymlinkOpts {
    #[doc = "Contract field."]
    pub check_targets: bool,
    #[doc = "Contract field."]
    pub limit: usize,
}
/// Cross-task contract reserved for the owning task.
pub enum SymlinkClass {
    #[doc = "Contract variant."]
    InsideRoot,
    #[doc = "Contract variant."]
    OutsideRoot,
    #[doc = "Contract variant."]
    OtherFs,
    #[doc = "Contract variant."]
    Dangling,
    #[doc = "Contract variant."]
    Loop,
    #[doc = "Contract variant."]
    Unreadable,
}
/// Cross-task contract reserved for the owning task.
pub struct SymlinkReport {
    #[doc = "Contract field."]
    pub total: u64,
    #[doc = "Contract field."]
    pub by_class: Vec<(SymlinkClass, u64)>,
    #[doc = "Contract field."]
    pub absolute: u64,
    #[doc = "Contract field."]
    pub relative: u64,
    #[doc = "Contract field."]
    pub examples: Vec<(RawPath, Vec<u8> /*target*/, SymlinkClass)>,
}
/// Cross-task contract reserved for the owning task.
pub fn analyze(
    root: &RawPath,
    gate: &dyn Gate,
    cancel: &AtomicBool,
    opts: &SymlinkOpts,
) -> io::Result<SymlinkReport> {
    let _ = root;
    let _ = gate;
    let _ = cancel;
    let _ = opts;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
