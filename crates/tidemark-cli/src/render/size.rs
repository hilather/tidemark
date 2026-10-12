//! Owned by: W0-B2
//! Shared binary-size and raw-path formatting.

/// Format binary units using the frozen core contract.
pub fn bytes(value: u64) -> String {
    tidemark_core::units::fmt_bytes(value)
}
/// Display raw path bytes with core escaping.
pub fn path(value: &tidemark_core::path::RawPath) -> String {
    value.display_lossy()
}
