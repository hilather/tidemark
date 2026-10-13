# Wave 3 — Queries and reports

**Goal:** turn snapshots into answers: what grew and why (`diff --explain`), how hard links tie trees together (`links`), symlink health (`symlinks`), wide-directory hazards (`wide`), and an ncdu-compatible export.

**Agents:** 3 in parallel. **Entry:** `wave-2-done`. Can overlap with Waves 4 and 5.

| Task | Title | Owned paths | Depends on |
|------|-------|-------------|------------|
| W3-A | Diff and explain | `tidemark-store/src/diff.rs`, `tidemark-cli/src/cmd/diff.rs` | W2 |
| W3-B | Link topology, regions, symlink analysis | `tidemark-store/src/links.rs`, `tidemark-fs/src/symlinks.rs`, `tidemark-cli/src/cmd/{links,symlinks}.rs` | W2 |
| W3-C | ncdu export and wide directories | `tidemark-store/src/export_ncdu.rs`, `tidemark-cli/src/cmd/{export,wide}.rs` | W2 |

**Merge order:** W3-B, then W3-A, then W3-C.

---

## W3-A — Diff and explain

**Read first:** `05-storage.md` §5, `08-interfaces.md` §1.2 (`diff`), `03-link-semantics.md` §3.

### Interface (`tidemark_store::diff`)

```rust
pub enum DiffMetric { Disk, Reclaimable, Files }
pub enum DiffClass { Grown, Shrunk, New, Removed, Same }
pub struct DiffRow { pub path: RawPath, pub depth_rel: u32, pub new: Option<u64>, pub old: Option<u64>,
                     pub delta: i64, pub class: DiffClass }
pub fn diff(new: &Snapshot, old: &Path, target: &RawPath, metric: DiffMetric,
            max_depth_rel: u32, limit: usize) -> Result<Vec<DiffRow>, StoreError>;

pub struct ExplainStep { pub path: RawPath, pub delta: i64, pub share_of_parent: f64 }
pub struct Explanation { pub target_delta: i64, pub steps: Vec<ExplainStep>,
                         pub own_delta: i64,               // delta of own_charged at the stopping directory
                         pub spread: Vec<DiffRow>,         // top contributors where descent stopped
                         pub spread_children: u32 }        // how many children shared the growth
pub fn explain(new: &Snapshot, old: &Path, target: &RawPath, metric: DiffMetric,
               ratio: f64, min_bytes: u64, top: usize) -> Result<Explanation, StoreError>;
```

### Details

- Implement the SQL in `05` §5.1 (ATTACH the old snapshot read-only and immutable). `depth_rel` is relative to the target. If the target doesn't exist in the old snapshot, every row is `New`; if it doesn't exist in the new one, report "target removed" and show the old subtree's top rows as `Removed`.
- **Explain** follows `05` §5.2. Defaults: `ratio 0.5`, `min_bytes 1 GiB` (CLI flags `--explain-ratio`, `--explain-min`). For negative growth, explain the largest shrink symmetrically.
- **Snapshot selection in the CLI:**
  - `--since DUR`: new = latest complete; old = `catalog.nearest_before(root, now − DUR)`. If the old one is more than 25% of DUR away from the requested time, print a notice with the actual age.
  - `--from A [--to B]`: explicit IDs; `--to` defaults to latest.
- **Output:** a tree-indented table (class glyphs `+ − ★ ✗`), with deltas in human units and ± signs. `--explain` prints the growth path block shown in `05` §5.2. `--json` carries both `diff` and `explanation`.

### Tests

- Synthetic snapshots built via `SnapshotWriter` with known differences: grown, shrunk, new and removed directories at several depths; a target missing in the old or new snapshot.
- Explain descends through a dominant chain and stops on spread growth; respects `min_bytes`; handles a tie deterministically (by name).
- Explain for growth concentrated in the "own files" of a directory stops there and reports `own_delta`.
- CLI `insta` snapshots for text and JSON.

### Acceptance

