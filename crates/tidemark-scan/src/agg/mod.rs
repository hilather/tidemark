//! Owned by: W2-A
//! Iterative metadata aggregation, hard-link accounting and bounded row streaming.

use tidemark_core::{
    path::{RawName, RawPath},
    scan::{DirId, DirRecord, GoneRecord, Ino, InodeAttrs, NsRecord},
    snapshot::{AnnotationRow, DirRow, ErrorRow, FileRow, LinkInstanceRow, LinkSetRow, SkipRow},
};
/// Root, selection and error-retention settings.
pub struct AggConfig {
    /// Contract field.
    pub root: RawPath,
    /// Contract field.
    pub scan_start: i64,
    /// Contract field.
    pub top_files: usize,
    /// Contract field.
    pub big_file_threshold: u64,
    /// Contract field.
    pub max_notable: usize,
    /// Contract field.
    pub link_instances: bool,
    /// Contract field.
    pub max_stored_errors: usize,
}

/// Metadata-only, bounded scan accumulator. Input errors are deferred to `prepare`.
pub struct Aggregator {
    cfg: AggConfig,
    nodes: Vec<Option<Node>>,
    links: rustc_hash::FxHashMap<Ino, LinkAcc>,
    bulk_links: Vec<LinkAcc>,
    candidates: Vec<FileRow>,
    skipped: Vec<SkipRow>,
    errors: Vec<ErrorRow>,
    gone: Vec<GoneRecord>,
    races: u64,
    failed: bool,
    instances: usize,
}

// Fixed ceilings bound synthetic or corrupt inputs, independently of scanner budgets.
const MAX_DIRS: usize = 4_000_000;
const MAX_ITEMS: usize = 32_000_000;
const NONE: u32 = u32::MAX;

