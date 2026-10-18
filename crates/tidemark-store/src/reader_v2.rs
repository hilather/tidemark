//! Owned by: WSNAP-reader2-core
//! Bounded chunk-backed directory queries for immutable v2 snapshots.
use super::*;
use crate::chunk::{self, ChunkDir, ChunkFile, ChunkLinkInstance, HashWidth};
use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, VecDeque},
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
const SHALLOW_COLUMNS: &str = super::LITE_COLUMNS;
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
    files: RefCell<SideCache<ChunkFile>>,
    links: RefCell<SideCache<ChunkLinkInstance>>,
    paths: RefCell<PathCache>,
    /// Directory chunks decoded so far (work-bound tests).
    #[cfg(test)]
    decodes: std::cell::Cell<u64>,
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
/// Validates a depth-limited shallow preorder as it streams past: parent links,
/// depths, subtree spans and contiguity (rows deeper than `limit` are absent).
struct ShallowWalk {
    limit: u32,
    end: u32,
    next: u64,
    started: bool,
    stack: Vec<(u32, u32, u32)>,
}
impl ShallowWalk {
    fn new(lo: u32, end: u32, limit: u32) -> Self {
        Self {
            limit,
            end,
            next: u64::from(lo),
            started: false,
            stack: Vec::new(),
        }
    }
    fn check(&mut self, r: &ChunkDir) -> Result<()> {
        if u64::from(r.id) != self.next || r.subtree_end < r.id || r.subtree_end > self.end {
            return Err(invalid());
        }
        while self.stack.last().is_some_and(|p| p.1 < r.id) {
            self.stack.pop();
        }
        match self.stack.last() {
            Some(&(pid, pend, pdepth)) => {
                if r.parent_id != Some(pid)
                    || Some(r.depth) != pdepth.checked_add(1)
                    || r.id > pend
                    || r.subtree_end > pend
                {
                    return Err(invalid());
                }
            }
            None if self.started => return Err(invalid()),
            None => {}
        }
        self.started = true;
        self.next = if r.depth < self.limit && r.subtree_end > r.id {
            u64::from(r.id) + 1
        } else {
            u64::from(r.subtree_end) + 1
        };
        self.stack.push((r.id, r.subtree_end, r.depth));
        Ok(())
    }
    fn finish(&self) -> Result<()> {
        if self.started && self.next == u64::from(self.end) + 1 {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
fn max_u32(v: u64) -> Result<u32> {
    u32::try_from(v).map_err(|_| invalid())
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
            files: RefCell::new(SideCache::new()),
            links: RefCell::new(SideCache::new()),
            paths: RefCell::new(PathCache::default()),
            #[cfg(test)]
            decodes: std::cell::Cell::new(0),
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
        #[cfg(test)]
        self.decodes.set(self.decodes.get() + 1);
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
            Some(row) => Ok(Some(Self::from_shallow(
                dir_row(row).map_err(plain_row_error)?,
            ))),
            None => Ok(None),
        }
    }
    pub(super) fn raw(&self, snap: &Snapshot, id: u32) -> Result<ChunkDir> {
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
            // The shallow copy must be exactly the parent's child set: first child
            // at id+1, each next child right after the previous subtree, and the
            // last subtree ending at the parent's.
            let mut expect = if parent.subtree_end > id {
                id.checked_add(1)
            } else {
                None
            };
            let child_depth = parent.depth.checked_add(1).ok_or_else(invalid)?;
            while let Some(row) = rows.next()? {
                let row = dir_row(row).map_err(plain_row_error)?;
                if Some(row.id) != expect
                    || row.parent_id != Some(id)
                    || row.depth != child_depth
                    || row.subtree_end < row.id
                    || row.subtree_end > parent.subtree_end
                {
                    return Err(invalid());
                }
                expect = if row.subtree_end < parent.subtree_end {
                    Some(row.subtree_end + 1)
                } else {
                    None
                };
                visit(Self::from_shallow(row))?;
            }
            if expect.is_some() {
                return Err(invalid());
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
    /// One page of `parent`'s children in name (= preorder) order, resuming at a
    /// next-sibling id and the last name returned; see [`Snapshot::children_by_name`].
    pub(super) fn children_by_name(
        &self,
        snap: &Snapshot,
        parent: u32,
        cursor: Option<&ChildCursor>,
        limit: usize,
        out: &mut Vec<ChildEntry>,
    ) -> Result<Option<ChildCursor>> {
        let p = match self.raw(snap, parent) {
            Ok(p) => p,
            Err(StoreError::Sql(rusqlite::Error::QueryReturnedNoRows)) => return Ok(None),
            Err(other) => return Err(other),
        };
        let first = parent.checked_add(1).ok_or_else(invalid)?;
        let (start, cursor_name) = match cursor {
            None => (first, None),
            Some(ChildCursor(CursorAt::Id(id, name))) => (*id, Some(name.as_slice())),
            Some(_) => return Err(bad("foreign child cursor")),
        };
        if p.subtree_end <= parent {
            return Ok(None);
        }
        if start < first || start > p.subtree_end {
            return Err(bad("child cursor lies outside the parent's subtree"));
        }
        let child_depth = p.depth.checked_add(1).ok_or_else(invalid)?;
        let mut prev: Option<Vec<u8>> = cursor_name.map(<[u8]>::to_vec);
        let mut taken = 0usize;
        let mut next = start;
        while next <= p.subtree_end {
            if taken == limit {
                let after = prev.ok_or_else(invalid)?;
                return Ok(Some(ChildCursor(CursorAt::Id(next, after))));
            }
            // Name order is preorder order, so the chunks serve every parent.
            let c = self.raw(snap, next).map_err(required)?;
            let ordered = prev.as_ref().is_none_or(|before| *before < c.name);
            if c.id != next
                || c.parent_id != Some(parent)
                || c.depth != child_depth
                || c.subtree_end < c.id
                || c.subtree_end > p.subtree_end
                || !ordered
                || RawName::new(c.name.clone()).is_err()
            {
                return Err(invalid());
            }
            prev = Some(c.name.clone());
            out.push(ChildEntry {
                id: c.id,
                name: c.name,
                depth: c.depth,
                subtree_end: c.subtree_end,
                t_disk: c.t_disk,
                t_reclaimable: c.t_reclaimable,
                t_files: c.t_files,
                own_charged: c.own_charged,
            });
            taken += 1;
            next = c.subtree_end.checked_add(1).ok_or_else(invalid)?;
        }
        Ok(None)
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
    /// The requested root from its directory chunk (the source `dir(id)` uses).
    /// A shallow copy, when present, must agree on id, parent, depth and span.
    fn authoritative_root(&self, snap: &Snapshot, id: u32) -> Result<ChunkDir> {
        let root = self.raw(snap, id)?;
        if let Some(shallow) = self.shallow_row(snap, id)? {
            if shallow.id != root.id
                || shallow.parent_id != root.parent_id
                || shallow.depth != root.depth
                || shallow.subtree_end != root.subtree_end
            {
                return Err(invalid());
            }
        }
        Ok(root)
    }
    pub(super) fn subtree_dirs(
        &self,
        snap: &Snapshot,
        id: u32,
        depth: u32,
        sort: SortKey,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        let root = self.authoritative_root(snap, id)?;
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
                    let mut walk = ShallowWalk::new(id, root.subtree_end, max_u32(max)?);
                    while let Some(row) = rows.next()? {
                        let r = Self::from_shallow(dir_row(row).map_err(plain_row_error)?);
                        walk.check(&r)?;
                        v(r)?;
                    }
                    walk.finish()
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
            let mut walk = ShallowWalk::new(0, hi, depth);
            while let Some(row) = rows.next()? {
                let r = Self::from_shallow(dir_row(row).map_err(plain_row_error)?);
                walk.check(&r)?;
                visit(r)?;
            }
            walk.finish()
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
        let root = self.authoritative_root(snap, 0)?;
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

// ---- notable files, link instances and path streaming ----
//
// File and link-instance chunks are located through their own metadata
// columns. Decoded chunks of both kinds are kept in two small caches (at most
// `SIDE_CACHE_CHUNKS` chunks and `SIDE_CACHE_BYTES` each), on top of the
// directory cache above; reconstructed directory paths are cached separately
// (`PATH_CACHE_BYTES`).
const SIDE_CACHE_CHUNKS: usize = 4;
const SIDE_CACHE_BYTES: usize = 8 * 1024 * 1024;
const PATH_CACHE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Files,
    Links,
}
impl Kind {
    fn table(self) -> &'static str {
        match self {
            Kind::Files => "file_chunks",
            Kind::Links => "li_chunks",
        }
    }
    fn keys(self) -> (&'static str, &'static str) {
        match self {
            Kind::Files => ("first_dir", "last_dir"),
            Kind::Links => ("first_ino", "last_ino"),
        }
    }
}
/// Chunk coordinates read without materialising the BLOB.
#[derive(Clone, Copy)]
struct Meta {
    first: u64,
    last: u64,
    lo: u64,
    hi: u64,
    len: usize,
}
fn nonneg(v: rusqlite::Result<i64>, what: &str) -> Result<u64> {
    v.ok()
        .and_then(|v| u64::try_from(v).ok())
        .ok_or_else(|| bad(what))
}
fn meta_sql(kind: Kind) -> String {
    let (lo, hi) = kind.keys();
    format!(
        "SELECT first_row,last_row,{lo},{hi},CASE WHEN typeof(data)='blob' THEN length(data) END,typeof(data) FROM {}",
        kind.table()
    )
}
fn read_meta(kind: Kind, r: &Row<'_>) -> Result<Meta> {
    let t = kind.table();
    let (lo_name, hi_name) = kind.keys();
    let first = nonneg(r.get(0), &format!("{t}.first_row out of range"))?;
    let last = nonneg(r.get(1), &format!("{t}.last_row out of range"))?;
    let lo = nonneg(r.get(2), &format!("{t}.{lo_name} out of range"))?;
    let hi = nonneg(r.get(3), &format!("{t}.{hi_name} out of range"))?;
    // `length(data)` is evaluated only for BLOBs: on TEXT it reads the payload.
    if r.get::<_, String>(5).as_deref() != Ok("blob") {
        return Err(bad(&format!("{t}.data is not a blob")));
    }
    let len = r
        .get::<_, Option<i64>>(4)
        .ok()
        .flatten()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| bad(&format!("{t}.data is not a blob")))?;
    if last < first {
        return Err(bad(&format!("{t}.last_row precedes first_row")));
    }
    if hi < lo {
        return Err(bad(&format!("{t}.{hi_name} precedes {lo_name}")));
    }
    if kind == Kind::Files && (last > u64::from(u32::MAX) || hi > u64::from(u32::MAX)) {
        return Err(bad(&format!("{t} row or directory id out of range")));
    }
    Ok(Meta {
        first,
        last,
        lo,
        hi,
        len,
    })
}
/// The chunk with the greatest `first_row <= row`.
fn meta_at(conn: &Connection, kind: Kind, row: u64) -> Result<Option<Meta>> {
    let sql = format!(
        "{} WHERE first_row<=? ORDER BY first_row DESC LIMIT 1",
        meta_sql(kind)
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query([sql_u64(row)?])?;
    rows.next()?.map(|r| read_meta(kind, r)).transpose()
}
/// The chunk after `last` (or the first chunk), which must start exactly there.
fn next_meta(conn: &Connection, kind: Kind, last: Option<u64>) -> Result<Option<Meta>> {
    let after = match last {
        Some(last) => Some(last.checked_add(1).ok_or_else(|| bad("row overflow"))?),
        None => None,
    };
    let sql = format!(
        "{} WHERE first_row>=? ORDER BY first_row LIMIT 1",
        meta_sql(kind)
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query([sql_u64(after.unwrap_or(0))?])?;
    let found = rows.next()?.map(|r| read_meta(kind, r)).transpose()?;
    match found {
        Some(m) if m.first != after.unwrap_or(0) => Err(bad(&format!(
            "{} intervals overlap or leave a gap",
            kind.table()
        ))),
        other => Ok(other),
    }
}
/// A chunk is usable only when contiguous and ordered with both neighbours.
fn check_adjacent(conn: &Connection, kind: Kind, m: Meta) -> Result<()> {
    let t = kind.table();
    let (lo, hi) = kind.keys();
    let gap = || bad(&format!("{t} intervals overlap or leave a gap"));
    let order = || bad(&format!("{t} chunks are not ordered by {lo}"));
    let prev = conn
        .prepare_cached(&format!(
            "SELECT last_row,{hi} FROM {t} WHERE first_row<? ORDER BY first_row DESC LIMIT 1"
        ))?
        .query_row([sql_u64(m.first)?], |r| {
            Ok((r.get::<_, i64>(0), r.get::<_, i64>(1)))
        })
        .optional()?;
    match prev {
        None if m.first != 0 => return Err(bad(&format!("{t} does not start at row 0"))),
        Some((last, prev_hi)) => {
            let last = nonneg(last, &format!("{t}.last_row out of range"))?;
            if last.checked_add(1) != Some(m.first) {
                return Err(gap());
            }
            if nonneg(prev_hi, &format!("{t}.{hi} out of range"))? > m.lo {
                return Err(order());
            }
        }
        None => {}
    }
    let next = conn
        .prepare_cached(&format!(
            "SELECT first_row,{lo} FROM {t} WHERE first_row>? ORDER BY first_row LIMIT 1"
        ))?
        .query_row([sql_u64(m.first)?], |r| {
            Ok((r.get::<_, i64>(0), r.get::<_, i64>(1)))
        })
        .optional()?;
    if let Some((first, next_lo)) = next {
        if m.last.checked_add(1) != Some(nonneg(first, &format!("{t}.first_row out of range"))?) {
            return Err(gap());
        }
        if nonneg(next_lo, &format!("{t}.{lo} out of range"))? < m.hi {
            return Err(order());
        }
    }
    Ok(())
}
fn side_data(conn: &Connection, kind: Kind, m: Meta) -> Result<Vec<u8>> {
    let t = kind.table();
    if m.len > MAX_CHUNK_BLOB {
        return Err(bad(&format!("{t}.data exceeds codec input ceiling")));
    }
    let data = conn
        .prepare_cached(&format!("SELECT data FROM {t} WHERE first_row=?"))?
        .query_row([sql_u64(m.first)?], |r| Ok(r.get::<_, Vec<u8>>(0)))
        .optional()?
        .ok_or_else(missing)?;
    data.map_err(|_| bad(&format!("{t}.data is not a blob")))
}
fn decode_files_chunk(conn: &Connection, m: Meta) -> Result<Vec<ChunkFile>> {
    check_adjacent(conn, Kind::Files, m)?;
    let rows = chunk::decode_files(&side_data(conn, Kind::Files, m)?)?;
    let agrees = u64::try_from(rows.len()).is_ok_and(|n| n == m.last - m.first + 1)
        && rows.first().map(|r| u64::from(r.dir_id)) == Some(m.lo)
        && rows.last().map(|r| u64::from(r.dir_id)) == Some(m.hi);
    if !agrees {
        return Err(bad("file_chunks metadata disagrees with chunk data"));
    }
    Ok(rows)
}
fn decode_links_chunk(conn: &Connection, m: Meta) -> Result<Vec<ChunkLinkInstance>> {
    check_adjacent(conn, Kind::Links, m)?;
    let rows = chunk::decode_link_instances(&side_data(conn, Kind::Links, m)?)?;
    let agrees = u64::try_from(rows.len()).is_ok_and(|n| n == m.last - m.first + 1)
        && rows.first().map(|r| r.ino) == Some(m.lo)
        && rows.last().map(|r| r.ino) == Some(m.hi);
    if !agrees {
        return Err(bad("li_chunks metadata disagrees with chunk data"));
    }
    Ok(rows)
}
fn link_key(l: &ChunkLinkInstance) -> (u64, u32, bool, &[u8]) {
    (
        l.ino,
        l.dir_id,
        l.name.is_some(),
        l.name.as_deref().unwrap_or_default(),
    )
}
/// Adjacent link instances must be in `(ino, dir_id, name)` order (NULL first).
fn check_link_order(before: &ChunkLinkInstance, after: &ChunkLinkInstance) -> Result<()> {
    if link_key(before) > link_key(after) {
        return Err(bad(
            "li_chunks rows are not ordered across a chunk boundary",
        ));
    }
    Ok(())
}
fn file_size(f: &ChunkFile) -> usize {
    f.name.as_ref().map_or(0, Vec::capacity)
}
fn link_size(l: &ChunkLinkInstance) -> usize {
    l.name.as_ref().map_or(0, Vec::capacity)
}

struct Slot<T> {
    first: u64,
    last: u64,
    rows: Arc<Vec<T>>,
    bytes: usize,
}
struct SideCache<T>(VecDeque<Slot<T>>);
impl<T> SideCache<T> {
    fn new() -> Self {
        Self(VecDeque::new())
    }
    fn find(&mut self, row: u64) -> Option<(u64, Arc<Vec<T>>)> {
        let i = self
            .0
            .iter()
            .position(|s| s.first <= row && row <= s.last)?;
        let slot = self.0.remove(i)?;
        let hit = (slot.first, slot.rows.clone());
        self.0.push_front(slot);
        Some(hit)
    }
    fn insert(&mut self, slot: Slot<T>) {
        if slot.bytes > SIDE_CACHE_BYTES {
            return;
        }
        while self.0.len() >= SIDE_CACHE_CHUNKS
            || self.0.iter().map(|s| s.bytes).sum::<usize>() + slot.bytes > SIDE_CACHE_BYTES
        {
            self.0.pop_back();
        }
        self.0.push_front(slot);
    }
}
fn side_load<T>(
    snap: &Snapshot,
    m: Meta,
    cache: &RefCell<SideCache<T>>,
    decode: fn(&Connection, Meta) -> Result<Vec<T>>,
    size: fn(&T) -> usize,
) -> Result<(u64, Arc<Vec<T>>)> {
    let cached = cache.borrow_mut().find(m.first);
    if let Some(hit) = cached {
        return Ok(hit);
    }
    let rows = Arc::new(decode(&snap.conn, m)?);
    let bytes = rows.capacity() * std::mem::size_of::<T>() + rows.iter().map(size).sum::<usize>();
    cache.borrow_mut().insert(Slot {
        first: m.first,
        last: m.last,
        rows: rows.clone(),
        bytes,
    });
    Ok((m.first, rows))
}
fn side_chunk<T>(
    snap: &Snapshot,
    kind: Kind,
    row: u64,
    cache: &RefCell<SideCache<T>>,
    decode: fn(&Connection, Meta) -> Result<Vec<T>>,
    size: fn(&T) -> usize,
) -> Result<(u64, Arc<Vec<T>>)> {
    let cached = cache.borrow_mut().find(row);
    if let Some(hit) = cached {
        return Ok(hit);
    }
    let m = meta_at(&snap.conn, kind, row)?
        .filter(|m| m.last >= row)
        .ok_or_else(|| bad(&format!("{} does not cover row {row}", kind.table())))?;
    side_load(snap, m, cache, decode, size)
}
#[derive(Default)]
struct PathCache {
    map: HashMap<u32, Arc<RawPath>>,
    bytes: usize,
}
fn valid_name(name: &[u8]) -> bool {
    !name.is_empty() && !name.contains(&b'/') && !name.contains(&0) && name != b"." && name != b".."
}

/// Bounds of a notable-file query, already converted to SQL integer ranges.
pub(super) struct FileBounds {
    pub(super) min: Option<u64>,
    pub(super) old: Option<i64>,
    pub(super) new: Option<i64>,
}
struct FileEntry {
    rev: u64,
    dir: Arc<RawPath>,
    id: u32,
    file: ChunkFile,
}
impl FileEntry {
    fn rank(&self) -> (u64, &[u8], &Option<Vec<u8>>, u32) {
        (self.rev, self.dir.as_bytes(), &self.file.name, self.id)
    }
}
impl PartialEq for FileEntry {
    fn eq(&self, other: &Self) -> bool {
        self.rank() == other.rank()
    }
}
impl Eq for FileEntry {}
impl PartialOrd for FileEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FileEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}
struct SetEntry {
    rev: u64,
    path: Arc<RawPath>,
    row: LinkSetRow,
    count: u64,
}
impl SetEntry {
    fn rank(&self) -> (u64, &[u8], u64) {
        (self.rev, self.path.as_bytes(), self.row.ino)
    }
}
impl PartialEq for SetEntry {
    fn eq(&self, other: &Self) -> bool {
        self.rank() == other.rank()
    }
}
impl Eq for SetEntry {}
impl PartialOrd for SetEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SetEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}
struct SharedEntry {
    rev: u64,
    path: RawPath,
    id: u32,
    total: u64,
}
impl SharedEntry {
    fn rank(&self) -> (u64, &[u8], u32) {
        (self.rev, self.path.as_bytes(), self.id)
    }
}
impl PartialEq for SharedEntry {
    fn eq(&self, other: &Self) -> bool {
        self.rank() == other.rank()
    }
}
impl Eq for SharedEntry {}
impl PartialOrd for SharedEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SharedEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}
fn instance_row_of(ino: u64, dir_id: u32, name: Option<&[u8]>) -> Result<LinkInstanceRow> {
    Ok(LinkInstanceRow {
        ino,
        dir_id,
        name: name
            .map(|n| RawName::new(n.to_vec()))
            .transpose()
            .map_err(|_| invalid())?,
    })
}

impl Backend {
    /// Cached, shared reconstruction of one directory's path.
    fn cached_path(&self, snap: &Snapshot, id: u32) -> Result<Arc<RawPath>> {
        let hit = self.paths.borrow().map.get(&id).cloned();
        if let Some(path) = hit {
            return Ok(path);
        }
        let row = self.raw(snap, id).map_err(required)?;
        let path = Arc::new(self.path(snap, &row)?);
        let size = path.as_bytes().len();
        if size <= PATH_CACHE_BYTES {
            let mut cache = self.paths.borrow_mut();
            if cache.bytes + size > PATH_CACHE_BYTES || cache.map.len() >= 65536 {
                cache.map.clear();
                cache.bytes = 0;
            }
            cache.bytes += size;
            cache.map.insert(id, path.clone());
        }
        Ok(path)
    }
    /// Directory metadata without path reconstruction; `None` past the last id.
    pub(super) fn dir_lite(&self, snap: &Snapshot, id: u32) -> Result<Option<DirRow>> {
        // The chunk cache answers without SQL; only a miss consults `dir_chunks`.
        let r = match self.raw(snap, id) {
            Ok(r) => r,
            Err(StoreError::Sql(rusqlite::Error::QueryReturnedNoRows)) => return Ok(None),
            Err(other) => return Err(other),
        };
        let path = RawPath::new(b"/".to_vec()).map_err(|_| invalid())?;
        Ok(Some(self.materialize_with(r, path)))
    }
    fn materialize_with(&self, r: ChunkDir, path: RawPath) -> DirRow {
        DirRow {
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
            path_hash: 0,
            path,
        }
    }
    pub(super) fn parent_of(&self, snap: &Snapshot, id: u32) -> Result<Option<u32>> {
        Ok(self.raw(snap, id)?.parent_id)
    }
    /// Sum of `own_apparent` over `lo..=hi`.
    pub(super) fn own_apparent_sum(&self, snap: &Snapshot, lo: u32, hi: u32) -> Result<u64> {
        let mut total = 0u64;
        self.range(snap, lo, hi, |r| {
            total = total
                .checked_add(r.own_apparent)
                .ok_or_else(|| StoreError::Integer("descendant apparent total".into()))?;
            Ok(())
        })?;
        Ok(total)
    }
    /// Visit every directory of `lo..=hi` in preorder with its path bytes.
    pub(super) fn visit_paths(
        &self,
        snap: &Snapshot,
        lo: u32,
        hi: u32,
        visit: &mut dyn FnMut(super::DirPath<'_>) -> Result<()>,
    ) -> Result<()> {
        let first = self.raw(snap, lo)?;
        let mut buf = self.path(snap, &first)?.as_bytes().to_vec();
        let mut stack = vec![(first.depth, buf.len())];
        visit(super::DirPath {
            id: first.id,
            subtree_end: first.subtree_end,
            depth: first.depth,
            path: &buf,
        })?;
        let Some(next) = lo.checked_add(1).filter(|n| *n <= hi) else {
            return Ok(());
        };
        self.range(snap, next, hi, |row| {
            while stack.last().is_some_and(|&(d, _)| d >= row.depth) {
                stack.pop();
            }
            let &(depth, len) = stack.last().ok_or_else(invalid)?;
            if depth.checked_add(1) != Some(row.depth) || !valid_name(&row.name) {
                return Err(invalid());
            }
            buf.truncate(len);
            if buf.len() > 1 {
                buf.push(b'/');
            }
            buf.extend_from_slice(&row.name);
            if buf.len() > MAX_PATH_BYTES {
                return Err(invalid());
            }
            stack.push((row.depth, buf.len()));
            visit(super::DirPath {
                id: row.id,
                subtree_end: row.subtree_end,
                depth: row.depth,
                path: &buf,
            })
        })
    }

    // ---- notable files ----
    fn file_rows_at(&self, snap: &Snapshot, row: u64) -> Result<(u64, Arc<Vec<ChunkFile>>)> {
        side_chunk(
            snap,
            Kind::Files,
            row,
            &self.files,
            decode_files_chunk,
            file_size,
        )
    }
    /// Stream the notable files of one directory, in stored (name) order.
    pub(super) fn dir_files(
        &self,
        snap: &Snapshot,
        id: u32,
        visit: &mut dyn FnMut(&ChunkFile) -> Result<()>,
    ) -> Result<()> {
        let d = self.raw(snap, id)?;
        let mut row = u64::from(d.file_lo);
        let end = row + u64::from(d.file_count);
        while row < end {
            let (first, rows) = self.file_rows_at(snap, row)?;
            let start = usize::try_from(row - first).map_err(|_| invalid())?;
            let take = usize::try_from(end - row)
                .map_err(|_| invalid())?
                .min(rows.len().saturating_sub(start));
            if take == 0 {
                return Err(invalid());
            }
            for f in &rows[start..start + take] {
                if f.dir_id != id {
                    return Err(invalid());
                }
                visit(f)?;
            }
            row += take as u64;
        }
        Ok(())
    }
    pub(super) fn subtree_files(
        &self,
        snap: &Snapshot,
        id: u32,
        end: u32,
        bounds: &FileBounds,
        limit: usize,
    ) -> Result<Vec<FileHit>> {
        sql_limit(limit)?;
        let cap = limit.min(crate::error::MAX_QUERY_ROWS + 1);
        let mut heap = BinaryHeap::<FileEntry>::new();
        if cap > 0 {
            let sql = format!(
                "{} WHERE last_dir>=?1 AND first_dir<=?2 ORDER BY first_row",
                meta_sql(Kind::Files)
            );
            // The directories claim the contiguous row span `lo_row..hi_row`
            // (`file_lo` never decreases; an empty directory sits at the running
            // offset). The chunks read must cover all of it.
            let (lo_row, hi_row) = {
                let (a, b) = (self.raw(snap, id)?, self.raw(snap, end)?);
                let hi = u64::from(b.file_lo) + u64::from(b.file_count);
                (u64::from(a.file_lo), hi)
            };
            if hi_row < lo_row {
                return Err(bad("directory file spans are not ordered"));
            }
            let mut stmt = snap.conn.prepare(&sql)?;
            let mut metas = stmt.query(params![id, end])?;
            let mut expected: Option<u64> = None;
            while let Some(r) = metas.next()? {
                let m = read_meta(Kind::Files, r)?;
                match expected {
                    None if m.first > lo_row && lo_row < hi_row => {
                        return Err(bad("file_chunks do not cover the directories' file span"))
                    }
                    Some(next) if m.first != next => {
                        return Err(bad("file_chunks intervals overlap or leave a gap"))
                    }
                    _ => {}
                }
                expected = Some(m.last + 1);
                let (first, rows) = side_load(snap, m, &self.files, decode_files_chunk, file_size)?;
                let start = rows.partition_point(|f| f.dir_id < id);
                for (offset, f) in rows[start..].iter().enumerate() {
                    if f.dir_id > end {
                        break;
                    }
                    let row_no = first + (start + offset) as u64;
                    if row_no < lo_row || row_no >= hi_row {
                        return Err(bad("file row lies outside its directory's file span"));
                    }
                    if bounds.min.is_some_and(|min| f.disk < min)
                        || bounds.old.is_some_and(|old| f.mtime > old)
                        || bounds.new.is_some_and(|new| f.mtime < new)
                    {
                        continue;
                    }
                    let rev = u64::MAX - f.disk;
                    if heap.len() >= cap && heap.peek().is_some_and(|w| rev > w.rev) {
                        continue;
                    }
                    let id = u32::try_from(row_no).map_err(|_| invalid())?;
                    let entry = FileEntry {
                        rev,
                        dir: self.cached_path(snap, f.dir_id)?,
                        id,
                        file: f.clone(),
                    };
                    if heap.len() < cap {
                        heap.push(entry);
                    } else if heap.peek().is_some_and(|w| entry < *w) {
                        heap.pop();
                        heap.push(entry);
                    }
                }
            }
            if lo_row < hi_row && expected.is_none_or(|next| next < hi_row) {
                return Err(bad("file_chunks do not cover the directories' file span"));
            }
        }
        crate::error::check_result_bound(heap.as_slice())?;
        heap.into_sorted_vec()
            .into_iter()
            .map(|e| {
                let name = e
                    .file
                    .name
                    .map(RawName::new)
                    .transpose()
                    .map_err(|_| invalid())?;
                let path = name.as_ref().map(|n| e.dir.join(n));
                Ok(FileHit {
                    row: FileRow {
                        id: e.id,
                        dir_id: e.file.dir_id,
                        name,
                        ino: e.file.ino,
                        disk: e.file.disk,
                        apparent: e.file.apparent,
                        mtime: e.file.mtime,
                        nlink: e.file.nlink,
                    },
                    path,
                })
            })
            .collect()
    }

    // ---- link instances ----
    /// The first chunk whose last inode is at least `ino`, by binary search on
    /// row numbers (chunk metadata only, no payload reads).
    fn li_lower_bound(&self, snap: &Snapshot, ino: u64) -> Result<Option<Meta>> {
        let sql = format!("{} ORDER BY first_row DESC LIMIT 1", meta_sql(Kind::Links));
        let tail = {
            let mut stmt = snap.conn.prepare_cached(&sql)?;
            let mut rows = stmt.query([])?;
            rows.next()?
                .map(|r| read_meta(Kind::Links, r))
                .transpose()?
        };
        let Some(tail) = tail else { return Ok(None) };
        if tail.hi < ino {
            return Ok(None);
        }
        let (mut lo, mut hi) = (0u64, tail.last);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let m = meta_at(&snap.conn, Kind::Links, mid)?
                .filter(|m| m.last >= mid)
                .ok_or_else(|| bad("li_chunks intervals overlap or leave a gap"))?;
            if m.hi >= ino {
                hi = m.first;
            } else {
                lo = m.last + 1;
            }
        }
        // `lo <= tail.last`: a row no chunk covers is a gap, not an empty result.
        meta_at(&snap.conn, Kind::Links, lo)?
            .filter(|m| m.last >= lo)
            .map(Some)
            .ok_or_else(|| bad("li_chunks intervals overlap or leave a gap"))
    }
    /// Visit `(row number, instance)` of one inode in stored order. The visitor
    /// returns `false` to stop. The full `(ino, dir_id, name)` order is checked
    /// across every chunk boundary of the inode's run, also after the visitor has
    /// stopped (the remaining chunks are decoded but not visited).
    fn li_visit(
        &self,
        snap: &Snapshot,
        ino: u64,
        mut visit: impl FnMut(u64, &ChunkLinkInstance) -> Result<bool>,
    ) -> Result<()> {
        let Some(mut m) = self.li_lower_bound(snap, ino)? else {
            return Ok(());
        };
        // The binary search trusts chunk metadata; confirm that the chunk before
        // the one found really ends below `ino` (this decodes and validates it).
        if let Some(before) = m.first.checked_sub(1) {
            let p = meta_at(&snap.conn, Kind::Links, before)?
                .filter(|p| p.last == before)
                .ok_or_else(|| bad("li_chunks intervals overlap or leave a gap"))?;
            let (_, rows) = side_load(snap, p, &self.links, decode_links_chunk, link_size)?;
            if rows.last().is_none_or(|r| r.ino >= ino) {
                return Err(bad("li_chunks are not ordered by first_ino"));
            }
        }
        let mut prev: Option<ChunkLinkInstance> = None;
        let mut stopped = false;
        loop {
            if m.lo > ino {
                return Ok(());
            }
            let (first, rows) = side_load(snap, m, &self.links, decode_links_chunk, link_size)?;
            if let (Some(before), Some(head)) = (&prev, rows.first()) {
                check_link_order(before, head)?;
            }
            let start = rows.partition_point(|r| r.ino < ino);
            for (offset, r) in rows[start..].iter().enumerate() {
                if r.ino != ino {
                    return Ok(());
                }
                if !stopped && !visit(first + (start + offset) as u64, r)? {
                    // Keep walking the inode's chunk range without visiting, so
                    // every boundary of the run is order-checked before the
                    // early stop is trusted (bounded by the inode's instances).
                    stopped = true;
                }
            }
            if m.hi > ino {
                return Ok(());
            }
            prev = rows.last().cloned();
            match next_meta(&snap.conn, Kind::Links, Some(m.last))? {
                Some(next) => m = next,
                None => return Ok(()),
            }
        }
    }
    fn instance_count(&self, snap: &Snapshot, ino: u64) -> Result<u64> {
        let mut n = 0u64;
        self.li_visit(snap, ino, |_, _| {
            n += 1;
            Ok(true)
        })?;
        Ok(n)
    }
    pub(super) fn link_instances(&self, snap: &Snapshot, ino: u64) -> Result<Vec<LinkInstanceRow>> {
        sql_u64(ino)?;
        let mut out = Vec::new();
        self.li_visit(snap, ino, |_, r| {
            out.push(instance_row_of(r.ino, r.dir_id, r.name.as_deref())?);
            Ok(out.len() <= crate::error::MAX_QUERY_ROWS)
        })?;
        crate::error::check_result_bound(&out)?;
        Ok(out)
    }
    pub(super) fn link_instances_page(
        &self,
        snap: &Snapshot,
        ino: u64,
        cursor: Option<&(i64, LinkInstanceRow)>,
        reverse: bool,
        limit: usize,
    ) -> Result<Vec<(i64, LinkInstanceRow)>> {
        if limit > crate::error::MAX_QUERY_ROWS {
            return Err(StoreError::Invalid("link page exceeds query cap".into()));
        }
        sql_u64(ino)?;
        sql_limit(limit)?;
        let after = cursor.map(|c| {
            (
                c.1.dir_id,
                c.1.name.is_some(),
                c.1.name.as_ref().map_or(&[][..], |n| n.as_bytes()),
                c.0,
            )
        });
        let mut forward = Vec::new();
        let mut tail = VecDeque::new();
        if limit > 0 {
            self.li_visit(snap, ino, |row, r| {
                // v1's rowid is one-based; stored order here is the sorted order.
                let rowid = i64::try_from(row)
                    .ok()
                    .and_then(|n| n.checked_add(1))
                    .ok_or_else(invalid)?;
                let key = (
                    r.dir_id,
                    r.name.is_some(),
                    r.name.as_deref().unwrap_or_default(),
                    rowid,
                );
                if let Some(after) = after {
                    let ord = key.cmp(&after);
                    if reverse && ord != Ordering::Less {
                        return Ok(false);
                    }
                    if !reverse && ord != Ordering::Greater {
                        return Ok(true);
                    }
                }
                let item = (rowid, instance_row_of(r.ino, r.dir_id, r.name.as_deref())?);
                if reverse {
                    tail.push_back(item);
                    if tail.len() > limit {
                        tail.pop_front();
                    }
                    Ok(true)
                } else {
                    forward.push(item);
                    Ok(forward.len() < limit)
                }
            })?;
        }
        Ok(if reverse {
            tail.into_iter().rev().collect()
        } else {
            forward
        })
    }

    // ---- link sets ----
    /// Link sets charged in `id..=end`, ranked by disk, LCA path bytes and inode.
    /// `region` maps an instance directory to its region (`None` = outside)
    /// when the query only wants sets spanning two or more regions.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn link_sets_under(
        &self,
        snap: &Snapshot,
        id: u32,
        end: u32,
        incomplete: bool,
        limit: usize,
        region: Option<&mut dyn FnMut(u32) -> Result<Option<u32>>>,
    ) -> Result<Vec<LinkSetHit>> {
        let cap = limit.min(crate::error::MAX_QUERY_ROWS + 1);
        let mut heap = BinaryHeap::<SetEntry>::new();
        let mut region = region;
        if cap > 0 {
            let mut stmt = snap.conn.prepare(
                "SELECT ino,nlink,found,complete,disk,apparent,mtime,lca_dir_id FROM link_sets WHERE lca_dir_id BETWEEN ?1 AND ?2 AND (?3=0 OR complete=0) ORDER BY lca_dir_id,ino",
            )?;
            let mut sets = stmt.query(params![id, end, incomplete])?;
            while let Some(r) = sets.next()? {
                let row = link_row(r).map_err(super::plain_row_error)?;
                let rev = u64::MAX - row.disk;
                if heap.len() >= cap && heap.peek().is_some_and(|w| rev > w.rev) {
                    continue;
                }
                let path = self.cached_path(snap, row.lca_dir_id)?;
                if heap.len() >= cap {
                    let better = heap.peek().is_some_and(|w| {
                        (rev, path.as_bytes(), row.ino) < (w.rev, w.path.as_bytes(), w.row.ino)
                    });
                    if !better {
                        continue;
                    }
                }
                if let Some(region) = region.as_deref_mut() {
                    let mut first: Option<Option<u32>> = None;
                    let mut crossing = false;
                    self.li_visit(snap, row.ino, |_, i| {
                        let here = region(i.dir_id)?;
                        match first {
                            None => first = Some(here),
                            Some(previous) if previous != here => {
                                crossing = true;
                                return Ok(false);
                            }
                            _ => {}
                        }
                        Ok(true)
                    })?;
                    if !crossing {
                        continue;
                    }
                }
                let count = self.instance_count(snap, row.ino)?;
                if heap.len() >= cap {
                    heap.pop();
                }
                heap.push(SetEntry {
                    rev,
                    path,
                    row,
                    count,
                });
            }
        }
        crate::error::check_result_bound(heap.as_slice())?;
        Ok(heap
            .into_sorted_vec()
            .into_iter()
            .map(|e| LinkSetHit {
                row: e.row,
                lca_path: (*e.path).clone(),
                instance_count: e.count,
            })
            .collect())
    }
    /// The ancestor of `dir_id` at `depth` inside `id..=end`, or `None`.
    pub(super) fn region_at_depth(
        &self,
        snap: &Snapshot,
        dir_id: u32,
        depth: u64,
        id: u32,
        end: u32,
    ) -> Result<Option<u32>> {
        let mut row = match self.raw(snap, dir_id) {
            Ok(row) => row,
            Err(StoreError::Sql(rusqlite::Error::QueryReturnedNoRows)) => return Ok(None),
            Err(other) => return Err(other),
        };
        if u64::from(row.depth) < depth {
            return Ok(None);
        }
        while u64::from(row.depth) > depth {
            let parent = row.parent_id.ok_or_else(invalid)?;
            if parent >= row.id {
                return Err(invalid());
            }
            row = self.raw(snap, parent).map_err(required)?;
        }
        Ok((row.id >= id && row.id <= end).then_some(row.id))
    }
    /// Stream link sets in inode order with their stored instance directories.
    pub(super) fn stream_sets(
        &self,
        snap: &Snapshot,
        start: u32,
        end: u32,
        cap: usize,
        visit: &mut dyn FnMut(&LinkSetHit, &[u32]) -> Result<()>,
    ) -> Result<()> {
        let mut stmt = snap.conn.prepare(
            "SELECT ino,nlink,found,complete,disk,apparent,mtime,lca_dir_id FROM link_sets NOT INDEXED WHERE lca_dir_id BETWEEN ?1 AND ?2 ORDER BY ino",
        )?;
        let mut sets = stmt.query(params![start, end])?;
        // Forward-only cursor over the inode-ordered instance chunks.
        let mut chunk: Vec<ChunkLinkInstance> = Vec::new();
        let mut pos = 0usize;
        let mut last: Option<u64> = None;
        let mut done = false;
        let mut dirs = Vec::new();
        while let Some(r) = sets.next()? {
            let row = link_row(r).map_err(super::plain_row_error)?;
            dirs.clear();
            loop {
                while pos < chunk.len() && chunk[pos].ino < row.ino {
                    pos += 1;
                }
                while pos < chunk.len() && chunk[pos].ino == row.ino {
                    if dirs.len() == cap {
                        return Err(StoreError::Invalid("per-set instance cap reached".into()));
                    }
                    dirs.push(chunk[pos].dir_id);
                    pos += 1;
                }
                if pos < chunk.len() || done {
                    break;
                }
                // Skip whole chunks below the wanted inode without decoding them.
                loop {
                    match next_meta(&snap.conn, Kind::Links, last)? {
                        None => {
                            done = true;
                            break;
                        }
                        Some(m) => {
                            last = Some(m.last);
                            if m.hi < row.ino {
                                continue;
                            }
                            chunk = decode_links_chunk(&snap.conn, m)?;
                            pos = 0;
                            break;
                        }
                    }
                }
                if done {
                    break;
                }
            }
            let hit = LinkSetHit {
                lca_path: (*self.cached_path(snap, row.lca_dir_id)?).clone(),
                row,
                instance_count: dirs.len() as u64,
            };
            visit(&hit, &dirs)?;
        }
        Ok(())
    }
    /// Directories with `t_disk > t_reclaimable`, most shared bytes first.
    pub(super) fn most_shared(
        &self,
        snap: &Snapshot,
        lo: u32,
        hi: u32,
        limit: usize,
    ) -> Result<Vec<(RawPath, u64, u64)>> {
        sql_limit(limit)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut heap = BinaryHeap::<SharedEntry>::new();
        self.projected_chunks(snap, lo, hi, |first, p| {
            for offset in 0..p.entries.len() {
                let id = first
                    .checked_add(u32::try_from(offset).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?;
                if id < lo || id > hi || p.t_disk[offset] <= p.t_reclaimable[offset] {
                    continue;
                }
                let shared = p.t_disk[offset] - p.t_reclaimable[offset];
                let rev = u64::MAX - shared;
                if heap.len() >= limit && heap.peek().is_some_and(|w| rev > w.rev) {
                    continue;
                }
                let row = self.raw(snap, id)?;
                let entry = SharedEntry {
                    rev,
                    path: self.path(snap, &row)?,
                    id,
                    total: p.t_disk[offset],
                };
                if heap.len() < limit {
                    heap.push(entry);
                } else if heap.peek().is_some_and(|w| entry < *w) {
                    heap.pop();
                    heap.push(entry);
                }
            }
            Ok(())
        })?;
        Ok(heap
            .into_sorted_vec()
            .into_iter()
            .map(|e| (e.path, u64::MAX - e.rev, e.total))
            .collect())
    }
}

#[cfg(test)]
mod child_cursor_tests {
    use super::*;
    use crate::writer::SnapshotWriter;
    use crate::writer_v2::{SnapshotWriterV2, WriterOptions};
    use std::path::Path;

    fn row(
        id: u32,
        parent: Option<u32>,
        depth: u32,
        name: &[u8],
        path: &RawPath,
        end: u32,
    ) -> DirRow {
        let n = u64::from(id);
        DirRow {
            id,
            parent_id: parent,
            subtree_end: end,
            depth,
            name: name.to_vec(),
            path: path.clone(),
            path_hash: path.hash64(),
            ino: n + 1,
            mtime: 0,
            dir_disk: 1,
            dir_apparent: 2,
            projid: None,
            entries: 1,
            errors: 0,
            own_files: 0,
            own_symlinks: 0,
            own_charged: n + 5,
            own_apparent: 2,
            t_dirs: 1,
            t_files: n % 5,
            t_inodes: 1,
            t_disk: n + 4096,
            t_apparent: 2,
            t_charged: 3,
            t_reclaimable: n % 3,
            t_errors: 0,
            age0: 0,
            age1: 0,
            age2: 0,
            age3: 0,
            age4: 0,
            age5: 0,
            newest_mtime: 0,
            max_entries: 1,
        }
    }
    /// A root with `width` children `c00000..`; every fourth has two leaf children
    /// and the first of those a short chain, so subtrees differ in size.
    fn tree(width: usize) -> Vec<DirRow> {
        struct Node {
            name: Vec<u8>,
            kids: Vec<Node>,
        }
        let leaf = |n: &str| Node {
            name: n.as_bytes().to_vec(),
            kids: vec![],
        };
        let kids = (0..width)
            .map(|i| Node {
                name: format!("c{i:05}").into_bytes(),
                kids: if i % 4 == 0 {
                    let mut g0 = leaf("g0");
                    if i == 0 {
                        g0.kids = vec![Node {
                            name: b"x".to_vec(),
                            kids: vec![leaf("y")],
                        }];
                    }
                    vec![g0, leaf("g1")]
                } else {
                    vec![]
                },
            })
            .collect();
        let root = Node {
            name: b"/r".to_vec(),
            kids,
        };
        // Iterative preorder with an explicit stack: (node, parent, depth, path).
        let mut out: Vec<DirRow> = Vec::new();
        let rootp = RawPath::new(b"/r".to_vec()).unwrap();
        let mut stack = vec![(&root, None::<u32>, 0u32, rootp, 0usize)];
        let mut open: Vec<u32> = Vec::new();
        while let Some((node, parent, depth, path, _)) = stack.pop() {
            let id = out.len() as u32;
            out.push(row(id, parent, depth, &node.name, &path, id));
            open.push(id);
            for kid in node.kids.iter().rev() {
                let p = path.join(&RawName::new(kid.name.clone()).unwrap());
                stack.push((kid, Some(id), depth + 1, p, 0));
            }
        }
        // subtree_end: last id of each directory's descendants.
        for i in (0..out.len()).rev() {
            if let Some(parent) = out[i].parent_id {
                let end = out[i].subtree_end;
                let p = &mut out[parent as usize];
                p.subtree_end = p.subtree_end.max(end);
            }
        }
        out
    }
    struct Fixture {
        _dirs: (tempfile::TempDir, tempfile::TempDir),
        v1: Snapshot,
        v2: Snapshot,
    }
    fn fixture(rows: Vec<DirRow>, plain_depth: u32, chunk_dirs: usize) -> Fixture {
        let d1 = test_support::private_tempdir().unwrap();
        let d2 = test_support::private_tempdir().unwrap();
        let mut m = crate::test_meta();
        m.root = rows[0].path.clone();
        m.dirs = rows.len() as u64;
        m.schema_version = 1;
        let mut w = SnapshotWriter::create(d1.path(), &m).unwrap();
        w.put_dirs(&rows).unwrap();
        let f1 = w.finish(&m).unwrap();
        m.schema_version = 2;
        let mut w = SnapshotWriterV2::create_with_options(
            d2.path(),
            &m,
            WriterOptions {
                plain_depth,
                chunk_dirs,
                ..Default::default()
            },
        )
        .unwrap();
        w.put_dirs(&rows).unwrap();
        let f2 = w.finish(&m).unwrap();
        let open = |f: &Path| Snapshot::open(f).unwrap();
        Fixture {
            v1: open(&f1),
            v2: open(&f2),
            _dirs: (d1, d2),
        }
    }
    fn entry(r: &DirRow) -> ChildEntry {
        ChildEntry {
            id: r.id,
            name: r.name.clone(),
            depth: r.depth,
            subtree_end: r.subtree_end,
            t_disk: r.t_disk,
            t_reclaimable: r.t_reclaimable,
            t_files: r.t_files,
            own_charged: r.own_charged,
        }
    }
    fn pages(s: &Snapshot, parent: u32, page: usize) -> Vec<ChildEntry> {
        let (mut all, mut cursor) = (Vec::new(), None);
        loop {
            let before = all.len();
            let next = s
                .children_by_name(parent, cursor.as_ref(), page, &mut all)
                .unwrap();
            assert!(all.len() - before <= page.max(1));
            match next {
                Some(c) => {
                    assert!(all.len() > before, "an unfinished page makes progress");
                    cursor = Some(c);
                }
                None => return all,
            }
        }
    }
    fn check(f: &Fixture, parents: &[u32], page_sizes: &[usize]) {
        for &parent in parents {
            let expect: Vec<ChildEntry> =
                f.v1.children(parent, SortKey::Name, 1_000_000)
                    .unwrap()
                    .iter()
                    .map(entry)
                    .collect();
            let by_sql: Vec<ChildEntry> =
                f.v2.children(parent, SortKey::Name, 1_000_000)
                    .unwrap()
                    .iter()
                    .map(entry)
                    .collect();
            assert_eq!(expect, by_sql, "parent {parent}");
            for &page in page_sizes {
                assert_eq!(expect, pages(&f.v1, parent, page), "v1 {parent} {page}");
                assert_eq!(expect, pages(&f.v2, parent, page), "v2 {parent} {page}");
            }
        }
    }

    #[test]
    fn pages_equal_children_by_name_across_shapes() {
        let rows = tree(41);
        let parents: Vec<u32> = rows
            .iter()
            .filter(|r| r.subtree_end > r.id)
            .map(|r| r.id)
            .chain([u32::MAX - 1, 7777])
            .collect();
        for plain_depth in [0, 2, 4] {
            for chunk_dirs in [1, 3, 5, 4096] {
                let f = fixture(rows.clone(), plain_depth, chunk_dirs);
                check(&f, &parents, &[1, 2, 7, 1000]);
                // A leaf has no children; limit 0 behaves like 1.
                let leaf = rows.iter().find(|r| r.subtree_end == r.id).unwrap().id;
                assert!(pages(&f.v2, leaf, 3).is_empty());
                let mut out = Vec::new();
                assert!(f
                    .v2
                    .children_by_name(0, None, 0, &mut out)
                    .unwrap()
                    .is_some());
                assert_eq!(out.len(), 1);
            }
        }
    }

    #[test]
    fn five_thousand_children_below_plain_depth() {
        // Root -> mid (depth 1) -> 5000 children; plain_depth 1 puts mid's children
        // in the chunks, plain_depth 2 in the shallow table.
        let mut rows = vec![];
        let root = RawPath::new(b"/r".to_vec()).unwrap();
        let mid = root.join(&RawName::new(b"mid".to_vec()).unwrap());
        rows.push(row(0, None, 0, b"/r", &root, 5001));
        rows.push(row(1, Some(0), 1, b"mid", &mid, 5001));
        for i in 0..5000u32 {
            let name = format!("k{i:05}").into_bytes();
            let p = mid.join(&RawName::new(name.clone()).unwrap());
            rows.push(row(2 + i, Some(1), 2, &name, &p, 2 + i));
        }
        for plain_depth in [1, 2] {
            let f = fixture(rows.clone(), plain_depth, 64);
            check(&f, &[1], &[7, 250, 100_000]);
        }
    }

    #[test]
    fn paging_a_wide_directory_decodes_each_chunk_once() {
        let width = 5000u32;
        let mut rows = vec![];
        let root = RawPath::new(b"/r".to_vec()).unwrap();
        rows.push(row(0, None, 0, b"/r", &root, width));
        for i in 0..width {
            let name = format!("k{i:05}").into_bytes();
            let p = root.join(&RawName::new(name.clone()).unwrap());
            rows.push(row(1 + i, Some(0), 1, &name, &p, 1 + i));
        }
        let chunk_dirs = 50;
        let f = fixture(rows, 0, chunk_dirs);
        let chunks = (width as u64 + 1).div_ceil(chunk_dirs as u64);
        for page in [1usize, 7, 64] {
            let backend_decodes = |b: &Backend| b.decodes.get();
            // A cold cache each time.
            let fresh = Snapshot::open(&f.v2.file).unwrap();
            let all = pages(&fresh, 0, page);
            assert_eq!(all.len(), width as usize);
            let decoded = backend_decodes(fresh.v2.as_ref().unwrap());
            // Work is O(W): every chunk decodes about once, not once per page.
            assert!(
                decoded <= chunks + 2,
                "page {page}: {decoded} decodes for {chunks} chunks"
            );
        }
    }

    #[test]
    fn corrupt_sibling_structure_is_invalid() {
        let rows = tree(12);
        let f = fixture(rows, 0, 4);
        let file = f.v2.file.clone();
        let tamper = |sql: &str| {
            let dir = test_support::private_tempdir().unwrap();
            let copy = dir.path().join("t.db");
            std::fs::copy(&file, &copy).unwrap();
            let conn = rusqlite::Connection::open(&copy).unwrap();
            conn.execute_batch(sql).unwrap();
            drop(conn);
            (dir, Snapshot::open(&copy).unwrap())
        };
        // A foreign cursor and an out-of-range cursor are rejected.
        let mut out = Vec::new();
        let foreign = ChildCursor(CursorAt::After(b"x".to_vec()));
        assert!(matches!(
            f.v2.children_by_name(0, Some(&foreign), 3, &mut out),
            Err(StoreError::Invalid(_))
        ));
        let outside = ChildCursor(CursorAt::Id(10_000, b"x".to_vec()));
        assert!(matches!(
            f.v2.children_by_name(0, Some(&outside), 3, &mut out),
            Err(StoreError::Invalid(_))
        ));
        let (_keep, snap) = tamper("DELETE FROM dir_chunks WHERE first_id=4;");
        let mut out = Vec::new();
        assert!(matches!(
            snap.children_by_name(0, None, 100, &mut out),
            Err(StoreError::Invalid(_))
        ));
    }

    fn v2_only(
        rows: &[DirRow],
        plain_depth: u32,
        chunk_dirs: usize,
    ) -> (tempfile::TempDir, Snapshot) {
        let d = test_support::private_tempdir().unwrap();
        let mut m = crate::test_meta();
        m.root = rows[0].path.clone();
        m.dirs = rows.len() as u64;
        m.schema_version = 2;
        let mut w = SnapshotWriterV2::create_with_options(
            d.path(),
            &m,
            WriterOptions {
                plain_depth,
                chunk_dirs,
                ..Default::default()
            },
        )
        .unwrap();
        w.put_dirs(rows).unwrap();
        let f = w.finish(&m).unwrap();
        (d, Snapshot::open(&f).unwrap())
    }
    fn wide(width: u32) -> Vec<DirRow> {
        let root = RawPath::new(b"/r".to_vec()).unwrap();
        let mut rows = vec![row(0, None, 0, b"/r", &root, width)];
        for i in 0..width {
            let name = format!("b{i:06}").into_bytes();
            let p = root.join(&RawName::new(name.clone()).unwrap());
            rows.push(row(1 + i, Some(0), 1, &name, &p, 1 + i));
        }
        rows
    }
    fn drain(s: &Snapshot, parent: u32, page: usize) -> Result<usize> {
        let (mut out, mut cursor, mut n) = (Vec::new(), None, 0);
        loop {
            out.clear();
            let next = s.children_by_name(parent, cursor.as_ref(), page, &mut out)?;
            n += out.len();
            match next {
                Some(c) => cursor = Some(c),
                None => return Ok(n),
            }
        }
    }
    fn is_invalid<T>(r: Result<T>) -> bool {
        matches!(r, Err(StoreError::Invalid(_)))
    }

    #[test]
    fn b3_names_must_ascend_across_page_boundaries() {
        // Names of ids 256 and 257 swapped: sorted within each 256-row page, not across.
        let mut rows = wide(257);
        let (a, b) = (rows[256].name.clone(), rows[257].name.clone());
        rows[256].name = b;
        rows[257].name = a;
        let (_d, s) = v2_only(&rows, 0, 64);
        assert!(is_invalid(drain(&s, 0, 256)));
        assert!(is_invalid(drain(&s, 0, 1000)));
        let (_d, sound) = v2_only(&wide(257), 0, 64);
        assert_eq!(drain(&sound, 0, 256).unwrap(), 257);
    }

    #[test]
    fn b2_child_names_are_validated() {
        for bad in [&b"../escaped"[..], b"a/b", b"", b"..", b"x\0y"] {
            let mut rows = wide(3);
            rows[2].name = bad.to_vec();
            // Keep sibling order sound so only the name rule can fire.
            rows[1].name = b"\x01".to_vec();
            rows[3].name = b"\xfe".to_vec();
            let (_d, s) = v2_only(&rows, 0, 2);
            assert!(is_invalid(drain(&s, 0, 10)), "{bad:?}");
        }
    }

    #[test]
    fn b5_depth_overflow_is_invalid_not_a_panic() {
        let root = RawPath::new(b"/r".to_vec()).unwrap();
        let a = root.join(&RawName::new(b"a".to_vec()).unwrap());
        let b = a.join(&RawName::new(b"b".to_vec()).unwrap());
        let rows = vec![
            row(0, None, 0, b"/r", &root, 2),
            row(1, Some(0), u32::MAX, b"a", &a, 2),
            row(2, Some(1), 1, b"b", &b, 2),
        ];
        let (_d, s) = v2_only(&rows, 0, 8);
        assert!(is_invalid(drain(&s, 1, 10)));
        assert!(is_invalid(drain(&s, 0, 10)));
    }

    #[test]
    fn s2_b4_shallow_rows_are_validated() {
        let rows = tree(6);
        let (d, s) = v2_only(&rows, 2, 4);
        let kids = s.children(0, SortKey::Name, 100).unwrap().len();
        assert_eq!(kids, 6);
        let copy = |sql: &str| {
            let dir = test_support::private_tempdir().unwrap();
            let file = dir.path().join("t.db");
            std::fs::copy(&s.file, &file).unwrap();
            let conn = rusqlite::Connection::open(&file).unwrap();
            conn.execute_batch(sql).unwrap();
            drop(conn);
            (dir, Snapshot::open(&file).unwrap())
        };
        // S2: an undecodable shallow row is Invalid, not Sql.
        let (_k, bogus) = copy("UPDATE shallow_dirs SET t_disk='bogus' WHERE id=1;");
        assert!(is_invalid(bogus.children(0, SortKey::Name, 100)));
        assert!(is_invalid(bogus.children_count(0, b"")));
        // B4: missing first, middle and last child rows.
        let kids: Vec<u32> = rows
            .iter()
            .filter(|r| r.parent_id == Some(0))
            .map(|r| r.id)
            .collect();
        for id in [kids[0], kids[2], kids[5]] {
            let (_k, snap) = copy(&format!("DELETE FROM shallow_dirs WHERE id={id};"));
            assert!(is_invalid(snap.children(0, SortKey::Name, 100)), "{id}");
            assert!(is_invalid(snap.children_count(0, b"")), "{id}");
            assert!(
                is_invalid(snap.children_page(0, SortKey::Disk, true, None, false, 10, b"")),
                "{id}"
            );
        }
        drop(d);
    }

    #[test]
    fn review10_b1_shallow_subtree_and_metrics_rows_are_validated() {
        let copy = |s: &Snapshot, sql: &str| {
            let dir = test_support::private_tempdir().unwrap();
            let file = dir.path().join("t.db");
            std::fs::copy(&s.file, &file).unwrap();
            let conn = rusqlite::Connection::open(&file).unwrap();
            conn.execute_batch(sql).unwrap();
            drop(conn);
            (dir, Snapshot::open(&file).unwrap())
        };
        for width in [1, 2] {
            let rows = tree(width);
            let (_d, s) = v2_only(&rows, 2, 4);
            assert!(s.subtree_dirs(0, 1, SortKey::Name, 100).is_ok());
            assert!(s.metrics_candidates(1, 100).is_ok());
            // Deleted first child row: contiguity breaks.
            let (_k, snap) = copy(&s, "DELETE FROM shallow_dirs WHERE id=1;");
            assert!(is_invalid(snap.subtree_dirs(0, 1, SortKey::Name, 100)));
            assert!(is_invalid(snap.metrics_candidates(1, 100)));
        }
        // A depth-2 row moved under the root keeps its chunk and the true span.
        let rows = tree(2);
        let (_d, s) = v2_only(&rows, 2, 4);
        let (_k, snap) = copy(
            &s,
            "UPDATE shallow_dirs SET parent_id=0,depth=1 WHERE id=2;",
        );
        assert!(is_invalid(snap.subtree_dirs(0, 1, SortKey::Name, 100)));
        assert!(is_invalid(snap.subtree_dirs(0, 2, SortKey::Name, 100)));
        assert!(is_invalid(snap.metrics_candidates(2, 100)));
        // Deleting the last in-range row leaves the enumeration short.
        let (_k, snap) = copy(&s, "DELETE FROM shallow_dirs WHERE id=6;");
        assert!(is_invalid(snap.subtree_dirs(0, 1, SortKey::Name, 100)));
    }

    /// Balanced three-way tree to depth 4 (121 directories).
    fn balanced121() -> Vec<DirRow> {
        let mut rows: Vec<DirRow> = Vec::new();
        let rootp = RawPath::new(b"/r".to_vec()).unwrap();
        let mut stack = vec![(None::<u32>, 0u32, b"/r".to_vec(), rootp)];
        while let Some((parent, depth, name, path)) = stack.pop() {
            let id = rows.len() as u32;
            rows.push(row(id, parent, depth, &name, &path, id));
            if depth < 4 {
                for k in (0..3).rev() {
                    let n = format!("n{k}").into_bytes();
                    let p = path.join(&RawName::new(n.clone()).unwrap());
                    stack.push((Some(id), depth + 1, n, p));
                }
            }
        }
        assert_eq!(rows.len(), 121);
        for i in (0..rows.len()).rev() {
            let mut end = i as u32;
            for r in rows.iter().skip(i + 1) {
                if r.parent_id == Some(i as u32) {
                    end = r.subtree_end;
                }
            }
            rows[i].subtree_end = end;
        }
        rows
    }

    #[test]
    fn fix5_valid_subtree_results_match_v1() {
        let f = fixture(balanced121(), 3, 16);
        for (root, depth) in [
            (0, 0),
            (0, 3),
            (0, 4),
            (0, u32::MAX),
            (1, 2),
            (5, 1),
            (40, 9),
        ] {
            for sort in [SortKey::Name, SortKey::Disk] {
                let a = f.v1.subtree_dirs(root, depth, sort, 1000).unwrap();
                let b = f.v2.subtree_dirs(root, depth, sort, 1000).unwrap();
                assert_eq!(a, b, "root {root} depth {depth}");
            }
        }
    }

    #[test]
    fn fix5_subtree_root_bounds_come_from_the_chunk() {
        let copy = |s: &Snapshot, sql: &str| {
            let dir = test_support::private_tempdir().unwrap();
            let file = dir.path().join("t.db");
            std::fs::copy(&s.file, &file).unwrap();
            let conn = rusqlite::Connection::open(&file).unwrap();
            conn.execute_batch(sql).unwrap();
            drop(conn);
            (dir, Snapshot::open(&file).unwrap())
        };
        // (a) root plus one child, truncated shallow span.
        let rows = tree(1);
        let (_d, s) = v2_only(&rows, 2, 4);
        let (_k, snap) = copy(&s, "UPDATE shallow_dirs SET subtree_end=0 WHERE id=0;");
        assert!(is_invalid(snap.subtree_dirs(0, 1, SortKey::Name, 100)));
        assert!(is_invalid(snap.metrics_candidates(1, 100)));
        // (b) wrong shallow depth on the root of a 121-dir tree.
        let rows = balanced121();
        let (_d, s) = v2_only(&rows, 3, 16);
        assert_eq!(s.subtree_dirs(0, 3, SortKey::Name, 1000).unwrap().len(), 40);
        let (_k, snap) = copy(&s, "UPDATE shallow_dirs SET depth=1 WHERE id=0;");
        assert!(is_invalid(snap.subtree_dirs(0, 3, SortKey::Name, 1000)));
    }
}
