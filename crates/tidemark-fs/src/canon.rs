//! Owned by: W1-A
//! Cross-task signatures reserved for W1-A.

use std::{io, path::Path};
use tidemark_core::path::RawPath;
/// Reserved for W1-A.
pub fn canonicalize(path: &Path) -> io::Result<RawPath> {
    let _ = path;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert_eq!(
            canonicalize(std::path::Path::new("/"))
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
