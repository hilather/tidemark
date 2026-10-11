//! Owned by: W2-B
//! Compact dent encoding and streaming merge join independent of XFS I/O.
use super::BulkError;
use crate::agg::Aggregator;
use smallvec::SmallVec;
use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};
use tidemark_core::scan::{DirId, FileKind, Gate, InodeAttrs, NsRecord};
use tidemark_sys::xfs::BulkInode;

const SELF: u32 = 1 << 31;
// Explicit cap for this API, which has no memory_budget parameter.
const MAX_DENTS: usize = 256 * 1024 * 1024 / 12;
const MESSAGE_ITEMS: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub(super) struct DentRef([u32; 3]);
impl DentRef {
    fn new(ino: u64, id: DirId, directory: bool) -> Result<Self, BulkError> {
        if id.0 >= SELF {
            return Err(BulkError::Limit("directory slot exceeds 31 bits"));
        }
        Ok(Self([
            ino as u32,
            (ino >> 32) as u32,
            id.0 | if directory { SELF } else { 0 },
        ]))
    }
    pub(super) fn ino(self) -> u64 {
        u64::from(self.0[0]) | (u64::from(self.0[1]) << 32)
    }
    fn dir(self) -> DirId {
        DirId(self.0[2] & !SELF)
    }
    fn directory(self) -> bool {
        self.0[2] & SELF != 0
    }
}

pub(super) fn append_namespace(dents: &mut Vec<DentRef>, ns: &NsRecord) -> Result<(), BulkError> {
    let count = dents
        .len()
        .checked_add(ns.children.len())
        .and_then(|n| n.checked_add(1))
        .ok_or(BulkError::Limit("dent count overflow"))?;
    if count > MAX_DENTS {
        return Err(BulkError::Limit("256 MiB dent budget"));
    }
    if ns.id.0 >= 1_000_000 {
        return Err(BulkError::Limit("one million directory slots"));
    }
    let own = DentRef::new(ns.ino, ns.id, true)?;
    if count > dents.capacity() {
        let capacity = count.max(dents.capacity().saturating_mul(2)).min(MAX_DENTS);
        dents
            .try_reserve_exact(capacity - dents.len())
            .map_err(|_| BulkError::Limit("dent allocation"))?;
    }
    dents.push(own);
    for &ino in &ns.children {
        dents.push(DentRef::new(ino, ns.id, false)?);
    }
    Ok(())
}

pub(super) struct Joined {
    pub(super) attrs: Option<InodeAttrs>,
    dirs: SmallVec<[DirId; 2]>,
    self_dirs: SmallVec<[DirId; 1]>,
    pub(super) races: SmallVec<[DirId; 2]>,
}
pub(super) trait Target {
    fn directory(&mut self, dir: DirId, a: &InodeAttrs);
    fn single(&mut self, dir: DirId, a: &InodeAttrs);
    fn links(&mut self, a: &InodeAttrs, dirs: &[DirId]);
    fn race(&mut self, dir: DirId);
}
impl Target for Aggregator {
    fn directory(&mut self, dir: DirId, a: &InodeAttrs) {
        self.set_dir_inode(dir, a);
    }
    fn single(&mut self, dir: DirId, a: &InodeAttrs) {
        self.add_single(dir, a);
    }
    fn links(&mut self, a: &InodeAttrs, dirs: &[DirId]) {
        self.add_link_set(a, dirs);
    }
    fn race(&mut self, dir: DirId) {
        self.add_races(Some(dir), 1);
    }
}
impl Joined {
    pub(super) fn dispatch(self, agg: &mut impl Target) {
        if let Some(a) = self.attrs {
            for dir in self.self_dirs {
                agg.directory(dir, &a);
            }
            if a.nlink > 1 && !self.dirs.is_empty() {
                agg.links(&a, &self.dirs);
            } else {
                for dir in self.dirs {
                    agg.single(dir, &a);
                }
            }
        }
        for dir in self.races {
            agg.race(dir);
        }
    }
}
pub(super) struct Batch {
    pub(super) streamed: u64,
    pub(super) items: Vec<Joined>,
}
fn kind(mode: u32) -> FileKind {
    match mode & 0o170000 {
        0o100000 => FileKind::Regular,
        0o040000 => FileKind::Dir,
        0o120000 => FileKind::Symlink,
        _ => FileKind::Other,
    }
}
fn matched(dents: &[DentRef], inode: Option<BulkInode>) -> Joined {
    let attrs = inode.map(|i| InodeAttrs {
        ino: i.ino,
        kind: kind(i.mode),
        nlink: i.nlink,
        disk: i.disk_bytes,
        apparent: i.size,
        mtime: i.mtime,
        projid: Some(i.projid),
    });
    let mut joined = Joined {
        attrs,
        dirs: SmallVec::new(),
        self_dirs: SmallVec::new(),
        races: SmallVec::new(),
    };
    for &dent in dents {
        match attrs {
            Some(a) if (a.kind == FileKind::Dir) == dent.directory() => {
                if dent.directory() {
                    joined.self_dirs.push(dent.dir());
                } else {
                    joined.dirs.push(dent.dir());
                }
            }
            _ => joined.races.push(dent.dir()),
        }
    }
    if attrs.is_some_and(|a| {
        a.kind != FileKind::Dir && (a.nlink == 0 || (a.nlink == 1 && joined.dirs.len() > 1))
    }) {
        // Multiple phase-1 names cannot all be live links of a single-link phase-2 inode.
        // Dropping these stale references avoids double charging an inode.
        joined.races.extend(joined.dirs.drain(..));
    }
    joined
}

