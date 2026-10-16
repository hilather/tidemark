//! Owned by: WPROCFS-fdskip2
//! Deterministic descriptor observation and resource-policy regressions.

use super::*;
use std::{cell::Cell, os::fd::AsRawFd};

#[test]
fn replacement_before_first_metadata_uses_replacement_flags() {
    let a = tempfile::NamedTempFile::new().unwrap();
    let b = tempfile::NamedTempFile::new().unwrap();
    let reader = fs::File::open(a.path()).unwrap();
    let writer = fs::OpenOptions::new().write(true).open(b.path()).unwrap();
    let pid = std::process::id();
    let old_flags = read_fdinfo_flags(pid, reader.as_raw_fd() as u32).unwrap();
    assert_eq!(old_flags & libc::O_ACCMODE as u32, libc::O_RDONLY as u32);
    // Model one proc descriptor slot: after A's early fdinfo observation,
    // replacement with B occurs immediately before the first metadata read.
    let replaced = Cell::new(false);
    let calls = Cell::new(0);
    let (metadata, _, flags) = read_descriptor(
        &[writer.metadata().unwrap().dev()],
        true,
        || {
            replaced.set(true);
            calls.set(calls.get() + 1);
            writer.metadata()
        },
        || {
            assert!(replaced.get());
            assert_eq!(calls.get(), 1);
            Ok(b"B".to_vec())
        },
        || {
            assert_eq!(calls.get(), 1);
            let file = if replaced.get() { &writer } else { &reader };
            read_fdinfo_flags(pid, file.as_raw_fd() as u32)
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(metadata.ino(), writer.metadata().unwrap().ino());
    assert_eq!(
        flags.unwrap() & libc::O_ACCMODE as u32,
        libc::O_WRONLY as u32
    );
}

#[test]
fn flags_are_optional_and_inode_replacement_is_rejected() {
    let a = tempfile::NamedTempFile::new().unwrap();
    let b = tempfile::NamedTempFile::new().unwrap();
    let calls = Cell::new(0);
    assert!(read_descriptor(
        &[a.as_file().metadata().unwrap().dev()],
        false,
        || {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                a.as_file()
            } else {
                b.as_file()
            }
            .metadata()
        },
        || Ok(Vec::new()),
        || panic!("flags were not requested"),
    )
    .unwrap()
    .is_none());
}
