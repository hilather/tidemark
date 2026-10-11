# Wave 2 — Scan engine

**Goal:** `tidemark scan` produces correct, deterministic, hard-link-aware snapshots in walk and bulk modes, and `top`, `files` and `snapshots` can read them.

**Agents:** 3 in parallel. **Entry:** `wave-1-done`.

| Task | Title | Owned paths | Depends on |
|------|-------|-------------|------------|
| W2-A | Aggregator and link resolution | `tidemark-scan/src/agg/**` | W1 (types only) |
| W2-B | Bulk pipeline (namespace walk + bulkstat join + names) | `tidemark-scan/src/bulk/**` | W1-B, W1-C; W2-A interface |
| W2-C | Orchestrator, mode selection, progress; CLI `scan`, `snapshots`, `top`, `files`; rendering | `tidemark-scan/src/{orchestrator,mode,progress}.rs`, `tidemark-cli/src/cmd/{scan,snapshots,top,files}.rs`, new files `tidemark-cli/src/render/{tree,scan_progress}.rs` | W1; W2-A/B interfaces |

The **W2-A interface below is the contract between the three tasks.** W2-A lands it first as a compiling skeleton commit (signatures only) within its first hours, so W2-B and W2-C can build against it. This is the only intra-wave dependency, and it is deliberate.

**Merge order:** W2-A → W2-B → W2-C.

---

## W2-A — Aggregator and link resolution

**Read first:** `03-link-semantics.md` (all, normative), `04-scanner.md` §1 and §3.2, `05-storage.md` §1, `09-testing.md` §3–4.

### Interface (`tidemark_scan::agg`)

```rust
pub struct AggConfig {
    pub root: RawPath,
    pub scan_start: i64,
    pub top_files: usize,
    pub big_file_threshold: u64,
    pub max_notable: usize,
    pub link_instances: bool,
    pub max_stored_errors: usize,
}

pub struct Aggregator;
impl Aggregator {
    pub fn new(cfg: AggConfig) -> Self;

    // Walk mode
    pub fn add_dir_record(&mut self, rec: DirRecord);
    pub fn add_gone(&mut self, g: &GoneRecord);                             // both modes: tolerate DirId holes (04 §2.3)

    // Bulk mode (fed by W2-B)
    pub fn add_skeleton(&mut self, ns: &NsRecord);                         // dirs, names, parents, entries, skips, errors
    pub fn set_dir_inode(&mut self, dir: DirId, a: &InodeAttrs);           // directory's own inode (self-ref)
    pub fn add_single(&mut self, dir: DirId, a: &InodeAttrs);              // nlink == 1 entry
    pub fn add_link_set(&mut self, a: &InodeAttrs, dirs: &[DirId]);         // nlink > 1; all in-root links
    pub fn add_races(&mut self, dir: Option<DirId>, n: u32);

    /// Compute everything: link sets, LCAs, union stamps, roll-ups, preorder ids, notable selection.
    pub fn prepare(self) -> Result<Prepared, AggError>;
}

pub struct Prepared;
impl Prepared {
    pub fn summary(&self) -> AggSummary;                                   // totals for meta + progress
    /// Bulk mode: notable files without names, as (directory path, ino).
    pub fn needs_names(&self) -> Vec<(RawPath, Ino)>;
    pub fn set_names(&mut self, names: Vec<(RawPath, Ino, RawName)>);
    /// Labels from the Postgres collector etc.; paths not in the tree are ignored (counted).
    pub fn annotate(&mut self, path: &RawPath, key: &str, value: &str);
    /// Stream all rows, dirs in preorder, children sorted by name bytes.
    pub fn write(self, sink: &mut dyn RowSink) -> Result<AggSummary, AggError>;
}

/// SinkError is a thiserror enum in tidemark-scan (Store(StoreError), Other(String)); no anyhow in libraries.
pub trait RowSink {
    fn dirs(&mut self, rows: &[DirRow]) -> Result<(), SinkError>;
    fn files(&mut self, rows: &[FileRow]) -> Result<(), SinkError>;
    fn link_sets(&mut self, rows: &[LinkSetRow]) -> Result<(), SinkError>;
    fn link_instances(&mut self, rows: &[LinkInstanceRow]) -> Result<(), SinkError>;
    fn skipped(&mut self, rows: &[SkipRow]) -> Result<(), SinkError>;
    fn errors(&mut self, rows: &[ErrorRow]) -> Result<(), SinkError>;
    fn annotations(&mut self, rows: &[AnnotationRow]) -> Result<(), SinkError>;
}
pub struct VecSink { pub dirs: Vec<DirRow>, /* … one Vec per row type … */ }   // tests, `top --live`

pub struct AggSummary { pub dirs: u64, pub files: u64, pub inodes: u64, pub t_disk: u64, pub t_apparent: u64,
                        pub t_reclaimable: u64, pub link_sets: u64, pub incomplete_link_sets: u64,
                        pub errors: u64, pub races: u64, pub skipped: u64, pub degraded: bool }
```

