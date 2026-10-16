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

#[test]
fn typed_and_raw_out_of_memory_abort() {
    let allocation_error = Vec::<u8>::new().try_reserve(usize::MAX).unwrap_err();
    let typed: io::Error = allocation_error.into();
    assert_eq!(typed.kind(), io::ErrorKind::OutOfMemory);
    assert_eq!(typed.raw_os_error(), None);
    let error = per_fd::<()>(Err(typed)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::OutOfMemory);
    assert_eq!(error.raw_os_error(), None);
    for errno in [libc::ENOMEM, libc::EMFILE, libc::ENFILE] {
        assert_eq!(
            per_fd::<()>(Err(io::Error::from_raw_os_error(errno)))
                .unwrap_err()
                .raw_os_error(),
            Some(errno)
        );
    }
}

#[test]
fn final_starttime_errors_discard_only_transient_processes() {
    fn inventory() -> Inventory {
        let mut result = Inventory {
            files: Vec::new(),
            paths: 0,
            limit: MAX_INVENTORY_BYTES,
            scratch: ID_SCRATCH_BYTES,
        };
        for pid in [1, 2] {
            result
                .push(OpenFile {
                    pid,
                    starttime: 42,
                    fd: 3,
                    target: b"held".to_vec(),
                    dev: 1,
                    ino: 1,
                    nlink: 1,
                    size: 0,
                    disk: 0,
                    flags: None,
                })
                .unwrap();
        }
        result
    }
    for errno in [
        libc::EMFILE,
        libc::ENFILE,
        libc::ENOMEM,
        libc::EIO,
        libc::EINVAL,
    ] {
        let mut records = inventory();
        let error = finish_process(
            &mut records,
            1,
            42,
            Err(io::Error::from_raw_os_error(errno)),
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(errno));
    }
    let mut records = inventory();
    assert_eq!(
        finish_process(&mut records, 1, 42, Err(io::ErrorKind::OutOfMemory.into()))
            .unwrap_err()
            .kind(),
        io::ErrorKind::OutOfMemory
    );
    for errno in [libc::ENOENT, libc::ESRCH, libc::EACCES, libc::EPERM] {
        let mut records = inventory();
        finish_process(
            &mut records,
            1,
            42,
            Err(io::Error::from_raw_os_error(errno)),
        )
        .unwrap();
        assert_eq!(records.files.len(), 1);
        assert_eq!(records.files[0].pid, 1);
        assert_eq!(records.paths, records.files[0].target.capacity());
    }
    for (observed, retained) in [(42, 2), (43, 1)] {
        let mut records = inventory();
        finish_process(&mut records, 1, 42, Ok(observed)).unwrap();
        assert_eq!(records.files.len(), retained);
    }
}
