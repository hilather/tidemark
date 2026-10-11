//! Owned by: W5-A
//! Cross-task interface stubs.

use std::{
    io,
    os::fd::{AsFd, BorrowedFd, OwnedFd},
    path::Path,
};

/// Reserved for W5-A.
pub struct FanotifyCaps {
    /// Contract field.
    pub available: bool,
    /// Contract field.
    pub fd_mode: bool,
    /// Contract field.
    pub report_fid: bool,
    /// Contract field.
    pub report_dfid_name: bool,
    /// Contract field.
    pub mark_filesystem: bool,
    /// Contract field.
    pub error: Option<String>,
}
/// Reserved for W5-A.
pub fn probe(mount: &Path) -> io::Result<FanotifyCaps> {
    let _ = mount;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W5-A.
pub enum InitMode {
    /// Contract variant.
    FdMode,
    /// Contract variant.
    Fid,
    /// Contract variant.
    DfidName,
}
/// Reserved for W5-A.
pub struct Fanotify {
    fd: OwnedFd,
}
impl Fanotify {
    /// Reserved for W5-A.
    pub fn init(mode: InitMode) -> io::Result<Fanotify> {
        let _ = mode;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "NotYetImplemented",
        ))
    }
    /// Reserved for W5-A.
    pub fn mark_mount(&self, path: &Path, mask: u64) -> io::Result<()> {
        let _ = path;
        let _ = mask;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "NotYetImplemented",
        ))
    }
    /// Reserved for W5-A.
    pub fn mark_filesystem(&self, path: &Path, mask: u64) -> io::Result<()> {
        let _ = path;
        let _ = mask;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "NotYetImplemented",
        ))
    }
    /// Reserved for W5-A.
    pub fn read<'b>(&self, buf: &'b mut [u8]) -> io::Result<EventIter<'b>> {
        let _ = buf;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "NotYetImplemented",
        ))
    }
    /// Reserved for W5-A.
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}
/// Reserved for W5-A.
pub enum RawEvent {
    /// Contract variant.
    Overflow,
    /// Contract variant.
    Fd {
        /// Kernel event field.
        mask: u64,
        /// Kernel event field.
        pid: i32,
        /// Kernel event field.
        fd: OwnedFd,
    },
    /// Contract variant.
    Fid {
        /// Kernel event field.
        mask: u64,
        /// Kernel event field.
        pid: i32,
        /// Kernel event field.
        fsid: [i32; 2],
        /// Kernel event field.
        handle_type: i32,
        /// Kernel event field.
        handle: Vec<u8>,
        /// Kernel event field.
        name: Option<Vec<u8>>,
    },
}
/// Reserved for W5-A.
pub fn open_by_handle(
    mount_fd: BorrowedFd<'_>,
    handle_type: i32,
    handle: &[u8],
) -> io::Result<OwnedFd> {
    let _ = mount_fd;
    let _ = handle_type;
    let _ = handle;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W5-A.
pub mod mask {
    /// Reserved for W5-A.
    pub const CLOSE_WRITE: u64 = 0x8;
    /// Reserved for W5-A.
    pub const MODIFY: u64 = 0x2;
    /// Reserved for W5-A.
    pub const CREATE: u64 = 0x100;
    /// Reserved for W5-A.
    pub const DELETE: u64 = 0x200;
    /// Reserved for W5-A.
    pub const MOVED_FROM: u64 = 0x40;
    /// Reserved for W5-A.
    pub const MOVED_TO: u64 = 0x80;
    /// Reserved for W5-A.
    pub const ONDIR: u64 = 0x40000000;
}

/// Event iterator shape reserved for W5-A.
pub struct EventIter<'b> {
    buffer: &'b [u8],
}
impl Iterator for EventIter<'_> {
    type Item = io::Result<RawEvent>;
    fn next(&mut self) -> Option<Self::Item> {
        let _ = self.buffer;
        None
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn every_stub_has_an_inert_result() -> io::Result<()> {
        let path = Path::new("/");
        assert_eq!(
            probe(path).err().map(|e| e.kind()),
            Some(io::ErrorKind::Unsupported)
        );
        for mode in [InitMode::FdMode, InitMode::Fid, InitMode::DfidName] {
            assert_eq!(
                Fanotify::init(mode).err().map(|e| e.kind()),
                Some(io::ErrorKind::Unsupported)
            );
        }
        // A harmless owned descriptor exercises otherwise unreachable instance methods.
        let f = Fanotify {
            fd: std::fs::File::open(path)?.into(),
        };
        assert_eq!(
            f.mark_mount(path, 0).err().map(|e| e.kind()),
            Some(io::ErrorKind::Unsupported)
        );
        assert_eq!(
            f.mark_filesystem(path, 0).err().map(|e| e.kind()),
            Some(io::ErrorKind::Unsupported)
        );
        assert_eq!(
            f.read(&mut []).err().map(|e| e.kind()),
            Some(io::ErrorKind::Unsupported)
        );
        assert_eq!(
            open_by_handle(f.as_fd(), 0, &[]).err().map(|e| e.kind()),
            Some(io::ErrorKind::Unsupported)
        );
        assert!(EventIter { buffer: &[] }.next().is_none());
        assert_eq!(mask::CLOSE_WRITE, 0x8);
        assert_eq!(mask::MODIFY, 0x2);
        assert_eq!(mask::CREATE, 0x100);
        assert_eq!(mask::DELETE, 0x200);
        assert_eq!(mask::MOVED_FROM, 0x40);
        assert_eq!(mask::MOVED_TO, 0x80);
        assert_eq!(mask::ONDIR, 0x40000000);
        Ok(())
    }
}
