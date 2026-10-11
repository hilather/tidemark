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
pub(super) fn resolve(fd: BorrowedFd<'_>) -> io::Result<(RawPath, rustix::fs::Stat, i64)> {
    let stat = rustix::fs::fstat(fd)?;
    let path = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))?;
    let raw = RawPath::from_path(&path).map_err(io::Error::other)?;
    let created = birth(fd, "", rustix::fs::AtFlags::EMPTY_PATH, stat.st_ctime);
    Ok((raw, stat, created))
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
