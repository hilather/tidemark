//! Owned by: W1-A
//! Resolve a user-supplied query root once, preserving non-UTF8 bytes.

use std::{io, path::Path};
use tidemark_core::path::RawPath;
/// Resolve existing paths and symlinks before scanning; the walker never follows links.
pub fn canonicalize(path: &Path) -> io::Result<RawPath> {
    RawPath::from_path(&std::fs::canonicalize(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    #[test]
    fn resolves_links_and_preserves_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'x', 255]));
        std::fs::create_dir(&target).unwrap();
        symlink(&target, dir.path().join("link")).unwrap();
        assert_eq!(
            canonicalize(&dir.path().join("link")).unwrap().as_path(),
            target
        );
        assert!(canonicalize(&dir.path().join("missing")).is_err());
    }
}
