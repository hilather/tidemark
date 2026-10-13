//! Owned by: W5-A
//! Linux fanotify bindings and bounds-checked event decoding.

#![allow(unsafe_code)]

use std::{
    ffi::CString,
    io,
    os::fd::{AsFd, BorrowedFd, OwnedFd},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
    path::Path,
};

/// Results of independently trying kernel initialization and mark variants.
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
/// Probe each reporting mode with harmless close-write marks; all groups close on return.
pub fn probe(mount: &Path) -> io::Result<FanotifyCaps> {
    let mut caps = FanotifyCaps {
        available: false,
        fd_mode: false,
        report_fid: false,
        report_dfid_name: false,
        mark_filesystem: false,
        error: None,
    };
    for mode in [InitMode::DfidName, InitMode::Fid, InitMode::FdMode] {
        match Fanotify::init(mode) {
            Ok(group) => {
                let filesystem = group.mark_filesystem(mount, mask::CLOSE_WRITE).is_ok();
                let mounted = group.mark_mount(mount, mask::CLOSE_WRITE).is_ok();
                let works = filesystem || mounted;
                caps.available |= works;
                caps.mark_filesystem |= filesystem;
                match mode {
                    InitMode::FdMode => caps.fd_mode = mounted,
                    InitMode::Fid => caps.report_fid = works,
                    InitMode::DfidName => caps.report_dfid_name = works,
                }
            }
            Err(e) => caps.error = Some(e.to_string()),
        }
    }
    if caps.available {
        caps.error = None;
    }
    Ok(caps)
}
/// Reserved for W5-A.
#[derive(Clone, Copy)]
pub enum InitMode {
    /// Contract variant.
    FdMode,
    /// Contract variant.
    Fid,
    /// Contract variant.
    DfidName,
}
/// Owns a nonblocking notification group; never requests permission events.
pub struct Fanotify {
    fd: OwnedFd,
}
impl Fanotify {
    /// Create a close-on-exec, nonblocking group in the requested reporting mode.
    pub fn init(mode: InitMode) -> io::Result<Fanotify> {
        // Flags from linux/fanotify.h: REPORT_FID=0x200, DFID_NAME=0xc00.
        let report = match mode {
            InitMode::FdMode => 0,
            InitMode::Fid => 0x200,
            InitMode::DfidName => 0xe00,
        };
        // SAFETY: fanotify_init has no pointer arguments; successful result is a new owned fd.
        let fd = unsafe {
            libc::fanotify_init(
                0x1 | 0x2 | report,
                (libc::O_RDONLY | libc::O_LARGEFILE | libc::O_CLOEXEC) as u32,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful syscall returned a unique descriptor.
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }
    /// Attach a notification mark to the named mount without following its final symlink.
    pub fn mark_mount(&self, path: &Path, mask: u64) -> io::Result<()> {
        self.mark(path, mask, 0x10)
    }
    /// Attach a notification mark to the filesystem containing path.
    pub fn mark_filesystem(&self, path: &Path, mask: u64) -> io::Result<()> {
        self.mark(path, mask, 0x100)
    }
    /// Read metadata records; EAGAIN yields an empty iterator.
    pub fn read<'b>(&self, buf: &'b mut [u8]) -> io::Result<EventIter<'b>> {
        // SAFETY: buf is writable for its full length and fd remains borrowed.
        let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::WouldBlock {
                return Ok(EventIter { buffer: &[] });
            }
            return Err(e);
        }
        Ok(EventIter {
            buffer: &buf[..n as usize],
        })
    }
    fn mark(&self, path: &Path, mask: u64, kind: u32) -> io::Result<()> {
        let path =
            CString::new(path.as_os_str().as_bytes()).map_err(|_| invalid("NUL in mark path"))?;
        // SAFETY: path is NUL terminated and valid throughout the call; fd is borrowed.
        let result = unsafe {
            libc::fanotify_mark(
                self.fd.as_raw_fd(),
                1 | 4 | kind,
                mask,
                libc::AT_FDCWD,
                path.as_ptr(),
            )
        };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    /// Borrow the group descriptor for polling.
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}
/// An owned notification decoded from kernel metadata and information records.
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
/// Resolve a bounded kernel handle with O_PATH, without opening file contents.
pub fn open_by_handle(
    mount_fd: BorrowedFd<'_>,
    handle_type: i32,
    handle: &[u8],
) -> io::Result<OwnedFd> {
    if handle.len() > 128 {
        return Err(invalid("file handle exceeds kernel maximum"));
    }
    // u64 storage gives file_handle sufficient alignment, including its variable tail.
    let mut storage = [0u64; 17];
    let ptr = storage.as_mut_ptr().cast::<u8>();
    // SAFETY: storage has 136 bytes, enough for the 8-byte header and capped handle.
    unsafe {
        std::ptr::copy_nonoverlapping((handle.len() as u32).to_ne_bytes().as_ptr(), ptr, 4);
        std::ptr::copy_nonoverlapping(handle_type.to_ne_bytes().as_ptr(), ptr.add(4), 4);
        std::ptr::copy_nonoverlapping(handle.as_ptr(), ptr.add(8), handle.len());
    }
    // SAFETY: aligned initialized file_handle is valid throughout the syscall.
    let fd = unsafe {
        libc::open_by_handle_at(
            mount_fd.as_raw_fd(),
            ptr.cast(),
            libc::O_PATH | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is newly returned and uniquely owned.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
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

/// Non-owning metadata from one event, with offsets into the original buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    /// Total length, including trailing information and padding.
    pub event_len: u32,
    /// Kernel metadata version (currently 3).
    pub version: u8,
    /// Length of the metadata header.
    pub metadata_len: u16,
    /// Notification mask.
    pub mask: u64,
    /// Descriptor number only; this record never owns or closes it.
    pub raw_fd: i32,
    /// Originating process ID.
    pub pid: i32,
    /// Trailing information byte range within the input buffer.
    pub information: std::ops::Range<usize>,
}

/// Decode kernel event bytes without syscalls, descriptor ownership or allocation.
/// Each record consumes at least 24 bytes; the first malformed record yields an
/// InvalidData error and ends iteration. Event lengths must be 8-byte aligned.
/// Information records are validated for FID-mode notifications, as in EventIter.
#[doc(hidden)]
pub fn parse_event_buffer(buffer: &[u8]) -> EventRecords<'_> {
    EventRecords { buffer, offset: 0 }
}

/// Allocation-free iterator returned by [`parse_event_buffer`].
pub struct EventRecords<'a> {
    buffer: &'a [u8],
    offset: usize,
}
impl Iterator for EventRecords<'_> {
    type Item = io::Result<EventRecord>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.buffer.len() {
            return None;
        }
        let b = &self.buffer[self.offset..];
        let result = parse_record(b, self.offset);
        match &result {
            Ok(record) => self.offset += record.event_len as usize,
            Err(_) => self.offset = self.buffer.len(),
        }
        Some(result)
    }
}
impl std::iter::FusedIterator for EventRecords<'_> {}

