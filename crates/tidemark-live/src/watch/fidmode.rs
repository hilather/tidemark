//! Owned by: W5-A
//! Resolve kernel file handles without reading file data or following names.
use super::budget::Budget;
// Include hash growth overlap, queue capacity slack and allocator headers.
const PATH_NODE: usize = 256;
const PATH_QUEUE: usize = 160;
const HANDLE_NODE: usize = 320;
const HANDLE_QUEUE: usize = 192;
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
    budget: Budget,
    charged: usize,
    nodes: usize,
}
impl PathSizes {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Default::default(),
            order: Default::default(),
            sequence: 0,
            cap,
            budget: Budget::new(),
            charged: 0,
            nodes: 0,
        }
    }
    pub fn set_budget(&mut self, budget: Budget) {
        self.budget = budget;
    }
    pub fn clear(&mut self) {
        self.entries = Default::default();
        self.order = Default::default();
        self.budget.release(self.charged);
        self.budget.release_nodes(self.nodes);
        self.charged = 0;
        self.nodes = 0;
    }
    pub fn remember(&mut self, path: &RawPath, id: (u64, u64), disk: i64) {
        if self.cap == 0 {
            return;
        }
        let old = self.entries.contains_key(path);
        let bytes = path.as_bytes().len() * (1 + usize::from(!old));
        if !self.budget.reserve(bytes) {
            return;
        }
        let nodes = PATH_QUEUE + usize::from(!old) * PATH_NODE;
        if !self.budget.reserve_nodes(nodes, false) {
            self.budget.release(bytes);
            return;
        }
        if self.entries.try_reserve(1).is_err() || self.order.try_reserve(1).is_err() {
            self.budget.release(bytes);
            self.budget.release_nodes(nodes);
            return;
        }
        self.nodes += nodes;
        self.charged += bytes;
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.insert(path.clone(), (id, disk, self.sequence));
        self.order.push_back((path.clone(), self.sequence));
        if self.order.len() > self.cap.saturating_mul(2) {
            let old_len = self.order.len();
            let mut released = 0;
            self.order.retain(|(p, s)| {
                let keep = self.entries.get(p).is_some_and(|v| v.2 == *s);
                if !keep {
                    released += p.as_bytes().len();
                }
                keep
            });
            self.order.shrink_to_fit();
            let nodes = (old_len - self.order.len()) * PATH_QUEUE;
            self.nodes -= nodes;
            self.budget.release_nodes(nodes);
            self.charged -= released;
            self.budget.release(released);
        }
        while self.entries.len() > self.cap {
            if let Some((p, s)) = self.order.pop_front() {
                let mut released = p.as_bytes().len();
                let mut nodes = PATH_QUEUE;
                if self.entries.get(&p).is_some_and(|v| v.2 == s) {
                    self.entries.remove(&p);
                    nodes += PATH_NODE;
                    released += p.as_bytes().len();
                }
                self.nodes -= nodes;
                self.budget.release_nodes(nodes);
                self.charged -= released;
                self.budget.release(released);
            }
        }
    }
    pub fn remove(&mut self, path: &RawPath) -> Option<((u64, u64), i64)> {
        self.entries.remove(path).map(|(id, disk, _)| {
            self.entries.shrink_to_fit();
            self.nodes -= PATH_NODE;
            self.budget.release_nodes(PATH_NODE);
            let bytes = path.as_bytes().len();
            self.charged -= bytes;
            self.budget.release(bytes);
            (id, disk)
        })
    }
}

