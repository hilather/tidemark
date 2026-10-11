# 05 — Storage: snapshots, catalog, time series

Two kinds of SQLite database, deliberately separate:

| Store | Lifetime | Writer | Pattern |
|-------|----------|--------|---------|
| `snapshots/<root-slug>/<scan_id>.db` | Immutable after the scan finishes; deleted by retention | The scan that created it | Bulk load once, then read-only (`immutable=1`) |
| `timeseries.db` | Long-lived | Daemon (single writer thread); CLI writes only when the daemon is absent | Small appends, WAL mode |

Why one file per snapshot: atomic creation by rename, trivial retention (delete a file), no vacuum churn, easy to copy one snapshot off-box for analysis, and diffs are just `ATTACH`.

---

## 1. Snapshot schema (`PRAGMA user_version = 1`)

```sql
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- Required keys: schema_version, tool_version, scan_id, root (hex of raw bytes), root_display,
-- mode (walk|bulk), mode_reason, status (complete|partial), degraded (0|1), exclude (JSON list), link_instances (all|none), started_at, finished_at (unix s),
-- host, kernel, os_release, fs_uuid, fs_dev, fs_blocksize, threads, throttle_profile,
-- dirs, files, inodes, errors, races, skipped, noatime_fallbacks, phase_ms (JSON), gate_stats (JSON)

CREATE TABLE dirs (
  id             INTEGER PRIMARY KEY,   -- preorder index; root = 0
  parent_id      INTEGER,               -- NULL for root
  subtree_end    INTEGER NOT NULL,      -- last preorder id in this subtree (inclusive)
  depth          INTEGER NOT NULL,      -- root = 0
  name           BLOB    NOT NULL,      -- last component; root row: full root path
  path           BLOB    NOT NULL,      -- full absolute path (raw bytes)
  path_hash      INTEGER NOT NULL,      -- xxh3_64(path) reinterpreted as i64
  ino            INTEGER NOT NULL,
  mtime          INTEGER NOT NULL,
  dir_disk       INTEGER NOT NULL,      -- this directory inode's own allocated bytes
  dir_apparent   INTEGER NOT NULL,      -- this directory inode's st_size
  projid         INTEGER,               -- bulk mode only
  entries        INTEGER NOT NULL,      -- direct entries
  errors         INTEGER NOT NULL,      -- errors in this directory itself
  own_files      INTEGER NOT NULL,      -- direct non-directory entries (link instances)
  own_symlinks   INTEGER NOT NULL,
  own_charged    INTEGER NOT NULL,      -- dir_disk + single-found inodes here + link sets whose LCA is here (03 §4)
  own_apparent   INTEGER NOT NULL,      -- same composition using apparent sizes (dir_apparent + …)
  t_dirs         INTEGER NOT NULL,
  t_files        INTEGER NOT NULL,
  t_inodes       INTEGER NOT NULL,
  t_disk         INTEGER NOT NULL,
  t_apparent     INTEGER NOT NULL,
  t_charged      INTEGER NOT NULL,
  t_reclaimable  INTEGER NOT NULL,
  t_errors       INTEGER NOT NULL,
  age0 INTEGER NOT NULL, age1 INTEGER NOT NULL, age2 INTEGER NOT NULL,
  age3 INTEGER NOT NULL, age4 INTEGER NOT NULL, age5 INTEGER NOT NULL,
  newest_mtime   INTEGER NOT NULL,
  max_entries    INTEGER NOT NULL
);

CREATE TABLE files (                    -- notable single-link files, plus one representative per notable link set
  id       INTEGER PRIMARY KEY,          -- zero-based notable-file row identifier (u32)
  dir_id   INTEGER NOT NULL,              -- representative: the link instance with the smallest (dir_id, name)
  name     BLOB,                        -- NULL if bulk-mode name resolution lost a race
  ino      INTEGER NOT NULL,
  disk     INTEGER NOT NULL,
  apparent INTEGER NOT NULL,
  mtime    INTEGER NOT NULL,
  nlink    INTEGER NOT NULL
);

CREATE TABLE link_sets (
  ino         INTEGER PRIMARY KEY,
  nlink       INTEGER NOT NULL,
  found       INTEGER NOT NULL,
  complete    INTEGER NOT NULL,         -- found >= nlink
  disk        INTEGER NOT NULL,
  apparent    INTEGER NOT NULL,
  mtime       INTEGER NOT NULL,
  lca_dir_id  INTEGER NOT NULL
);
CREATE TABLE link_instances (
  ino    INTEGER NOT NULL,
  dir_id INTEGER NOT NULL,
  name   BLOB                           -- NULL in bulk mode (resolved live on demand)
);

CREATE TABLE skipped     (dir_id INTEGER NOT NULL, name BLOB NOT NULL, reason TEXT NOT NULL);   -- mount|other_fs|excluded
CREATE TABLE errors      (dir_id INTEGER, path BLOB NOT NULL, op TEXT NOT NULL, errno INTEGER NOT NULL);
CREATE TABLE annotations (dir_id INTEGER NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL);  -- generic key/value labels

-- Created AFTER bulk load (see §2):
CREATE UNIQUE INDEX dirs_path      ON dirs(path_hash, path);
CREATE INDEX        dirs_parent    ON dirs(parent_id);
CREATE INDEX        files_disk     ON files(disk DESC);
CREATE INDEX        files_dir      ON files(dir_id);
CREATE INDEX        li_ino         ON link_instances(ino);
CREATE INDEX        li_dir         ON link_instances(dir_id);
CREATE INDEX        ls_lca         ON link_sets(lca_dir_id);
```

