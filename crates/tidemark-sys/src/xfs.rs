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
    /// Hard-link count (v1 is limited by its 16-bit ABI).
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
/// Iterate allocated inodes in [start, end), preferring v5 when supported.
/// v1 resumes strictly after lastip; v5 resumes at its returned next inode.
/// Each error is yielded once and terminates the iterator. No syscall occurs for
/// an empty interval. The caller must charge these metadata calls to its Gate.
pub fn bulkstat(fd: BorrowedFd<'_>, start: u64, end: u64, batch: usize) -> BulkstatIter<'_> {
    BulkstatIter {
        fd,
        start,
        end,
        batch,
        last: start.saturating_sub(1),
        next_ino: start,
        blocksize: None,
        agno: None,
        backend: Backend::Unknown,
        legacy: Vec::new(),
        modern: Vec::new(),
        done: start >= end,
    }
}
#[derive(Clone, Copy)]
enum Backend {
    Unknown,
    V1,
    V5,
}
/// Bounded, descriptor-borrowing iterator over ascending XFS inode metadata.
pub struct BulkstatIter<'a> {
    fd: BorrowedFd<'a>,
    start: u64,
    end: u64,
    batch: usize,
    last: u64,
    next_ino: u64,
    blocksize: Option<u32>,
    agno: Option<u32>,
    backend: Backend,
    legacy: Vec<MaybeUninit<xfs_bstat>>,
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
    // SAFETY: the contiguous allocation has the UAPI flexible-array layout and
    // advertised capacity; the kernel retains no pointers after this call.
    ioctl_result(unsafe { libc::ioctl(fd.as_raw_fd(), XFS_IOC_BULKSTAT, buffer.as_mut_ptr()) })?;
    // SAFETY: the aligned initialized header remains in the live allocation.
    let header = unsafe { buffer.as_ptr().cast::<xfs_bulk_ireq>().read() };
    if header.ocount as usize > batch {
        return Err(invalid_data("invalid v5 output count"));
    }
    Ok((header.ocount as usize, header.ino))
}
// v1 bs_blocks is in filesystem blocks. Conversion is centralized here;
// bulkstat_blocks_match_statx validates this against statx's 512-byte units.
fn blocks_to_bytes(blocks: u64, blocksize: u32) -> Option<u64> {
    blocks.checked_mul(u64::from(blocksize))
}
fn bulk_disk_bytes(bs: &xfs_bstat, blocksize: u32) -> u64 {
    blocks_to_bytes(bs.bs_blocks.max(0) as u64, blocksize).unwrap_or(u64::MAX)
}
fn convert_v1(bs: &xfs_bstat, blocksize: u32) -> io::Result<BulkInode> {
    if bs.bs_size < 0
        || bs.bs_blocks < 0
        || blocks_to_bytes(bs.bs_blocks as u64, blocksize).is_none()
    {
        return Err(invalid_data("negative or overflowing bulkstat size"));
    }
    Ok(BulkInode {
        ino: bs.bs_ino,
        mode: u32::from(bs.bs_mode),
        nlink: u32::from(bs.bs_nlink),
        uid: bs.bs_uid,
        gid: bs.bs_gid,
        size: bs.bs_size as u64,
        disk_bytes: bulk_disk_bytes(bs, blocksize),
        mtime: bs.bs_mtime.tv_sec,
        projid: u32::from(bs.bs_projid_hi) << 16 | u32::from(bs.bs_projid_lo),
        xflags: bs.bs_xflags,
    })
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
        if matches!(self.backend, Backend::Unknown | Backend::V5) {
            if self.modern.is_empty() {
                self.modern.resize(8 + 24 * self.batch, 0);
            }
            match request_v5(
                self.fd,
                self.next_ino,
                self.batch,
                self.agno,
                &mut self.modern,
            ) {
                Ok((count, next)) => {
                    self.backend = Backend::V5;
                    let mut result = Vec::with_capacity(count);
                    for i in 0..count {
                        // SAFETY: count was checked against allocated slots; the
                        // header is 8 u64s, each record 24 u64s. Storage is aligned,
                        // initialized and live; all record fields are integers.
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
                    return self.finish_batch(result);
                }
                Err(e)
                    if matches!(self.backend, Backend::Unknown)
                        && matches!(
                            e.raw_os_error(),
                            Some(libc::ENOTTY | libc::EOPNOTSUPP | libc::ENOSYS)
                        ) =>
                {
                    self.backend = Backend::V1;
                    self.modern = Vec::new();
                }
                Err(e) => return Err(e),
            }
        }
        if self.legacy.is_empty() {
            self.legacy.resize(self.batch, MaybeUninit::zeroed());
        }
        let previous = self.last;
        let count = request_v1(self.fd, &mut self.last, &mut self.legacy)?;
        let mut result = Vec::with_capacity(count);
        for slot in &self.legacy[..count] {
            // SAFETY: the count is validated and these slots were initialized to
            // zero before the kernel filled them. All bstat fields are integers.
            let bs = unsafe { slot.assume_init_ref() };
            result.push(convert_v1(bs, blocksize)?);
        }
        if count > 0 && self.last <= previous {
            return Err(invalid_data("v1 cursor did not advance"));
        }
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
    fn blocks_are_filesystem_units_and_times_are_signed_seconds() {
        let mut raw = xfs_bstat {
            bs_blocks: 7,
            bs_size: 3,
            bs_projid_hi: 0x1234,
            bs_projid_lo: 0xabcd,
            bs_nlink: u16::MAX,
            bs_mtime: xfs_bstime {
                tv_sec: -1,
                tv_nsec: 999_999_999,
            },
            ..Default::default()
        };
        assert_eq!(bulk_disk_bytes(&raw, 4096), 28672);
        let converted = convert_v1(&raw, 4096).unwrap();
        assert_eq!(converted.mtime, -1);
        assert_eq!(converted.projid, 0x1234abcd);
        assert_eq!(converted.nlink, 65535);
        for sec in [i64::MIN, -2_147_483_649, 0, 2_147_483_648, i64::MAX] {
            raw.bs_mtime.tv_sec = sec;
            assert_eq!(convert_v1(&raw, 4096).unwrap().mtime, sec);
        }
        raw.bs_blocks = -1;
        assert!(convert_v1(&raw, 4096).is_err());
        raw.bs_blocks = i64::MAX;
        assert!(convert_v1(&raw, 4096).is_err());
        raw.bs_blocks = 0;
        raw.bs_size = -1;
        assert!(convert_v1(&raw, 4096).is_err());
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
    #[cfg(feature = "privileged")]
    #[test]
    fn legacy_v1_matches_metadata() {
        use std::os::unix::fs::MetadataExt;
        let fs = test_support::xfs::LoopXfs::new(512 << 20, &[], "noatime");
        let path = fs.mountpoint.join("legacy");
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let mut file = File::create(&path).unwrap();
        file.write_all(&[0x42; 4097]).unwrap();
        file.set_len(65536).unwrap();
        file.sync_all().unwrap();
        let metadata = std::fs::symlink_metadata(path).unwrap();
        let mount = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NOATIME | libc::O_CLOEXEC)
            .open(&fs.mountpoint)
            .unwrap();
        let mut iter = bulkstat(mount.as_fd(), 0, u64::MAX, 1);
        iter.backend = Backend::V1;
        let found = iter
            .flat_map(|r| r.unwrap())
            .find(|r| r.ino == metadata.ino())
            .unwrap();
        assert_eq!(
            (
                found.size,
                found.disk_bytes,
                found.nlink,
                found.mode,
                found.mtime
            ),
            (
                metadata.size(),
                metadata.blocks() * 512,
                metadata.nlink() as u32,
                metadata.mode(),
                metadata.mtime()
            )
        );
    }
}