### Algorithm

Implement `03` §4 Steps A–F exactly:

- **Storage:** a `Vec<Node>` indexed by `DirId`. `Node` holds `parent: u32` (`u32::MAX` for root), `depth: u32` (matches core without truncation), `name: Box<[u8]>`, own stats, union accumulators, charged/reclaim accumulators, age buckets, counters, `stamp: u64`, and children (`Vec<u32>`, filled in `prepare`). Keep `Node` lean; measure its size in a test and document it (target ≤ 200 bytes plus the name).
- **Walk mode** link sets: an `FxHashMap<Ino, LinkSetAcc>` built from `DirRecord.links`. In bulk mode, sets arrive complete via `add_link_set`, so no map is needed.
- **Depth:** computed when the parent is known. Records can arrive child-before-parent in walk mode. Buffer orphans until the parent arrives, then compute depths in `prepare` with an iterative pass. Never recurse; trees can be very deep.
- **Notable selection:** exactly `03` §4 Step G: deterministic order `(disk desc, final dir id, ino)`, representative = smallest `(final dir id, name)` instance. Selection runs after renumbering, so final ids are known. Workers' local heaps (W1-B) only pre-filter, and must admit ties at the boundary.
- **projid (bulk mode only):** copy each directory's own project ID into `DirRow.projid` (NULL in walk mode). The current product does not otherwise use it (`06-project-quotas-deferred.md`).
- **Write:** iterative DFS over children sorted by name bytes. Assign preorder ids with checked bounds; assign zero-based notable-file `u32` ids with checked conversion (`max_notable <= u32::MAX`). Build each `path` incrementally in one reusable buffer. Compute `subtree_end`. Emit `DirRow`s in batches of 10,000, then the other row types, translating `DirId`s to final ids through a `Vec<u32>` map.
- `degraded = errors > 0`.

### Tests

- **Golden:** `link_semantics_basic` through `ModelTree::to_dir_records()` equals the `03` §6 table exactly. Also run on disk through the W1-B walker, comparing `t_disk` per directory against `du` (directory blocks included).
- **Property (proptest):** for random `ModelTree`s (up to 2,000 dirs, 20,000 files, 2,000 link sets, 30% incomplete), every metric of every directory equals `test_support::reference`. Invariants 1–5 of `03` §3 hold. Shuffling `DirRecord` arrival order yields identical `VecSink` output (invariant 6).
- **Bulk inputs:** the same `ModelTree` fed through the bulk API (`add_skeleton` / `set_dir_inode` / `add_single` / `add_link_set`) yields `VecSink` output identical to the walk path, except `link_instances.name` (NULL in bulk).
- **Scale:** 2M synthetic directories / 30M synthetic entries (generated in memory, without the filesystem) finish `prepare` + `write` to a counting sink in bounded memory. Record time and peak RSS in the handoff. Run as an `#[ignore]` test.
- **Deep tree:** depth 10,000 synthetic, no stack overflow.

### Acceptance

- [ ] Golden, property and bulk-equivalence tests pass.
- [ ] No recursion anywhere in `agg` (reviewer checks).
- [ ] Memory per directory and per link set documented, with measured numbers.

---

## W2-B — Bulk pipeline

