//! Owned by: W5-A
//! Resolve kernel file handles without reading file data or following names.
use std::{
    io,
    os::fd::{AsFd, BorrowedFd},
};
use tidemark_core::path::{RawName, RawPath};
use tidemark_sys::fanotify::{mask, open_by_handle};
pub(super) fn resolve(
    mount: BorrowedFd<'_>,
    ty: i32,
    handle: &[u8],
    name: Option<&[u8]>,
    event_mask: u64,
    cache: &mut HandlePaths,
) -> io::Result<(RawPath, Option<rustix::fs::Stat>, i64)> {
    let fd = open_by_handle(mount, ty, handle)?;
    resolve_fd(fd.as_fd(), ty, handle, name, event_mask, cache)
}
pub(super) fn resolve_fd(
    fd: BorrowedFd<'_>,
    ty: i32,
    handle: &[u8],
    name: Option<&[u8]>,
    event_mask: u64,
    cache: &mut HandlePaths,
) -> io::Result<(RawPath, Option<rustix::fs::Stat>, i64)> {
    let Some(name) = name else {
        let (path, stat, created) = super::fdmode::resolve(fd.as_fd())?;
        return Ok((path, Some(stat), created));
    };
    // A directory rename invalidates cached descendant paths as well as its own handle.
    if event_mask & (mask::MOVED_FROM | mask::MOVED_TO) != 0 {
        cache.clear();
    }
    let key = (ty, handle.to_vec());
    let directory = match cache.get(&key) {
        Some(path) => path,
        None => {
            let (path, _, _) = super::fdmode::resolve(fd.as_fd())?;
            cache.put(key, path.clone());
            path
        }
    };
    let name = RawName::new(name.to_vec()).map_err(io::Error::other)?;
    let path = directory.join(&name);
    if event_mask & (mask::DELETE | mask::MOVED_FROM) != 0 {
        return Ok((path, None, 0));
    }
    let stat = rustix::fs::statat(fd, name.as_path(), rustix::fs::AtFlags::SYMLINK_NOFOLLOW)?;
    let created = rustix::fs::statx(
        fd,
        name.as_path(),
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        rustix::fs::StatxFlags::BTIME,
    )
    .ok()
    .filter(|s| s.stx_mask & rustix::fs::StatxFlags::BTIME.bits() != 0)
    .map_or(stat.st_ctime, |s| s.stx_btime.tv_sec);
    Ok((path, Some(stat), created))
}

pub(super) struct PathSizes {
    entries: std::collections::HashMap<RawPath, ((u64, u64), i64, u64)>,
    order: std::collections::VecDeque<(RawPath, u64)>,
    sequence: u64,
    cap: usize,
}
impl PathSizes {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Default::default(),
            order: Default::default(),
            sequence: 0,
            cap,
        }
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
    pub fn remember(&mut self, path: &RawPath, id: (u64, u64), disk: i64) {
        if self.cap == 0 {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.insert(path.clone(), (id, disk, self.sequence));
        self.order.push_back((path.clone(), self.sequence));
        if self.order.len() > self.cap.saturating_mul(2) {
            self.order
                .retain(|(p, s)| self.entries.get(p).is_some_and(|v| v.2 == *s));
        }
        while self.entries.len() > self.cap {
            if let Some((p, s)) = self.order.pop_front() {
                if self.entries.get(&p).is_some_and(|v| v.2 == s) {
                    self.entries.remove(&p);
                }
            }
        }
    }
    pub fn remove(&mut self, path: &RawPath) -> Option<((u64, u64), i64)> {
        self.entries.remove(path).map(|(id, disk, _)| (id, disk))
    }
}

/// The documented 100,000-handle cap; only paths are cached, never directory fds.
pub(super) struct HandlePaths {
    entries: std::collections::HashMap<(i32, Vec<u8>), (RawPath, u64)>,
    order: std::collections::VecDeque<((i32, Vec<u8>), u64)>,
    sequence: u64,
    cap: usize,
}
impl HandlePaths {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Default::default(),
            order: Default::default(),
            sequence: 0,
            cap,
        }
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
    pub fn get(&mut self, key: &(i32, Vec<u8>)) -> Option<RawPath> {
        let path = self.entries.get(key)?.0.clone();
        self.put(key.clone(), path.clone());
        Some(path)
    }
    pub fn put(&mut self, key: (i32, Vec<u8>), path: RawPath) {
        if self.cap == 0 {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.insert(key.clone(), (path, self.sequence));
        self.order.push_back((key, self.sequence));
        if self.order.len() > self.cap.saturating_mul(2) {
            self.order
                .retain(|(k, s)| self.entries.get(k).is_some_and(|v| v.1 == *s));
        }
        while self.entries.len() > self.cap {
            if let Some((k, s)) = self.order.pop_front() {
                if self.entries.get(&k).is_some_and(|v| v.1 == s) {
                    self.entries.remove(&k);
                }
            }
        }
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn handle_path_lru_and_invalidation() {
        let path = RawPath::new(b"/dir".to_vec()).unwrap();
        let mut cache = HandlePaths::new(2);
        cache.put((1, vec![1]), path.clone());
        cache.put((1, vec![2]), path.clone());
        assert_eq!(cache.get(&(1, vec![1])), Some(path.clone()));
        cache.put((1, vec![3]), path);
        assert!(cache.get(&(1, vec![2])).is_none());
        cache.clear();
        assert!(cache.entries.is_empty());
        assert!(cache.order.is_empty());
    }
    #[test]
    fn remembered_delete_estimates_and_capacity() {
        let a = RawPath::new(b"/a".to_vec()).unwrap();
        let b = RawPath::new(b"/b".to_vec()).unwrap();
        let mut cache = PathSizes::new(1);
        cache.remember(&a, (1, 1), 100);
        cache.remember(&a, (1, 1), 150);
        assert_eq!(cache.remove(&a), Some(((1, 1), 150)));
        assert!(cache.remove(&a).is_none());
        cache.remember(&a, (1, 1), 100);
        cache.remember(&b, (1, 2), 200);
        assert!(cache.remove(&a).is_none());
        assert_eq!(cache.entries.len(), 1);
    }
}