/// The documented 100,000-handle cap; only paths are cached, never directory fds.
pub(super) struct HandlePaths {
    entries: std::collections::HashMap<(i32, Vec<u8>), (RawPath, u64)>,
    order: std::collections::VecDeque<((i32, Vec<u8>), u64)>,
    sequence: u64,
    cap: usize,
    budget: Budget,
    charged: usize,
    nodes: usize,
}
impl HandlePaths {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Default::default(),
            order: Default::default(),
            sequence: 0,
            cap,
            budget: Budget::new(),
            charged: 0,
            nodes: 0,
        }
    }
    pub fn set_budget(&mut self, budget: Budget) {
        self.budget = budget;
    }
    pub fn clear(&mut self) {
        self.entries = Default::default();
        self.order = Default::default();
        self.budget.release(self.charged);
        self.budget.release_nodes(self.nodes);
        self.charged = 0;
        self.nodes = 0;
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
        let Some(handle) = super::budget::copy(&key.1) else {
            return;
        };
        let key = (key.0, handle);
        // Refresh existing handles without adding duplicate path allocations.
        let old_bytes = self
            .entries
            .get(&key)
            .map_or(0, |(p, _)| p.as_bytes().len() + key.1.len());
        let bytes = key.1.len() * 2 + path.as_bytes().len();
        if !self.budget.reserve(bytes) {
            return;
        }
        let nodes = HANDLE_QUEUE + usize::from(!self.entries.contains_key(&key)) * HANDLE_NODE;
        if !self.budget.reserve_nodes(nodes, false) {
            self.budget.release(bytes);
            return;
        }
        if self.entries.try_reserve(1).is_err() || self.order.try_reserve(1).is_err() {
            self.budget.release(bytes);
            self.budget.release_nodes(nodes);
            return;
        }
        self.nodes += nodes;
        self.charged += bytes;
        self.sequence = self.sequence.wrapping_add(1);
        self.entries
            .insert(key.clone(), (path.clone(), self.sequence));
        self.charged -= old_bytes;
        self.budget.release(old_bytes);
        self.order.push_back((key, self.sequence));
        if self.order.len() > self.cap.saturating_mul(2) {
            let old_len = self.order.len();
            let mut released = 0;
            self.order.retain(|(k, s)| {
                let keep = self.entries.get(k).is_some_and(|v| v.1 == *s);
                if !keep {
                    released += k.1.capacity();
                }
                keep
            });
            self.order.shrink_to_fit();
            let nodes = (old_len - self.order.len()) * HANDLE_QUEUE;
            self.nodes -= nodes;
            self.budget.release_nodes(nodes);
            self.charged -= released;
            self.budget.release(released);
        }
        while self.entries.len() > self.cap {
            if let Some((k, s)) = self.order.pop_front() {
                let mut released = k.1.capacity();
                let mut nodes = HANDLE_QUEUE;
                if self.entries.get(&k).is_some_and(|v| v.1 == s) {
                    if let Some((p, _)) = self.entries.remove(&k) {
                        released += k.1.len() + p.as_bytes().len();
                        nodes += HANDLE_NODE;
                    }
                }
                self.nodes -= nodes;
                self.budget.release_nodes(nodes);
                self.charged -= released;
                self.budget.release(released);
            }
        }
    }
}
impl Drop for PathSizes {
    fn drop(&mut self) {
        self.clear();
    }
}
impl Drop for HandlePaths {
    fn drop(&mut self) {
        self.clear();
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn shared_cache_budget_tracks_repeated_entries_eviction_and_clear() {
        let budget = Budget::with_limit(2048);
        let mut paths = PathSizes::new(10);
        let mut handles = HandlePaths::new(10);
        paths.set_budget(budget.clone());
        handles.set_budget(budget.clone());
        for n in 0..100 {
            let path = RawPath::new(format!("/{}-{n}", "x".repeat(100)).into_bytes()).unwrap();
            paths.remember(&path, (1, n), 7);
            handles.put((1, vec![n as u8; 100]), path.clone());
            let path_bytes = paths
                .entries
                .keys()
                .map(|p| p.as_bytes().len())
                .sum::<usize>()
                + paths
                    .order
                    .iter()
                    .map(|(p, _)| p.as_bytes().len())
                    .sum::<usize>();
            let handle_bytes = handles
                .entries
                .iter()
                .map(|(k, (p, _))| k.1.capacity() + p.as_bytes().len())
                .sum::<usize>()
                + handles
                    .order
                    .iter()
                    .map(|(k, _)| k.1.capacity())
                    .sum::<usize>();
            assert_eq!(budget.used(), path_bytes + handle_bytes);
            assert!(budget.used() <= 2048);
            paths.remove(&path);
        }
        paths.clear();
        handles.clear();
        assert_eq!(budget.used(), 0);
    }
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