**Read first:** `04-scanner.md` §3, §5, §7, §8, `03-link-semantics.md` §4.

### Interface (`tidemark_scan::bulk`)

```rust
pub struct BulkParams { pub walk: WalkConfig /* stat_entries forced false */, pub batch: usize, pub threads: usize }
pub struct BulkStats { pub phase_ms: [u64; 3], pub dents: u64, pub inodes_streamed: u64, pub joined: u64,
                       pub races: u64, pub ags: u32, pub names_resolved: u64, pub names_missing: u64 }
pub fn estimate_memory(used_inodes: u64, dirs_estimate: u64) -> u64;      // 04 §7 formula
pub fn run_bulk(p: &BulkParams, mount_fd: BorrowedFd<'_>, geo: &XfsGeometry, gate: &dyn Gate,
                cancel: &AtomicBool, agg: Aggregator) -> Result<(Prepared, BulkStats), BulkError>;
```

`run_bulk` performs phases 1–2, calls `agg.prepare()`, then runs phase 3 (names via `tidemark_fs::walk::read_dir_entries` for `prepared.needs_names()`) and `set_names`, and returns the `Prepared`.

### Implementation notes

- **v5 only:** `tidemark_sys::xfs::bulkstat` uses v5 metadata only; `Unsupported` means bulk mode is unavailable, never permission to use v1 records.

- **Gate ownership:** W2-C resolves leaf devices and builds the `ScanGate`; W2-B uses the supplied gate without duplicating profile selection.
- **Phase 1:** run `walk()` with `stat_entries = false` on a scoped thread. The collector thread consumes `WalkOutput::Ns`, calls `agg.add_skeleton(&ns)`, and appends `DentRef`s: 12 bytes each (`[u32; 3]` or `#[repr(C, packed(4))]`, read by copy only), one per child plus one self-reference per directory with the high bit of `slot` set.
- **Sort:** `rayon` `par_sort_unstable_by_key` on `ino`.
- **Phase 2:**
  - For each AG, take the dent slice via `partition_point` on the AG's inode range.
  - Workers (`threads`, capped at `agcount`) iterate `tidemark_sys::xfs::bulkstat(fd, start, end, batch)`, charging `gate.acquire(ceil(batch / inodes_per_cluster))` conservatively before every iterator call (including terminal/error calls); the first charge also covers geometry. They merge-join with their slice.
  - They produce `Joined` messages in batches of 4,096 over a bounded channel to the thread that owns the `Aggregator`. That thread dispatches each to `set_dir_inode` / `add_single` / `add_link_set` / `add_races`.
  - Translate `mode` to `FileKind`. `disk` comes from `BulkInode.disk_bytes`, already in bytes; never multiply it again.
- **Kind mismatch:** if the dent was a child entry but the inode is now a directory (or vice versa), count a race.
- **Phase 3:** group needed names by directory, read each directory once (gated), match inode numbers, and report `names_missing` for files gone since phase 1.
- **Cancellation** is checked between batches; on cancel, drain the channel and return `BulkError::Cancelled`.

### Tests

- Unit: the merge-join on synthetic sorted inputs (gaps, duplicates, missing inodes on both sides, AG boundaries); `estimate_memory` arithmetic.
- **Privileged (VM, loop XFS): `bulk_equals_walk_*`.** For every mandatory fixture, run the walk path (W1-B + W2-A) and the bulk path into `VecSink`s and assert equality of all `dirs` rows (except `projid`), `files`, `link_sets`, `skipped` and `errors`. `link_instances` must be equal ignoring `name`.
- Privileged: a mount with `ftype=0` → `run_bulk` refuses with a typed error (the orchestrator falls back to walk).
- Privileged performance smoke: 1M-file generated tree; bulk and walk times recorded (no gate yet).

### Acceptance

- [ ] Bulk/walk equivalence passes on Rocky 8.8 and 8.10 VMs.
- [ ] Peak RSS of bulk mode on the 1M-file tree is within 20% of `estimate_memory` (recorded).

---

## W2-C — Orchestrator, mode selection, progress, CLI

