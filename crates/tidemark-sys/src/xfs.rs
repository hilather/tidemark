//! Owned by: W1-C
//! Cross-task signatures reserved for W1-C.

use std::{io, os::fd::BorrowedFd};

/// Reserved for W1-C.
pub const XFS_SUPER_MAGIC: i64 = 0x5846_5342;
/// Reserved for W1-C.
pub fn is_xfs(fd: BorrowedFd<'_>) -> io::Result<bool> {
    let _ = fd;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-C.
pub struct XfsGeometry {
    #[doc = "Field specified by the owning task."]
    pub blocksize: u32,
    #[doc = "Field specified by the owning task."]
    pub agblocks: u32,
    #[doc = "Field specified by the owning task."]
    pub agcount: u32,
    #[doc = "Field specified by the owning task."]
    pub inodesize: u32,
    #[doc = "Field specified by the owning task."]
    pub dirblocksize: u32,
    #[doc = "Field specified by the owning task."]
    pub uuid: [u8; 16],
    #[doc = "Field specified by the owning task."]
    pub v5: Option<bool>,
    #[doc = "Field specified by the owning task."]
    pub ftype: Option<bool>,
    #[doc = "Field specified by the owning task."]
    pub reflink: Option<bool>,
}
/// Reserved for W1-C.
pub fn geometry(fd: BorrowedFd<'_>) -> io::Result<XfsGeometry> {
    let _ = fd;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-C.
pub struct BulkstatSupport {
    #[doc = "Field specified by the owning task."]
    pub v1: bool,
    #[doc = "Field specified by the owning task."]
    pub v5: bool,
}
/// Reserved for W1-C.
pub fn bulkstat_probe(fd: BorrowedFd<'_>) -> BulkstatSupport {
    let _ = fd;
    BulkstatSupport {
        v1: false,
        v5: false,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Reserved for W1-C.
pub struct BulkInode {
    #[doc = "Field specified by the owning task."]
    pub ino: u64,
    #[doc = "Field specified by the owning task."]
    pub mode: u32,
    #[doc = "Field specified by the owning task."]
    pub nlink: u32,
    #[doc = "Field specified by the owning task."]
    pub uid: u32,
    #[doc = "Field specified by the owning task."]
    pub gid: u32,
    #[doc = "Field specified by the owning task."]
    pub size: u64,
    #[doc = "Field specified by the owning task."]
    pub disk_bytes: u64,
    #[doc = "Field specified by the owning task."]
    pub mtime: i64,
    #[doc = "Field specified by the owning task."]
    pub projid: u32,
    #[doc = "Field specified by the owning task."]
    pub xflags: u32,
}
/// Reserved for W1-C.
pub fn ag_ino_range(geo: &XfsGeometry, agno: u32) -> (u64, u64) {
    let _ = geo;
    let _ = agno;
    (0, 0)
}
/// Reserved for W1-C.
pub fn bulkstat(fd: BorrowedFd<'_>, start: u64, end: u64, batch: usize) -> BulkstatIter<'_> {
    let _ = fd;
    let _ = start;
    let _ = end;
    let _ = batch;
    BulkstatIter { fd, yielded: false }
}

/// Iterator shape left to W1-C; emits one unsupported error, then terminates.
pub struct BulkstatIter<'a> {
    fd: BorrowedFd<'a>,
    yielded: bool,
}
impl Iterator for BulkstatIter<'_> {
    type Item = io::Result<Vec<BulkInode>>;
    fn next(&mut self) -> Option<Self::Item> {
        let _ = self.fd;
        if self.yielded {
            None
        } else {
            self.yielded = true;
            Some(Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "NotYetImplemented",
            )))
        }
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        let stdin = std::io::stdin();
        assert_eq!(
            is_xfs(std::os::fd::AsFd::as_fd(&stdin))
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            geometry(std::os::fd::AsFd::as_fd(&stdin))
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        let caps = bulkstat_probe(std::os::fd::AsFd::as_fd(&stdin));
        assert!(!caps.v1 && !caps.v5);
        assert_eq!(
            ag_ino_range(
                &XfsGeometry {
                    blocksize: 0,
                    agblocks: 0,
                    agcount: 0,
                    inodesize: 0,
                    dirblocksize: 0,
                    uuid: [0; 16],
                    v5: None,
                    ftype: None,
                    reflink: None
                },
                0
            ),
            (0, 0)
        );
        let mut iter = bulkstat(std::os::fd::AsFd::as_fd(&stdin), 0, 0, 0);
        assert_eq!(
            iter.next().unwrap().err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(iter.next().is_none());
    }
}