Notes:

- **Subtree queries are range scans.** Directory *d*'s subtree is `id BETWEEN d.id AND d.subtree_end`. For example, "largest files under X" is `files JOIN dirs ON files.dir_id = dirs.id WHERE dirs.id BETWEEN ? AND ?`, which needs no recursion.
- `path_hash` makes cross-snapshot joins cheap; `path` equality guards against collisions.
- Raw bytes everywhere (`BLOB`). Never store lossy UTF-8.
- Expected size: about 150–250 bytes per directory row, plus files and links. Two million directories with half a million link sets should be in the low single-digit GB range at most. W7-B measures it; `link_instances` can be disabled with `scan.link_instances = "none"`.

### Rust row and metadata contracts (SCR-001)

Directory ids/depth, direct entry/error counts, link counts and project ids are `u32`; inode/device ids, sizes and subtree counters are `u64`; Unix seconds/path hashes are `i64`; errno is `i32`. SQL nullability maps to `Option`. `files.id` is a zero-based `u32` notable-file row identifier, with checked assignment and `1 <= max_notable <= u32::MAX`. `DirRow.name` is `Vec<u8>` (root stores its full path); other names are `RawName`, paths `RawPath`. `LinkInstanceRow.name` is `None` for bulk snapshots by design, resolved live on demand.

`SnapshotMeta` has every required key above in documented order: schema_version/threads `u32`; counts/sizes/devices `u64`; timestamps `i64`; scan_id `ScanId`; root `RawPath`; mode `ScanMode`; status `SnapshotStatus`; degraded `bool`; exclude `Vec<String>`; link_instances `LinkInstances`; other descriptive scalars `String` (fs_uuid empty if unknown). `phase_ms` is deterministic `BTreeMap<String, u64>`. `gate_stats` is `tidemark_core::scan::GateStats { mean_rate: f64, min_rate: f64, throttled_ms: u64, decreases: u64, increases: u64 }`.

All SQLite readers/writers use checked integer conversions: reject negative unsigned values, out-of-range widths and `u64` values not representable by SQLite's signed integer; never wrap or truncate.

## 2. Write path (`SnapshotWriter`, W1-E)

1. Create `<scan_id>.db.tmp` (mode 0600, owner root:root; retained after rename to `.db`). Set `PRAGMA page_size=32768` before creating any table, then `journal_mode=OFF`, `synchronous=OFF`, `locking_mode=EXCLUSIVE`, `temp_store=MEMORY`, `cache_size=-262144` (256 MiB).
2. Create tables **without** secondary indexes.
3. Insert rows with prepared statements in large transactions (about 50,000 rows each). `dirs` rows arrive in ascending `id` order (preorder).
4. `finish()`: create indexes, run `ANALYZE`, write the final `meta`, close, `fsync` the file, publish without replacement to `<scan_id>.db` (refuse an existing id and retry with a fresh `ScanId`), then `fsync` the directory.
5. On error or drop without `finish()`, delete the `.tmp`.

Durability during the build is pointless (a crash discards the scan), so the expensive journaling settings are off. Durability of the finished file comes from the final `fsync` and rename.

## 3. Read path (`Snapshot`, W1-E)

Open with URI `file:<path>?immutable=1&mode=ro`, so SQLite takes no locks and does no change detection. The file never changes after rename.