fn parse_record(b: &[u8], offset: usize) -> io::Result<EventRecord> {
    if b.len() < 24 {
        return Err(invalid("truncated metadata"));
    }
    let event_len = u32_at(b, 0);
    let len = event_len as usize;
    let metadata_len = u16::from_ne_bytes([b[6], b[7]]);
    let meta = usize::from(metadata_len);
    if b[4] != 3
        || len < 24
        || len > b.len()
        || len % 8 != 0
        || meta < 24
        || meta > len
        || meta % 8 != 0
    {
        return Err(invalid("invalid fanotify metadata"));
    }
    let mask = u64::from_ne_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]);
    let raw_fd = u32_at(b, 16) as i32;
    if raw_fd < 0 && mask & 0x4000 == 0 {
        // This also bounds every variable-length information record before decoding.
        decode_information(b, meta, len)?;
    }
    Ok(EventRecord {
        event_len,
        version: b[4],
        metadata_len,
        mask,
        raw_fd,
        pid: u32_at(b, 20) as i32,
        information: offset + meta..offset + len,
    })
}

/// Bounds-checked event iterator; dropping it closes descriptors in unread records.
pub struct EventIter<'b> {
    buffer: &'b [u8],
}
impl Iterator for EventIter<'_> {
    type Item = io::Result<RawEvent>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.is_empty() {
            return None;
        }
        // SAFETY: EventIter is constructed only from a kernel read (or tests
        // transferring unique descriptors). Take ownership once, including on
        // validation failure, so malformed kernel records retain cleanup behavior.
        let fd = if self.buffer.len() >= 24 {
            let raw_fd = u32_at(self.buffer, 16) as i32;
            if raw_fd >= 0 {
                Some(unsafe { OwnedFd::from_raw_fd(raw_fd) })
            } else {
                None
            }
        } else {
            None
        };
        let result = decode(self.buffer, fd);
        match result {
            Ok((event, len)) => {
                self.buffer = &self.buffer[len..];
                Some(Ok(event))
            }
            Err(e) => {
                let len = if self.buffer.len() >= 24 {
                    u32_at(self.buffer, 0) as usize
                } else {
                    0
                };
                self.buffer = if len >= 24 && len <= self.buffer.len() {
                    &self.buffer[len..]
                } else {
                    &[]
                };
                Some(Err(e))
            }
        }
    }
}
impl Drop for EventIter<'_> {
    fn drop(&mut self) {
        // Consume unread records so abandoned batches never leak event descriptors.
        while self.next().is_some() {}
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
// linux/fanotify.h byte offsets: metadata event_len=0, vers=4, metadata_len=6,
// mask=8, fd=16, pid=20 (24 bytes). Info header type=0, len=2 (4 bytes);
// FID fsid=4, file_handle bytes=12, handle_type=16, handle=20.
fn decode(b: &[u8], fd: Option<OwnedFd>) -> io::Result<(RawEvent, usize)> {
    let record = parse_event_buffer(b)
        .next()
        .ok_or_else(|| invalid("empty buffer"))??;
    let len = record.event_len as usize;
    if record.mask & 0x4000 != 0 {
        return Ok((RawEvent::Overflow, len));
    }
    if let Some(fd) = fd {
        return Ok((
            RawEvent::Fd {
                mask: record.mask,
                pid: record.pid,
                fd,
            },
            len,
        ));
    }
    let info = decode_information(b, usize::from(record.metadata_len), len)?;
    Ok((
        RawEvent::Fid {
            mask: record.mask,
            pid: record.pid,
            fsid: info.fsid,
            handle_type: info.handle_type,
            handle: info.handle.to_vec(),
            name: info.name.map(<[u8]>::to_vec),
        },
        len,
    ))
}
struct FidInfo<'a> {
    fsid: [i32; 2],
    handle_type: i32,
    handle: &'a [u8],
    name: Option<&'a [u8]>,
}
fn decode_information(b: &[u8], metadata_len: usize, len: usize) -> io::Result<FidInfo<'_>> {
    let mut at = metadata_len;
    let mut event = None;
    let mut priority = 0;
    while at < len {
        if len - at < 4 {
            return Err(invalid("truncated info header"));
        }
        let record_len = u16::from_ne_bytes([b[at + 2], b[at + 3]]) as usize;
        if record_len < 4 || record_len > len - at {
            return Err(invalid("invalid info length"));
        }
        let r = &b[at..at + record_len];
        if matches!(r[0], 1..=3) {
            if r.len() < 20 {
                return Err(invalid("truncated file handle"));
            }
            let bytes = u32_at(r, 12) as usize;
            if bytes > 128 || bytes > r.len() - 20 {
                return Err(invalid("invalid handle length"));
            }
            let name = if r[0] == 2 {
                let tail = &r[20 + bytes..];
                let end = tail
                    .iter()
                    .position(|v| *v == 0)
                    .ok_or_else(|| invalid("unterminated name"))?;
                Some(&tail[..end])
            } else {
                None
            };
            let candidate_priority = match r[0] {
                2 => 3,
                1 => 2,
                _ => 1,
            };
            if candidate_priority > priority {
                priority = candidate_priority;
                event = Some(FidInfo {
                    fsid: [u32_at(r, 4) as i32, u32_at(r, 8) as i32],
                    handle_type: u32_at(r, 16) as i32,
                    handle: &r[20..20 + bytes],
                    name,
                });
            }
        }
        at += record_len;
    }
    event.ok_or_else(|| invalid("missing FID record"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    fn metadata(mask: u64, fd: i32, len: u32) -> Vec<u8> {
        let mut b = vec![0; 24];
        b[..4].copy_from_slice(&len.to_ne_bytes());
        b[4] = 3;
        b[6..8].copy_from_slice(&24u16.to_ne_bytes());
        b[8..16].copy_from_slice(&mask.to_ne_bytes());
        b[16..20].copy_from_slice(&fd.to_ne_bytes());
        b[20..24].copy_from_slice(&42i32.to_ne_bytes());
        b
    }
    #[test]
    fn multiple_overflows_and_truncated_tail() {
        let mut bytes = metadata(0x4000, -1, 24);
        bytes.extend(metadata(0x4000, -1, 24));
        bytes.push(0);
        let mut iter = EventIter { buffer: &bytes };
        assert!(matches!(iter.next().unwrap().unwrap(), RawEvent::Overflow));
        assert!(matches!(iter.next().unwrap().unwrap(), RawEvent::Overflow));
        assert!(iter.next().unwrap().is_err());
        assert!(iter.next().is_none());
    }
    fn fid(named: bool) -> Vec<u8> {
        let mut record = vec![if named { 2 } else { 1 }, 0, 0, 0];
        record.extend(7i32.to_ne_bytes());
        record.extend(8i32.to_ne_bytes());
        record.extend(3u32.to_ne_bytes());
        record.extend(9i32.to_ne_bytes());
        record.extend([1, 2, 3]);
        if named {
            record.extend(b"raw\xff\0");
        }
        record.resize(record.len().next_multiple_of(8), 0);
        let len = record.len() as u16;
        record[2..4].copy_from_slice(&len.to_ne_bytes());
        let mut bytes = metadata(mask::CREATE, -1, 24 + u32::from(len));
        bytes.extend(record);
        bytes
    }
    #[test]
    fn fid_with_and_without_name_and_invalid_lengths() {
        for named in [false, true] {
            let bytes = fid(named);
            match (EventIter { buffer: &bytes }).next().unwrap().unwrap() {
                RawEvent::Fid {
                    fsid,
                    handle_type,
                    handle,
                    name,
                    ..
                } => {
                    assert_eq!(fsid, [7, 8]);
                    assert_eq!(handle_type, 9);
                    assert_eq!(handle, [1, 2, 3]);
                    assert_eq!(
                        name,
                        if named {
                            Some(b"raw\xff".to_vec())
                        } else {
                            None
                        }
                    );
                }
                _ => panic!("expected FID"),
            }
            for end in 0..bytes.len() {
                if end == 0 {
                    continue;
                }
                assert!(EventIter {
                    buffer: &bytes[..end]
                }
                .next()
                .unwrap()
                .is_err());
            }
        }
        let mut b = fid(true);
        b[36..40].copy_from_slice(&u32::MAX.to_ne_bytes());
        assert!(EventIter { buffer: &b }.next().unwrap().is_err());
        let mut b = fid(true);
        b[47..].fill(1);
        assert!(EventIter { buffer: &b }.next().unwrap().is_err());
        for len in [0, 23, u32::MAX] {
            let b = metadata(0x4000, -1, len);
            assert!(EventIter { buffer: &b }.next().unwrap().is_err());
        }
    }
    #[test]
    fn descriptors_close_on_error_and_abandoned_batch() {
        const CHILD: &str = "TIDEMARK_W5A_PARSER_FD_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "fanotify::tests::descriptors_close_on_error_and_abandoned_batch",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
        let before = count();

        // Check each exact descriptor rather than all process fds (parallel tests open files).
        use std::os::fd::IntoRawFd;
        let first = std::fs::File::open("/").unwrap().into_raw_fd();
        let second = std::fs::File::open("/").unwrap().into_raw_fd();
        let mut bytes = metadata(mask::CLOSE_WRITE, first, 24);
        bytes.extend(metadata(mask::CLOSE_WRITE, second, 24));
        let mut iter = EventIter { buffer: &bytes };
        drop(iter.next());
        drop(iter);
        assert_closed(first);
        assert_closed(second);
        let fd = std::fs::File::open("/").unwrap().into_raw_fd();
        let mut bytes = metadata(mask::CLOSE_WRITE, fd, 24);
        bytes[4] = 2;
        assert!(EventIter { buffer: &bytes }.next().unwrap().is_err());
        assert_closed(fd);
        assert_eq!(count(), before);
    }
    fn assert_closed(fd: i32) {
        // SAFETY: fcntl validates the integer fd; no pointers or ownership transfer.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    }
    #[cfg(feature = "privileged")]
    #[test]
    fn fanotify_probe_reports_capabilities() {
        let temp = tempfile::tempdir().unwrap();
        let caps = probe(temp.path()).unwrap();
        eprintln!(
            "fanotify caps: available={} fd={} fid={} dfid_name={} filesystem={} error={:?}",
            caps.available,
            caps.fd_mode,
            caps.report_fid,
            caps.report_dfid_name,
            caps.mark_filesystem,
            caps.error
        );
        assert!(caps.fd_mode, "{}", caps.error.unwrap_or_default());
    }
}