- [ ] `tidemark diff /fixture --from A --to B --explain` on a fixture mutated between two scans names the mutated directory (end-to-end test).
- [ ] Diff of two 1M-directory snapshots, depth 3, returns in < 5 s on the dev box (recorded).

---

## W3-B — Link topology, regions, symlink analysis

**Read first:** `03-link-semantics.md` (all), `08-interfaces.md` (`links`, `symlinks`).

### Interface

```rust
// tidemark_store::links
pub enum RegionSpec { Globs(Vec<String>), Depth(u32) }      // regions under the report root
pub struct Regions;                                          // laminar preorder intervals
impl Regions {
    pub fn build(snap: &Snapshot, under: &RawPath, spec: &RegionSpec) -> Result<Regions, StoreError>;
    pub fn region_of(&self, dir_id: u32) -> Option<RegionId>; // deepest containing region; None = outside all
    pub fn name(&self, r: RegionId) -> &RawPath;
}
pub struct CrossingSet { pub ino: u64, pub disk: u64, pub nlink: u32, pub found: u32,
                         pub regions: Vec<Option<RegionId>>, pub sample_dirs: Vec<u32> }
pub fn crossing_sets(snap: &Snapshot, regions: &Regions, limit: usize)
    -> Result<(Vec<CrossingSet>, CrossingTotals), StoreError>;
pub struct LinkReport { pub totals: LinkTotals, pub crossing: Vec<CrossingSet>, pub region_pairs: Vec<RegionPair>,
                        pub incomplete_top_dirs: Vec<(RawPath, u64 /*pinned bytes*/, u64 /*sets*/)>,
                        pub largest_sets: Vec<LinkSetHit>, pub most_shared_dirs: Vec<(RawPath, u64 /*shared*/, u64 /*t_disk*/)> }
pub fn link_report(snap: &Snapshot, under: &RawPath, spec: Option<&RegionSpec>, top: usize)
    -> Result<LinkReport, StoreError>;

// tidemark_fs::symlinks
pub struct SymlinkOpts { pub check_targets: bool, pub limit: usize }
pub enum SymlinkClass { InsideRoot, OutsideRoot, OtherFs, Dangling, Loop, Unreadable }
pub struct SymlinkReport { pub total: u64, pub by_class: Vec<(SymlinkClass, u64)>,
                           pub absolute: u64, pub relative: u64,
                           pub examples: Vec<(RawPath, Vec<u8> /*target*/, SymlinkClass)> }
pub fn analyze(root: &RawPath, gate: &dyn Gate, cancel: &AtomicBool, opts: &SymlinkOpts) -> io::Result<SymlinkReport>;
```

### Details

- **`Regions`:** load region directories (glob matches, or every directory at the given depth under `under`) as preorder intervals `[id, subtree_end]`. They are laminar, so sort by `id` and, for a point, binary-search the last start ≤ point, then walk to enclosing intervals via a precomputed parent pointer until one contains the point.
- **`crossing_sets`:** iterate `link_instances` ordered by `ino` (streamed). Map each instance's directory to a region; a set crosses if it has more than one distinct region value (including `None` = outside every region but inside the root). An incomplete set also has links outside the scan root; report that separately as `found < nlink`.
- **Report sections (text):**
  1. Totals: link sets, instances, bytes in sets, incomplete sets with bytes.
  2. Crossing sets (if regions are given): count and bytes, top region pairs (A ↔ B: sets, bytes).
  3. Directories with the most pinned bytes from incomplete sets.
  4. Largest link sets with LCA path.
  5. Directories with the largest `shared`.