```rust
impl Snapshot {
    pub fn open(path: &Path) -> Result<Snapshot>;
    pub fn meta(&self) -> &SnapshotMeta;
    pub fn root(&self) -> Result<DirRow>;
    pub fn dir(&self, id: u32) -> Result<DirRow>;
    pub fn dir_by_path(&self, path: &RawPath) -> Result<Option<DirRow>>;
    pub fn children(&self, id: u32, sort: SortKey, limit: usize) -> Result<Vec<DirRow>>;
    pub fn subtree_dirs(&self, id: u32, max_depth_rel: u32, sort: SortKey, limit: usize) -> Result<Vec<DirRow>>;
    pub fn subtree_files(&self, id: u32, filter: &FileFilter, limit: usize) -> Result<Vec<FileHit>>;
    pub fn wide_dirs(&self, id: u32, min_entries: u32, limit: usize) -> Result<Vec<DirRow>>;
    pub fn link_sets_under(&self, id: u32, filter: &LinkFilter, limit: usize) -> Result<Vec<LinkSetHit>>;
    pub fn link_instances(&self, ino: u64) -> Result<Vec<LinkInstanceRow>>;
    pub fn skipped_under(&self, id: u32) -> Result<Vec<SkipRow>>;
    pub fn errors_under(&self, id: u32, limit: usize) -> Result<Vec<ErrorRow>>;
    pub fn annotations(&self, id: u32) -> Result<Vec<(String, String)>>;
}
pub enum SortKey { Disk, Apparent, Reclaimable, Charged, Files, Inodes, Newest, Name }
```

`FileFilter { min_size: Option<u64>, older_than: Option<Duration>, newer_than: Option<Duration> }` measures ages relative to snapshot `started_at`. `LinkFilter { incomplete: bool, boundary: Vec<String>, boundary_depth: Option<u32> }` rejects simultaneous boundary globs and boundary depth. Boundary globs match complete raw directory path bytes, with `*` confined to a component. A matching directory defines a subtree region; the deepest matching ancestor wins, and unmatched instances share an outside region. Select sets crossing distinct regions. Region storage and pattern count use the existing 1,000,000-row cap; exceeding it errors rather than truncates (W1-E fix). `FileHit { row: FileRow, path: Option<RawPath> }` preserves unresolved bulk-race paths as `None`; render them honestly. `LinkSetHit { row: LinkSetRow, lca_path: RawPath, instance_count: u64 }` counts stored instance rows, not discovered `row.found` (especially with `link_instances=none`). Readers enforce the checked conversions in §1. `SortKey` stays single-snapshot; CLI growth sorting uses two snapshots (`08` §1.2).

## 4. Catalog

- **Source of truth:** the `scans` table in `timeseries.db` (§6).
- **Rebuild:** when the database is missing or healthy, `tidemark snapshots rebuild-catalog` opens it normally and rebuilds from valid `snapshots/*/*.db` metadata.
- **Open preflight:** Every open of an existing `timeseries.db`, including `Catalog::open` and `TimeSeries::open`, first reads its first 16 bytes without SQLite. If the file exists, is not empty, and does not start with `SQLite format 3\0`, return the corruption error without opening it with SQLite, preserving the DB/WAL/SHM bytes. Missing or empty files are created as today. Valid-header files open normally, with SQLite’s own WAL recovery. Reading Tidemark’s own state file is allowed; the metadata-only rule applies to scanned files.
- **Corruption recovery:** `Catalog::open` remains non-destructive. After an open reports corruption, callers stop writers and close all catalog/sample connections, then call `tidemark_store::catalog::recover_catalog(state_dir)` to preserve the database and WAL/SHM sidecars by rename, create a fresh database and rebuild the catalog from valid snapshot files. The daemon at startup and `snapshots rebuild-catalog` use this path only on reported corruption and log the preserved paths. Per `docs/handoff/W1-E-fix.md`, preservation uses an exclusively created private `timeseries-recovery-N` directory inside `state_dir`, with at most 10,000 namespace attempts and directory syncs; no artifacts are deleted. `Catalog::recovery_files() -> &[PathBuf]` reports the preserved paths. Historical samples are unavailable in the new database but remain in those artifacts.
- **"Latest"** is the newest `status = complete` snapshot for that root. Partial snapshots must be selected explicitly.

## 5. Diff and explain (W3-A)

### 5.1 Directory diff