#[derive(Clone, Default)]
struct Totals {
    disk: u64,
    apparent: u64,
    charged: u64,
    reclaim: u64,
    files: u64,
    inodes: u64,
    dirs: u64,
    errors: u64,
    age: [u64; 6],
    newest: i64,
    max_entries: u32,
}
struct Node {
    parent: u32,
    depth: u32,
    name: Box<[u8]>,
    children: Vec<u32>,
    ino: Ino,
    mtime: i64,
    dir_disk: u64,
    dir_apparent: u64,
    projid: Option<u32>,
    entries: u32,
    errors: u32,
    own_files: u64,
    symlinks: u64,
    own_charged: u64,
    own_apparent: u64,
    union: [u64; 3],
    stamp: u64,
    totals: Totals,
}
struct LinkAcc {
    attrs: InodeAttrs,
    instances: smallvec::SmallVec<[(DirId, Option<RawName>); 2]>,
}
fn add(to: &mut u64, n: u64) -> Result<(), AggError> {
    *to = to.checked_add(n).ok_or(AggError::Overflow)?;
    Ok(())
}
fn count(n: usize) -> Result<u32, AggError> {
    u32::try_from(n).map_err(|_| AggError::Overflow)
}
impl Aggregator {
    /// Create an accumulator; caps are validated before preparation.
    pub fn new(cfg: AggConfig) -> Self {
        let failed = cfg.max_notable == 0
            || count(cfg.max_notable).is_err()
            || cfg.max_stored_errors > MAX_ITEMS;
        Self {
            cfg,
            nodes: Vec::new(),
            links: Default::default(),
            bulk_links: Vec::new(),
            candidates: Vec::new(),
            skipped: Vec::new(),
            errors: Vec::new(),
            gone: Vec::new(),
            races: 0,
            failed,
            instances: 0,
        }
    }
    fn node(&mut self, id: DirId) -> Option<&mut Node> {
        let result = self.nodes.get_mut(id.0 as usize).and_then(Option::as_mut);
        if result.is_none() {
            self.failed = true;
        }
        result
    }
    /// Consume one walk record, including its single-link totals and candidates.
    pub fn add_dir_record(&mut self, rec: DirRecord) {
        if self.failed {
            return;
        }
        if (rec.id.0 == 0) != rec.parent.is_none() || (rec.id.0 != 0 && rec.name.is_none()) {
            self.failed = true;
            return;
        }
        let idx = rec.id.0 as usize;
        if idx >= MAX_DIRS || self.skipped.len().saturating_add(rec.skipped.len()) > MAX_ITEMS {
            self.failed = true;
            return;
        }
        if self.nodes.len() <= idx {
            self.nodes.resize_with(idx + 1, || None);
        }
        if self.nodes[idx].is_some() {
            self.failed = true;
            return;
        }
        let errors = match count(rec.errors.len()) {
            Ok(n) => n,
            Err(_) => {
                self.failed = true;
                return;
            }
        };
        let mut t = Totals {
            disk: rec.own.disk,
            apparent: rec.own.apparent,
            charged: rec.own.disk,
            reclaim: rec.own.disk,
            files: rec.own.files,
            inodes: rec.own.files,
            dirs: 1,
            errors: u64::from(errors),
            age: rec.own.age.0,
            newest: if rec.own.files > 0 {
                rec.own.newest_mtime.max(rec.mtime)
            } else {
                rec.mtime
            },
            max_entries: rec.entries,
        };
        let ok = add(&mut t.disk, rec.dir_disk)
            .and_then(|_| add(&mut t.apparent, rec.dir_apparent))
            .and_then(|_| add(&mut t.charged, rec.dir_disk))
            .and_then(|_| add(&mut t.reclaim, rec.dir_disk))
            .and_then(|_| {
                add(
                    &mut t.age[tidemark_core::time::age_bucket(self.cfg.scan_start, rec.mtime)],
                    rec.dir_disk,
                )
            });
        if ok.is_err() {
            self.failed = true;
        }
        self.nodes[idx] = Some(Node {
            parent: rec.parent.map_or(NONE, |p| p.0),
            depth: 0,
            name: rec
                .name
                .map_or_else(|| self.cfg.root.as_bytes().into(), |n| n.as_bytes().into()),
            children: Vec::new(),
            ino: rec.ino,
            mtime: rec.mtime,
            dir_disk: rec.dir_disk,
            dir_apparent: rec.dir_apparent,
            projid: rec.projid,
            entries: rec.entries,
            errors,
            own_files: rec.own.files,
            symlinks: rec.own.symlinks,
            own_charged: t.charged,
            own_apparent: t.apparent,
            union: [0; 3],
            stamp: 0,
            totals: t,
        });
        for f in rec.notable {
            self.candidate(FileRow {
                id: 0,
                dir_id: rec.id.0,
                name: f.name,
                ino: f.ino,
                disk: f.disk,
                apparent: f.apparent,
                mtime: f.mtime,
                nlink: 1,
            });
        }
        for l in rec.links {
            if self.instances >= MAX_ITEMS {
                self.failed = true;
                break;
            }
            self.instances += 1;
            let a = InodeAttrs {
                ino: l.ino,
                kind: l.kind,
                nlink: l.nlink,
                disk: l.disk,
                apparent: l.apparent,
                mtime: l.mtime,
                projid: None,
            };
            let set = self.links.entry(l.ino).or_insert_with(|| LinkAcc {
                attrs: a,
                instances: Default::default(),
            });
            let nlink = set.attrs.nlink.max(a.nlink);
            set.attrs = a;
            set.attrs.nlink = nlink;
            set.instances.push((rec.id, l.name));
        }
        self.skipped
            .extend(rec.skipped.into_iter().map(|s| SkipRow {
                dir_id: rec.id.0,
                name: s.name,
                reason: s.reason,
            }));
        // Retain a deterministic bounded sample: sort/truncate again when writing.
        for e in rec.errors {
            self.errors.push(ErrorRow {
                dir_id: Some(rec.id.0),
                path: e.path,
                op: e.op,
                errno: e.errno,
            });
            if self.errors.len() > self.cfg.max_stored_errors {
                self.trim_errors();
            }
        }
        self.add_races(Some(rec.id), rec.races);
    }
    fn trim_errors(&mut self) {
        self.errors.sort_by(|a, b| {
            a.path
                .as_bytes()
                .cmp(b.path.as_bytes())
                .then((a.op as u8).cmp(&(b.op as u8)))
                .then(a.errno.cmp(&b.errno))
        });
        self.errors.truncate(self.cfg.max_stored_errors);
    }
    fn candidate(&mut self, f: FileRow) {
        if self.candidates.len() >= MAX_ITEMS {
            self.failed = true;
        } else {
            self.candidates.push(f);
        }
    }
    /// Record a queued directory that could not be traversed, preserving id holes.
    pub fn add_gone(&mut self, g: &GoneRecord) {
        if self.gone.len() >= MAX_DIRS {
            self.failed = true;
        } else {
            self.gone.push(g.clone());
        }
    }
    /// Install the namespace portion of a bulk directory.
    pub fn add_skeleton(&mut self, ns: &NsRecord) {
        self.add_dir_record(DirRecord {
            id: ns.id,
            parent: ns.parent,
            name: ns.name.clone(),
            ino: ns.ino,
            dev: ns.dev,
            mtime: i64::MIN,
            dir_disk: 0,
            dir_apparent: 0,
            projid: None,
            entries: ns.entries,
            own: Default::default(),
            links: Vec::new(),
            notable: Vec::new(),
            skipped: ns.skipped.clone(),
            errors: ns.errors.clone(),
            races: ns.races,
        });
    }
    /// Set a bulk directory's self-reference inode attributes.
    pub fn set_dir_inode(&mut self, dir: DirId, a: &InodeAttrs) {
        let bucket = tidemark_core::time::age_bucket(self.cfg.scan_start, a.mtime);
        if let Some(n) = self.node(dir) {
            if n.dir_disk != 0 || n.dir_apparent != 0 {
                self.failed = true;
                return;
            }
            n.ino = a.ino;
            n.mtime = a.mtime;
            n.dir_disk = a.disk;
            n.dir_apparent = a.apparent;
            n.projid = a.projid;
            n.totals.newest = n.totals.newest.max(a.mtime);
            let ok = add(&mut n.totals.disk, a.disk)
                .and_then(|_| add(&mut n.totals.apparent, a.apparent))
                .and_then(|_| add(&mut n.totals.charged, a.disk))
                .and_then(|_| add(&mut n.totals.reclaim, a.disk))
                .and_then(|_| add(&mut n.totals.age[bucket], a.disk))
                .and_then(|_| add(&mut n.own_charged, a.disk))
                .and_then(|_| add(&mut n.own_apparent, a.apparent));
            if ok.is_err() {
                self.failed = true;
            }
        }
    }
    /// Add a bulk single-link non-directory inode.
    pub fn add_single(&mut self, dir: DirId, a: &InodeAttrs) {
        let bucket = tidemark_core::time::age_bucket(self.cfg.scan_start, a.mtime);
        if let Some(n) = self.node(dir) {
            let ok = add(&mut n.totals.disk, a.disk)
                .and_then(|_| add(&mut n.totals.apparent, a.apparent))
                .and_then(|_| add(&mut n.totals.charged, a.disk))
                .and_then(|_| add(&mut n.totals.reclaim, a.disk))
                .and_then(|_| add(&mut n.totals.age[bucket], a.disk))
                .and_then(|_| add(&mut n.own_charged, a.disk))
                .and_then(|_| add(&mut n.own_apparent, a.apparent))
                .and_then(|_| add(&mut n.own_files, 1))
                .and_then(|_| add(&mut n.totals.files, 1))
                .and_then(|_| add(&mut n.totals.inodes, 1));
            let symlink_error = a.kind == tidemark_core::scan::FileKind::Symlink
                && add(&mut n.symlinks, 1).is_err();
            if ok.is_err() || symlink_error {
                self.failed = true;
            } else {
                n.totals.newest = n.totals.newest.max(a.mtime);
            }
        }
        self.candidate(FileRow {
            id: 0,
            dir_id: dir.0,
            name: None,
            ino: a.ino,
            disk: a.disk,
            apparent: a.apparent,
            mtime: a.mtime,
            nlink: 1,
        });
    }
    /// Add one complete bulk join group; multiplicity of directories is preserved.
    pub fn add_link_set(&mut self, a: &InodeAttrs, dirs: &[DirId]) {
        if dirs.is_empty() || self.instances.saturating_add(dirs.len()) > MAX_ITEMS {
            self.failed = true;
            return;
        }
        self.instances += dirs.len();
        self.bulk_links.push(LinkAcc {
            attrs: *a,
            instances: dirs.iter().map(|d| (*d, None)).collect(),
        });
    }
    /// Add race counts; unattributed races still contribute to the summary.
    pub fn add_races(&mut self, _dir: Option<DirId>, n: u32) {
        if add(&mut self.races, u64::from(n)).is_err() {
            self.failed = true;
        }
    }
    /// Resolve links, compute iterative rollups and assign deterministic preorder ids.
    pub fn prepare(self) -> Result<Prepared, AggError> {
        prepare(self)
    }
}