- `--show-paths`: for listed sets, print instance paths, from `link_instances.name` in walk-mode snapshots. In bulk-mode snapshots, resolve each live via `read_dir_entries` on the instance directory, marked `(live)` or `(gone)`.
- **`symlinks`** (maps `--check-targets` → `SymlinkOpts.check_targets`; `--dangling`/`--escaping` filter the example list): a parallel traversal (reuse the W1-B walker's directory iteration utilities, or a simple gated single-thread traversal; a single thread is acceptable). For each `DT_LNK`, `readlinkat`, then classify:
  - Relative targets resolve against the link's directory.
  - With `check_targets`, resolve components with gated no-follow metadata/readlink operations, at most 40 expansions. Stop before checking a target beyond the selected mount, including same-device binds: `OtherFs` means unchecked beyond that boundary. Inside-boundary `ENOENT` is `Dangling`, and cycles/expansion limit are `Loop` (R5-25).
  - Without `check_targets`, classify lexically only (inside/outside root).
  - Enumeration never descends through found links. Reading link text can update that link’s atime under mount policy; command-line path resolution may follow links (R5-23/25).

### Tests

- `Regions` with nested globs and depth mode on synthetic snapshots; a property test comparing against brute-force containment.
- `crossing_sets` on `hardlink_web` (scanned) vs a hand-computed expectation.
- `link_report` golden on `link_semantics_basic`: inode 100 crosses `a↔b` with depth-1 regions; inode 102 is incomplete; inode 103 stays inside region `c`.
- Symlinks on `symlink_loops` and a fixture with absolute, relative, dangling, cross-filesystem (privileged: a link to a loop mount) and looping links.

### Acceptance

- [ ] `tidemark links /data --boundary-depth 2` runs on a 1M-link-set synthetic snapshot in < 10 s (recorded).
- [ ] `crossing_sets` and `Regions` are documented with rustdoc examples.

---

## W3-C — ncdu export and wide directories

**Read first:** `05-storage.md` §10, the ncdu JSON format documentation (https://dev.yorhel.nl/ncdu/jsonfmt), `08-interfaces.md` (`export`, `wide`).

### Interface

```rust
pub struct NcduOptions { pub utf8_lossy: bool, pub max_depth: Option<u32> }
pub fn export_ncdu(snap: &Snapshot, under: &RawPath, out: &mut dyn Write, opts: &NcduOptions) -> Result<ExportStats, StoreError>;
```

### Details

- Streaming writer: iterate directories in preorder (`id` order within `[under.id, under.subtree_end]`) with an explicit stack of open JSON arrays. **Never build the tree in memory.**
- Top-level: `[1, 2, {"progname":"tidemark","progver":…,"timestamp":…}, <root dir>]`.
- Per directory:
  - The directory infoblock: `name` (the full path for the top element), `dsize` = `dir_disk` (the directory inode's own blocks), `ino`, `dev` on the root only.
  - Notable single-link files as file infoblocks.
  - One synthetic `"[other files: N]"` entry carrying the rest of `own_charged`.
- Names: raw bytes, written with correct JSON escaping for control characters, quote and backslash. Invalid UTF-8 is written raw (as ncdu does) unless `--utf8-lossy`.
- `wide`: `snap.wide_dirs(id, min_entries, limit)`, rendered with entries, `t_disk` and path, plus a hint line explaining why very wide directories are slow (directory btree depth, `ls` sort cost) and pointing at the app owner.

### Tests

- Export of `link_semantics_basic`: parse with a JSON parser (when `--utf8-lossy`); the sum of all `dsize` equals `t_charged(root)`; structure matches the format (arrays and objects in the right places).
- If `ncdu` is installed in the VM image: `ncdu -f export.json -o /dev/null` (import, then re-export) succeeds (privileged suite; skip if missing).
- `nonutf8` fixture export: valid with `--utf8-lossy`; raw bytes preserved without it.
- 1M-directory synthetic snapshot export streams with flat memory (RSS delta < 64 MiB).

### Acceptance

- [ ] An exported file loads in ncdu 1.x or 2.x on the VM (manual check recorded if ncdu is not packaged).
- [ ] The export metadata block carries a `"note"` key stating that sizes are the charged (additive) view.

---

## Wave 3 gate checklist

- [ ] All tests green; the end-to-end diff/explain story works on the VM: scan → mutate → scan → `diff --since 1h --explain` names the right directory.
- [ ] Tag `wave-3-done`.