```sql
ATTACH DATABASE 'file:<old>?immutable=1&mode=ro' AS o;
-- :n_lo/:n_hi = subtree range of the target path in the new snapshot (main)
-- :o_lo/:o_hi = subtree range of the same path in the old snapshot (if it exists)
-- :max_depth is ABSOLUTE: target.depth + the requested relative depth.
-- SQLite only allows output-column terms in ORDER BY of a compound SELECT, so wrap it.
SELECT * FROM (
SELECT n.path, n.depth, n.t_disk AS new_disk, o2.t_disk AS old_disk,
       n.t_disk - COALESCE(o2.t_disk, 0) AS delta, n.t_reclaimable AS new_reclaim, o2.t_reclaimable AS old_reclaim
FROM main.dirs n
LEFT JOIN o.dirs o2 ON o2.path_hash = n.path_hash AND o2.path = n.path
WHERE n.id BETWEEN :n_lo AND :n_hi AND n.depth <= :max_depth
UNION ALL
SELECT o2.path, o2.depth, NULL, o2.t_disk, -o2.t_disk, NULL, o2.t_reclaimable
FROM o.dirs o2
WHERE o2.id BETWEEN :o_lo AND :o_hi AND o2.depth <= :max_depth
  AND NOT EXISTS (SELECT 1 FROM main.dirs n WHERE n.path_hash = o2.path_hash AND n.path = o2.path)
)
ORDER BY ABS(delta) DESC, path
LIMIT :limit;
```

Output classes: `grown`, `shrunk`, `new`, `removed`. Report the delta in `t_disk` (default) or in `t_reclaimable`/`t_files` (`--metric`).

### 5.2 Explain: the growth path

Start at the target directory with delta Δ > 0. Repeat:

1. Compute the children's deltas (one join over `parent_id`).
2. Let *c* be the child with the largest positive delta.
3. If `Δc ≥ explain_ratio × Δ` (default 0.5) and `Δc ≥ explain_min` (default 1 GiB), descend into *c* (Δ = Δc).
4. Otherwise stop, and print the top contributors at this level, including the "own files" delta (`own_charged`).

Example output:

```
/data                 +121.4 GiB
└─ reports            +118.0 GiB   (97%)
   └─ acme            +117.2 GiB   (99%)
      └─ 2026-10      +116.9 GiB  (100%)  → growth is spread across 312 subdirectories; top 5:
         raw-0412  +18.2 GiB · raw-0411 +17.9 GiB · raw-0410 +17.7 GiB · …
```

The same descent works on `dir_series` history (growth between any two scans), so `now` and `diff` share the explanation style.

## 6. Time-series database (`timeseries.db`, `PRAGMA user_version = 1`)

`journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`. One writer thread in the daemon; readers are the CLI and API handlers.

```sql
CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE scans (
  scan_id TEXT PRIMARY KEY, root BLOB NOT NULL, root_slug TEXT NOT NULL, mode TEXT NOT NULL,
  started_at INTEGER NOT NULL, finished_at INTEGER, status TEXT NOT NULL,   -- running|complete|partial|failed
  degraded INTEGER NOT NULL DEFAULT 0, file TEXT, t_disk INTEGER, t_inodes INTEGER, t_dirs INTEGER,
  errors INTEGER, duration_ms INTEGER, tool_version TEXT NOT NULL);
CREATE INDEX scans_root_time ON scans(root_slug, started_at);

-- res: 0 = raw (sample interval), 1 = 5-minute, 2 = 1-hour. Gauges keep the last value in the bucket.
CREATE TABLE fs_samples (mount TEXT NOT NULL, res INTEGER NOT NULL, ts INTEGER NOT NULL,
  size INTEGER NOT NULL, used INTEGER NOT NULL, avail INTEGER NOT NULL,
  inodes INTEGER NOT NULL, inodes_used INTEGER NOT NULL,
  PRIMARY KEY (mount, res, ts)) WITHOUT ROWID;

CREATE TABLE lvm_samples (vg TEXT NOT NULL, lv TEXT NOT NULL, res INTEGER NOT NULL, ts INTEGER NOT NULL,
  lv_size INTEGER NOT NULL, vg_free INTEGER NOT NULL,
  data_pct REAL, meta_pct REAL,                                   -- thin pools / thin LVs only
  PRIMARY KEY (vg, lv, res, ts)) WITHOUT ROWID;

CREATE TABLE dir_series (root_slug TEXT NOT NULL, path_hash INTEGER NOT NULL, ts INTEGER NOT NULL,
  scan_id TEXT NOT NULL, path BLOB NOT NULL,
  t_disk INTEGER NOT NULL, t_reclaimable INTEGER NOT NULL, t_files INTEGER NOT NULL,
  PRIMARY KEY (root_slug, path_hash, ts)) WITHOUT ROWID;          -- dirs with depth <= series_depth, max 20,000 rows per scan (largest first)

CREATE TABLE hot_minutes (mount TEXT NOT NULL, scope TEXT NOT NULL, key BLOB NOT NULL, ts INTEGER NOT NULL,
  events INTEGER NOT NULL, files INTEGER NOT NULL, bytes_delta INTEGER NOT NULL, lossy INTEGER NOT NULL,
  PRIMARY KEY (mount, scope, key, ts)) WITHOUT ROWID;              -- scope: dir|unit|comm; top-K per minute only

CREATE TABLE phantom_samples (ts INTEGER NOT NULL, mount TEXT NOT NULL, pid INTEGER NOT NULL, comm TEXT NOT NULL, unit TEXT,
  path BLOB NOT NULL, dev INTEGER NOT NULL, ino INTEGER NOT NULL, bytes INTEGER NOT NULL);
CREATE TABLE writer_samples  (ts INTEGER NOT NULL, mount TEXT NOT NULL, pid INTEGER NOT NULL, comm TEXT NOT NULL, unit TEXT,
  path BLOB NOT NULL, dev INTEGER NOT NULL, ino INTEGER NOT NULL, size INTEGER NOT NULL, disk INTEGER NOT NULL);
CREATE INDEX phantom_ts ON phantom_samples(ts);
CREATE INDEX writer_ts  ON writer_samples(ts);

CREATE TABLE alerts (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, rule TEXT NOT NULL, subject TEXT NOT NULL,
  state TEXT NOT NULL, value REAL, message TEXT NOT NULL);         -- state: firing|resolved|renotify
```

