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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formatting() {
        assert_eq!(bytes(1024), "1.00 KiB");
        use std::os::unix::ffi::OsStrExt;
        let raw = tidemark_core::path::RawPath::from_path(std::path::Path::new(
            std::ffi::OsStr::from_bytes(b"/a\xff\n"),
        ))
        .unwrap();
        assert_eq!(path(&raw), "/a\\xFF\\x0A");
    }
}