/// Final metrics and row selections, ready for optional names and annotations.
pub struct Prepared {
    cfg: AggConfig,
    nodes: Vec<Option<Node>>,
    order: Vec<u32>,
    ids: Vec<u32>,
    ends: Vec<u32>,
    files: Vec<FileRow>,
    links: Vec<LinkSetRow>,
    instances: Vec<LinkInstanceRow>,
    skipped: Vec<SkipRow>,
    errors: Vec<ErrorRow>,
    annotations: Vec<AnnotationRow>,
    summary: AggSummary,
    ignored_annotations: u64,
    invalid_annotations: bool,
}
impl Prepared {
    /// Return exact whole-root totals.
    pub fn summary(&self) -> AggSummary {
        self.summary.clone()
    }
    fn path(&self, old: u32) -> RawPath {
        let mut chain = Vec::new();
        let mut v = old;
        while v != NONE {
            chain.push(v);
            v = self.nodes[v as usize].as_ref().map_or(NONE, |n| n.parent);
        }
        let mut bytes = self.cfg.root.as_bytes().to_vec();
        for id in chain.into_iter().rev().skip(1) {
            if let Some(n) = &self.nodes[id as usize] {
                if bytes.last() != Some(&b'/') {
                    bytes.push(b'/');
                }
                bytes.extend_from_slice(&n.name);
            }
        }
        // Infallible: prepare validates the tree, root is an absolute RawPath,
        // and every non-root component came from a validated RawName.
        #[allow(clippy::expect_used)]
        RawPath::new(bytes).expect("validated absolute path components")
    }
    fn lookup(&self, path: &RawPath) -> Option<u32> {
        if !path.starts_with(&self.cfg.root) {
            return None;
        }
        let suffix = &path.as_bytes()[self.cfg.root.as_bytes().len()..];
        let mut old = 0;
        for part in suffix.split(|&b| b == b'/').filter(|p| !p.is_empty()) {
            let node = self.nodes[old as usize].as_ref()?;
            let index = node
                .children
                .binary_search_by(|child| {
                    self.nodes[*child as usize]
                        .as_ref()
                        .map_or(&[][..], |n| n.name.as_ref())
                        .cmp(part)
                })
                .ok()?;
            old = node.children[index];
        }
        Some(old)
    }
    /// Return selected representatives whose bulk names are absent.
    pub fn needs_names(&self) -> Vec<(RawPath, Ino)> {
        self.files
            .iter()
            .filter(|f| f.name.is_none())
            .map(|f| (self.path(self.order[f.dir_id as usize]), f.ino))
            .collect()
    }
    /// Resolve names, choosing the smallest raw name for each representative.
    pub fn set_names(&mut self, names: Vec<(RawPath, Ino, RawName)>) {
        // Only selected representatives occupy the lookup table, regardless of
        // how many unrelated names the caller supplies.
        let mut map = std::collections::BTreeMap::new();
        for f in &self.files {
            map.insert(
                (self.path(self.order[f.dir_id as usize]), f.ino),
                f.name.clone(),
            );
        }
        for (path, ino, name) in names {
            if let Some(old) = map.get_mut(&(path, ino)) {
                if old
                    .as_ref()
                    .is_none_or(|old| name.as_bytes() < old.as_bytes())
                {
                    *old = Some(name);
                }
            }
        }
        for i in 0..self.files.len() {
            let path = self.path(self.order[self.files[i].dir_id as usize]);
            if let Some(name) = map.get(&(path, self.files[i].ino)) {
                self.files[i].name = name.clone();
            }
        }
    }
    /// Attach a label to a scanned path; unknown paths are ignored.
    pub fn annotate(&mut self, path: &RawPath, key: &str, value: &str) {
        if let Some(old) = self.lookup(path) {
            if self.annotations.len() >= MAX_ITEMS {
                self.invalid_annotations = true;
                return;
            }
            self.annotations.push(AnnotationRow {
                dir_id: self.ids[old as usize],
                key: key.into(),
                value: value.into(),
            });
        } else if let Some(n) = self.ignored_annotations.checked_add(1) {
            self.ignored_annotations = n;
        } else {
            self.invalid_annotations = true;
        }
    }
    /// Stream rows in bounded batches; propagate sink failures without further writes.
    pub fn write(mut self, sink: &mut dyn RowSink) -> Result<AggSummary, AggError> {
        if self.invalid_annotations {
            return Err(AggError::Capacity);
        }
        tracing::debug!(
            ignored_annotations = self.ignored_annotations,
            "aggregation annotations"
        );
        let mut rows = Vec::with_capacity(10_000);
        let mut row_bytes = 0usize;
        let mut bytes = self.cfg.root.as_bytes().to_vec();
        let mut stack: Vec<(u32, usize)> = Vec::new();
        for &old in &self.order {
            let n = self.nodes[old as usize].as_ref().ok_or(AggError::Tree)?;
            while stack.last().is_some_and(|(depth, _)| *depth >= n.depth) {
                if let Some((_, len)) = stack.pop() {
                    bytes.truncate(len);
                }
            }
            let previous = bytes.len();
            if n.depth != 0 {
                if bytes.last() != Some(&b'/') {
                    bytes.push(b'/');
                }
                bytes.extend_from_slice(&n.name);
            }
            stack.push((n.depth, previous));
            let path = RawPath::new(bytes.clone()).map_err(|_| AggError::Tree)?;
            let t = &n.totals;
            row_bytes = row_bytes
                .checked_add(bytes.len() * 2)
                .ok_or(AggError::Overflow)?;
            rows.push(DirRow {
                id: self.ids[old as usize],
                parent_id: (n.parent != NONE).then(|| self.ids[n.parent as usize]),
                subtree_end: self.ends[old as usize],
                depth: n.depth,
                name: n.name.to_vec(),
                path_hash: path.hash64(),
                path,
                ino: n.ino,
                mtime: n.mtime,
                dir_disk: n.dir_disk,
                dir_apparent: n.dir_apparent,
                projid: n.projid,
                entries: n.entries,
                errors: n.errors,
                own_files: n.own_files,
                own_symlinks: n.symlinks,
                own_charged: n.own_charged,
                own_apparent: n.own_apparent,
                t_dirs: t.dirs,
                t_files: t.files,
                t_inodes: t.inodes,
                t_disk: t.disk,
                t_apparent: t.apparent,
                t_charged: t.charged,
                t_reclaimable: t.reclaim,
                t_errors: t.errors,
                age0: t.age[0],
                age1: t.age[1],
                age2: t.age[2],
                age3: t.age[3],
                age4: t.age[4],
                age5: t.age[5],
                newest_mtime: t.newest,
                max_entries: t.max_entries,
            });
            if rows.len() == 10_000 || row_bytes >= 4 * 1024 * 1024 {
                sink.dirs(&rows)?;
                rows.clear();
                row_bytes = 0;
            }
        }
        if !rows.is_empty() {
            sink.dirs(&rows)?;
        }
        for rows in self.files.chunks(10_000) {
            sink.files(rows)?;
        }
        for rows in self.links.chunks(10_000) {
            sink.link_sets(rows)?;
        }
        for rows in self.instances.chunks(10_000) {
            sink.link_instances(rows)?;
        }
        for rows in self.skipped.chunks(10_000) {
            sink.skipped(rows)?;
        }
        for rows in self.errors.chunks(10_000) {
            sink.errors(rows)?;
        }
        self.annotations
            .sort_by(|a, b| (a.dir_id, &a.key, &a.value).cmp(&(b.dir_id, &b.key, &b.value)));
        for rows in self.annotations.chunks(10_000) {
            sink.annotations(rows)?;
        }
        Ok(self.summary)
    }
}