**Read first:** `04-scanner.md` §6–9, `05-storage.md` §2, §4, §7–8, `08-interfaces.md` §1, `02-architecture.md` §6.

### Interface (`tidemark_scan`)

```rust
pub enum ModeRequest { Auto, Walk, Bulk }
pub struct ScanRequest { pub root: RawPath, pub mode: ModeRequest, pub throttle: Option<String>,
                         pub threads: Option<usize>, pub exclude_extra: Vec<String>, pub keep_partial: bool,
                         pub annotations: Vec<(RawPath, String, String)> }
pub struct Progress { pub phase: &'static str, pub dirs: u64, pub entries: u64, pub errors: u64,
                      pub races: u64, pub rate: f64, pub elapsed: Duration, pub eta: Option<Duration> }
pub struct ScanOutcome { pub scan_id: String, pub file: Option<PathBuf>, pub status: SnapshotStatus,
                         pub degraded: bool, pub mode: ScanMode, pub mode_reason: String,
                         pub summary: AggSummary, pub duration: Duration }
pub struct ModeFacts { pub is_xfs: bool, pub bulkstat_v5: bool, pub cap_sys_admin: bool, pub ftype: Option<bool>,
                       pub root_is_mountpoint: bool, pub prev_root_inodes: Option<u64>, pub fs_used_inodes: u64,
                       pub dirs_estimate: u64 }
pub fn choose_mode(f: &ModeFacts, req: &ModeRequest, memory_budget: u64) -> (ScanMode, String);   // pure
/// Where catalog and dir_series rows go. The daemon passes a sink that forwards to its single
/// ts-writer thread; the CLI (daemon absent) passes a sink that writes timeseries.db directly.
pub trait ScanRecorder: Send + Sync {
    fn scan_started(&self, e: &ScanEntry) -> Result<(), ScanError>;
    fn scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError>;
    fn dir_series(&self, root: &RawPath, scan_id: &str, ts: i64, rows: &[DirRow]) -> Result<(), ScanError>;
}
pub fn run_scan(cfg: &Config, req: &ScanRequest, recorder: &dyn ScanRecorder, cancel: &AtomicBool,
                progress: &(dyn Fn(&Progress) + Sync)) -> Result<ScanOutcome, ScanError>;
```

### `run_scan` sequence

1. The CLI canonicalizes the original path before constructing `ScanRequest.root` as `RawPath`; never lexically normalize before realpath. Require a directory; find its mount (W1-A); compute `skip_mounts`.
2. Look up the root's `[[roots]]` config, or defaults when scanning an unconfigured path.
3. **Host-wide scan lock:** take an exclusive `flock` on `state_dir/scan.lock` (non-blocking). If it's held, return `ScanError::Busy` (ADR-015). The daemon scheduler takes the same lock, so a CLI scan and a daemon scan can never overlap.
4. **Space guard** (`05` §8). If refused, return `ScanError::NoSpace`.
5. **Gate:** retain the `ScanGate` returned by `gate_from_config(profile_name, &cfg.throttle, &leaves, sampler)` with `leaves = resolve_leaves(dev)` (empty on unavailable discovery) and a bounded sampler built from ordered, all-or-error `read_disk_stats(&leaves)`.
6. Fallible `ScanId::new(now)` (propagate invalid UTC-year errors; retry a collision with a fresh id, never replace an existing snapshot); `recorder.scan_started`.
7. Choose the mode (`choose_mode`, `04` §7): auto requires v5. Explicit `--mode bulk` without v5 fails with exit code 5 and a reason naming missing v5 bulkstat. Auto may fall back to walk on a bulk failure before phase 2 starts and records why.
8. Size the walker output channel from the scan memory budget with `04` §7: `capacity * top_files * bytes_per_candidate <= queued_records_budget`; reserve producer candidates and other state separately, including any threshold class. Run walk (W1-B → W2-A on a collector thread) or bulk (W2-B) with `scan_gate.gate()`. Report progress every 2 s.
9. `prepared.annotate(…)` for every request annotation.
10. `SnapshotWriter::create` → `prepared.write(&mut WriterSink)` → `finish(meta)`. Fill `meta` per `05` §1, including phase timings and final gate stats from `scan_gate.stop()` after all scan acquisitions finish. Use `scan_gate.stats()` for observations during the scan; Drop also stops the controller. Profile `max` with rate 0 means unthrottled (`13` G2).
11. `recorder.scan_finished`; `recorder.dir_series` for directories at depth ≤ `series_depth` (capped at `MAX_SERIES_ROWS = 20_000`, largest first); apply `retention_plan` for this root and delete the files it names.
12. Return the outcome. The exit code is 3 if degraded or partial.

