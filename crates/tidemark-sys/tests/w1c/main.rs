//! Owned by: W1-C
//! Disposable-VM XFS ioctl validation against independent metadata oracles.
#![cfg(feature = "privileged")]
#![allow(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    mem::MaybeUninit,
    os::{
        fd::AsFd,
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::Path,
    process::Command,
};
use test_support::xfs::LoopXfs;
use tidemark_sys::xfs::{ag_ino_range, bulkstat, bulkstat_probe, geometry, is_xfs, BulkInode};

fn open_mount(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NOATIME | libc::O_CLOEXEC)
        .open(path)
        .unwrap()
}
fn all(file: &File, start: u64, end: u64, batch: usize) -> Vec<BulkInode> {
    bulkstat(file.as_fd(), start, end, batch)
        .flat_map(|r| r.unwrap())
        .collect()
}
fn value(text: &str, key: &str) -> u32 {
    text.split_whitespace()
        .find_map(|word| word.strip_prefix(key))
        .unwrap_or_else(|| panic!("xfs_info missing {key}: {text}"))
        .trim_end_matches(',')
        .parse()
        .unwrap()
}
#[test]
fn geometry_matches_xfs_info() {
    let fs = LoopXfs::new(512 << 20, &["-m", "reflink=1"], "noatime");
    let mount = open_mount(&fs.mountpoint);
    assert!(is_xfs(mount.as_fd()).unwrap());
    let geo = geometry(mount.as_fd()).unwrap();
    let result = Command::new("xfs_info")
        .arg(&fs.mountpoint)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(result.status.success());
    let output = String::from_utf8(result.stdout).unwrap();
    let meta = output
        .lines()
        .find(|line| line.starts_with("meta-data="))
        .unwrap();
    let data = output
        .lines()
        .find(|line| line.starts_with("data"))
        .unwrap();
    let naming = output
        .lines()
        .find(|line| line.starts_with("naming"))
        .unwrap();
    assert_eq!(geo.agcount, value(meta, "agcount="));
    assert_eq!(geo.agblocks, value(meta, "agsize="));
    assert_eq!(geo.inodesize, value(meta, "isize="));
    assert_eq!(geo.blocksize, value(data, "bsize="));
    assert_eq!(geo.dirblocksize, value(naming, "bsize="));
    assert_eq!(geo.ftype, Some(value(naming, "ftype=") != 0));
    assert_eq!(geo.v5, Some(true));
    assert_eq!(geo.reflink, Some(true));
    assert_ne!(geo.uuid, [0; 16]);
}
fn fixture(path: &Path) {
    fs::create_dir(path.join("dir")).unwrap();
    let mut file = File::create(path.join("dir/file")).unwrap();
    file.write_all(&[0x5a; 8193]).unwrap();
    file.set_times(
        std::fs::FileTimes::new()
            .set_modified(std::time::UNIX_EPOCH - std::time::Duration::from_secs(123)),
    )
    .unwrap();
    file.sync_all().unwrap();
    fs::hard_link(path.join("dir/file"), path.join("alias")).unwrap();
    std::os::unix::fs::symlink("dir/file", path.join("symlink")).unwrap();
    File::create(path.join("sparse"))
        .unwrap()
        .set_len(1 << 20)
        .unwrap();
}
fn statx(path: &Path) -> libc::statx {
    let name = CString::new(path.as_os_str().as_bytes()).unwrap();
    let mut out = MaybeUninit::<libc::statx>::zeroed();
    // SAFETY: name is NUL-terminated and live for the call; out is an aligned,
    // writable statx-sized buffer. AT_SYMLINK_NOFOLLOW prevents link traversal.
    let rc = unsafe {
        libc::statx(
            libc::AT_FDCWD,
            name.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW | libc::AT_NO_AUTOMOUNT,
            libc::STATX_BASIC_STATS,
            out.as_mut_ptr(),
        )
    };
    assert_eq!(rc, 0, "statx: {}", std::io::Error::last_os_error());
    // SAFETY: successful statx initialized this previously zeroed buffer.
    let out = unsafe { out.assume_init() };
    assert_eq!(
        out.stx_mask & libc::STATX_BASIC_STATS,
        libc::STATX_BASIC_STATS
    );
    out
}
fn metadata_walk(root: &Path) -> BTreeMap<u64, (u64, u64, u32, i64)> {
    let mut out = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        let meta = statx(&path);
        out.insert(
            meta.stx_ino,
            (
                u64::from(meta.stx_nlink),
                meta.stx_size,
                u32::from(meta.stx_mode),
                meta.stx_mtime.tv_sec,
            ),
        );
        if u32::from(meta.stx_mode) & libc::S_IFMT == libc::S_IFDIR {
            pending.extend(fs::read_dir(path).unwrap().map(|e| e.unwrap().path()));
        }
    }
    out
}
#[test]
fn bulkstat_covers_all_inodes() {
    let fs = LoopXfs::new(512 << 20, &[], "noatime");
    fixture(&fs.mountpoint);
    let expected = metadata_walk(&fs.mountpoint);
    let mount = open_mount(&fs.mountpoint);
    let records = all(&mount, 0, u64::MAX, 2);
    assert!(records.windows(2).all(|r| r[0].ino < r[1].ino));
    // XFS also reports internal quota/metadata inodes, which have no pathname.
    let actual: BTreeMap<_, _> = records
        .into_iter()
        .filter(|r| expected.contains_key(&r.ino))
        .map(|r| (r.ino, (u64::from(r.nlink), r.size, r.mode, r.mtime)))
        .collect();
    assert_eq!(actual, expected);
}
#[test]
fn bulkstat_ag_ranges_partition() {
    let fs = LoopXfs::new(512 << 20, &["-d", "agcount=4"], "noatime");
    fixture(&fs.mountpoint);
    let mount = open_mount(&fs.mountpoint);
    let geo = geometry(mount.as_fd()).unwrap();
    let full = all(&mount, 0, u64::MAX, 3);
    let mut partitioned = Vec::new();
    for agno in 0..geo.agcount {
        let (start, end) = ag_ino_range(&geo, agno);
        let records = all(&mount, start, end, 1);
        assert!(records.iter().all(|r| r.ino >= start && r.ino < end));
        partitioned.extend(records);
    }
    assert_eq!(partitioned, full);
    let unique: BTreeSet<_> = partitioned.iter().map(|r| r.ino).collect();
    assert_eq!(unique.len(), partitioned.len());
}
#[test]
fn bulkstat_blocks_match_statx() {
    let fs = LoopXfs::new(1 << 30, &[], "noatime");
    let mut expected = BTreeMap::new();
    let mut random = 0xcafef00du64;
    for i in 0..10_000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let path = fs.mountpoint.join(format!("f{i:05}"));
        let mut file = File::create(&path).unwrap();
        let size = random % 81920;
        if i % 3 == 0 {
            file.set_len(size + (1 << 20)).unwrap();
            if i % 2 == 0 {
                file.seek(SeekFrom::Start(size)).unwrap();
                file.write_all(&[0x42; 512]).unwrap();
            }
        } else {
            file.write_all(&vec![0x42; size as usize]).unwrap();
        }
        // Flush delayed allocation before comparing the two metadata views.
        file.sync_all().unwrap();
        let meta = statx(&path);
        expected.insert(meta.stx_ino, meta.stx_blocks * 512);
    }
    let mount = open_mount(&fs.mountpoint);
    let records = all(&mount, 0, u64::MAX, 257);
    let mut matched = 0;
    for record in records {
        if let Some(bytes) = expected.get(&record.ino) {
            assert_eq!(record.disk_bytes, *bytes, "inode {}", record.ino);
            matched += 1;
        }
    }
    assert_eq!(matched, 10_000);
}
#[test]
fn bulkstat_probe_records_kernel_support() {
    let fs = LoopXfs::new(512 << 20, &[], "noatime");
    let mount = open_mount(&fs.mountpoint);
    let support = bulkstat_probe(mount.as_fd());
    let release = Command::new("uname").arg("-r").output().unwrap();
    eprintln!(
        "kernel={} v1={} v5={}",
        String::from_utf8_lossy(&release.stdout).trim(),
        support.v1,
        support.v5
    );
    assert!(support.v1);
    assert!(!all(&mount, 0, u64::MAX, 1).is_empty());
}