/// Reject cross-AG records: disjoint ranges make global duplicates impossible.
pub(super) fn checked_range(
    source: impl Iterator<Item = io::Result<Vec<BulkInode>>>,
    lo: u64,
    hi: u64,
) -> impl Iterator<Item = io::Result<Vec<BulkInode>>> {
    source.map(move |batch| {
        let batch = batch?;
        if batch.iter().any(|i| i.ino < lo || i.ino >= hi) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bulk inode outside AG range",
            ));
        }
        Ok(batch)
    })
}

/// Iterator seam ensures synthetic sources exercise the same gating and join.
pub(super) fn stream(
    dents: &[DentRef],
    mut source: impl Iterator<Item = io::Result<Vec<BulkInode>>>,
    gate: &dyn Gate,
    cost: u32,
    cancel: &AtomicBool,
    mut emit: impl FnMut(Batch) -> Result<(), BulkError>,
) -> Result<(), BulkError> {
    let mut pos = 0;
    let mut previous = None;
    let mut batch = Batch {
        streamed: 0,
        items: Vec::with_capacity(MESSAGE_ITEMS),
    };
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(BulkError::Cancelled);
        }
        gate.acquire(cost.saturating_add(u32::from(previous.is_none())));
        let Some(inodes) = source.next() else {
            break;
        };
        let inodes = inodes.map_err(|e| {
            if e.kind() == io::ErrorKind::Unsupported {
                BulkError::Unsupported("v5 bulkstat required")
            } else {
                BulkError::Io(e)
            }
        })?;
        for inode in inodes {
            // Each AG owns a disjoint range. Duplicates/order violations indicate a broken source.
            if previous.is_some_and(|ino| inode.ino <= ino) {
                return Err(BulkError::Limit("duplicate or unordered bulk inode"));
            }
            previous = Some(inode.ino);
            batch.streamed += 1;
            while pos < dents.len() && dents[pos].ino() < inode.ino {
                let end = pos + dents[pos..].partition_point(|d| d.ino() == dents[pos].ino());
                batch.items.push(matched(&dents[pos..end], None));
                pos = end;
                flush(&mut batch, &mut emit)?;
            }
            if pos < dents.len() && dents[pos].ino() == inode.ino {
                let end = pos + dents[pos..].partition_point(|d| d.ino() == inode.ino);
                batch.items.push(matched(&dents[pos..end], Some(inode)));
                pos = end;
            }
            flush(&mut batch, &mut emit)?;
            // Bound even messages consisting entirely of out-of-root inodes.
            if batch.streamed >= MESSAGE_ITEMS as u64 {
                emit(std::mem::replace(
                    &mut batch,
                    Batch {
                        streamed: 0,
                        items: Vec::with_capacity(MESSAGE_ITEMS),
                    },
                ))?;
            }
        }
    }
    while pos < dents.len() {
        let end = pos + dents[pos..].partition_point(|d| d.ino() == dents[pos].ino());
        batch.items.push(matched(&dents[pos..end], None));
        pos = end;
        flush(&mut batch, &mut emit)?;
    }
    if batch.streamed > 0 || !batch.items.is_empty() {
        emit(batch)?;
    }
    Ok(())
}
fn flush(
    batch: &mut Batch,
    emit: &mut impl FnMut(Batch) -> Result<(), BulkError>,
) -> Result<(), BulkError> {
    if batch.items.len() >= MESSAGE_ITEMS {
        emit(std::mem::replace(
            batch,
            Batch {
                streamed: 0,
                items: Vec::with_capacity(MESSAGE_ITEMS),
            },
        ))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    #[derive(Default)]
    struct Recorder {
        directory: Vec<(DirId, InodeAttrs)>,
        single: Vec<(DirId, InodeAttrs)>,
        links: Vec<(InodeAttrs, Vec<DirId>)>,
        races: Vec<DirId>,
    }
    impl Target for Recorder {
        fn directory(&mut self, d: DirId, a: &InodeAttrs) {
            self.directory.push((d, *a));
        }
        fn single(&mut self, d: DirId, a: &InodeAttrs) {
            self.single.push((d, *a));
        }
        fn links(&mut self, a: &InodeAttrs, d: &[DirId]) {
            self.links.push((*a, d.to_vec()));
        }
        fn race(&mut self, d: DirId) {
            self.races.push(d);
        }
    }
    struct CountingGate(AtomicU32);
    impl Gate for CountingGate {
        fn acquire(&self, c: u32) {
            self.0.fetch_add(c, Ordering::Relaxed);
        }
        fn current_rate(&self) -> f64 {
            0.0
        }
    }
    fn inode(ino: u64, mode: u32, nlink: u32) -> BulkInode {
        BulkInode {
            ino,
            mode,
            nlink,
            uid: 0,
            gid: 0,
            size: 123,
            disk_bytes: 456,
            mtime: 7,
            projid: 9,
            xflags: 0,
        }
    }
    fn dent(ino: u64, dir: u32, own: bool) -> DentRef {
        DentRef::new(ino, DirId(dir), own).unwrap()
    }
    #[test]
    fn compact_layout_and_checked_slots() {
        assert_eq!(std::mem::size_of::<DentRef>(), 12);
        eprintln!(
            "DentRef={} Joined={} InodeAttrs={}",
            std::mem::size_of::<DentRef>(),
            std::mem::size_of::<Joined>(),
            std::mem::size_of::<InodeAttrs>()
        );
        let d = dent(u64::MAX, 0x7fff_ffff, true);
        assert_eq!(d.ino(), u64::MAX);
        assert_eq!(d.dir(), DirId(0x7fff_ffff));
        assert!(DentRef::new(1, DirId(0x8000_0000), true).is_err());
    }
    #[test]
    fn join_gaps_links_units_and_type_races() {
        let dents = [
            dent(2, 0, true),
            dent(3, 0, false),
            dent(4, 1, false),
            dent(4, 1, false),
            dent(4, 2, false),
            dent(5, 0, false),
            dent(6, 3, true),
            dent(7, 2, false),
        ];
        let gate = CountingGate(AtomicU32::new(0));
        let batches = vec![
            Ok(vec![
                inode(1, 0o100000, 1),
                inode(2, 0o040000, 2),
                inode(3, 0o120000, 1),
            ]),
            Ok(vec![
                inode(4, 0o100000, 65_537),
                inode(5, 0o040000, 2),
                inode(6, 0o100000, 1),
                inode(8, 0o100000, 1),
            ]),
        ];
        let mut recorder = Recorder::default();
        let mut streamed = 0;
        stream(
            &dents,
            batches.into_iter(),
            &gate,
            3,
            &AtomicBool::new(false),
            |b| {
                streamed += b.streamed;
                for j in b.items {
                    j.dispatch(&mut recorder);
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(streamed, 7);
        assert_eq!(gate.0.load(Ordering::Relaxed), 10); // geometry, two data and one terminal call
        assert_eq!(recorder.directory[0].1.disk, 456);
        assert_eq!(recorder.directory[0].1.projid, Some(9));
        assert_eq!(recorder.single[0].1.kind, FileKind::Symlink);
        assert_eq!(recorder.links[0].0.nlink, 65_537);
        assert_eq!(recorder.links[0].1, [DirId(1), DirId(1), DirId(2)]);
        assert_eq!(recorder.races, [DirId(0), DirId(3), DirId(2)]);
    }
    #[test]
    fn raced_single_link_aliases_are_not_double_charged() {
        let j = matched(
            &[dent(1, 0, false), dent(1, 1, false)],
            Some(inode(1, 0o100000, 1)),
        );
        let mut r = Recorder::default();
        j.dispatch(&mut r);
        assert!(r.single.is_empty());
        assert_eq!(r.races, [DirId(0), DirId(1)]);
        let j = matched(&[dent(1, 0, false)], Some(inode(1, 0o100000, 0)));
        j.dispatch(&mut r);
        assert!(r.single.is_empty());
    }
    #[test]
    fn duplicate_inode_across_batches_is_rejected() {
        let a = inode(2, 0o100000, 1);
        let r = stream(
            &[],
            vec![Ok(vec![a]), Ok(vec![a])].into_iter(),
            &CountingGate(AtomicU32::new(0)),
            1,
            &AtomicBool::new(false),
            |_| Ok(()),
        );
        assert!(matches!(r, Err(BulkError::Limit(_))));
    }
    #[test]
    fn gating_precedes_source_and_unsupported_is_typed() {
        let gate = CountingGate(AtomicU32::new(0));
        let source = std::iter::once_with(|| {
            assert_eq!(gate.0.load(Ordering::Relaxed), 5);
            Err(io::Error::from(io::ErrorKind::Unsupported))
        });
        assert!(matches!(
            stream(&[], source, &gate, 4, &AtomicBool::new(false), |_| Ok(())),
            Err(BulkError::Unsupported(_))
        ));
    }
    #[test]
    fn cancellation_never_advances_source() {
        let source = std::iter::once_with(|| -> io::Result<Vec<BulkInode>> {
            panic!("advanced after cancellation")
        });
        assert!(matches!(
            stream(
                &[],
                source,
                &CountingGate(AtomicU32::new(0)),
                1,
                &AtomicBool::new(true),
                |_| Ok(())
            ),
            Err(BulkError::Cancelled)
        ));
    }
    #[test]
    fn duplicate_across_allocation_groups_is_rejected() {
        let a = inode(15, 0o100000, 1);
        let mut first = checked_range(std::iter::once(Ok(vec![a])), 0, 16);
        assert!(first.next().unwrap().is_ok());
        let mut second = checked_range(std::iter::once(Ok(vec![a])), 16, 32);
        assert_eq!(
            second.next().unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    #[test]
    fn messages_and_ag_boundaries() {
        let dents = [dent(15, 0, false), dent(16, 1, false), dent(31, 2, false)];
        let mut recorder = Recorder::default();
        for (lo, hi) in [(0, 16), (16, 32)] {
            let begin = dents.partition_point(|d| d.ino() < lo);
            let end = dents.partition_point(|d| d.ino() < hi);
            stream(
                &dents[begin..end],
                vec![Ok((lo..hi).map(|i| inode(i, 0o100000, 1)).collect())].into_iter(),
                &CountingGate(AtomicU32::new(0)),
                1,
                &AtomicBool::new(false),
                |b| {
                    for j in b.items {
                        j.dispatch(&mut recorder);
                    }
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(recorder.single.len(), 3);
        let mut total = 0;
        stream(
            &[],
            vec![Ok((0..10_000).map(|i| inode(i, 0o100000, 1)).collect())].into_iter(),
            &CountingGate(AtomicU32::new(0)),
            1,
            &AtomicBool::new(false),
            |b| {
                assert!(b.streamed <= 4096);
                assert!(b.items.len() <= 4096);
                total += b.streamed;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(total, 10_000);
    }
}
