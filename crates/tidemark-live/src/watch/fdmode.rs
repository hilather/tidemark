//! Owned by: W5-A
//! Metadata-only descriptor resolution and bounded inode size history.
use std::{
    collections::{HashMap, VecDeque},
    io,
    os::fd::{AsRawFd, BorrowedFd},
};
use tidemark_core::path::RawPath;
pub(super) struct Sizes {
    values: HashMap<(u64, u64), (i64, u64)>,
    order: VecDeque<((u64, u64), u64)>,
    sequence: u64,
    cap: usize,
    started: i64,
}
impl Sizes {
    pub fn new(cap: usize, started: i64) -> Self {
        Self {
            values: HashMap::new(),
            order: VecDeque::new(),
            sequence: 0,
            cap,
            started,
        }
    }
    pub fn delta(&mut self, id: (u64, u64), disk: i64, created: i64, now: i64) -> i64 {
        let previous = self.values.get(&id).map(|v| v.0);
        let delta = previous.map_or_else(
            || {
                if created >= self.started && created >= now.saturating_sub(120) && created <= now {
                    disk
                } else {
                    0
                }
            },
            |v| disk.saturating_sub(v),
        );
        if self.cap == 0 {
            return 0;
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.values.insert(id, (disk, self.sequence));
        self.order.push_back((id, self.sequence));
        if self.order.len() > self.cap.saturating_mul(2) {
            self.order
                .retain(|(id, seq)| self.values.get(id).is_some_and(|v| v.1 == *seq));
        }
        while self.values.len() > self.cap {
            if let Some((key, seq)) = self.order.pop_front() {
                if self.values.get(&key).is_some_and(|v| v.1 == seq) {
                    self.values.remove(&key);
                }
            }
        }
        delta
    }
}
// FID resolution still consumes rustix::Stat; keep its existing internal adapter.
// The fd-mode watcher uses resolve_event below and never calls this adapter.
pub(super) fn resolve(fd: BorrowedFd<'_>) -> io::Result<(RawPath, rustix::fs::Stat, i64)> {
    let stat = rustix::fs::fstat(fd)?;
    let path = descriptor_path(fd)?;
    let raw = RawPath::from_path(&path).map_err(io::Error::other)?;
    let created = birth(fd, "", rustix::fs::AtFlags::EMPTY_PATH, stat.st_ctime);
    Ok((raw, stat, created))
}
#[derive(Clone, Copy)]
pub(super) struct EventStat {
    pub st_dev: u64,
    pub st_ino: u64,
    #[allow(dead_code)] // Kept for parity with descriptor metadata.
    pub st_size: i64,
    pub st_blocks: i64,
    #[allow(dead_code)] // Kept for parity with descriptor metadata.
    pub st_mode: u32,
    pub st_ctime: i64,
}
impl From<rustix::fs::Stat> for EventStat {
    fn from(s: rustix::fs::Stat) -> Self {
        Self {
            st_dev: s.st_dev,
            st_ino: s.st_ino,
            st_size: s.st_size,
            st_blocks: s.st_blocks,
            st_mode: s.st_mode,
            st_ctime: s.st_ctime,
        }
    }
}
fn event_stat(s: rustix::fs::Statx) -> io::Result<(EventStat, i64)> {
    let required = rustix::fs::StatxFlags::BASIC_STATS.bits();
    if s.stx_mask & required != required {
        return Err(io::Error::other("statx basic metadata unavailable"));
    }
    let stat = EventStat {
        st_dev: rustix::fs::makedev(s.stx_dev_major, s.stx_dev_minor),
        st_ino: s.stx_ino,
        st_size: i64::try_from(s.stx_size).map_err(io::Error::other)?,
        st_blocks: i64::try_from(s.stx_blocks).map_err(io::Error::other)?,
        st_mode: u32::from(s.stx_mode),
        st_ctime: s.stx_ctime.tv_sec,
    };
    let created = if s.stx_mask & rustix::fs::StatxFlags::BTIME.bits() != 0 {
        s.stx_btime.tv_sec
    } else {
        stat.st_ctime
    };
    Ok((stat, created))
}
pub(super) fn resolve_event_with_fallback(
    fd: BorrowedFd<'_>,
    path_fd: BorrowedFd<'_>,
    fallback: Option<&RawPath>,
) -> io::Result<(RawPath, EventStat, i64)> {
    resolve_with_fallback(path_fd, fallback, || {
        rustix::fs::statx(
            fd,
            "",
            rustix::fs::AtFlags::EMPTY_PATH,
            rustix::fs::StatxFlags::BASIC_STATS | rustix::fs::StatxFlags::BTIME,
        )
        .map_err(Into::into)
    })
}
#[cfg(test)]
fn resolve_with(
    fd: BorrowedFd<'_>,
    stat: impl FnOnce() -> io::Result<rustix::fs::Statx>,
) -> io::Result<(RawPath, EventStat, i64)> {
    resolve_with_fallback(fd, None, stat)
}
fn resolve_with_fallback(
    fd: BorrowedFd<'_>,
    fallback: Option<&RawPath>,
    stat: impl FnOnce() -> io::Result<rustix::fs::Statx>,
) -> io::Result<(RawPath, EventStat, i64)> {
    let (stat, created) = match stat().and_then(event_stat) {
        Ok(value) => value,
        // Previously a failed birth-time statx still recorded fstat metadata.
        // Retain that behavior on unsupported kernels/filesystems or seccomp.
        Err(_) => {
            let stat = EventStat::from(rustix::fs::fstat(fd)?);
            (stat, stat.st_ctime)
        }
    };
    let raw = match descriptor_path(fd)
        .and_then(|path| RawPath::from_path(&path).map_err(io::Error::other))
    {
        Ok(path) => path,
        Err(error) => match fallback {
            Some(root) => root.clone(),
            None => return Err(error),
        },
    };
    Ok((raw, stat, created))
}
pub(super) fn descriptor_path(fd: BorrowedFd<'_>) -> io::Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = [0u8; 32];
    let length = {
        let mut cursor = io::Cursor::new(buffer.as_mut_slice());
        write!(cursor, "/proc/self/fd/{}", fd.as_raw_fd())?;
        cursor.position() as usize
    };
    std::fs::read_link(std::ffi::OsStr::from_bytes(&buffer[..length]))
}

pub(super) fn birth(
    fd: BorrowedFd<'_>,
    name: &str,
    flags: rustix::fs::AtFlags,
    fallback: i64,
) -> i64 {
    rustix::fs::statx(fd, name, flags, rustix::fs::StatxFlags::BTIME)
        .ok()
        .filter(|s| s.stx_mask & rustix::fs::StatxFlags::BTIME.bits() != 0)
        .map_or(fallback, |s| s.stx_btime.tv_sec)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn new_files_old_files_eviction_and_signed_delta() {
        let mut sizes = Sizes::new(2, 1000);
        assert_eq!(sizes.delta((1, 1), 100, 999, 1001), 0);
        assert_eq!(sizes.delta((1, 1), 120, 999, 1002), 20);
        assert_eq!(sizes.delta((1, 1), 10, 999, 1003), -110);
        assert_eq!(sizes.delta((1, 2), 50, 1001, 1003), 50);
        sizes.delta((1, 1), 10, 999, 1004);
        sizes.delta((1, 3), 30, 1002, 1004);
        assert!(!sizes.values.contains_key(&(1, 2)));
        assert_eq!(sizes.delta((1, 2), 100, 1001, 1300), 0);
        for _ in 0..100 {
            sizes.delta((1, 2), 100, 1001, 1300);
        }
        assert!(sizes.order.len() <= 4);
        assert_eq!(Sizes::new(0, 0).delta((1, 1), 10, 1, 1), 0);
    }
    #[test]
    fn single_statx_matches_fstat_and_ctime_fallback() {
        use std::os::fd::AsFd;
        let file = tempfile::tempfile().unwrap();
        file.set_len(12345).unwrap();
        let old = rustix::fs::fstat(file.as_fd()).unwrap();
        let calls = std::cell::Cell::new(0);
        let sx = rustix::fs::statx(
            file.as_fd(),
            "",
            rustix::fs::AtFlags::EMPTY_PATH,
            rustix::fs::StatxFlags::BASIC_STATS | rustix::fs::StatxFlags::BTIME,
        )
        .unwrap();
        let (_, new, created) = resolve_with(file.as_fd(), || {
            calls.set(calls.get() + 1);
            Ok(sx)
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(
            (
                new.st_dev,
                new.st_ino,
                new.st_size,
                new.st_blocks,
                new.st_mode,
                new.st_ctime
            ),
            (
                old.st_dev,
                old.st_ino,
                old.st_size,
                old.st_blocks,
                old.st_mode,
                old.st_ctime
            )
        );
        assert_eq!(
            created,
            birth(
                file.as_fd(),
                "",
                rustix::fs::AtFlags::EMPTY_PATH,
                old.st_ctime
            )
        );
        let mut absent = sx;
        absent.stx_mask &= !rustix::fs::StatxFlags::BTIME.bits();
        assert_eq!(event_stat(absent).unwrap().1, old.st_ctime);
        let (_, fallback, created) = resolve_with(file.as_fd(), || {
            calls.set(calls.get() + 1);
            Err(io::Error::from_raw_os_error(
                rustix::io::Errno::NOSYS.raw_os_error(),
            ))
        })
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(fallback.st_ino, old.st_ino);
        assert_eq!(created, old.st_ctime);
    }
    #[test]
    fn resolve_only_metadata_and_raw_path() {
        let dir = tempfile::tempdir().unwrap();
        let fd = std::fs::File::open(dir.path()).unwrap();
        use std::os::fd::AsFd;
        let (path, st, _) = resolve(fd.as_fd()).unwrap();
        assert_eq!(path.as_path(), dir.path());
        use std::os::unix::fs::MetadataExt;
        assert_eq!(st.st_ino, fd.metadata().unwrap().ino());
    }
}

/// Select a host path only when its mount provenance is known. None means root fallback.
pub(super) fn mapped_path<'a>(
    event_mount: Option<u64>,
    watched_mount: u64,
    original: &'a RawPath,
    reopened: Option<&'a RawPath>,
    mount: &RawPath,
) -> Option<&'a RawPath> {
    let path = match event_mount {
        Some(id) if id == watched_mount => Some(original),
        Some(_) => reopened,
        None => None,
    }?;
    (path.as_bytes() != b"/" && path != mount && path.starts_with(mount)).then_some(path)
}

#[cfg(test)]
mod mapping_tests {
    use super::*;
    #[test]
    fn provenance_and_bounds() {
        let mount = RawPath::new(b"/data".to_vec()).unwrap();
        let host = RawPath::new(b"/data/a (deleted)".to_vec()).unwrap();
        let foreign = RawPath::new(b"/bind/a".to_vec()).unwrap();
        let root = RawPath::new(b"/".to_vec()).unwrap();
        assert_eq!(mapped_path(Some(1), 1, &host, None, &mount), Some(&host));
        assert_eq!(
            mapped_path(Some(2), 1, &foreign, Some(&host), &mount),
            Some(&host)
        );
        for reopened in [None, Some(&foreign), Some(&root)] {
            assert_eq!(mapped_path(Some(2), 1, &foreign, reopened, &mount), None);
        }
        assert_eq!(mapped_path(None, 1, &host, Some(&host), &mount), None);
    }
}
