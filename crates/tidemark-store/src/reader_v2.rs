//! Owned by: WSNAP-reader2-core
//! Bounded chunk-backed directory queries for immutable v2 snapshots.
use super::*;
use crate::chunk::{self, ChunkDir, HashWidth};
use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::{BinaryHeap, VecDeque},
    sync::Arc,
};

fn missing() -> StoreError {
    StoreError::Sql(rusqlite::Error::QueryReturnedNoRows)
}
fn required(error: StoreError) -> StoreError {
    match error {
        StoreError::Sql(rusqlite::Error::QueryReturnedNoRows) => invalid(),
        other => other,
    }
}
fn invalid() -> StoreError {
    StoreError::Invalid("invalid v2 directory data".into())
}
fn bad(what: &str) -> StoreError {
    StoreError::Invalid(format!("invalid v2 snapshot: {what}"))
}
// Mirrors the codec's compressed-input ceiling (`chunk.rs`: MAX_BYTES + 1 MiB,
// private there) so oversized blobs are rejected before they are read.
const MAX_CHUNK_BLOB: usize = 40 * 1024 * 1024 + 1024 * 1024;
/// Stored chunk coordinates, read without materialising the BLOB.
#[derive(Clone, Copy)]
struct ChunkRef {
    first: u32,
    last: u32,
    len: usize,
}
fn id_col(v: rusqlite::Result<i64>, what: &str) -> Result<u32> {
    v.ok()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| bad(what))
}
fn chunk_ref(
    first: rusqlite::Result<i64>,
    last: rusqlite::Result<i64>,
    len: rusqlite::Result<Option<i64>>,
    kind: rusqlite::Result<String>,
) -> Result<ChunkRef> {
    let first = id_col(first, "dir_chunks.first_id out of range")?;
    let last = id_col(last, "dir_chunks.last_id out of range")?;
    // `length(data)` is evaluated only for BLOBs: on TEXT it reads the payload.
    if kind.as_deref() != Ok("blob") {
        return Err(bad("dir_chunks.data is not a blob"));
    }
    let len = len
        .ok()
        .flatten()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| bad("dir_chunks.data is not a blob"))?;
    if last < first {
        return Err(bad("dir_chunks.last_id precedes first_id"));
    }
    Ok(ChunkRef { first, last, len })
}
/// Reads the data of one chunk, rejecting oversized blobs before allocation.
fn chunk_data(conn: &Connection, c: ChunkRef) -> Result<Vec<u8>> {
    if c.len > MAX_CHUNK_BLOB {
        return Err(bad("dir_chunks.data exceeds codec input ceiling"));
    }
    let data = conn
        .query_row(
            "SELECT data FROM dir_chunks WHERE first_id=?",
            [c.first],
            |r| Ok(r.get::<_, Vec<u8>>(0)),
        )
        .optional()?
        .ok_or_else(missing)?;
    data.map_err(|_| bad("dir_chunks.data is not a blob"))
}
/// A chunk is usable only when its interval is exactly contiguous with its
/// neighbours: the first chunk starts at 0, and no chunk overlaps or leaves a gap.
fn check_neighbours(conn: &Connection, c: ChunkRef) -> Result<()> {
    let prev = conn
        .query_row(
            "SELECT last_id FROM dir_chunks WHERE first_id<? ORDER BY first_id DESC LIMIT 1",
            [c.first],
            |r| Ok(r.get::<_, i64>(0)),
        )
        .optional()?;
    match prev {
        None if c.first != 0 => return Err(bad("dir_chunks does not start at id 0")),
        Some(last) => {
            let last = id_col(last, "dir_chunks.last_id out of range")?;
            if last.checked_add(1) != Some(c.first) {
                return Err(bad("dir_chunks intervals overlap or leave a gap"));
            }
        }
        None => {}
    }
    let next = conn
        .query_row(
            "SELECT first_id FROM dir_chunks WHERE first_id>? ORDER BY first_id LIMIT 1",
            [c.first],
            |r| Ok(r.get::<_, i64>(0)),
        )
        .optional()?;
    if let Some(first) = next {
        if c.last.checked_add(1) != Some(id_col(first, "dir_chunks.first_id out of range")?) {
            return Err(bad("dir_chunks intervals overlap or leave a gap"));
        }
    }
    Ok(())
}
// At most 32 chunks and 8 MiB retained, plus one transient codec-bounded chunk
// (40 MiB decoded columns plus MAX_ROWS full rows). Arc keeps the single
// active range chunk alive during callbacks; parent reconstruction may retain
// that one chunk outside the cache. Heap/result storage is capped separately.
// Paths may exceed PATH_MAX when scanners use directory-relative syscalls.
// Bound reconstruction independently of tree depth and without recursion.
const MAX_PATH_BYTES: usize = 256 * 1024 * 1024;
const SHALLOW_COLUMNS: &str = "SELECT id,parent_id,subtree_end,depth,name,x'2f',0,ino,mtime,dir_disk,dir_apparent,projid,entries,errors,own_files,own_symlinks,own_charged,own_apparent,t_dirs,t_files,t_inodes,t_disk,t_apparent,t_charged,t_reclaimable,t_errors,age0,age1,age2,age3,age4,age5,newest_mtime,max_entries";
const CACHE_BYTES: usize = 8 * 1024 * 1024;
struct Cached {
    first: u32,
    last: u32,
    rows: Arc<Vec<ChunkDir>>,
    bytes: usize,
}
pub(super) struct Backend {
    plain_depth: u32,
    hash: HashWidth,
    cache: RefCell<VecDeque<Cached>>,
}
#[derive(Eq, PartialEq)]
struct Hit {
    key: (i128, Vec<u8>, u32),
    row: ChunkDir,
}
impl Ord for Hit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}
impl PartialOrd for Hit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
fn key(row: &ChunkDir, sort: SortKey) -> i128 {
    match sort {
        SortKey::Disk => row.t_disk.into(),
        SortKey::Apparent => row.t_apparent.into(),
        SortKey::Charged => row.t_charged.into(),
        SortKey::Reclaimable => row.t_reclaimable.into(),
        SortKey::Files => row.t_files.into(),
        SortKey::Inodes => row.t_inodes.into(),
        SortKey::Newest => row.newest_mtime.into(),
        SortKey::Name => 0,
    }
}
fn contains(name: &[u8], filter: &[u8]) -> bool {
    filter.is_empty() || name.windows(filter.len()).any(|w| w == filter)
}
impl Backend {
    fn from_shallow(row: DirRow) -> ChunkDir {
        ChunkDir {
            id: row.id,
            parent_id: row.parent_id,
            subtree_end: row.subtree_end,
            depth: row.depth,
            name: row.name,
            ino: row.ino,
            mtime: row.mtime,
            dir_disk: row.dir_disk,
            dir_apparent: row.dir_apparent,
            projid: row.projid,
            entries: row.entries,
            errors: row.errors,
            own_files: row.own_files,
            own_symlinks: row.own_symlinks,
            own_charged: row.own_charged,
            own_apparent: row.own_apparent,
            t_dirs: row.t_dirs,
            t_files: row.t_files,
            t_inodes: row.t_inodes,
            t_disk: row.t_disk,
            t_apparent: row.t_apparent,
            t_charged: row.t_charged,
            t_reclaimable: row.t_reclaimable,
            t_errors: row.t_errors,
            age0: row.age0,
            age1: row.age1,
            age2: row.age2,
            age3: row.age3,
            age4: row.age4,
            age5: row.age5,
            newest_mtime: row.newest_mtime,
            max_entries: row.max_entries,
            file_lo: 0,
            file_count: 0,
        }
    }
    pub(super) fn open(conn: &Connection) -> Result<Self> {
        let get = |key| -> Result<u32> {
            let s = conn
                .query_row("SELECT value FROM meta WHERE key=?", [key], |r| {
                    r.get::<_, String>(0)
                })
                .optional()
                .map_err(|e| match e {
                    rusqlite::Error::InvalidColumnType(..)
                    | rusqlite::Error::FromSqlConversionFailure(..) => {
                        bad(&format!("meta key {key} is not text"))
                    }
                    other => StoreError::Sql(other),
                })?
                .ok_or_else(|| bad(&format!("meta key {key} is missing")))?;
            s.parse()
                .map_err(|_| bad(&format!("meta key {key} is not a number")))
        };
        Ok(Self {
            plain_depth: get("plain_depth")?,
            hash: HashWidth::new(get("hash_width")?)?,
            cache: RefCell::new(VecDeque::new()),
        })
    }
    fn chunk(&self, snap: &Snapshot, id: u32) -> Result<Arc<Vec<ChunkDir>>> {
        let mut cache = self.cache.borrow_mut();
        if let Some(i) = cache.iter().position(|c| c.first <= id && id <= c.last) {
            let entry = cache.remove(i).ok_or_else(invalid)?;
            let rows = entry.rows.clone();
            cache.push_front(entry);
            return Ok(rows);
        }
        let found = self.locate(snap, id)?.ok_or_else(missing)?;
        let ChunkRef { first, last, .. } = found;
        check_neighbours(&snap.conn, found)?;
        let data = chunk_data(&snap.conn, found)?;
        let rows = Arc::new(chunk::decode_dirs(&data, first)?);
        if rows.last().map(|r| r.id) != Some(last) {
            return Err(invalid());
        }
        if id > last {
            return Err(missing());
        }
        let bytes = rows.capacity() * std::mem::size_of::<ChunkDir>()
            + rows.iter().map(|r| r.name.capacity()).sum::<usize>();
        if bytes <= CACHE_BYTES {
            while cache.len() >= 32
                || cache.iter().map(|c| c.bytes).sum::<usize>() + bytes > CACHE_BYTES
            {
                cache.pop_back();
            }
            cache.push_front(Cached {
                first,
                last,
                rows: rows.clone(),
                bytes,
            });
        }
        Ok(rows)
    }
    fn locate(&self, snap: &Snapshot, id: u32) -> Result<Option<ChunkRef>> {
        snap.conn
            .query_row(
                "SELECT first_id,last_id,CASE WHEN typeof(data)='blob' THEN length(data) END,typeof(data) FROM dir_chunks WHERE first_id<=? ORDER BY first_id DESC LIMIT 1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0),
                        r.get::<_, i64>(1),
                        r.get::<_, Option<i64>>(2),
                        r.get::<_, String>(3),
                    ))
                },
            )
            .optional()?
            .map(|(a, b, c, d)| chunk_ref(a, b, c, d))
            .transpose()
    }
    // Shallow rows (depth <= plain_depth) come from SQL without touching chunks.
    fn shallow_row(&self, snap: &Snapshot, id: u32) -> Result<Option<ChunkDir>> {
        let mut stmt = snap
            .conn
            .prepare(&format!("{SHALLOW_COLUMNS} FROM shallow_dirs WHERE id=?"))?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::from_shallow(dir_row(row)?))),
            None => Ok(None),
        }
    }
    fn raw(&self, snap: &Snapshot, id: u32) -> Result<ChunkDir> {
        let rows = self.chunk(snap, id)?;
        let first = rows.first().ok_or_else(invalid)?.id;
        rows.get((id - first) as usize).cloned().ok_or_else(invalid)
    }
    fn path(&self, snap: &Snapshot, row: &ChunkDir) -> Result<RawPath> {
        let mut names = Vec::new();
        let mut current = row.clone();
        let mut bytes = 0usize;
        while let Some(parent) = current.parent_id {
            if parent >= current.id || names.len() >= crate::error::MAX_QUERY_ROWS {
                return Err(invalid());
            }
            bytes = bytes
                .checked_add(current.name.len() + 1)
                .ok_or_else(invalid)?;
            if bytes > MAX_PATH_BYTES {
                return Err(invalid());
            }
            names.push(RawName::new(current.name.clone()).map_err(|_| invalid())?);
            let next = self.raw(snap, parent).map_err(required)?;
            if next.depth.checked_add(1) != Some(current.depth)
                || next.subtree_end < current.subtree_end
            {
                return Err(invalid());
            }
            current = next;
        }
        if current.id != 0 || current.depth != 0 {
            return Err(invalid());
        }
        let root = RawPath::new(current.name).map_err(|_| invalid())?;
        let total = bytes
            .checked_add(root.as_bytes().len())
            .ok_or_else(invalid)?;
        if total > MAX_PATH_BYTES {
            return Err(invalid());
        }
        let mut raw = Vec::with_capacity(total);
        raw.extend_from_slice(root.as_bytes());
        for name in names.iter().rev() {
            if raw.len() > 1 {
                raw.push(b'/');
            }
            raw.extend_from_slice(name.as_bytes());
        }
        let path = RawPath::new(raw).map_err(|_| invalid())?;
        Ok(path)
    }
    fn materialize(&self, snap: &Snapshot, r: ChunkDir) -> Result<DirRow> {
        let path = self.path(snap, &r)?;
        Ok(DirRow {
            id: r.id,
            parent_id: r.parent_id,
            subtree_end: r.subtree_end,
            depth: r.depth,
            name: r.name,
            ino: r.ino,
            mtime: r.mtime,
            dir_disk: r.dir_disk,
            dir_apparent: r.dir_apparent,
            projid: r.projid,
            entries: r.entries,
            errors: r.errors,
            own_files: r.own_files,
            own_symlinks: r.own_symlinks,
            own_charged: r.own_charged,
            own_apparent: r.own_apparent,
            t_dirs: r.t_dirs,
            t_files: r.t_files,
            t_inodes: r.t_inodes,
            t_disk: r.t_disk,
            t_apparent: r.t_apparent,
            t_charged: r.t_charged,
            t_reclaimable: r.t_reclaimable,
            t_errors: r.t_errors,
            age0: r.age0,
            age1: r.age1,
            age2: r.age2,
            age3: r.age3,
            age4: r.age4,
            age5: r.age5,
            newest_mtime: r.newest_mtime,
            max_entries: r.max_entries,
            path_hash: path.hash64(),
            path,
        })
    }
    pub(super) fn dir(&self, snap: &Snapshot, id: u32) -> Result<DirRow> {
        self.materialize(snap, self.raw(snap, id)?)
    }
    pub(super) fn dir_by_path(&self, snap: &Snapshot, path: &RawPath) -> Result<Option<DirRow>> {
        let hash = self
            .hash
            .key(u64::from_ne_bytes(path.hash64().to_ne_bytes()));
        let mut stmt = snap
            .conn
            .prepare("SELECT id FROM dir_lookup WHERE hash=? ORDER BY id")?;
        let mut rows = stmt.query([hash])?;
        while let Some(row) = rows.next()? {
            let dir = self.dir(snap, row.get(0)?).map_err(required)?;
            if &dir.path == path {
                return Ok(Some(dir));
            }
        }
        Ok(None)
    }
    fn visit_children(
        &self,
        snap: &Snapshot,
        id: u32,
        mut visit: impl FnMut(ChunkDir) -> Result<()>,
    ) -> Result<()> {
        // Missing parent is an empty listing in v1.
        let exists = self.locate(snap, id)?.is_some_and(|c| c.last >= id);
        if !exists {
            return Ok(());
        }
        let parent = self.raw(snap, id)?;
        if parent.depth < self.plain_depth {
            let mut stmt = snap.conn.prepare(&format!(
                "{SHALLOW_COLUMNS} FROM shallow_dirs WHERE parent_id=? ORDER BY id"
            ))?;
            let mut rows = stmt.query([id])?;
            while let Some(row) = rows.next()? {
                let row = dir_row(row)?;
                visit(Self::from_shallow(row))?;
            }
        } else {
            let mut next = id.checked_add(1);
            while let Some(child_id) = next.filter(|n| *n <= parent.subtree_end) {
                let child = self.raw(snap, child_id).map_err(required)?;
                if child.parent_id != Some(id) || child.subtree_end > parent.subtree_end {
                    return Err(invalid());
                }
                next = child.subtree_end.checked_add(1);
                visit(child)?;
            }
        }
        Ok(())
    }
    fn range(
        &self,
        snap: &Snapshot,
        lo: u32,
        hi: u32,
        mut visit: impl FnMut(ChunkDir) -> Result<()>,
    ) -> Result<()> {
        let mut next = lo;
        loop {
            let rows = self.chunk(snap, next).map_err(required)?;
            for row in rows.iter().filter(|r| r.id >= next && r.id <= hi) {
                visit(row.clone())?;
            }
            let last = rows.last().ok_or_else(invalid)?.id;
            if last >= hi {
                break;
            }
            next = last.checked_add(1).ok_or_else(invalid)?;
        }
        Ok(())
    }
    fn collect(
        &self,
        snap: &Snapshot,
        limit: usize,
        feed: impl FnOnce(&mut dyn FnMut(ChunkDir) -> Result<()>) -> Result<()>,
        key: impl Fn(&ChunkDir) -> Result<(i128, Vec<u8>, u32)>,
    ) -> Result<Vec<DirRow>> {
        sql_limit(limit)?;
        let cap = limit.min(crate::error::MAX_QUERY_ROWS + 1);
        let mut heap = BinaryHeap::<Hit>::new();
        feed(&mut |row| {
            if cap == 0 {
                return Ok(());
            }
            let hit = Hit {
                key: key(&row)?,
                row,
            };
            if heap.len() < cap {
                heap.push(hit);
            } else if heap.peek().is_some_and(|worst| hit.key < worst.key) {
                heap.pop();
                heap.push(hit);
            }
            Ok(())
        })?;
        crate::error::check_result_bound(heap.as_slice())?;
        heap.into_sorted_vec()
            .into_iter()
            .map(|hit| self.materialize(snap, hit.row))
            .collect()
    }
    pub(super) fn children(
        &self,
        snap: &Snapshot,
        id: u32,
        sort: SortKey,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        self.collect(
            snap,
            limit,
            |v| self.visit_children(snap, id, v),
            |r| Ok((-key(r, sort), r.name.clone(), r.id)),
        )
    }
    pub(super) fn children_count(&self, snap: &Snapshot, id: u32, filter: &[u8]) -> Result<u64> {
        let mut count = 0;
        self.visit_children(snap, id, |r| {
            if contains(&r.name, filter) {
                count += 1;
            }
            Ok(())
        })?;
        Ok(count)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn children_page(
        &self,
        snap: &Snapshot,
        id: u32,
        sort: SortKey,
        descending: bool,
        cursor: Option<&DirRow>,
        reverse: bool,
        limit: usize,
        filter: &[u8],
    ) -> Result<Vec<DirRow>> {
        if limit > crate::error::MAX_QUERY_ROWS {
            return Err(StoreError::Invalid("child page exceeds query cap".into()));
        }
        // Use the v1 cursor conversions, including rejecting out-of-domain caller values.
        let cursor_key = cursor
            .map(|r| -> Result<i128> {
                Ok(match sort {
                    SortKey::Disk => sql_u64(r.t_disk)?.into(),
                    SortKey::Apparent => sql_u64(r.t_apparent)?.into(),
                    SortKey::Charged => sql_u64(r.t_charged)?.into(),
                    SortKey::Reclaimable => sql_u64(r.t_reclaimable)?.into(),
                    SortKey::Files => sql_u64(r.t_files)?.into(),
                    SortKey::Inodes => sql_u64(r.t_inodes)?.into(),
                    SortKey::Newest => r.newest_mtime.into(),
                    SortKey::Name => 0,
                })
            })
            .transpose()?;
        let compare = |r: &ChunkDir| {
            let primary = if descending ^ reverse {
                -key(r, sort)
            } else {
                key(r, sort)
            };
            let mut name = r.name.clone();
            // Bytewise descending order cannot be represented by complementing bytes
            // (prefix names reverse too), so use an explicit rank encoding with terminator.
            if if sort == SortKey::Name {
                descending ^ reverse
            } else {
                reverse
            } {
                name = r
                    .name
                    .iter()
                    .flat_map(|b| [0, 255 - *b])
                    .chain([1])
                    .collect();
            }
            (primary, name, 0)
        };
        self.collect(
            snap,
            limit,
            |v| {
                self.visit_children(snap, id, |r| {
                    if !contains(&r.name, filter) {
                        return Ok(());
                    }
                    if let (Some(c), Some(k)) = (cursor, cursor_key) {
                        let mut cmp = key(&r, sort).cmp(&k);
                        if descending ^ reverse {
                            cmp = cmp.reverse();
                        }
                        if cmp == Ordering::Equal {
                            cmp = r.name.cmp(&c.name);
                            if if sort == SortKey::Name {
                                descending ^ reverse
                            } else {
                                reverse
                            } {
                                cmp = cmp.reverse();
                            }
                        }
                        if cmp != Ordering::Greater {
                            return Ok(());
                        }
                    }
                    v(r)
                })
            },
            |r| Ok(compare(r)),
        )
    }
    // Metric range scans compare projected keys before materializing names or
    // full rows. Equal keys still decode names to preserve v1's tie ordering.
    fn projected_subtree(
        &self,
        snap: &Snapshot,
        lo: u32,
        hi: u32,
        max_depth: u64,
        sort: SortKey,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        sql_limit(limit)?;
        let cap = limit.min(crate::error::MAX_QUERY_ROWS + 1);
        if cap == 0 {
            return Ok(Vec::new());
        }
        let mut heap = BinaryHeap::<Hit>::new();
        self.projected_chunks(snap, lo, hi, |first, p| {
            for offset in 0..p.entries.len() {
                let id = first
                    .checked_add(u32::try_from(offset).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?;
                if id < lo || id > hi {
                    continue;
                }
                let value: i128 = match sort {
                    SortKey::Disk => p.t_disk[offset].into(),
                    SortKey::Reclaimable => p.t_reclaimable[offset].into(),
                    SortKey::Files => p.t_files[offset].into(),
                    SortKey::Newest => p.newest_mtime[offset].into(),
                    _ => return Err(invalid()),
                };
                if heap.len() == cap && heap.peek().is_some_and(|worst| -value > worst.key.0) {
                    continue;
                }
                let row = self.raw(snap, id)?;
                if u64::from(row.depth) > max_depth {
                    continue;
                }
                let hit = Hit {
                    key: (-value, row.name.clone(), id),
                    row,
                };
                if heap.len() < cap {
                    heap.push(hit);
                } else if heap.peek().is_some_and(|worst| hit.key < worst.key) {
                    heap.pop();
                    heap.push(hit);
                }
            }
            Ok(())
        })?;
        crate::error::check_result_bound(heap.as_slice())?;
        heap.into_sorted_vec()
            .into_iter()
            .map(|h| self.materialize(snap, h.row))
            .collect()
    }
    pub(super) fn subtree_dirs(
        &self,
        snap: &Snapshot,
        id: u32,
        depth: u32,
        sort: SortKey,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        let shallow_root = self.shallow_row(snap, id)?;
        let root = match shallow_root {
            Some(root) => root,
            None => self.raw(snap, id)?,
        };
        let max = u64::from(root.depth) + u64::from(depth);
        if max <= u64::from(self.plain_depth) && root.depth <= self.plain_depth {
            // Every requested row is a shallow row: no chunk is decoded.
            return self.collect(
                snap,
                limit,
                |v| {
                    let mut stmt = snap.conn.prepare(&format!(
                        "{SHALLOW_COLUMNS} FROM shallow_dirs WHERE id>=? AND id<=? AND depth<=? ORDER BY id"
                    ))?;
                    let mut rows = stmt.query(params![id, root.subtree_end, i64::try_from(max).map_err(|_| invalid())?])?;
                    while let Some(row) = rows.next()? {
                        v(Self::from_shallow(dir_row(row)?))?;
                    }
                    Ok(())
                },
                |r| Ok((-key(r, sort), r.name.clone(), r.id)),
            );
        }
        if matches!(
            sort,
            SortKey::Disk | SortKey::Reclaimable | SortKey::Files | SortKey::Newest
        ) {
            return self.projected_subtree(snap, id, root.subtree_end, max, sort, limit);
        }
        self.collect(
            snap,
            limit,
            |v| {
                self.range(snap, id, root.subtree_end, |r| {
                    if u64::from(r.depth) <= max {
                        v(r)
                    } else {
                        Ok(())
                    }
                })
            },
            |r| Ok((-key(r, sort), r.name.clone(), r.id)),
        )
    }
    pub(super) fn wide_dirs(
        &self,
        snap: &Snapshot,
        id: u32,
        min: u32,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        let root = self.raw(snap, id)?;
        self.collect(
            snap,
            limit,
            |v| {
                self.entries_range(snap, id, root.subtree_end, min, |r| {
                    if r.entries >= min {
                        v(r)
                    } else {
                        Ok(())
                    }
                })
            },
            |r| Ok((-i128::from(r.entries), r.name.clone(), r.id)),
        )
    }
    fn eligible(
        &self,
        snap: &Snapshot,
        depth: u32,
        hi: u32,
        mut visit: impl FnMut(ChunkDir) -> Result<()>,
    ) -> Result<()> {
        if depth <= self.plain_depth {
            let mut stmt = snap.conn.prepare(&format!(
                "{SHALLOW_COLUMNS} FROM shallow_dirs WHERE depth<=? ORDER BY id"
            ))?;
            let mut rows = stmt.query([depth])?;
            while let Some(row) = rows.next()? {
                visit(Self::from_shallow(dir_row(row)?))?;
            }
            Ok(())
        } else {
            self.range(snap, 0, hi, |r| {
                if r.depth <= depth {
                    visit(r)
                } else {
                    Ok(())
                }
            })
        }
    }
    // Every projected chunk is fully validated by the codec. The directory
    // table bounds must agree, and no gaps are allowed inside a range.
    fn projected_chunks(
        &self,
        snap: &Snapshot,
        lo: u32,
        hi: u32,
        mut visit: impl FnMut(u32, chunk::DirProjection) -> Result<()>,
    ) -> Result<()> {
        let mut stmt = snap.conn.prepare("SELECT first_id,last_id,CASE WHEN typeof(data)='blob' THEN length(data) END,typeof(data) FROM dir_chunks WHERE first_id >= (SELECT first_id FROM dir_chunks WHERE first_id<=?1 ORDER BY first_id DESC LIMIT 1) AND first_id<=?2 ORDER BY first_id")?;
        let mut chunks = stmt.query(params![lo, hi])?;
        let mut expected = None;
        let mut covered = false;
        let mut seen = None;
        while let Some(chunk) = chunks.next()? {
            let c = chunk_ref(chunk.get(0), chunk.get(1), chunk.get(2), chunk.get(3))?;
            if expected.is_some_and(|id| id != c.first) {
                return Err(bad("dir_chunks intervals overlap or leave a gap"));
            }
            if seen.is_none() {
                check_neighbours(&snap.conn, c)?;
            }
            seen = Some(c);
            let data = chunk_data(&snap.conn, c)?;
            let projection = chunk::decode_dir_projection(&data, c.first)?;
            let count = u32::try_from(projection.entries.len()).map_err(|_| invalid())?;
            if count == 0 || c.first.checked_add(count - 1) != Some(c.last) {
                return Err(invalid());
            }
            visit(c.first, projection)?;
            covered = c.last >= hi;
            expected = c.last.checked_add(1);
        }
        if let Some(c) = seen {
            check_neighbours(&snap.conn, c)?;
        }
        if !covered {
            return Err(invalid());
        }
        Ok(())
    }
    // Avoid allocating full rows/names for excluded entries thresholds.
    fn entries_range(
        &self,
        snap: &Snapshot,
        lo: u32,
        hi: u32,
        min: u32,
        mut visit: impl FnMut(ChunkDir) -> Result<()>,
    ) -> Result<()> {
        if min == 0 {
            return self.range(snap, lo, hi, visit);
        }
        self.projected_chunks(snap, lo, hi, |first, p| {
            for (offset, entries) in p.entries.into_iter().enumerate() {
                let id = first
                    .checked_add(u32::try_from(offset).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?;
                if id >= lo && id <= hi && entries >= min {
                    visit(self.raw(snap, id)?)?;
                }
            }
            Ok(())
        })
    }
    pub(super) fn metrics_candidates(
        &self,
        snap: &Snapshot,
        depth: u32,
        limit: usize,
    ) -> Result<(Vec<DirRow>, Vec<DirRow>, u64, std::path::PathBuf)> {
        if limit > crate::error::MAX_QUERY_ROWS {
            return Err(StoreError::Invalid(
                "metrics candidate limit exceeds 1,000,000-row query cap".into(),
            ));
        }
        let root = match self.shallow_row(snap, 0)? {
            Some(root) => root,
            None => self.raw(snap, 0)?,
        };
        let mut count = 0;
        let mut query = |sort| {
            self.collect(
                snap,
                limit,
                |v| {
                    self.eligible(snap, depth, root.subtree_end, |r| {
                        if r.depth <= depth {
                            count += 1;
                            v(r)
                        } else {
                            Ok(())
                        }
                    })
                },
                |r| Ok((-key(r, sort), self.path(snap, r)?.as_bytes().to_vec(), r.id)),
            )
        };
        let used = query(SortKey::Disk)?;
        let reclaim = query(SortKey::Reclaimable)?;
        Ok((used, reclaim, count / 2, snap.file.clone()))
    }
}