### CLI

- `scan` (direct mode; uses a `DirectRecorder` that writes `timeseries.db`, valid only while the daemon is not running): text progress on stderr (single updating line on a TTY; one line per 10 s otherwise). Final summary: mode and reason, duration, dirs/files, `t_disk`, reclaimable, link sets (incomplete), errors, races, snapshot path. `--json` prints `ScanOutcome`. **W6-A adds the daemon branch:** when `tidemarkd` is up, `scan` calls `scan.start` and follows `scan.status`, and `--cancel` calls `scan.cancel`.
- `snapshots list|show|prune [--dry-run]|rebuild-catalog`. Direct catalog opens perform the header preflight before SQLite (`05` §4) and stay non-destructive; only reported corruption triggers `catalog::recover_catalog`, with all connections closed and writers stopped. Log preserved recovery file names (`05` §4). A healthy or missing catalog uses ordinary open/rebuild.
- `top`: per `08` §1.4.
  - Columns SIZE, RECLAIM, SHARED (with `H`), FILES, NEWEST (optional `--wide`), and Δ24h when the catalog has a complete snapshot within ±6 h of now − 24 h (looked up per displayed path in the old snapshot).
  - `--depth N` renders a tree; `--sort` and `--min-size` as in the spec. The CLI wrapper’s `Growth` case compares two snapshots (the selected snapshot and the older complete snapshot used for Δ24h), orders by descending signed disk-byte delta and breaks ties by raw path bytes; error if no older snapshot exists. It is not a single-snapshot `SortKey` variant.
  - `--live`: walk PATH now (walk mode, gate profile `gentle`, into `VecSink`) and show live values next to the snapshot values with deltas, without writing a snapshot.
- `files`: largest files under PATH with path, size, mtime and nlink, plus `(shared)` for link-set representatives.
- `render/tree.rs`: tree layout with Unicode glyphs (ASCII fallback when `LANG` is not UTF-8), built on W0-B's `render/table.rs` and `render/size.rs`.

### Tests

- `choose_mode` truth table: every condition in `04` §7, with reasons.
- End-to-end (unprivileged): `tidemark scan <fixture>` then `top` / `files` / `snapshots` produce `insta` snapshots, with timestamps redacted.
- **Oracle:** after scanning each mandatory fixture in walk mode, `top --depth 99 --json` values equal `du -sxB1` / `--apparent-size` and `find` counts for every directory, with apparent-size oracle normalization as specified in `09-testing.md` §3.
- Cancellation: SIGINT during a scan leaves no `.db` and a catalog row with `status = failed` (or `partial` with `--keep-partial`).
- The space-guard refusal path (injected) leaves no partial files.
- Privileged: `auto` chooses bulk on a loop XFS mount root and walk on a small subdirectory; reasons are recorded in `meta`.

### Acceptance

- [ ] `tidemark scan /mnt/fixture && tidemark top /mnt/fixture` works end to end on the VM in both modes.
- [ ] Oracle tests pass for every mandatory fixture.
- [ ] `--json` outputs carry schema tags; `insta` snapshots are committed.

---

## Wave 2 gate checklist

- [ ] All oracle, property and golden tests green; bulk ≡ walk on both VMs.
- [ ] **Manual sanity run** on a staging copy of real report data (or a large generated tree): scan in both modes, compare `top` against `du` on 3 random subtrees, and record times in `docs/handoff/W2-gate.md`.
- [ ] Gate review confirms that `03` is implemented literally (spot-check the code against Steps A–F).
- [ ] Tag `wave-2-done`.
