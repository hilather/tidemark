//! Owned by: W0-C
//! Temporary fixtures and independent metadata reference oracles.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
pub mod fixture;
pub mod golden;
#[cfg(feature = "privileged")]
pub mod lvm;
pub mod oracle;
pub mod random;
pub mod reference;
#[cfg(feature = "privileged")]
pub mod xfs;
pub use fixture::{materialize, materialize_file, Fixture};
pub use random::{model_tree, random_tree, ModelTree, RandomTreeParams};

/// Create an explicitly private temporary directory, independent of the caller's umask.
pub fn private_tempdir() -> std::io::Result<tempfile::TempDir> {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
}

/// Create private fixture directories, including missing ancestors.
/// Only fixture construction uses this helper; existing modes are left alone.
pub fn private_dir(path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

/// Write a private fixture file using explicit permissions for new files.
pub fn private_write(
    path: impl AsRef<std::path::Path>,
    bytes: impl AsRef<[u8]>,
) -> std::io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes.as_ref())
}