/// Receives bounded batches of snapshot rows.
pub trait RowSink {
    /// Accept a batch of rows in deterministic order.
    fn dirs(&mut self, rows: &[DirRow]) -> Result<(), SinkError>;
    /// Accept a batch of rows in deterministic order.
    fn files(&mut self, rows: &[FileRow]) -> Result<(), SinkError>;
    /// Accept a batch of rows in deterministic order.
    fn link_sets(&mut self, rows: &[LinkSetRow]) -> Result<(), SinkError>;
    /// Accept a batch of rows in deterministic order.
    fn link_instances(&mut self, rows: &[LinkInstanceRow]) -> Result<(), SinkError>;
    /// Accept a batch of rows in deterministic order.
    fn skipped(&mut self, rows: &[SkipRow]) -> Result<(), SinkError>;
    /// Accept a batch of rows in deterministic order.
    fn errors(&mut self, rows: &[ErrorRow]) -> Result<(), SinkError>;
    /// Accept a batch of rows in deterministic order.
    fn annotations(&mut self, rows: &[AnnotationRow]) -> Result<(), SinkError>;
}
#[derive(Default)]
/// In-memory row sink for tests and live reports.
pub struct VecSink {
    /// Contract field.
    pub dirs: Vec<DirRow>,
    /// Contract field.
    pub files: Vec<FileRow>,
    /// Contract field.
    pub link_sets: Vec<LinkSetRow>,
    /// Contract field.
    pub link_instances: Vec<LinkInstanceRow>,
    /// Contract field.
    pub skipped: Vec<SkipRow>,
    /// Contract field.
    pub errors: Vec<ErrorRow>,
    /// Contract field.
    pub annotations: Vec<AnnotationRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
/// Whole-root accounting totals.
pub struct AggSummary {
    /// Contract field.
    pub dirs: u64,
    /// Contract field.
    pub files: u64,
    /// Contract field.
    pub inodes: u64,
    /// Contract field.
    pub t_disk: u64,
    /// Contract field.
    pub t_apparent: u64,
    /// Contract field.
    pub t_reclaimable: u64,
    /// Contract field.
    pub link_sets: u64,
    /// Contract field.
    pub incomplete_link_sets: u64,
    /// Contract field.
    pub errors: u64,
    /// Contract field.
    pub races: u64,
    /// Contract field.
    pub skipped: u64,
    /// Contract field.
    pub degraded: bool,
}

mod lca;
mod linkset;
mod notable;
mod rollup;

/// Aggregation failures.
#[derive(Debug, thiserror::Error)]
pub enum AggError {
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// Arithmetic or identifier conversion exceeded its contract width.
    #[error("aggregation arithmetic overflow")]
    Overflow,
    /// Input exceeded a documented memory ceiling or was invalid.
    #[error("aggregation input invalid or exceeds capacity")]
    Capacity,
    /// Namespace is not a connected tree rooted at directory zero.
    #[error("invalid directory tree")]
    Tree,
    /// A row sink refused a batch.
    #[error(transparent)]
    Sink(#[from] SinkError),
}

/// Row sink failures.
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    /// Storage failure.
    #[error(transparent)]
    Store(#[from] tidemark_store::StoreError),
    /// Other sink failure.
    #[error("{0}")]
    Other(String),
}
impl RowSink for VecSink {
    fn dirs(&mut self, rows: &[DirRow]) -> Result<(), SinkError> {
        self.dirs.extend_from_slice(rows);
        Ok(())
    }
    fn files(&mut self, rows: &[FileRow]) -> Result<(), SinkError> {
        self.files.extend_from_slice(rows);
        Ok(())
    }
    fn link_sets(&mut self, rows: &[LinkSetRow]) -> Result<(), SinkError> {
        self.link_sets.extend_from_slice(rows);
        Ok(())
    }
    fn link_instances(&mut self, rows: &[LinkInstanceRow]) -> Result<(), SinkError> {
        self.link_instances.extend_from_slice(rows);
        Ok(())
    }
    fn skipped(&mut self, rows: &[SkipRow]) -> Result<(), SinkError> {
        self.skipped.extend_from_slice(rows);
        Ok(())
    }
    fn errors(&mut self, rows: &[ErrorRow]) -> Result<(), SinkError> {
        self.errors.extend_from_slice(rows);
        Ok(())
    }
    fn annotations(&mut self, rows: &[AnnotationRow]) -> Result<(), SinkError> {
        self.annotations.extend_from_slice(rows);
        Ok(())
    }
}

use rollup::prepare;

#[cfg(test)]
mod measurements {
    use super::*;
    #[test]
    fn storage_layout() {
        eprintln!(
            "Node={} Option<Node>={} Totals={} LinkAcc={} instance={} FileRow={}",
            std::mem::size_of::<Node>(),
            std::mem::size_of::<Option<Node>>(),
            std::mem::size_of::<Totals>(),
            std::mem::size_of::<LinkAcc>(),
            std::mem::size_of::<(DirId, Option<RawName>)>(),
            std::mem::size_of::<FileRow>()
        );
        // Layout ceiling, including inline union and additive accumulators.
        assert!(std::mem::size_of::<Node>() <= 400);
        assert!(std::mem::size_of::<LinkAcc>() <= 160);
    }
}
