//! Owned by: W1-C
//! Read-only XFS geometry and bounded bulk inode iteration.
#![allow(unsafe_code)]

use crate::gen::bindings::*;
use std::{
    io,
    mem::MaybeUninit,
    os::fd::{AsRawFd, BorrowedFd},
};

/// XFS filesystem magic returned by fstatfs.
pub const XFS_SUPER_MAGIC: i64 = 0x5846_5342;
/// Check the filesystem containing an already-open descriptor.
pub fn is_xfs(fd: BorrowedFd<'_>) -> io::Result<bool> {
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: fd is borrowed for this call; stat points to a writable statfs-sized
    // buffer. Only a successful fstatfs result is read.
    if unsafe { libc::fstatfs(fd.as_raw_fd(), stat.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstatfs succeeded and initialized the statfs fields.
    let stat = unsafe { stat.assume_init() };
    Ok(stat.f_type == XFS_SUPER_MAGIC)
}
/// Filesystem geometry and superblock feature flags.
pub struct XfsGeometry {
    /// Data block size in bytes.
    pub blocksize: u32,
    /// Filesystem blocks per allocation group.
    pub agblocks: u32,
    /// Number of allocation groups.
    pub agcount: u32,
    /// Inode size in bytes.
    pub inodesize: u32,
    /// Directory block size in bytes.
    pub dirblocksize: u32,
    /// Filesystem UUID bytes.
    pub uuid: [u8; 16],
    /// Whether the superblock uses version 5.
    pub v5: Option<bool>,
    /// Whether directory entries record inode types.
    pub ftype: Option<bool>,
    /// Whether shared data extents are supported.
    pub reflink: Option<bool>,
}

/// Read the baseline geometry ioctl, including its superblock flags word.
/// Errors retain their OS errno, including EPERM/PermissionDenied.
pub fn geometry(fd: BorrowedFd<'_>) -> io::Result<XfsGeometry> {
    let mut raw = xfs_fsop_geom_v1::default();
    // SAFETY: raw has the measured 112-byte EL8 C layout and remains writable
    // for the entire synchronous ioctl; the descriptor remains borrowed.
    let rc = unsafe { libc::ioctl(fd.as_raw_fd(), XFS_IOC_FSGEOMETRY_V1, &mut raw) };
    ioctl_result(rc)?;
    Ok(XfsGeometry {
        blocksize: raw.blocksize,
        agblocks: raw.agblocks,
        agcount: raw.agcount,
        inodesize: raw.inodesize,
        dirblocksize: raw.dirblocksize,
        uuid: raw.uuid,
        v5: Some(raw.flags & XFS_FSOP_GEOM_FLAGS_V5SB != 0),
        ftype: Some(raw.flags & XFS_FSOP_GEOM_FLAGS_FTYPE != 0),
        reflink: Some(raw.flags & XFS_FSOP_GEOM_FLAGS_REFLINK != 0),
    })
}
/// Availability to this caller (permission failures count as unavailable).
/// Bulk metadata mode requires `v5 == true`; v1 is diagnostic only.
pub struct BulkstatSupport {
    /// Legacy pointer-based bulkstat is callable.
    pub v1: bool,
    /// Upstream v5 bulkstat is callable.
    pub v5: bool,
}
/// Probe each interface independently with a one-record metadata request.
pub fn bulkstat_probe(fd: BorrowedFd<'_>) -> BulkstatSupport {
    let mut last = 0;
    let mut legacy = [MaybeUninit::zeroed()];
    let v1 = request_v1(fd, &mut last, &mut legacy).is_ok();
    let mut modern = vec![0u64; 8 + 24];
    let v5 = request_v5(fd, 0, 1, None, &mut modern).is_ok();
    BulkstatSupport { v1, v5 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Metadata for one allocated inode.
pub struct BulkInode {
    /// Filesystem inode number.
    pub ino: u64,
    /// File type and permission bits.
    pub mode: u32,
    /// Exact 32-bit hard-link count from v5 bulkstat.
    pub nlink: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Apparent size in bytes.
    pub size: u64,
    /// Allocated bytes, including attributed shared extents.
    pub disk_bytes: u64,
    /// Signed Unix modification time in whole seconds.
    pub mtime: i64,
    /// Project quota ID.
    pub projid: u32,
    /// XFS extended inode flags.
    pub xflags: u32,
}

/// Inode interval [start, end) for an allocation group.
/// Invalid geometries, out-of-range groups or unrepresentable bounds return (0, 0).
pub fn ag_ino_range(geo: &XfsGeometry, agno: u32) -> (u64, u64) {
    if agno >= geo.agcount
        || geo.agblocks == 0
        || geo.inodesize == 0
        || !geo.blocksize.is_power_of_two()
        || !geo.inodesize.is_power_of_two()
        || geo.inodesize > geo.blocksize
    {
        return (0, 0);
    }
    // XFS agblklog is ceil(log2(agblocks)), not floor(log2(agblocks)).
    let block_bits = u32::BITS - (geo.agblocks - 1).leading_zeros();
    let inode_bits = (geo.blocksize / geo.inodesize).trailing_zeros();
    let Some(span) = 1u64.checked_shl(block_bits + inode_bits) else {
        return (0, 0);
    };
    match (
        span.checked_mul(u64::from(agno)),
        span.checked_mul(u64::from(agno) + 1),
    ) {
        (Some(start), Some(end)) => (start, end),
        _ => (0, 0),
    }
}

// Bounds memory to < 32 MiB across kernel and converted buffers. The caller
// receives InvalidInput rather than silent batch truncation for larger values.
const MAX_BATCH: usize = 65_536;
/// Iterate allocated inodes in [start, end) using v5 bulkstat only.
/// V5 resumes at its returned next inode. Unsupported kernels must use walk mode.
/// Each error is yielded once and terminates the iterator. No syscall occurs for
/// an empty interval. The caller must charge these metadata calls to its Gate.
pub fn bulkstat(fd: BorrowedFd<'_>, start: u64, end: u64, batch: usize) -> BulkstatIter<'_> {
    BulkstatIter {
        fd,
        start,
        end,
        batch,
        next_ino: start,
        blocksize: None,
        agno: None,
        modern: Vec::new(),
        done: start >= end,
    }
}
/// Bounded, descriptor-borrowing iterator over ascending XFS inode metadata.
pub struct BulkstatIter<'a> {
    fd: BorrowedFd<'a>,
    start: u64,
    end: u64,
    batch: usize,
    next_ino: u64,
    blocksize: Option<u32>,
    agno: Option<u32>,
    modern: Vec<u64>,
    done: bool,
}
fn ioctl_result(rc: libc::c_int) -> io::Result<()> {
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn request_v1(
    fd: BorrowedFd<'_>,
    last: &mut u64,
    buffer: &mut [MaybeUninit<xfs_bstat>],
) -> io::Result<usize> {
    #[cfg(test)]
    if tests::V1_MOCK.with(|mock| mock.get()) {
        tests::V1_CALLS.with(|calls| calls.set(calls.get() + 1));
        return Ok(1);
    }
    let mut count = 0i32;
    let mut req = xfs_fsop_bulkreq {
        lastip: last,
        icount: buffer.len() as i32,
        ubuffer: buffer.as_mut_ptr().cast(),
        ocount: &mut count,
    };
    // SAFETY: req has the measured C layout; its pointers reference last/count
    // and buffer.len() writable bstat slots, alive and exclusively borrowed
    // throughout the synchronous ioctl. icount matches the allocation.
    ioctl_result(unsafe { libc::ioctl(fd.as_raw_fd(), XFS_IOC_FSBULKSTAT, &mut req) })?;
    if count < 0 || count as usize > buffer.len() {
        return Err(invalid_data("invalid bulkstat output count"));
    }
    Ok(count as usize)
}
fn v5_ioctl(fd: BorrowedFd<'_>, buffer: &mut [u64]) -> io::Result<()> {
    #[cfg(test)]
    if let Some(result) = tests::V5_MOCK.with(|mock| mock.borrow_mut().as_mut().map(|f| f(buffer)))
    {
        return result;
    }
    // SAFETY: request_v5 supplies an aligned, initialized allocation matching
    // the advertised header and record capacity for this synchronous ioctl.
    ioctl_result(unsafe { libc::ioctl(fd.as_raw_fd(), XFS_IOC_BULKSTAT, buffer.as_mut_ptr()) })
}
fn request_v5(
    fd: BorrowedFd<'_>,
    ino: u64,
    batch: usize,
    agno: Option<u32>,
    buffer: &mut [u64],
) -> io::Result<(usize, u64)> {
    let header = xfs_bulk_ireq {
        ino,
        icount: batch as u32,
        flags: if agno.is_some() {
            XFS_BULK_IREQ_AGNO
        } else {
            0
        },
        agno: agno.unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: u64 storage is 8-byte aligned, enough for the 64-byte header plus
    // batch 192-byte records, and held exclusively until the ioctl returns.
    // All fields have integer bit patterns. Header write initializes reserved fields.
    unsafe {
        buffer.as_mut_ptr().cast::<xfs_bulk_ireq>().write(header);
    }
    v5_ioctl(fd, buffer)?;
    // SAFETY: the aligned initialized header remains in the live allocation.
    let header = unsafe { buffer.as_ptr().cast::<xfs_bulk_ireq>().read() };
    if header.ocount as usize > batch {
        return Err(invalid_data("invalid v5 output count"));
    }
    Ok((header.ocount as usize, header.ino))
}
// v5 bs_blocks is in filesystem blocks. Conversion is centralized here;
// bulkstat_blocks_match_statx validates this against statx's 512-byte units.
fn blocks_to_bytes(blocks: u64, blocksize: u32) -> Option<u64> {
    blocks.checked_mul(u64::from(blocksize))
}
fn convert_v5(bs: &xfs_bulkstat, blocksize: u32) -> io::Result<BulkInode> {
    Ok(BulkInode {
        ino: bs.bs_ino,
        mode: u32::from(bs.bs_mode),
        nlink: bs.bs_nlink,
        uid: bs.bs_uid,
        gid: bs.bs_gid,
        size: bs.bs_size,
        disk_bytes: blocks_to_bytes(bs.bs_blocks, blocksize)
            .ok_or_else(|| invalid_data("overflowing v5 block count"))?,
        mtime: bs.bs_mtime,
        projid: bs.bs_projectid,
        xflags: u32::try_from(bs.bs_xflags)
            .map_err(|_| invalid_data("unrepresentable v5 xflags"))?,
    })
}
impl BulkstatIter<'_> {
    fn read_batch(&mut self) -> io::Result<Vec<BulkInode>> {
        if self.batch == 0 || self.batch > MAX_BATCH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "bulkstat batch must be 1..=65536",
            ));
        }
        let blocksize = match self.blocksize {
            Some(value) => value,
            None => {
                let geo = geometry(self.fd)?;
                if !geo.blocksize.is_power_of_two() {
                    return Err(invalid_data("invalid XFS block size"));
                }
                let (_, span) = ag_ino_range(&geo, 0);
                if span != 0 && self.start.is_multiple_of(span) {
                    if let Ok(agno) = u32::try_from(self.start / span) {
                        if ag_ino_range(&geo, agno) == (self.start, self.end) {
                            self.agno = Some(agno);
                        }
                    }
                }
                self.blocksize = Some(geo.blocksize);
                geo.blocksize
            }
        };
        if self.modern.is_empty() {
            self.modern.resize(8 + 24 * self.batch, 0);
        }
        let (count, next) =
            request_v5(
                self.fd,
                self.next_ino,
                self.batch,
                self.agno,
                &mut self.modern,
            )
            .map_err(|error| {
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOTTY | libc::EINVAL | libc::EOPNOTSUPP | libc::ENOSYS)
                ) {
                    io::Error::new(io::ErrorKind::Unsupported,
                    "v5 bulkstat is unavailable; v1 is not used because its link counts are 16-bit")
                } else {
                    error
                }
            })?;
        let mut result = Vec::with_capacity(count);
        for i in 0..count {
            // SAFETY: count was checked against allocated slots; the header is
            // 8 u64s, each record 24 u64s. Storage is aligned, initialized and
            // live; all record fields are integers.
            let bs = unsafe {
                self.modern
                    .as_ptr()
                    .add(8 + i * 24)
                    .cast::<xfs_bulkstat>()
                    .read()
            };
            result.push(convert_v5(&bs, blocksize)?);
        }
        if count > 0 && next <= self.next_ino {
            return Err(invalid_data("v5 cursor did not advance"));
        }
        self.next_ino = next;
        self.finish_batch(result)
    }
    fn finish_batch(&mut self, mut result: Vec<BulkInode>) -> io::Result<Vec<BulkInode>> {
        if result.is_empty() {
            self.done = true;
            return Ok(result);
        }
        let mut previous = None;
        for record in &result {
            if record.ino < self.start || previous.is_some_and(|ino| record.ino <= ino) {
                return Err(invalid_data(
                    "bulkstat returned unordered or out-of-range inodes",
                ));
            }
            previous = Some(record.ino);
        }
        if let Some(first_outside) = result.iter().position(|r| r.ino >= self.end) {
            result.truncate(first_outside);
            self.done = true;
        }
        if let Some(last) = result.last() {
            self.start = last.ino.saturating_add(1);
        }
        Ok(result)
    }
}
impl Iterator for BulkstatIter<'_> {
    type Item = io::Result<Vec<BulkInode>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.read_batch() {
            Ok(batch) if batch.is_empty() => None,
            Ok(batch) => Some(Ok(batch)),
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}
impl std::iter::FusedIterator for BulkstatIter<'_> {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::{fs::File, os::fd::AsFd};
    type Mock = Box<dyn FnMut(&mut [u64]) -> io::Result<()>>;
    thread_local! {
        pub(super) static V1_MOCK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        pub(super) static V1_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        pub(super) static V5_MOCK: std::cell::RefCell<Option<Mock>> = const { std::cell::RefCell::new(None) };
    }
    #[test]
    fn unavailable_v5_never_returns_legacy_metadata() {
        let file = File::open("/").unwrap();
        V1_MOCK.with(|mock| mock.set(true));
        assert_eq!(
            request_v1(file.as_fd(), &mut 0, &mut [MaybeUninit::zeroed()]).unwrap(),
            1
        );
        V1_CALLS.with(|calls| calls.set(0));
        for errno in [libc::ENOTTY, libc::EINVAL, libc::EOPNOTSUPP, libc::ENOSYS] {
            V5_MOCK.with(|mock| {
                *mock.borrow_mut() =
                    Some(Box::new(move |_| Err(io::Error::from_raw_os_error(errno))))
            });
            let mut iter = bulkstat(file.as_fd(), 0, u64::MAX, 1);
            // Geometry is supplied to avoid real ioctls in this unprivileged test.
            iter.blocksize = Some(4096);
            let error = iter.next().unwrap().unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Unsupported);
            assert!(error.to_string().contains("v5 bulkstat is unavailable"));
            assert!(error
                .to_string()
                .contains("v1 is not used because its link counts are 16-bit"));
            assert!(iter.next().is_none());
            V1_CALLS.with(|calls| assert_eq!(calls.get(), 0));
            V5_MOCK.with(|mock| *mock.borrow_mut() = None);
        }
        V1_MOCK.with(|mock| mock.set(false));
    }
    #[test]
    fn v5_permission_errors_keep_errno() {
        let file = File::open("/").unwrap();
        V5_MOCK.with(|mock| {
            *mock.borrow_mut() = Some(Box::new(|_| Err(io::Error::from_raw_os_error(libc::EPERM))))
        });
        let mut iter = bulkstat(file.as_fd(), 0, u64::MAX, 1);
        iter.blocksize = Some(4096);
        let error = iter.next().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(error.raw_os_error(), Some(libc::EPERM));
        assert!(iter.next().is_none());
        V5_MOCK.with(|mock| *mock.borrow_mut() = None);
    }

    fn geo(agblocks: u32, blocksize: u32, inodesize: u32) -> XfsGeometry {
        XfsGeometry {
            blocksize,
            agblocks,
            inodesize,
            agcount: 4,
            dirblocksize: blocksize,
            uuid: [0; 16],
            v5: None,
            ftype: None,
            reflink: None,
        }
    }
    #[test]
    fn ag_ranges_use_ceiling_block_log() {
        let geometry = geo(1000, 4096, 512);
        assert_eq!(ag_ino_range(&geometry, 0), (0, 8192));
        assert_eq!(ag_ino_range(&geometry, 1), (8192, 16384));
        assert_eq!(ag_ino_range(&geometry, 3), (24576, 32768));
        assert_eq!(ag_ino_range(&geo(1024, 4096, 256), 2), (32768, 49152));
        assert_eq!(ag_ino_range(&geo(1, 4096, 512), 0), (0, 8));
        assert_eq!(ag_ino_range(&geometry, 4), (0, 0));
        assert_eq!(ag_ino_range(&geo(0, 4096, 512), 0), (0, 0));
        assert_eq!(ag_ino_range(&geo(1024, 4096, 0), 0), (0, 0));
        assert_eq!(ag_ino_range(&geo(1024, 4096, 8192), 0), (0, 0));
        assert_eq!(ag_ino_range(&geo(1024, 4000, 512), 0), (0, 0));
        assert_eq!(ag_ino_range(&geo(u32::MAX, 1 << 31, 1), 2), (0, 0));
    }
    #[test]
    fn v5_preserves_wide_links_and_project_id() {
        let raw = xfs_bulkstat {
            bs_nlink: 70_000,
            bs_projectid: 0xfedcba98,
            bs_blocks: 10,
            bs_mtime: -123,
            ..Default::default()
        };
        let converted = convert_v5(&raw, 4096).unwrap();
        assert_eq!(
            (
                converted.nlink,
                converted.projid,
                converted.mtime,
                converted.disk_bytes
            ),
            (70_000, 0xfedcba98, -123, 40960)
        );
        assert!(convert_v5(
            &xfs_bulkstat {
                bs_blocks: u64::MAX,
                ..raw
            },
            4096
        )
        .is_err());
        assert!(convert_v5(
            &xfs_bulkstat {
                bs_xflags: u64::MAX,
                ..raw
            },
            4096
        )
        .is_err());
    }
    fn inode(ino: u64) -> BulkInode {
        BulkInode {
            ino,
            mode: 0,
            nlink: 0,
            uid: 0,
            gid: 0,
            size: 0,
            disk_bytes: 0,
            mtime: 0,
            projid: 0,
            xflags: 0,
        }
    }
    #[test]
    fn batches_truncate_at_exclusive_end_and_reject_duplicates() {
        let file = File::open("/").unwrap();
        let mut iter = bulkstat(file.as_fd(), 10, 20, 3);
        assert_eq!(
            iter.finish_batch(vec![inode(10), inode(19), inode(20)])
                .unwrap(),
            vec![inode(10), inode(19)]
        );
        assert!(iter.next().is_none());
        let mut iter = bulkstat(file.as_fd(), 10, 20, 3);
        assert!(iter.finish_batch(vec![inode(10), inode(10)]).is_err());
        assert!(iter.finish_batch(vec![inode(9)]).is_err());
        assert!(iter.finish_batch(vec![inode(12), inode(11)]).is_err());
        assert_eq!(
            iter.finish_batch(vec![inode(10), inode(11)]).unwrap().len(),
            2
        );
        assert!(iter.finish_batch(vec![inode(11)]).is_err());
        assert!(iter.finish_batch(vec![]).unwrap().is_empty());
        assert!(iter.next().is_none());
    }
    #[test]
    fn empty_range_is_inert_and_invalid_batch_fails_once() {
        let file = File::open("/").unwrap();
        assert!(bulkstat(file.as_fd(), 1, 1, 0).next().is_none());
        for batch in [0, MAX_BATCH + 1, usize::MAX] {
            let mut iter = bulkstat(file.as_fd(), 0, u64::MAX, batch);
            assert_eq!(
                iter.next().unwrap().unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(iter.next().is_none());
        }
    }
}
