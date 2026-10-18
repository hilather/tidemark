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
const SHALLOW_COLUMNS: &str = "id,parent_id,subtree_end,depth,name,ino,mtime,dir_disk,dir_apparent,projid,entries,errors,own_files,own_symlinks,own_charged,own_apparent,t_dirs,t_files,t_inodes,t_disk,t_apparent,t_charged,t_reclaimable,t_errors,age0,age1,age2,age3,age4,age5,newest_mtime,max_entries";
const CACHE_BYTES: usize = 8 * 1024 * 1024;
type Held = Arc<Vec<ChunkDir>>;
/// A query's requested directory: its chunk row and the decoded chunk it came
/// from, kept alive so an ancestor climb cannot evict it before the scan reuses
/// it (review17 S1). Built only by `Backend::requested`.
struct Root {
    row: ChunkDir,
    held: Held,
}
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
    /// Directory chunks streamed by `scan` so far (work-bound tests).
    #[cfg(test)]
    scanned: std::cell::Cell<u64>,
}
// ---- ranking ----

/// Tie order after the primary key: raw name bytes ascending (v1
/// `ORDER BY metric DESC, name, id`), descending (reverse name pages), or path
/// bytes (`metrics_candidates`: `ORDER BY metric DESC, path, id`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tie {
    Name,
    NameDesc,
    Path,
}
/// A ranked candidate carrying its reconstructed path; no key material is cloned.
struct Hit {
    key: i128,
    tie: Tie,
    row: ChunkDir,
    path: RawPath,
}
impl Hit {
    fn rank(&self) -> (i128, &[u8], u32) {
        let bytes = match self.tie {
            Tie::Path => self.path.as_bytes(),
            Tie::Name | Tie::NameDesc => self.row.name.as_slice(),
        };
        (self.key, bytes, self.row.id)
    }
}
fn rank_cmp(a: (i128, &[u8], u32), b: (i128, &[u8], u32), tie: Tie) -> Ordering {
    a.0.cmp(&b.0)
        .then_with(|| match tie {
            Tie::NameDesc => b.1.cmp(a.1),
            Tie::Name | Tie::Path => a.1.cmp(b.1),
        })
        .then_with(|| a.2.cmp(&b.2))
}
impl Ord for Hit {
    fn cmp(&self, other: &Self) -> Ordering {
        rank_cmp(self.rank(), other.rank(), self.tie)
    }
}
impl PartialOrd for Hit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Hit {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Hit {}
/// Bounded top-N over candidates offered in any order: a vector until `cap`
/// candidates are held, then a max-heap evicting the worst. `sql_limit`, the
/// `MAX_QUERY_ROWS` cap and the result bound apply exactly as in v1.
struct TopN {
    cap: usize,
    tie: Tie,
    vec: Vec<Hit>,
    heap: BinaryHeap<Hit>,
}
impl TopN {
    fn new(limit: usize, tie: Tie) -> Result<Self> {
        sql_limit(limit)?;
        Ok(Self {
            cap: limit.min(crate::error::MAX_QUERY_ROWS + 1),
            tie,
            vec: Vec::new(),
            heap: BinaryHeap::new(),
        })
    }
    /// Whether a candidate ranked `(key, tie bytes, id)` would enter the result.
    fn admits(&self, key: i128, bytes: &[u8], id: u32) -> bool {
        if self.cap == 0 {
            return false;
        }
        match self.heap.peek() {
            Some(worst) => rank_cmp((key, bytes, id), worst.rank(), self.tie) == Ordering::Less,
            None => true,
        }
    }
    /// Insert a candidate that `admits` accepted.
    fn push(&mut self, hit: Hit) {
        if self.heap.is_empty() {
            self.vec.push(hit);
            if self.vec.len() == self.cap {
                self.heap = BinaryHeap::from(std::mem::take(&mut self.vec));
            }
        } else {
            self.heap.pop();
            self.heap.push(hit);
        }
    }
    fn finish(self) -> Result<Vec<Hit>> {
        if self.heap.is_empty() {
            crate::error::check_result_bound(&self.vec)?;
            let mut hits = self.vec;
            hits.sort_unstable();
            Ok(hits)
        } else {
            crate::error::check_result_bound(self.heap.as_slice())?;
            Ok(self.heap.into_sorted_vec())
        }
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
fn view_key(v: &chunk::DirView<'_>, sort: SortKey) -> i128 {
    match sort {
        SortKey::Disk => v.t_disk.into(),
        SortKey::Apparent => v.t_apparent.into(),
        SortKey::Charged => v.t_charged.into(),
        SortKey::Reclaimable => v.t_reclaimable.into(),
        SortKey::Files => v.t_files.into(),
        SortKey::Inodes => v.t_inodes.into(),
        SortKey::Newest => v.newest_mtime.into(),
        SortKey::Name => 0,
    }
}
/// Validates a depth-limited preorder as it streams past (contiguity, parent
/// links, depths, subtree spans, names) and builds every row's path from the
/// walk root's path, as `visit_paths` does. Rows deeper than `limit` are not
/// offered; the walk root's own path is `base`. The first row offered must be
/// `root` itself (the requested directory, already checked by `requested`), so
/// no row of the walk is exempt from validation.
struct Walk<'r> {
    root: &'r ChunkDir,
    limit: u64,
    next: u64,
    started: bool,
    stack: Vec<(u32, u32, u32, usize)>,
    buf: Vec<u8>,
}
impl<'r> Walk<'r> {
    fn new(root: &'r ChunkDir, limit: u64, base: &RawPath) -> Self {
        Self {
            root,
            limit,
            next: u64::from(root.id),
            started: false,
            stack: Vec::new(),
            buf: base.as_bytes().to_vec(),
        }
    }
    /// Checks the next row and returns its path bytes (valid until the next step).
    fn step(
        &mut self,
        id: u32,
        parent_id: Option<u32>,
        subtree_end: u32,
        depth: u32,
        name: &[u8],
    ) -> Result<&[u8]> {
        if u64::from(id) != self.next || subtree_end < id || subtree_end > self.root.subtree_end {
            return Err(invalid());
        }
        while self.stack.last().is_some_and(|p| p.1 < id) {
            self.stack.pop();
        }
        match self.stack.last() {
            Some(&(pid, pend, pdepth, len)) => {
                if parent_id != Some(pid)
                    || Some(depth) != pdepth.checked_add(1)
                    || id > pend
                    || subtree_end > pend
                    || !valid_name(name)
                {
                    return Err(invalid());
                }
                self.buf.truncate(len);
                if self.buf.len() > 1 {
                    self.buf.push(b'/');
                }
                self.buf.extend_from_slice(name);
                if self.buf.len() > MAX_PATH_BYTES {
                    return Err(invalid());
                }
            }
            None if self.started => return Err(invalid()),
            None => {
                let r = self.root;
                if id != r.id
                    || parent_id != r.parent_id
                    || subtree_end != r.subtree_end
                    || depth != r.depth
                    || name != r.name.as_slice()
                {
                    return Err(invalid());
                }
            }
        }
        self.started = true;
        self.next = if u64::from(depth) < self.limit && subtree_end > id {
            u64::from(id) + 1
        } else {
            u64::from(subtree_end) + 1
        };
        self.stack.push((id, subtree_end, depth, self.buf.len()));
        Ok(&self.buf)
    }
    fn finish(&self) -> Result<()> {
        if self.started && self.next == u64::from(self.root.subtree_end) + 1 {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
fn contains(name: &[u8], filter: &[u8]) -> bool {
    filter.is_empty() || name.windows(filter.len()).any(|w| w == filter)
}
/// Decodes a `shallow_dirs` row selected as `SHALLOW_COLUMNS` (no path columns).
fn shallow_dir(r: &Row<'_>) -> rusqlite::Result<ChunkDir> {
    use crate::error::read_u64 as u;
    Ok(ChunkDir {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        subtree_end: r.get(2)?,
        depth: r.get(3)?,
        name: r.get(4)?,
        ino: u(r, 5)?,
        mtime: r.get(6)?,
        dir_disk: u(r, 7)?,
        dir_apparent: u(r, 8)?,
        projid: r.get(9)?,
        entries: r.get(10)?,
        errors: r.get(11)?,
        own_files: u(r, 12)?,
        own_symlinks: u(r, 13)?,
        own_charged: u(r, 14)?,
        own_apparent: u(r, 15)?,
        t_dirs: u(r, 16)?,
        t_files: u(r, 17)?,
        t_inodes: u(r, 18)?,
        t_disk: u(r, 19)?,
        t_apparent: u(r, 20)?,
        t_charged: u(r, 21)?,
        t_reclaimable: u(r, 22)?,
        t_errors: u(r, 23)?,
        age0: u(r, 24)?,
        age1: u(r, 25)?,
        age2: u(r, 26)?,
        age3: u(r, 27)?,
        age4: u(r, 28)?,
        age5: u(r, 29)?,
        newest_mtime: r.get(30)?,
        max_entries: r.get(31)?,
        file_lo: 0,
        file_count: 0,
    })
}
impl Backend {
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
            #[cfg(test)]
            scanned: std::cell::Cell::new(0),
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
    /// The cached chunk covering exactly `first..=last`, without touching LRU order.
    fn cached(&self, first: u32, last: u32) -> Option<Arc<Vec<ChunkDir>>> {
        let cache = self.cache.borrow();
        let hit = cache.iter().find(|c| c.first == first && c.last == last)?;
        Some(hit.rows.clone())
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
        let mut stmt = snap.conn.prepare_cached(&format!(
            "SELECT {SHALLOW_COLUMNS} FROM shallow_dirs WHERE id=?"
        ))?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(shallow_dir(row).map_err(plain_row_error)?)),
            None => Ok(None),
        }
    }
    pub(super) fn raw(&self, snap: &Snapshot, id: u32) -> Result<ChunkDir> {
        Ok(self.raw_held(snap, id)?.0)
    }
    /// `raw`, also returning the decoded chunk so a caller can keep it alive
    /// past LRU eviction.
    fn raw_held(&self, snap: &Snapshot, id: u32) -> Result<(ChunkDir, Held)> {
        let rows = self.chunk(snap, id)?;
        let first = rows.first().ok_or_else(invalid)?.id;
        let row = rows
            .get((id - first) as usize)
            .cloned()
            .ok_or_else(invalid)?;
        Ok((row, rows))
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
    // ---- the requested directory ----
    //
    // Every directory query starts from one requested id and checks the same
    // things about it, in the same order, before reading anything else:
    //
    // 1. `requested`: the row from its chunk (`Root::held` keeps that chunk
    //    alive, so an ancestor climb cannot evict it before a scan reuses it);
    //    the shallow copy must exist exactly when `depth <= plain_depth` and
    //    agree on id, parent, depth, span and name; the name must be a valid
    //    component (a valid root path for id 0, which must have depth 0).
    //    This never decodes another chunk.
    // 2. `root_path`: the full path, climbing every ancestor (link, depth, span
    //    and name checked by `path`), shared through the path cache. It may
    //    decode ancestor chunks. Which queries climb, and when, is the only
    //    per-query policy (coordinator decisions, reviews 14–18):
    //    - `subtree_dirs`, `wide_dirs`, `metrics_candidates`, `children_page`:
    //      eagerly, before any row is enumerated, so a rejection never depends
    //      on the limit, the filter or whether a row is admitted;
    //    - `children`: on the first admitted row (no production caller; the
    //      pre-rewrite reader climbed per returned row);
    //    - `children_count`: never (it returns no path).
    // 3. Enumeration: every sibling or walked row is checked whether or not it
    //    is admitted (`hop_children`, `visit_shallow_children`, `Walk`); only
    //    the shallow `children_page` narrows this to the rows it returns, with
    //    `check_shallow_children` covering the child set.
    //
    // Argument checks stay where v1 performs them: before the lookup for the
    // child listings (v1 never reads the parent), after it for the range
    // queries (v1's `dir(id)` runs first).

    /// The requested directory of a query (step 1 above); a missing id is the
    /// SQL "no rows" error, as `dir(id)` reports it.
    fn requested(&self, snap: &Snapshot, id: u32) -> Result<Root> {
        let (row, held) = self.raw_held(snap, id)?;
        match self.shallow_row(snap, id)? {
            Some(shallow) => {
                if shallow.id != row.id
                    || shallow.parent_id != row.parent_id
                    || shallow.depth != row.depth
                    || shallow.subtree_end != row.subtree_end
                    || shallow.name != row.name
                {
                    return Err(invalid());
                }
            }
            None if row.depth <= self.plain_depth => return Err(invalid()),
            None => {}
        }
        if row.id == 0 {
            if row.depth != 0 {
                return Err(invalid());
            }
            RawPath::new(row.name.clone()).map_err(|_| invalid())?;
        } else if !valid_name(&row.name) {
            return Err(invalid());
        }
        Ok(Root { row, held })
    }
    /// `requested` for a child listing: `None` for an absent id (an empty listing in v1).
    fn listing_root(&self, snap: &Snapshot, id: u32) -> Result<Option<Root>> {
        if !self.locate(snap, id)?.is_some_and(|c| c.last >= id) {
            return Ok(None);
        }
        self.requested(snap, id).map(Some)
    }
    /// The validated full path of a requested directory (step 2 above).
    fn root_path(&self, snap: &Snapshot, root: &Root) -> Result<Arc<RawPath>> {
        self.path_of(snap, &root.row)
    }
    /// The shallow copy must hold exactly `parent`'s child set: first child at
    /// id+1, each next child right after the previous subtree, and the last
    /// subtree ending at the parent's. Reads three integers per child.
    fn check_shallow_children(&self, snap: &Snapshot, parent: &ChunkDir) -> Result<()> {
        let mut stmt = snap.conn.prepare_cached(
            "SELECT id,subtree_end,depth FROM shallow_dirs WHERE parent_id=? ORDER BY id",
        )?;
        let mut rows = stmt.query([parent.id])?;
        let mut expect = if parent.subtree_end > parent.id {
            parent.id.checked_add(1)
        } else {
            None
        };
        let child_depth = parent.depth.checked_add(1).ok_or_else(invalid)?;
        while let Some(row) = rows.next()? {
            let cols = (|| -> rusqlite::Result<(u32, u32, u32)> {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })();
            let (id, end, depth) = cols.map_err(plain_row_error)?;
            if Some(id) != expect || depth != child_depth || end < id || end > parent.subtree_end {
                return Err(invalid());
            }
            expect = if end < parent.subtree_end {
                Some(end + 1)
            } else {
                None
            };
        }
        if expect.is_some() {
            return Err(invalid());
        }
        Ok(())
    }
    /// Visits the children of a parent at or below the plain depth by hopping
    /// over sibling subtrees inside the decoded chunks. Every child is checked
    /// as `visit_shallow_children` checks a shallow one (parent, depth, span,
    /// name), whether or not the caller keeps it.
    fn hop_children(
        &self,
        snap: &Snapshot,
        parent: &ChunkDir,
        mut visit: impl FnMut(ChunkDir) -> Result<()>,
    ) -> Result<()> {
        let child_depth = parent.depth.checked_add(1).ok_or_else(invalid)?;
        let mut next = parent.id.checked_add(1);
        while let Some(child_id) = next.filter(|n| *n <= parent.subtree_end) {
            let child = self.raw(snap, child_id).map_err(required)?;
            if child.parent_id != Some(parent.id)
                || child.depth != child_depth
                || child.subtree_end < child.id
                || child.subtree_end > parent.subtree_end
                || !valid_name(&child.name)
            {
                return Err(invalid());
            }
            next = child.subtree_end.checked_add(1);
            visit(child)?;
        }
        Ok(())
    }
    /// A child checked against its parent; its path is the parent's plus the name.
    fn child_path(parent: &ChunkDir, base: &RawPath, c: &ChunkDir) -> Result<RawPath> {
        if c.parent_id != Some(parent.id)
            || Some(c.depth) != parent.depth.checked_add(1)
            || c.id <= parent.id
            || c.id > parent.subtree_end
            || c.subtree_end < c.id
            || c.subtree_end > parent.subtree_end
            || !valid_name(&c.name)
        {
            return Err(invalid());
        }
        let mut bytes = Vec::with_capacity(base.as_bytes().len() + 1 + c.name.len());
        bytes.extend_from_slice(base.as_bytes());
        if bytes.len() > 1 {
            bytes.push(b'/');
        }
        bytes.extend_from_slice(&c.name);
        if bytes.len() > MAX_PATH_BYTES {
            return Err(invalid());
        }
        RawPath::new(bytes).map_err(|_| invalid())
    }
    fn materialized(&self, row: ChunkDir, path: RawPath) -> DirRow {
        let mut d = self.materialize_with(row, path);
        d.path_hash = d.path.hash64();
        d
    }
    fn finish(&self, top: TopN) -> Result<Vec<DirRow>> {
        let out = top
            .finish()?
            .into_iter()
            .map(|h| self.materialized(h.row, h.path))
            .collect();
        Ok(out)
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
    pub(super) fn children(
        &self,
        snap: &Snapshot,
        id: u32,
        sort: SortKey,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        let mut top = TopN::new(limit, Tie::Name)?;
        let Some(root) = self.listing_root(snap, id)? else {
            return Ok(Vec::new());
        };
        let parent = &root.row;
        // The parent's path is rebuilt only once a row is going to be returned.
        let mut base: Option<Arc<RawPath>> = None;
        let visit = |c: ChunkDir| {
            let k = -key(&c, sort);
            if top.admits(k, &c.name, c.id) {
                let b = match &base {
                    Some(b) => b,
                    None => base.insert(self.root_path(snap, &root)?),
                };
                let path = Self::child_path(parent, b, &c)?;
                top.push(Hit {
                    key: k,
                    tie: Tie::Name,
                    row: c,
                    path,
                });
            }
            Ok(())
        };
        if parent.depth < self.plain_depth {
            // Every sibling is decoded (one pass), as `children_count` does.
            self.visit_shallow_children(snap, parent, visit)?;
        } else {
            self.hop_children(snap, parent, visit)?;
        }
        self.finish(top)
    }
    /// Every child of a shallow parent as a full row, checked like
    /// `check_shallow_children` plus its name (one pass; undecodable rows are
    /// `Invalid`).
    fn visit_shallow_children(
        &self,
        snap: &Snapshot,
        parent: &ChunkDir,
        mut visit: impl FnMut(ChunkDir) -> Result<()>,
    ) -> Result<()> {
        let mut stmt = snap.conn.prepare_cached(&format!(
            "SELECT {SHALLOW_COLUMNS} FROM shallow_dirs WHERE parent_id=? ORDER BY id"
        ))?;
        let mut rows = stmt.query([parent.id])?;
        let mut expect = if parent.subtree_end > parent.id {
            parent.id.checked_add(1)
        } else {
            None
        };
        let child_depth = parent.depth.checked_add(1).ok_or_else(invalid)?;
        while let Some(row) = rows.next()? {
            let c = shallow_dir(row).map_err(plain_row_error)?;
            if Some(c.id) != expect
                || c.parent_id != Some(parent.id)
                || c.depth != child_depth
                || c.subtree_end < c.id
                || c.subtree_end > parent.subtree_end
                || !valid_name(&c.name)
            {
                return Err(invalid());
            }
            expect = if c.subtree_end < parent.subtree_end {
                Some(c.subtree_end + 1)
            } else {
                None
            };
            visit(c)?;
        }
        if expect.is_some() {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn children_count(&self, snap: &Snapshot, id: u32, filter: &[u8]) -> Result<u64> {
        let Some(root) = self.listing_root(snap, id)? else {
            return Ok(0);
        };
        let parent = &root.row;
        let mut count = 0;
        let tally = |c: ChunkDir| {
            if contains(&c.name, filter) {
                count += 1;
            }
            Ok(())
        };
        if parent.depth < self.plain_depth {
            self.visit_shallow_children(snap, parent, tally)?;
        } else {
            self.hop_children(snap, parent, tally)?;
        }
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
        let column = match sort {
            SortKey::Disk => "t_disk",
            SortKey::Apparent => "t_apparent",
            SortKey::Reclaimable => "t_reclaimable",
            SortKey::Charged => "t_charged",
            SortKey::Files => "t_files",
            SortKey::Inodes => "t_inodes",
            SortKey::Newest => "newest_mtime",
            SortKey::Name => "name",
        };
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
        let primary_desc = descending ^ reverse;
        let name_desc = if sort == SortKey::Name {
            primary_desc
        } else {
            reverse
        };
        let Some(root) = self.listing_root(snap, id)? else {
            return Ok(Vec::new());
        };
        let parent = &root.row;
        // Eager (policy above): an empty, filtered-out or zero-limit page
        // still rejects a malformed parent name or ancestry.
        let base = self.root_path(snap, &root)?;
        if parent.depth < self.plain_depth {
            // SQLite's bounded sorter picks the page (the v1 statement on the
            // shallow table); the child set itself is checked separately.
            self.check_shallow_children(snap, parent)?;
            let key_value = match (cursor, sort) {
                (None, _) => rusqlite::types::Value::Null,
                (Some(r), SortKey::Name) => rusqlite::types::Value::Blob(r.name.clone()),
                (Some(_), _) => rusqlite::types::Value::Integer(
                    i64::try_from(cursor_key.unwrap_or(0)).map_err(|_| invalid())?,
                ),
            };
            let cmp = if primary_desc { "<" } else { ">" };
            let name_cmp = if name_desc { "<" } else { ">" };
            let direction = if primary_desc { "DESC" } else { "ASC" };
            let name_direction = if name_desc { "DESC" } else { "ASC" };
            // Omit the cursor predicate entirely for the first page.
            let after = if cursor.is_some() {
                format!(" AND ({column}{cmp}?3 OR ({column}=?3 AND name{name_cmp}?4))")
            } else {
                String::new()
            };
            let columns = SHALLOW_COLUMNS
                .split(',')
                .map(|c| format!("s.{c}"))
                .collect::<Vec<_>>()
                .join(",");
            let mut stmt = snap.conn.prepare_cached(&format!(
                "WITH page AS (SELECT id FROM shallow_dirs WHERE parent_id=?1 AND (length(?2)=0 OR instr(name,?2)>0){after} ORDER BY {column} {direction},name {name_direction} LIMIT ?5) SELECT {columns} FROM page JOIN shallow_dirs s ON s.id=page.id ORDER BY {column} {direction},name {name_direction}"
            ))?;
            let mut rows = stmt.query(params![
                id,
                filter,
                key_value,
                cursor.map(|r| r.name.as_slice()).unwrap_or_default(),
                sql_limit(limit)?
            ])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                let c = shallow_dir(row).map_err(plain_row_error)?;
                let path = Self::child_path(parent, &base, &c)?;
                out.push(self.materialized(c, path));
            }
            return Ok(out);
        }
        let tie = if name_desc { Tie::NameDesc } else { Tie::Name };
        let mut top = TopN::new(limit, tie)?;
        self.hop_children(snap, parent, |c| {
            if !contains(&c.name, filter) {
                return Ok(());
            }
            if let (Some(cur), Some(k)) = (cursor, cursor_key) {
                let mut cmp = key(&c, sort).cmp(&k);
                if primary_desc {
                    cmp = cmp.reverse();
                }
                if cmp == Ordering::Equal {
                    cmp = c.name.cmp(&cur.name);
                    if name_desc {
                        cmp = cmp.reverse();
                    }
                }
                if cmp != Ordering::Greater {
                    return Ok(());
                }
            }
            let k = if primary_desc {
                -key(&c, sort)
            } else {
                key(&c, sort)
            };
            if top.admits(k, &c.name, c.id) {
                let path = Self::child_path(parent, &base, &c)?;
                top.push(Hit {
                    key: k,
                    tie,
                    row: c,
                    path,
                });
            }
            Ok(())
        })?;
        self.finish(top)
    }
    /// Streams every row of `lo..=hi` in preorder straight from the codec:
    /// nothing is cached or materialised, chunk coordinates are checked as in
    /// `projected_chunks` (no gaps or overlaps, neighbours of the first and last
    /// chunk, table bounds agreeing with the decoded row count).
    fn scan(
        &self,
        snap: &Snapshot,
        held: &Held,
        lo: u32,
        hi: u32,
        mut visit: impl FnMut(&chunk::DirView<'_>) -> Result<()>,
    ) -> Result<()> {
        let mut stmt = snap.conn.prepare_cached("SELECT first_id,last_id,CASE WHEN typeof(data)='blob' THEN length(data) END,typeof(data) FROM dir_chunks WHERE first_id >= (SELECT first_id FROM dir_chunks WHERE first_id<=?1 ORDER BY first_id DESC LIMIT 1) AND first_id<=?2 ORDER BY first_id")?;
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
            let mut count = 0u32;
            // A chunk the caller holds (the requested root's, kept across the
            // ancestor climb) or the LRU already holds was fully validated when decoded:
            // reuse it (lookup only; streamed chunks are never inserted).
            let held_hit = held
                .first()
                .zip(held.last())
                .is_some_and(|(a, b)| a.id == c.first && b.id == c.last);
            let hit = if held_hit {
                Some(held.clone())
            } else {
                self.cached(c.first, c.last)
            };
            if let Some(rows) = hit {
                for r in rows.iter() {
                    count = count.checked_add(1).ok_or_else(invalid)?;
                    if r.id >= lo && r.id <= hi {
                        visit(&r.view())?;
                    }
                }
            } else {
                let data = chunk_data(&snap.conn, c)?;
                #[cfg(test)]
                self.scanned.set(self.scanned.get() + 1);
                chunk::visit_dirs(&data, c.first, |v| {
                    count = count.checked_add(1).ok_or_else(invalid)?;
                    if v.id >= lo && v.id <= hi {
                        visit(v)?;
                    }
                    Ok(())
                })?;
            }
            if count == 0 || c.first.checked_add(count - 1) != Some(c.last) {
                return Err(invalid());
            }
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
    pub(super) fn subtree_dirs(
        &self,
        snap: &Snapshot,
        id: u32,
        depth: u32,
        sort: SortKey,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        let root = self.requested(snap, id)?;
        let base = self.root_path(snap, &root)?;
        let mut top = TopN::new(limit, Tie::Name)?;
        let max = u64::from(root.row.depth) + u64::from(depth);
        let mut walk = Walk::new(&root.row, max, &base);
        if max <= u64::from(self.plain_depth) {
            // Every requested row is a shallow row: no chunk is decoded.
            let mut stmt = snap.conn.prepare_cached(&format!(
                "SELECT {SHALLOW_COLUMNS} FROM shallow_dirs WHERE id>=? AND id<=? AND depth<=? ORDER BY id"
            ))?;
            let mut rows = stmt.query(params![
                id,
                root.row.subtree_end,
                i64::try_from(max).map_err(|_| invalid())?
            ])?;
            while let Some(row) = rows.next()? {
                let r = shallow_dir(row).map_err(plain_row_error)?;
                let path = walk.step(r.id, r.parent_id, r.subtree_end, r.depth, &r.name)?;
                let k = -key(&r, sort);
                if top.admits(k, &r.name, r.id) {
                    let path = RawPath::new(path.to_vec()).map_err(|_| invalid())?;
                    top.push(Hit {
                        key: k,
                        tie: Tie::Name,
                        row: r,
                        path,
                    });
                }
            }
        } else {
            self.scan(snap, &root.held, id, root.row.subtree_end, |v| {
                if u64::from(v.depth) > max {
                    return Ok(());
                }
                let path = walk.step(v.id, v.parent_id, v.subtree_end, v.depth, v.name)?;
                let k = -view_key(v, sort);
                if top.admits(k, v.name, v.id) {
                    let path = RawPath::new(path.to_vec()).map_err(|_| invalid())?;
                    top.push(Hit {
                        key: k,
                        tie: Tie::Name,
                        row: v.to_owned(),
                        path,
                    });
                }
                Ok(())
            })?;
        }
        walk.finish()?;
        self.finish(top)
    }
    pub(super) fn wide_dirs(
        &self,
        snap: &Snapshot,
        id: u32,
        min: u32,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        // Eager root validation (policy above): an empty, filtered-out or
        // zero-limit result may decode the root's ancestor chunks. Hits below
        // the root build their paths lazily, relative to "/" while walking.
        let root = self.requested(snap, id)?;
        let base = self.root_path(snap, &root)?;
        let mut top = TopN::new(limit, Tie::Name)?;
        let rel_root = RawPath::new(b"/".to_vec()).map_err(|_| invalid())?;
        let mut walk = Walk::new(&root.row, u64::MAX, &rel_root);
        self.scan(snap, &root.held, id, root.row.subtree_end, |v| {
            let rel = walk.step(v.id, v.parent_id, v.subtree_end, v.depth, v.name)?;
            if v.entries < min {
                return Ok(());
            }
            let k = -i128::from(v.entries);
            if top.admits(k, v.name, v.id) {
                let b = base.as_bytes();
                let mut full = Vec::with_capacity(b.len() + rel.len());
                full.extend_from_slice(b);
                if rel.len() > 1 {
                    if full.len() > 1 {
                        full.push(b'/');
                    }
                    full.extend_from_slice(&rel[1..]);
                }
                if full.len() > MAX_PATH_BYTES {
                    return Err(invalid());
                }
                let path = RawPath::new(full).map_err(|_| invalid())?;
                top.push(Hit {
                    key: k,
                    tie: Tie::Name,
                    row: v.to_owned(),
                    path,
                });
            }
            Ok(())
        })?;
        walk.finish()?;
        self.finish(top)
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
        let root = self.requested(snap, 0)?;
        let base = self.root_path(snap, &root)?;
        let mut count = 0u64;
        let mut tops = [
            (SortKey::Disk, TopN::new(limit, Tie::Path)?),
            (SortKey::Reclaimable, TopN::new(limit, Tie::Path)?),
        ];
        let mut walk = Walk::new(&root.row, u64::from(depth), &base);
        if depth <= self.plain_depth {
            let mut stmt = snap.conn.prepare_cached(&format!(
                "SELECT {SHALLOW_COLUMNS} FROM shallow_dirs WHERE depth<=? ORDER BY id"
            ))?;
            let mut rows = stmt.query([depth])?;
            while let Some(row) = rows.next()? {
                let r = shallow_dir(row).map_err(plain_row_error)?;
                let path = walk.step(r.id, r.parent_id, r.subtree_end, r.depth, &r.name)?;
                count += 1;
                for (sort, top) in tops.iter_mut() {
                    let k = -key(&r, *sort);
                    if top.admits(k, path, r.id) {
                        let path = RawPath::new(path.to_vec()).map_err(|_| invalid())?;
                        top.push(Hit {
                            key: k,
                            tie: Tie::Path,
                            row: r.clone(),
                            path,
                        });
                    }
                }
            }
        } else {
            // One pass feeds both rankings.
            self.scan(snap, &root.held, 0, root.row.subtree_end, |v| {
                if v.depth > depth {
                    return Ok(());
                }
                let path = walk.step(v.id, v.parent_id, v.subtree_end, v.depth, v.name)?;
                count += 1;
                for (sort, top) in tops.iter_mut() {
                    let k = -view_key(v, *sort);
                    if top.admits(k, path, v.id) {
                        let path = RawPath::new(path.to_vec()).map_err(|_| invalid())?;
                        top.push(Hit {
                            key: k,
                            tie: Tie::Path,
                            row: v.to_owned(),
                            path,
                        });
                    }
                }
                Ok(())
            })?;
        }
        walk.finish()?;
        let mut results = Vec::with_capacity(2);
        for (_, top) in tops {
            results.push(self.finish(top)?);
        }
        let reclaim = results.pop().ok_or_else(invalid)?;
        let used = results.pop().ok_or_else(invalid)?;
        Ok((used, reclaim, count, snap.file.clone()))
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
        if let Some(path) = self.paths.borrow().map.get(&id).cloned() {
            return Ok(path);
        }
        let row = self.raw(snap, id).map_err(required)?;
        self.path_of(snap, &row)
    }
    /// `cached_path` for a row already in hand (no chunk lookup for the row itself).
    fn path_of(&self, snap: &Snapshot, row: &ChunkDir) -> Result<Arc<RawPath>> {
        let id = row.id;
        if let Some(path) = self.paths.borrow().map.get(&id).cloned() {
            return Ok(path);
        }
        let path = Arc::new(self.path(snap, row)?);
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

    // ---- gate-8 fix: walk, scan and work bounds ----

    type Step<'a> = (u32, Option<u32>, u32, u32, &'a [u8]);
    /// Runs a `Walk` over the rows and returns every path it produced.
    fn walk_paths(base: &[u8], end: u32, limit: u64, rows: &[Step<'_>]) -> Result<Vec<Vec<u8>>> {
        walk_from(&(0, None, end, 0, b"/r"), base, limit, rows)
    }
    /// Runs a `Walk` anchored on `root` (the requested row) over `rows`.
    fn walk_from(
        root: &Step<'_>,
        base: &[u8],
        limit: u64,
        rows: &[Step<'_>],
    ) -> Result<Vec<Vec<u8>>> {
        let base = RawPath::new(base.to_vec()).unwrap();
        let root = ChunkDir {
            id: root.0,
            parent_id: root.1,
            subtree_end: root.2,
            depth: root.3,
            name: root.4.to_vec(),
            ..ChunkDir::default()
        };
        let mut walk = Walk::new(&root, limit, &base);
        let mut out = Vec::new();
        for &(id, parent, subtree_end, depth, name) in rows {
            out.push(walk.step(id, parent, subtree_end, depth, name)?.to_vec());
        }
        walk.finish()?;
        Ok(out)
    }
    /// Preorder `/`, `/a` (with child `a1`), `/b`, `/c`: ids 0..=4.
    fn walk_rows() -> Vec<Step<'static>> {
        vec![
            (0, None, 4, 0, b"/r"),
            (1, Some(0), 2, 1, b"a"),
            (2, Some(1), 2, 2, b"a1"),
            (3, Some(0), 3, 1, b"b"),
            (4, Some(0), 4, 1, b"c"),
        ]
    }

    #[test]
    fn walk_builds_paths_and_skips_below_the_limit() {
        let rows = walk_rows();
        // Depth limit 1: row 2 is hopped over, so it is not offered.
        let kept: Vec<Step<'_>> = rows.iter().copied().filter(|r| r.0 != 2).collect();
        let p = walk_paths(b"/", 4, 1, &kept).unwrap();
        assert_eq!(p, [&b"/"[..], b"/a", b"/b", b"/c"]);
        let p = walk_paths(b"/x", 4, 1, &kept).unwrap();
        assert_eq!(p, [&b"/x"[..], b"/x/a", b"/x/b", b"/x/c"]);
        let p = walk_paths(b"/x", 4, u64::MAX, &rows).unwrap();
        assert_eq!(p[2], b"/x/a/a1");
        assert_eq!(p[3], b"/x/b");
    }

    #[test]
    fn walk_rejects_every_structural_violation() {
        let ok = walk_rows();
        let mutate = |f: &dyn Fn(&mut Vec<Step<'static>>)| {
            let mut rows = ok.clone();
            f(&mut rows);
            walk_paths(b"/r", 4, u64::MAX, &rows)
        };
        assert!(walk_paths(b"/r", 4, u64::MAX, &ok).is_ok());
        // Gap: id 2 missing.
        assert!(is_invalid(mutate(&|r| {
            r.remove(2);
        })));
        // Wrong parent, wrong depth, span beyond the parent's, span beyond the walk.
        assert!(is_invalid(mutate(&|r| r[2].1 = Some(0))));
        assert!(is_invalid(mutate(&|r| r[2].3 = 1)));
        assert!(is_invalid(mutate(&|r| r[2].3 = 3)));
        assert!(is_invalid(mutate(&|r| r[1].2 = 3)));
        assert!(is_invalid(mutate(&|r| r[4].2 = 5)));
        assert!(is_invalid(mutate(&|r| r[4].2 = 3)));
        // A second root.
        assert!(is_invalid(mutate(&|r| r[3].1 = None)));
        // Names.
        for name in [&b""[..], b".", b"..", b"a/b", b"a\0"] {
            assert!(is_invalid(mutate(&|r| r[3].4 = name)), "{name:?}");
        }
        // Short walk and an empty walk.
        assert!(is_invalid(mutate(&|r| {
            r.pop();
        })));
        assert!(is_invalid(mutate(&|r| r.clear())));
        // The first row must be the requested row itself: every column.
        let root = ok[0];
        assert!(walk_from(&root, b"/r", u64::MAX, &ok).is_ok());
        let sub: Vec<Step<'_>> = ok[1..3].to_vec();
        assert!(walk_from(&ok[1], b"/r/a", u64::MAX, &sub).is_ok());
        for (what, bad) in [
            ("name", (0, None, 4, 0, &b"/x"[..])),
            ("span", (0, None, 3, 0, &b"/r"[..])),
            ("depth", (0, None, 4, 1, &b"/r"[..])),
            ("parent", (0, Some(0), 4, 0, &b"/r"[..])),
        ] {
            assert!(is_invalid(walk_from(&bad, b"/r", u64::MAX, &ok)), "{what}");
        }
        assert!(is_invalid(walk_from(&ok[1], b"/r/a", u64::MAX, &ok[2..3])));
    }

    fn tampered(s: &Snapshot, sql: &str) -> (tempfile::TempDir, Snapshot) {
        let dir = test_support::private_tempdir().unwrap();
        let file = dir.path().join("t.db");
        std::fs::copy(&s.file, &file).unwrap();
        let conn = rusqlite::Connection::open(&file).unwrap();
        conn.execute_batch(sql).unwrap();
        drop(conn);
        (dir, Snapshot::open(&file).unwrap())
    }
    fn deep_queries(s: &Snapshot) -> [bool; 3] {
        [
            is_invalid(s.subtree_dirs(0, u32::MAX, SortKey::Disk, 100)),
            is_invalid(s.wide_dirs(0, 0, 100)),
            is_invalid(s.metrics_candidates(3, 100)),
        ]
    }

    #[test]
    fn scan_rejects_tampered_chunks() {
        let (_d, s) = v2_only(&tree(12), 0, 4);
        assert_eq!(deep_queries(&s), [false; 3]);
        for sql in [
            "UPDATE dir_chunks SET data=x'00' WHERE first_id=4;",
            "UPDATE dir_chunks SET last_id=last_id+1 WHERE first_id=4;",
            "DELETE FROM dir_chunks WHERE first_id=4;",
            "UPDATE dir_chunks SET first_id=3 WHERE first_id=4;",
            "UPDATE dir_chunks SET data=zeroblob(42*1024*1024) WHERE first_id=4;",
        ] {
            let (_k, bad) = tampered(&s, sql);
            assert_eq!(deep_queries(&bad), [true; 3], "{sql}");
        }
    }

    #[test]
    fn scan_from_a_subtree_root_touches_only_its_chunks() {
        // tree(12), chunks of 4: ids 1..=5 (c00000 and its kids) span chunks 0 and 1.
        let rows = tree(12);
        assert_eq!(rows[1].subtree_end, 5);
        let (_d, s) = v2_only(&rows, 0, 4);
        let expect = s.wide_dirs(1, 0, 100).unwrap();
        assert_eq!(expect.len(), 5);
        let (_k, outside) = tampered(&s, "UPDATE dir_chunks SET data=x'00' WHERE first_id>=8;");
        assert_eq!(outside.wide_dirs(1, 0, 100).unwrap(), expect);
        assert_eq!(
            outside
                .subtree_dirs(1, u32::MAX, SortKey::Disk, 100)
                .unwrap()
                .len(),
            5
        );
        let (_k, inside) = tampered(&s, "UPDATE dir_chunks SET data=x'00' WHERE first_id=4;");
        assert!(is_invalid(inside.wide_dirs(1, 0, 100)));
    }

    fn decodes(s: &Snapshot) -> u64 {
        s.v2.as_ref().unwrap().decodes.get()
    }
    fn scanned(s: &Snapshot) -> u64 {
        s.v2.as_ref().unwrap().scanned.get()
    }

    #[test]
    fn shallow_work_is_bounded_by_the_root_chunk() {
        let (_d, s) = v2_only(&balanced121(), 3, 16);
        let fresh = Snapshot::open(&s.file).unwrap();
        assert_eq!(
            fresh.subtree_dirs(0, 3, SortKey::Disk, 1000).unwrap().len(),
            40
        );
        assert!(decodes(&fresh) <= 1, "{} decodes", decodes(&fresh));
        assert_eq!(scanned(&fresh), 0);
        let fresh = Snapshot::open(&s.file).unwrap();
        assert!(!fresh.metrics_candidates(3, 1000).unwrap().0.is_empty());
        assert!(decodes(&fresh) <= 1, "{} decodes", decodes(&fresh));
        assert_eq!(scanned(&fresh), 0);
        // A 5,000-child parent below the plain depth: one page costs at most one chunk.
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
        let (_d, s) = v2_only(&rows, 2, 64);
        for sort in [SortKey::Name, SortKey::Disk] {
            let fresh = Snapshot::open(&s.file).unwrap();
            let page = fresh
                .children_page(1, sort, true, None, false, 50, b"")
                .unwrap();
            assert_eq!(page.len(), 50);
            assert!(decodes(&fresh) <= 1, "{} decodes", decodes(&fresh));
            assert_eq!(scanned(&fresh), 0);
        }
    }

    #[test]
    fn deep_subtree_scan_streams_each_chunk_once() {
        let (_d, s) = v2_only(&balanced121(), 0, 16);
        let fresh = Snapshot::open(&s.file).unwrap();
        assert_eq!(
            fresh.subtree_dirs(0, 4, SortKey::Disk, 1000).unwrap().len(),
            121
        );
        // The root chunk is decoded once (cached by `raw`); the scan reuses it.
        assert_eq!(scanned(&fresh) + decodes(&fresh), 121u64.div_ceil(16));
        assert!(decodes(&fresh) <= 1, "{} decodes", decodes(&fresh));
    }

    #[test]
    fn deep_queries_decode_each_chunk_at_most_once_and_reuse_the_cache() {
        let (_d, s) = v2_only(&balanced121(), 0, 16);
        let total = |s: &Snapshot| decodes(s) + scanned(s);
        let fresh = Snapshot::open(&s.file).unwrap();
        assert_eq!(
            fresh.subtree_dirs(0, 4, SortKey::Name, 1000).unwrap().len(),
            121
        );
        assert!(total(&fresh) <= 8, "{} chunk decodes", total(&fresh));
        let fresh = Snapshot::open(&s.file).unwrap();
        let (used, reclaim, count, _) = fresh.metrics_candidates(4, 1000).unwrap();
        assert_eq!((used.len(), reclaim.len(), count), (121, 121, 121));
        assert!(total(&fresh) <= 8, "{} chunk decodes", total(&fresh));
        // The root chunk is in the LRU: scanning it again streams nothing for it.
        let fresh = Snapshot::open(&s.file).unwrap();
        fresh.dir(0).unwrap();
        let before = (decodes(&fresh), scanned(&fresh));
        fresh.subtree_dirs(0, 4, SortKey::Name, 1000).unwrap();
        assert_eq!(decodes(&fresh), before.0);
        assert_eq!(scanned(&fresh), before.1 + 7);
    }

    #[test]
    fn empty_results_do_no_extra_chunk_work() {
        for plain in [0, 3] {
            let (_d, s) = v2_only(&balanced121(), plain, 16);
            let total = |s: &Snapshot| decodes(s) + scanned(s);
            for limit in [0, 1, 1000] {
                let fresh = Snapshot::open(&s.file).unwrap();
                assert!(fresh.children(40, SortKey::Name, limit).unwrap().is_empty());
                assert!(total(&fresh) <= 1, "children {limit}: {}", total(&fresh));
            }
            // Zero limit runs the same enumeration and checks as limit 1.
            for (root, depth) in [(0, 4), (40, 0)] {
                let work = |limit| {
                    let fresh = Snapshot::open(&s.file).unwrap();
                    let r = fresh.subtree_dirs(root, depth, SortKey::Disk, limit);
                    (r.unwrap().len(), total(&fresh))
                };
                let (n0, w0) = work(0);
                let (_, w1) = work(1);
                assert_eq!(n0, 0);
                assert!(w0 <= w1, "subtree({root},{depth}): {w0} > {w1}");
            }
            // Filtered-out wide queries validate the root's path eagerly
            // (at most the review15 counts), but build no hit paths.
            for (leaf, bound) in [(40, 3), (120, 4)] {
                let fresh = Snapshot::open(&s.file).unwrap();
                assert!(fresh.wide_dirs(leaf, 2, 1).unwrap().is_empty());
                assert!(total(&fresh) <= bound, "wide {leaf}: {}", total(&fresh));
            }
            // An empty child page (leaf, zero limit, nothing matching the
            // filter) validates the parent's path eagerly too: the same bound
            // as a wide query, never more. `children_count` never climbs.
            for (parent, bound) in [(40, 3), (120, 4), (37, 3)] {
                for (limit, filter) in [(8, &b""[..]), (0, b""), (8, b"zz")] {
                    let fresh = Snapshot::open(&s.file).unwrap();
                    let page = fresh
                        .children_page(parent, SortKey::Name, false, None, false, limit, filter)
                        .unwrap();
                    assert!(page.is_empty() || parent == 37);
                    assert!(
                        total(&fresh) <= bound,
                        "page {parent} {limit} {filter:?}: {}",
                        total(&fresh)
                    );
                }
                let fresh = Snapshot::open(&s.file).unwrap();
                fresh.children_count(parent, b"").unwrap();
                assert!(total(&fresh) <= 1, "count {parent}: {}", total(&fresh));
            }
        }
    }

    fn chain_rows(links: u32) -> Vec<DirRow> {
        let mut path = RawPath::new(b"/r".to_vec()).unwrap();
        let mut rows = vec![row(0, None, 0, b"/r", &path, links)];
        for id in 1..=links {
            let name = format!("c{id}").into_bytes();
            path = path.join(&RawName::new(name.clone()).unwrap());
            rows.push(row(id, Some(id - 1), id, &name, &path, links));
        }
        rows
    }
    /// Depth-eight chain of final children `z`, each ancestor preceded by 4,095 leaves.
    fn leafy_chain_rows() -> Vec<DirRow> {
        let mut path = RawPath::new(b"/r".to_vec()).unwrap();
        let end = 8 * 4096;
        let mut rows = vec![row(0, None, 0, b"/r", &path, end)];
        for level in 0..8u32 {
            let parent = level * 4096;
            for i in 0..4095u32 {
                let name = format!("a{i:04}").into_bytes();
                let p = path.join(&RawName::new(name.clone()).unwrap());
                let id = parent + 1 + i;
                rows.push(row(id, Some(parent), level + 1, &name, &p, id));
            }
            path = path.join(&RawName::new(b"z".to_vec()).unwrap());
            let id = parent + 4096;
            rows.push(row(id, Some(parent), level + 1, b"z", &path, end));
        }
        rows
    }

    #[test]
    fn wide_dirs_keeps_the_requested_chunk_across_root_validation() {
        let cases = [
            (chain_rows(32), 32, 1, 33u64),
            (chain_rows(33), 33, 1, 34),
            (leafy_chain_rows(), 32768, 2, 9),
        ];
        for (rows, id, plain, bound) in cases {
            let chunk_dirs = if rows.len() > 100 { 4096 } else { 1 };
            let f = fixture(rows, plain, chunk_dirs);
            let fresh = Snapshot::open(&f.v2.file).unwrap();
            let got = fresh.wide_dirs(id, 0, 1).unwrap();
            assert_eq!(got.len(), 1);
            assert_eq!(
                format!("{got:?}"),
                format!("{:?}", f.v1.wide_dirs(id, 0, 1).unwrap())
            );
            let work = decodes(&fresh) + scanned(&fresh);
            assert!(work <= bound, "id {id}: {work} > {bound}");
        }
    }

    #[test]
    fn zero_limit_keeps_corruption_checks() {
        let copy = |s: &Snapshot, sql: &str| {
            let dir = test_support::private_tempdir().unwrap();
            let file = dir.path().join("t.db");
            std::fs::copy(&s.file, &file).unwrap();
            let conn = rusqlite::Connection::open(&file).unwrap();
            conn.execute_batch(sql).unwrap();
            drop(conn);
            (dir, Snapshot::open(&file).unwrap())
        };
        let (_d, s) = v2_only(&balanced121(), 2, 16);
        for sql in [
            "DELETE FROM shallow_dirs WHERE id=41;",
            "UPDATE shallow_dirs SET own_charged='x' WHERE id=41;",
            "UPDATE shallow_dirs SET depth=2 WHERE id=41;",
        ] {
            let (_k, snap) = copy(&s, sql);
            assert!(
                is_invalid(snap.subtree_dirs(0, 1, SortKey::Name, 0)),
                "{sql}"
            );
            assert!(
                is_invalid(snap.subtree_dirs(0, 1, SortKey::Name, 1)),
                "{sql}"
            );
        }
        let (_d, s) = v2_only(&balanced121(), 0, 16);
        let (_k, snap) = copy(&s, "UPDATE dir_chunks SET data=x'00' WHERE first_id=32;");
        assert!(is_invalid(snap.wide_dirs(0, 0, 0)));
        assert!(is_invalid(snap.subtree_dirs(0, 4, SortKey::Name, 0)));
    }

    #[test]
    fn wide_dirs_always_validates_the_requested_root() {
        type Mutation = fn(&mut [DirRow]);
        let cases: [(&str, u32, Mutation); 3] = [
            ("name a/b", 40, |r| r[40].name = b"a/b".to_vec()),
            ("root name", 0, |r| r[0].name = b"/r\0bad".to_vec()),
            ("parent 0", 40, |r| r[40].parent_id = Some(0)),
        ];
        for plain in [0, 3] {
            for (what, id, mutate) in cases {
                let mut rows = balanced121();
                mutate(&mut rows);
                let (_d, s) = v2_only(&rows, plain, 16);
                for (min, limit) in [(0, 0), (0, 1), (2, 1)] {
                    assert!(
                        is_invalid(s.wide_dirs(id, min, limit)),
                        "{what} plain={plain} wide_dirs({id},{min},{limit})"
                    );
                }
                for limit in [0, 1] {
                    assert!(
                        is_invalid(s.subtree_dirs(id, 0, SortKey::Name, limit)),
                        "{what} plain={plain} subtree_dirs({id},0,{limit})"
                    );
                    if id == 0 {
                        assert!(
                            is_invalid(s.metrics_candidates(0, limit)),
                            "{what} plain={plain} metrics_candidates(0,{limit})"
                        );
                    }
                }
            }
        }
    }
}
