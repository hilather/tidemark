//! Owned by: W1-C
//! Hand-transcribed XFS ABI, not bindgen output.
// Source: rockylinux:8, xfsprogs-devel-5.0.0-12.el8 and
// kernel-headers-4.18.0-553.172.1.el8_10, /usr/include/xfs/xfs_fs.h.
// Coordinator's gcc measurements on x86_64: bstime 16/8, bstat 136/8,
// bulkreq 32/8, geom_v1 112/8, geom 112/8 (size/alignment in bytes).
// v5 bulk_ireq, bulkstat and flexible-array bulkstat_req: coordinator gcc
// measurements on x86_64 in rockylinux:9, xfsprogs-devel-6.4.0-7.el9.
// Their kernel ABI also applies to EL8, whose userspace header lacks v5.
// No generation command: these definitions were transcribed by hand.
#![allow(non_camel_case_types, dead_code)]

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct xfs_bstime {
    pub tv_sec: i64,
    pub tv_nsec: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct xfs_bstat {
    pub bs_ino: u64,
    pub bs_mode: u16,
    pub bs_nlink: u16,
    pub bs_uid: u32,
    pub bs_gid: u32,
    pub bs_rdev: u32,
    pub bs_blksize: i32,
    pub bs_size: i64,
    pub bs_atime: xfs_bstime,
    pub bs_mtime: xfs_bstime,
    pub bs_ctime: xfs_bstime,
    pub bs_blocks: i64,
    pub bs_xflags: u32,
    pub bs_extsize: i32,
    pub bs_extents: i32,
    pub bs_gen: u32,
    pub bs_projid_lo: u16,
    pub bs_forkoff: u16,
    pub bs_projid_hi: u16,
    pub bs_pad: [u8; 6],
    pub bs_cowextsize: u32,
    pub bs_dmevmask: u32,
    pub bs_dmstate: u16,
    pub bs_aextents: u16,
}
#[repr(C)]
pub(crate) struct xfs_fsop_bulkreq {
    pub lastip: *mut u64,
    pub icount: i32,
    pub ubuffer: *mut libc::c_void,
    pub ocount: *mut i32,
}
#[repr(C)]
#[derive(Default)]
pub(crate) struct xfs_fsop_geom_v1 {
    pub blocksize: u32,
    pub rtextsize: u32,
    pub agblocks: u32,
    pub agcount: u32,
    pub logblocks: u32,
    pub sectsize: u32,
    pub inodesize: u32,
    pub imaxpct: u32,
    pub datablocks: u64,
    pub rtblocks: u64,
    pub rtextents: u64,
    pub logstart: u64,
    pub uuid: [u8; 16],
    pub sunit: u32,
    pub swidth: u32,
    pub version: i32,
    pub flags: u32,
    pub logsectsize: u32,
    pub rtsectsize: u32,
    pub dirblocksize: u32,
}
#[repr(C)]
#[derive(Default)]
pub(crate) struct xfs_fsop_geom {
    pub blocksize: u32,
    pub rtextsize: u32,
    pub agblocks: u32,
    pub agcount: u32,
    pub logblocks: u32,
    pub sectsize: u32,
    pub inodesize: u32,
    pub imaxpct: u32,
    pub datablocks: u64,
    pub rtblocks: u64,
    pub rtextents: u64,
    pub logstart: u64,
    pub uuid: [u8; 16],
    pub sunit: u32,
    pub swidth: u32,
    pub version: i32,
    pub flags: u32,
    pub logsectsize: u32,
    pub rtsectsize: u32,
    pub dirblocksize: u32,
    pub logsunit: u32,
}
const fn ioctl(direction: u32, number: u32, size: usize) -> libc::c_ulong {
    ((direction << 30) | ((size as u32) << 16) | (b'X' as u32) << 8 | number) as libc::c_ulong
}
pub(crate) const XFS_IOC_FSGEOMETRY_V1: libc::c_ulong =
    ioctl(2, 100, size_of::<xfs_fsop_geom_v1>());
pub(crate) const XFS_IOC_FSGEOMETRY: libc::c_ulong = ioctl(2, 124, size_of::<xfs_fsop_geom>());
pub(crate) const XFS_IOC_FSBULKSTAT: libc::c_ulong = ioctl(3, 101, size_of::<xfs_fsop_bulkreq>());
pub(crate) const XFS_IOC_FSBULKSTAT_SINGLE: libc::c_ulong =
    ioctl(3, 102, size_of::<xfs_fsop_bulkreq>());
pub(crate) const XFS_IOC_FSINUMBERS: libc::c_ulong = ioctl(3, 103, size_of::<xfs_fsop_bulkreq>());
pub(crate) const XFS_FSOP_GEOM_FLAGS_V5SB: u32 = 0x8000;
pub(crate) const XFS_FSOP_GEOM_FLAGS_FTYPE: u32 = 0x10000;
pub(crate) const XFS_FSOP_GEOM_FLAGS_REFLINK: u32 = 0x100000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct xfs_bulk_ireq {
    pub ino: u64,
    pub flags: u32,
    pub icount: u32,
    pub ocount: u32,
    pub agno: u32,
    pub reserved: [u64; 5],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct xfs_bulkstat {
    pub bs_ino: u64,
    pub bs_size: u64,
    pub bs_blocks: u64,
    pub bs_xflags: u64,
    pub bs_atime: i64,
    pub bs_mtime: i64,
    pub bs_ctime: i64,
    pub bs_btime: i64,
    pub bs_gen: u32,
    pub bs_uid: u32,
    pub bs_gid: u32,
    pub bs_projectid: u32,
    pub bs_atime_nsec: u32,
    pub bs_mtime_nsec: u32,
    pub bs_ctime_nsec: u32,
    pub bs_btime_nsec: u32,
    pub bs_blksize: u32,
    pub bs_rdev: u32,
    pub bs_cowextsize_blks: u32,
    pub bs_extsize_blks: u32,
    pub bs_nlink: u32,
    pub bs_extents: u32,
    pub bs_aextents: u32,
    pub bs_version: u16,
    pub bs_forkoff: u16,
    pub bs_sick: u16,
    pub bs_checked: u16,
    pub bs_mode: u16,
    pub bs_pad2: u16,
    pub bs_extents64: u64,
    pub bs_pad: [u64; 6],
}
#[repr(C)]
#[derive(Default)]
pub(crate) struct xfs_bulkstat_req {
    pub hdr: xfs_bulk_ireq,
    pub bulkstat: [xfs_bulkstat; 0],
}
pub(crate) const XFS_IOC_BULKSTAT: libc::c_ulong = ioctl(2, 127, size_of::<xfs_bulkstat_req>());
pub(crate) const XFS_BULK_IREQ_AGNO: u32 = 1;
pub(crate) const XFS_BULK_IREQ_SPECIAL: u32 = 2;
pub(crate) const XFS_BULKSTAT_VERSION_V5: u16 = 5;
pub(crate) const XFS_IOC_INUMBERS: libc::c_ulong = ioctl(2, 128, size_of::<xfs_bulk_ireq>());

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};
    macro_rules! layout {
        ($ty:ty, $size:expr, $align:expr; $($field:ident : $offset:expr),* $(,)?) => {
            assert_eq!(size_of::<$ty>(), $size);
            assert_eq!(align_of::<$ty>(), $align);
            $(assert_eq!(offset_of!($ty, $field), $offset, stringify!($field));)*
        };
    }
    #[test]
    fn rocky8_x86_64_layouts() {
        layout!(xfs_bstime, 16, 8; tv_sec: 0, tv_nsec: 8);
        layout!(xfs_bstat, 136, 8;
            bs_ino: 0, bs_mode: 8, bs_nlink: 10, bs_uid: 12, bs_gid: 16,
            bs_rdev: 20, bs_blksize: 24, bs_size: 32, bs_atime: 40,
            bs_mtime: 56, bs_ctime: 72, bs_blocks: 88, bs_xflags: 96,
            bs_extsize: 100, bs_extents: 104, bs_gen: 108, bs_projid_lo: 112,
            bs_forkoff: 114, bs_projid_hi: 116, bs_pad: 118,
            bs_cowextsize: 124, bs_dmevmask: 128, bs_dmstate: 132, bs_aextents: 134);
        layout!(xfs_fsop_bulkreq, 32, 8; lastip: 0, icount: 8, ubuffer: 16, ocount: 24);
        layout!(xfs_fsop_geom, 112, 8; blocksize: 0, rtextsize: 4, agblocks: 8, agcount: 12, logblocks: 16, sectsize: 20, inodesize: 24, imaxpct: 28, datablocks: 32, rtblocks: 40, rtextents: 48, logstart: 56, uuid: 64, sunit: 80, swidth: 84, version: 88, flags: 92, logsectsize: 96, rtsectsize: 100, dirblocksize: 104, logsunit: 108);
        layout!(xfs_fsop_geom_v1, 112, 8; blocksize: 0, rtextsize: 4, agblocks: 8, agcount: 12, logblocks: 16, sectsize: 20, inodesize: 24, imaxpct: 28, datablocks: 32, rtblocks: 40, rtextents: 48, logstart: 56, uuid: 64, sunit: 80, swidth: 84, version: 88, flags: 92, logsectsize: 96, rtsectsize: 100, dirblocksize: 104);
    }
    #[test]
    fn rocky8_measured_field_widths() {
        macro_rules! widths {
            ($value:expr; $($field:ident : $width:expr),* $(,)?) => {
                let value = $value;
                $(assert_eq!(std::mem::size_of_val(&value.$field), $width, stringify!($field));)*
            };
        }
        widths!(xfs_bstime::default(); tv_sec: 8, tv_nsec: 4);
        widths!(xfs_bstat::default(); bs_ino: 8, bs_mode: 2, bs_nlink: 2,
            bs_uid: 4, bs_gid: 4, bs_rdev: 4, bs_blksize: 4, bs_size: 8,
            bs_atime: 16, bs_mtime: 16, bs_ctime: 16, bs_blocks: 8,
            bs_xflags: 4, bs_extsize: 4, bs_extents: 4, bs_gen: 4,
            bs_projid_lo: 2, bs_forkoff: 2, bs_projid_hi: 2,
            bs_dmevmask: 4, bs_dmstate: 2, bs_aextents: 2);
        widths!(xfs_fsop_bulkreq { lastip: std::ptr::null_mut(), icount: 0,
            ubuffer: std::ptr::null_mut(), ocount: std::ptr::null_mut() };
            lastip: 8, icount: 4, ubuffer: 8, ocount: 8);
        widths!(xfs_fsop_geom::default(); blocksize: 4, agblocks: 4, agcount: 4,
            inodesize: 4, flags: 4, uuid: 16, dirblocksize: 4, version: 4);
    }
    #[test]
    fn rocky8_ioctl_numbers_and_sizes() {
        for (actual, expected, size) in [
            (
                XFS_IOC_FSGEOMETRY_V1,
                0x80705864,
                size_of::<xfs_fsop_geom_v1>(),
            ),
            (XFS_IOC_FSGEOMETRY, 0x8070587c, size_of::<xfs_fsop_geom>()),
            (
                XFS_IOC_FSBULKSTAT,
                0xc0205865,
                size_of::<xfs_fsop_bulkreq>(),
            ),
            (
                XFS_IOC_FSBULKSTAT_SINGLE,
                0xc0205866,
                size_of::<xfs_fsop_bulkreq>(),
            ),
            (
                XFS_IOC_FSINUMBERS,
                0xc0205867,
                size_of::<xfs_fsop_bulkreq>(),
            ),
        ] {
            assert_eq!(actual, expected);
            assert_eq!((actual >> 16) & 0x3fff, size as libc::c_ulong);
        }
    }
    #[test]
    fn upstream_v5_layout() {
        layout!(xfs_bulk_ireq, 64, 8; ino: 0, flags: 8, icount: 12, ocount: 16, agno: 20, reserved: 24);
        layout!(xfs_bulkstat, 192, 8; bs_ino: 0, bs_size: 8, bs_blocks: 16, bs_xflags: 24, bs_atime: 32, bs_mtime: 40, bs_ctime: 48, bs_btime: 56, bs_gen: 64, bs_uid: 68, bs_gid: 72, bs_projectid: 76, bs_atime_nsec: 80, bs_mtime_nsec: 84, bs_ctime_nsec: 88, bs_btime_nsec: 92, bs_blksize: 96, bs_rdev: 100, bs_cowextsize_blks: 104, bs_extsize_blks: 108, bs_nlink: 112, bs_extents: 116, bs_aextents: 120, bs_version: 124, bs_forkoff: 126, bs_sick: 128, bs_checked: 130, bs_mode: 132, bs_pad2: 134, bs_extents64: 136, bs_pad: 144);
        layout!(xfs_bulkstat_req, 64, 8; hdr: 0, bulkstat: 64);
        assert_eq!(XFS_IOC_BULKSTAT, 0x8040587f);
        assert_eq!(
            (XFS_IOC_BULKSTAT >> 16) & 0x3fff,
            size_of::<xfs_bulkstat_req>() as libc::c_ulong
        );
        assert_eq!(XFS_IOC_INUMBERS, 0x80405880);
        assert_eq!(
            (XFS_IOC_INUMBERS >> 16) & 0x3fff,
            size_of::<xfs_bulk_ireq>() as libc::c_ulong
        );
        assert_eq!(XFS_BULK_IREQ_AGNO, 1);
        assert_eq!(XFS_BULK_IREQ_SPECIAL, 2);
        assert_eq!(XFS_BULKSTAT_VERSION_V5, 5);
    }
}