### 6.1 Retention and downsampling (`compact()`, hourly)

| Data | Raw | 5-min | 1-hour |
|------|-----|-------|--------|
| fs_samples, lvm_samples | 48 h | 35 d | 400 d |
| hot_minutes | 7 d | — | — |
| phantom_samples, writer_samples | 7 d | — | — |
| dir_series | one row per scan, kept 400 d | — | — |
| alerts | 400 d | — | — |

Downsampling inserts bucket rows (last value per bucket) with `INSERT OR IGNORE`, then deletes expired raw rows. Each step is idempotent, so a crash mid-compaction is harmless.

## 7. Snapshot retention

Configured per root:

```toml
[retention.snapshots]
recent = 4      # keep the 4 newest complete snapshots
daily  = 14     # plus the newest snapshot of each of the last 14 days
weekly = 8      # plus the newest snapshot of each of the last 8 ISO weeks
```

Retention always keeps each root's newest complete snapshot, even with `recent = 0`, expired calendar windows or an all-zero policy.

Partial and failed snapshots are deleted after 24 h. `tidemark snapshots prune --dry-run` shows what would go.

## 8. Space guard (NFR-7)

Before starting a scan, the daemon and CLI check the state filesystem:

- `estimate = previous snapshot size × 1.25`, or `dirs_estimate × 300 B` if there is no previous snapshot.
- If `state_dir` usage plus the estimate would exceed `max_state_size`, or free space would drop below the floor, **first prune**: delete the oldest snapshots that aren't in the `recent` set or each root's protected newest complete snapshot (largest root first) until the scan fits. Only if it still doesn't fit, refuse to start and raise alert `state_space`. The floor is `max(min_free_pct × size, 2 GiB)`.
- Pruning never deletes the `recent` set or a root's newest complete snapshot; it logs every deletion.
- During the scan, the writer re-checks free space every 100,000 rows and aborts cleanly if the floor is crossed.

## 9. Schema evolution

- Both databases carry `PRAGMA user_version`.
- **Snapshots are never migrated.** Readers support the current and previous `user_version`. Older snapshots are reported as "unsupported version" and remain available to `prune`.
- `timeseries.db` is migrated forward in place by numbered migration functions inside a transaction, with a backup copy written first (`timeseries.db.bak-v<N>`).

## 10. ncdu export (W3-C, FR-22)

`tidemark export ncdu [PATH] [-o FILE]` writes ncdu JSON format version `[1, 2, {metadata}, <root dir>]` (format documented at dev.yorhel.nl/ncdu/jsonfmt):

| ncdu element | Tidemark source |
|--------------|-----------------|
| Directory infoblock | `name` (root: full path), `dsize` = `dir_disk` (the directory's own blocks only), `ino`, `dev` |
| Subdirectories | Recursive arrays in preorder |
| Notable **single-link** files in that directory | File infoblocks with `dsize`, `asize`, `ino` |
| Everything else charged to the directory | One synthetic file `"[other files: N]"`: `dsize` = `own_charged` − `dir_disk` − notable single-link files here; `asize` = `own_apparent` − `dir_apparent` − their apparent sizes |
| Hard-link sets | Not exported as files. Their bytes are inside the synthetic entry of the directory they are charged to (their LCA): the "charged" view. |

ncdu then shows **charged** sizes, which add up, with a header note. Names are written as raw bytes, as ncdu itself does; `--utf8-lossy` replaces invalid sequences so strict JSON tools can read the output.
