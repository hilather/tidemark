//! Owned by: W1-D
//! Cross-task signatures reserved for W1-D.

use std::io;
use tidemark_core::scan::{DevId, Ino};
/// Reserved for W1-D.
pub fn list_pids() -> io::Result<Vec<u32>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-D.
pub fn read_comm(pid: u32) -> io::Result<String> {
    let _ = pid;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-D.
pub fn parse_stat_starttime(stat: &str) -> Option<u64> {
    let _ = stat;
    None
}
/// Reserved for W1-D.
pub fn read_starttime(pid: u32) -> io::Result<u64> {
    let _ = pid;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-D.
pub fn parse_cgroup_unit(cgroup: &str) -> Option<String> {
    let _ = cgroup;
    None
}
/// Reserved for W1-D.
pub fn read_cgroup_unit(pid: u32) -> io::Result<Option<String>> {
    let _ = pid;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-D.
pub fn list_fds(pid: u32) -> io::Result<Vec<u32>> {
    let _ = pid;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-D.
pub fn read_fd_target(pid: u32, fd: u32) -> io::Result<Vec<u8>> {
    let _ = pid;
    let _ = fd;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-D.
pub fn parse_fdinfo_flags(fdinfo: &str) -> Option<u32> {
    let _ = fdinfo;
    None
}
/// Reserved for W1-D.
pub fn read_fdinfo_flags(pid: u32, fd: u32) -> io::Result<u32> {
    let _ = pid;
    let _ = fd;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-D.
pub fn read_uid(pid: u32) -> io::Result<u32> {
    let _ = pid;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}

/// Open regular-file metadata; shape specified by W1-D.
pub struct OpenFile {
    #[doc = "Field specified by the owning task."]
    pub pid: u32,
    #[doc = "Field specified by the owning task."]
    pub fd: u32,
    #[doc = "Field specified by the owning task."]
    pub target: Vec<u8>,
    #[doc = "Field specified by the owning task."]
    pub dev: DevId,
    #[doc = "Field specified by the owning task."]
    pub ino: Ino,
    #[doc = "Field specified by the owning task."]
    pub nlink: u64,
    #[doc = "Field specified by the owning task."]
    pub size: u64,
    #[doc = "Field specified by the owning task."]
    pub disk: u64,
    #[doc = "Field specified by the owning task."]
    pub flags: Option<u32>,
}
/// Reserved for W1-D.
pub fn scan_open_files(devs: &[DevId], want_flags: bool) -> io::Result<Vec<OpenFile>> {
    let _ = devs;
    let _ = want_flags;
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
            list_pids().err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            read_comm(0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(parse_stat_starttime("").is_none());
        assert_eq!(
            read_starttime(0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(parse_cgroup_unit("").is_none());
        assert_eq!(
            read_cgroup_unit(0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            list_fds(0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            read_fd_target(0, 0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(parse_fdinfo_flags("").is_none());
        assert_eq!(
            read_fdinfo_flags(0, 0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            read_uid(0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            scan_open_files(&[], false).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
