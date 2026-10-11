# Wave 1 — Platform primitives

**Goal:** all low-level building blocks, each independently tested: mount and device discovery with the probe, the parallel walker, XFS FFI (geometry, bulkstat), throttling with procfs parsers, and the SQLite store.

**Agents:** 5 in parallel. **Entry:** `wave-0-done`. Tasks compile against the Wave 0 stubs and contracts. Cross-task calls go through the stub signatures, which return `NotYetImplemented` until the owning task merges, and callers treat that as "unknown".

| Task | Title | Owned paths | Depends on |
|------|-------|-------------|------------|
| W1-A | Mounts, block devices, canonical paths, probe | `tidemark-fs/src/{mount,blockdev,canon,probe}.rs`, `tidemark-cli/src/cmd/probe.rs` | W0 |
| W1-B | Parallel walker (walk + namespace modes) | `tidemark-fs/src/walk/**`, `tidemark-fs/benches/walk.rs` | W0 |
| W1-C | XFS ioctls: geometry and bulkstat (FFI) | `tidemark-sys/src/xfs.rs`, `tidemark-sys/src/gen/**` | W0 |
| W1-D | Throttle, ioprio, procfs parsers, attribution cache | `tidemark-sys/src/ioprio.rs`, `tidemark-fs/src/{throttle,procfs,attribution}.rs` | W0 |
| W1-E | Snapshot store, catalog, time series, retention, space guard | `tidemark-store/src/{writer,reader,catalog,timeseries,retention,space_guard}.rs`, `tidemark-store/migrations/**` | W0 |

**Merge order:** W1-C → W1-D → W1-A → W1-E → W1-B.

---

## W1-A — Mounts, block devices, canonical paths, probe

**Read first:** `04-scanner.md` §2.2 and §6.2, `01-requirements.md` FR-1, `07-live-monitoring.md` §3.1, `02-architecture.md` §9.

### Interfaces

```rust
// tidemark-fs::mount
pub struct MountInfo {
    pub id: u32, pub parent: u32, pub major: u32, pub minor: u32, pub dev: DevId,
    pub root: RawPath, pub mount_point: RawPath,
    pub mount_opts: Vec<String>, pub fs_type: String, pub source: String, pub super_opts: Vec<String>,
}
pub fn parse_mountinfo(bytes: &[u8]) -> Result<Vec<MountInfo>, ParseError>;   // pure; fuzzed in W7-C
pub fn read_mountinfo() -> io::Result<Vec<MountInfo>>;                         // /proc/self/mountinfo
pub fn mount_for_path<'a>(m: &'a [MountInfo], path: &RawPath) -> Option<&'a MountInfo>; // longest match; later mounts win
pub fn submounts_under(m: &[MountInfo], root: &RawPath) -> Vec<RawPath>;       // strictly below root
pub fn other_mounts_of_dev<'a>(m: &'a [MountInfo], dev: DevId, except: &RawPath) -> Vec<&'a MountInfo>; // bind mounts

// tidemark-fs::blockdev  (types DevKind, BlockDevInfo, DiskStat live in tidemark-core::dev)
pub fn resolve_leaves(dev: DevId) -> io::Result<Vec<BlockDevInfo>>;
pub fn parse_disk_stat(line: &str) -> Option<DiskStat>;                        // pure; 11+ fields
pub fn read_disk_stats(devs: &[BlockDevInfo]) -> io::Result<Vec<DiskStat>>;    // used as the throttle sampler

// tidemark-fs::canon
pub fn canonicalize(path: &Path) -> io::Result<RawPath>;

// tidemark-fs::probe
pub struct ProbeReport { pub host: HostFacts, pub roots: Vec<RootFacts>, pub recommendations: Vec<Recommendation> }
pub fn probe(cfg: &Config, extra_roots: &[RawPath]) -> ProbeReport;            // never fails; unknowns are Option/None
```

### Details

- **mountinfo parsing:** fields per proc(5). Optional fields end at `-`. Decode octal escapes (`\040` space, `\011` tab, `\012` newline, `\134` backslash) into raw bytes. `dev = makedev(major, minor)` (use `libc::makedev`).
- **`resolve_leaves`:** start at `/sys/dev/block/<maj>:<min>`. If it has a `partition` file, step to the parent directory (the whole disk). If `slaves/` is non-empty (dm, md), recurse into each slave. Leaves are devices with no slaves. Record `queue/rotational`, the active scheduler (the bracketed entry in `queue/scheduler`), and for md, `md/level`. Hardware RAID LUNs appear as a single `sdX` leaf.
- **Probe contents** (each field optional, plus a short `why` for unknowns):
  - **Host:** OS release (`/etc/os-release`, `/etc/rocky-release`), kernel (`uname -r`), whether this is the latest Rocky minor (8.10 for EL8; warn on anything older), cgroup version (`statfs("/sys/fs/cgroup").f_type`), CPUs, memory, effective capabilities (`CapEff` from `/proc/self/status`), mount-namespace equality (`/proc/self/ns/mnt` vs `/proc/1/ns/mnt`), `/sys/kernel/btf/vmlinux` present, `bcc-tools` installed.
  - **Per root:** mount point, fs type, source device, options (atime mode: `relatime`/`noatime`/`strictatime`), `statvfs` totals, XFS geometry and features (calls `tidemark_sys::xfs::geometry`), bulkstat support (`bulkstat_probe`), LVM mapping (`tidemark_fs::lvm::collect`; stub until W4-A), leaf devices (resolved through dm/LVM and md) with scheduler/rotational/md level, other mounts of the same device (bind mounts), and fanotify capabilities (`tidemark_sys::fanotify::probe`; stub until W5-A). **Postgres is not probed here**: `tidemark-fs` must not depend on `tidemark-pg`. `cmd/probe.rs` appends a Postgres section by calling `tidemark_pg::probe_from_config` (stub until W5-C).
- **Recommendations** (pure function over the facts, unit-tested):
  - Scheduler is not `bfq` → "ionice ineffective; Tidemark's throttle is used".
  - Not the latest minor release → recommend updating.
  - Bind mounts present → watcher fd mode limitation.
  - Mount namespace differs → the watcher will refuse to run.
  - `ftype=0` → bulk mode unavailable.
  - Reflink enabled → `t_disk` may overstate physical use for reflinked copies.
- **`cmd/probe.rs`:** text output grouped by section, with ✓/✗/? markers; `--json` emits `JsonDoc{schema:"tidemark.probe.v1"}`.

### Tests

- Unit: mountinfo parser on real samples (commit 3–4 captured from EL8 systems, including bind mounts, escaped spaces and `/dev/mapper/*` LVM sources); `parse_disk_stat` with 11-, 15- and 17-field variants; recommendation rules table-driven.
- Privileged: probe on a loop XFS reports the correct geometry; `resolve_leaves` on a loop-LVM filesystem returns the loop PVs; a bind mount of a subdirectory shows up in `other_mounts_of_dev` and in `submounts_under`.

### Acceptance

- [ ] `tidemark probe` on the VM prints every section; unknowns are explained, never panics.
- [ ] `resolve_leaves` on an md RAID test VM (or a loop-device md array in the VM) returns the member disks.
- [ ] JSON output validates against an `insta` snapshot.

---

## W1-B — Parallel walker

**Read first:** `04-scanner.md` §1, §2, §3.1, §4, §8 (all of it, carefully), `03-link-semantics.md` §5.

### Interfaces

```rust
pub struct WalkConfig {
    pub root: RawPath,
    pub threads: usize,
    pub stat_entries: bool,            // true → WalkOutput::Dir (walk mode); false → WalkOutput::Ns (bulk phase 1)
    pub exclude: globset::GlobSet,     // full-path globs
    pub skip_mounts: Vec<RawPath>,     // from mount::submounts_under
    pub big_file_threshold: u64,
    pub top_files: usize,
    pub scan_start: i64,
    pub getdents_buf: usize,
    pub io_idle: bool,                 // per-worker tidemark_sys::ioprio::set_current_thread_idle()
}
// Emits WalkOutput::Dir / ::Ns per directory and WalkOutput::Gone for queued children that vanished,
// were replaced (inode changed vs the parent's d_ino) or are on another filesystem (04 §2.2).
pub struct WalkSummary {
    pub dirs: u64, pub entries: u64, pub errors: u64, pub races: u64,
    pub noatime_fallbacks: u64, pub elapsed: Duration, pub root_dev: DevId, pub root_ino: Ino,
}
pub fn walk(cfg: &WalkConfig, gate: &dyn Gate, cancel: &AtomicBool,
            out: crossbeam_channel::Sender<WalkOutput>) -> Result<WalkSummary, WalkError>;

/// One directory, used by bulk phase 3, `links --show-paths` and `top --live`.
pub fn read_dir_entries(dir: &RawPath, gate: &dyn Gate) -> io::Result<Vec<(RawName, Ino, FileKind)>>;
```

### Implementation notes

- Implement exactly `04` §2.1–2.2, including `O_NOATIME` with EPERM fallback and the `noatime_fallbacks` counter.
- `getdents64` via `rustix::fs::Dir` (or `rustix::fs::RawDir` with a caller-provided buffer of `getdents_buf` bytes). Use `statx` via `rustix::fs::statx` with `AT_SYMLINK_NOFOLLOW | AT_NO_AUTOMOUNT`.
- **Paths longer than `PATH_MAX`:** when `openat(AT_FDCWD, path)` returns `ENAMETOOLONG`, fall back to component-wise `openat` from the root with `O_NOFOLLOW` at each step. Test with depth 200.
- **`DT_UNKNOWN`:** classify with `statx` (mode only). The `ftype=0` loop-XFS test covers it.
- **Notable files:** worker-local `BinaryHeap<Reverse<(u64, …)>>` of size `top_files`; emit entries with `disk ≥ big_file_threshold` or admitted to the heap.
- **Backpressure:** `out` is a bounded channel (capacity 1,024 records). Workers block when the consumer is slow.
- **Cancellation:** check `cancel` before each directory; return `Err(WalkError::Cancelled)` after draining in-flight workers.
- **No `unsafe`.** `rustix` gives safe wrappers.

### Tests

- Unit and integration (unprivileged, `test-support` fixtures):
  - `link_semantics_basic`: the exact `DirRecord`s (own stats, link refs, entries).
  - `symlink_loops` terminates; symlinks are counted as files and never followed.
  - `nonutf8` names round-trip byte-exact.
  - Exclude globs skip both directories and files, recorded as `Skip(Excluded)`.
  - An artificial `skip_mounts` entry is skipped as `Skip(Mount)`.
  - `wide_dir` (120k entries): one record with `entries = 120000`, memory bounded (assert peak RSS delta < 256 MiB via `/proc/self/statm`).
  - `deep` (depth 200) completes.
  - Namespace mode (`stat_entries=false`) emits `NsRecord`s whose `children` inode numbers match `statx`.
  - Cancellation mid-walk returns `Cancelled` quickly.
  - A race test where a background thread deletes subtrees while walking produces races, not errors.
- Privileged: `noatime_no_writes` (strictatime mount; atimes unchanged after walk); a real bind mount inside the root is skipped; `ftype=0` loop XFS walk is correct.
- Bench: `benches/walk.rs` over a generated tree (100k files), reporting entries/s.

### Acceptance

- [ ] All tests pass; the walk of `link_semantics_basic` matches the expected records exactly.
- [ ] Peak open fds ≤ `threads + 8` during a walk (asserted in a test by sampling `/proc/self/fd`).
- [ ] A `cargo clippy` run with `-D warnings` is clean, and the module has no `unsafe`.

---

## W1-C — XFS ioctls: geometry and bulkstat (FFI)

**Read first:** `04-scanner.md` §5, `AGENTS.md` (unsafe rules).

### Interfaces

```rust
// tidemark-sys::xfs
pub const XFS_SUPER_MAGIC: i64 = 0x5846_5342;
pub fn is_xfs(fd: BorrowedFd<'_>) -> io::Result<bool>;
pub struct XfsGeometry { pub blocksize: u32, pub agblocks: u32, pub agcount: u32, pub inodesize: u32,
                         pub dirblocksize: u32, pub uuid: [u8; 16],
                         pub v5: Option<bool>, pub ftype: Option<bool>, pub reflink: Option<bool> }
pub fn geometry(fd: BorrowedFd<'_>) -> io::Result<XfsGeometry>;
pub struct BulkstatSupport { pub v1: bool, pub v5: bool }
pub fn bulkstat_probe(fd: BorrowedFd<'_>) -> BulkstatSupport;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkInode { pub ino: u64, pub mode: u32, pub nlink: u32, pub uid: u32, pub gid: u32,
                       pub size: u64, pub disk_bytes: u64, pub mtime: i64, pub projid: u32, pub xflags: u32 }
pub fn ag_ino_range(geo: &XfsGeometry, agno: u32) -> (u64, u64);   // [start, end)
pub fn bulkstat(fd: BorrowedFd<'_>, start: u64, end: u64, batch: usize) -> BulkstatIter<'_>;
// BulkstatIter: Iterator<Item = io::Result<Vec<BulkInode>>>, ascending ino, all < end

```

### Implementation notes

- **Bindings:** run `bindgen` once on a Rocky 8 VM against `<xfs/xfs_fs.h>` (xfsprogs-devel) for the structs, and hand-define the ioctl numbers (`04` §5.1), with an allow-list of exactly the needed items. Commit as `src/gen/bindings.rs` with a header noting the source package versions and the command used. Add `size_of`/`align_of`/`offset_of` tests for `xfs_fsop_bulkreq`, `xfs_bstat`, `xfs_bstime` and the geometry structs.
- **Units:**
  - `disk_bytes` is derived from `bs_blocks` in **one function** (`fn bulk_disk_bytes(bs: &xfs_bstat, blocksize: u32) -> u64`) with the privileged validation test `bulkstat_blocks_match_statx`.
  - `mtime` combines `tv_sec` with sign handling.
- **Iteration semantics:** v1 returns inodes `> *lastip`. Start at `start.saturating_sub(1)` (`start = 0` for AG 0). Stop when an inode ≥ `end` appears (truncate that batch) or `ocount == 0`.
- **Errors:** `EPERM` (no `CAP_SYS_ADMIN`) maps to a typed `io::Error` that callers translate into "bulk mode unavailable".
- **Every `unsafe` block** gets a `// SAFETY:` comment explaining buffer sizes and lifetimes. Buffers are `Vec<MaybeUninit<xfs_bstat>>` sized by `batch`.

### Tests (mostly privileged, on loop XFS)

- `geometry_matches_xfs_info` (parse `xfs_info` output in the test).
- `bulkstat_covers_all_inodes`: on a fixture, the set of `(ino, nlink, size, mode, mtime)` from bulkstat equals the `statx` walk for every inode under the mount.
- `bulkstat_blocks_match_statx` (10,000 random inodes; generate files of varied sizes including sparse ones).
- `bulkstat_ag_ranges_partition`: iterating AG by AG yields the same set as one full-range iteration, with no duplicates.
- Unprivileged: layout tests; `ag_ino_range` arithmetic against known geometries.

### Acceptance

- [ ] All privileged tests pass on Rocky 8.8 **and** 8.10 VMs; results are in the handoff note.
- [ ] `bulkstat_probe` correctly reports v5 availability on each VM (record the finding: it tells us whether EL8 backported v5 bulkstat).
- [ ] No `unsafe` outside `tidemark-sys`; every `unsafe` block is commented.

---

## W1-D — Throttle, ioprio, procfs parsers, attribution cache

**Read first:** `04-scanner.md` §6, `07-live-monitoring.md` §4.3.

### Interfaces

```rust
// tidemark-sys::ioprio
pub fn set_current_thread_idle() -> io::Result<()>;                // ioprio_set(IOPRIO_WHO_PROCESS, 0, IDLE)
pub fn set_current_thread_best_effort(level: u8) -> io::Result<()>;

// tidemark-fs::throttle
pub struct AdaptiveParams { pub max_ops: u32, pub min_ops: u32, pub max_util_pct: f64,
                            pub target_latency_ms: f64, pub interval: Duration }
pub type StatSampler = Box<dyn FnMut() -> io::Result<Vec<DiskStat>> + Send>;
pub struct FixedGate;      // token bucket at a fixed rate; impl Gate
pub struct AdaptiveGate;   // token bucket + controller thread; impl Gate
impl AdaptiveGate {
    pub fn start(p: AdaptiveParams, sampler: StatSampler) -> Arc<AdaptiveGate>;
    pub fn stats(&self) -> GateStats;
    pub fn stop(&self);
}
pub struct GateStats { pub mean_rate: f64, pub min_rate: f64, pub throttled_ms: u64, pub decreases: u64, pub increases: u64 }
/// Pure controller step for unit tests: returns the new rate.
pub fn controller_step(rate: f64, prev: &[DiskStat], cur: &[DiskStat], dt_ms: f64, p: &AdaptiveParams) -> f64;
/// `profile` is a profile name: "gentle" | "normal" | any user-defined profile → AdaptiveGate;
/// "max" → NoopGate (throttle off). Unknown names are a config error caught by Config::validate.
pub fn gate_from_config(profile: &str, cfg: &ThrottleConfig, leaves: &[BlockDevInfo], sampler: StatSampler)
    -> Result<Arc<dyn Gate>, ConfigIssue>;

// tidemark-fs::procfs
pub fn list_pids() -> io::Result<Vec<u32>>;
pub fn read_comm(pid: u32) -> io::Result<String>;
pub fn parse_stat_starttime(stat: &str) -> Option<u64>;   // handle comm with spaces and ')' — use the LAST ')'
pub fn read_starttime(pid: u32) -> io::Result<u64>;
pub fn parse_cgroup_unit(cgroup: &str) -> Option<String>; // v1 "N:name=systemd:/system.slice/x.service", v2 "0::/…"
pub fn read_cgroup_unit(pid: u32) -> io::Result<Option<String>>;
pub fn list_fds(pid: u32) -> io::Result<Vec<u32>>;
pub fn read_fd_target(pid: u32, fd: u32) -> io::Result<Vec<u8>>;
pub fn parse_fdinfo_flags(fdinfo: &str) -> Option<u32>;   // "flags:\t0100002" (octal)
pub fn read_fdinfo_flags(pid: u32, fd: u32) -> io::Result<u32>;
pub fn read_uid(pid: u32) -> io::Result<u32>;

/// One pass over /proc/[pid]/fd/*: every fd whose target is a regular file on one of `devs`.
/// Shared primitive for phantom detection and open writers (W5-B procscan).
pub struct OpenFile { pub pid: u32, pub fd: u32, pub target: Vec<u8> /* readlink text */,
                      pub dev: DevId, pub ino: Ino, pub nlink: u64, pub size: u64, pub disk: u64,
                      pub flags: Option<u32> /* fdinfo flags when want_flags */ }
pub fn scan_open_files(devs: &[DevId], want_flags: bool) -> io::Result<Vec<OpenFile>>;

// tidemark-fs::attribution
pub struct AttributionCache;
impl AttributionCache {
    pub fn new(ttl: Duration, capacity: usize) -> Self;
    pub fn lookup(&self, pid: u32) -> Attribution;          // never fails: unknown → comm "?", unit None
}
```

### Details

- **Token bucket:** `acquire(cost)` blocks (sleeping, not spinning) until tokens are available. Burst = 0.25 s of tokens. Thread-safe and low-contention: use an atomic "next allowed time" (nanoseconds) with a CAS loop; sleep on `parking_lot` only when ahead.
- **Controller:** exactly `04` §6.2. AIMD, worst leaf device. `util = Δio_ticks / Δt_ms × 100`; `await = Δ(read_ticks + write_ticks) / max(1, Δ(read_ios + write_ios))`. If the sampler errors, keep the current rate and log once.
- **Attribution cache:** keyed by `(pid, starttime)`, LRU of `capacity` (default 16,384), TTL refresh. A unit is the last cgroup path component ending in `.service`, `.scope` or `.slice`; user sessions map to `user-<uid>`.

### Tests

- `controller_step` table tests: idle device → rate rises additively to max; util over cap → halves; await over target → halves; min clamp; multiple devices use the worst.
- Token bucket: 4 threads acquiring cost 1 at 1,000/s for 2 s → total within ±5%.
- Parsers: captured real `/proc` samples, including comm with spaces and parentheses, cgroup v1 and v2 lines, user scopes, fdinfo flags with `O_WRONLY|O_APPEND`.
- Attribution: PID reuse (same PID, different starttime) is not served from the stale cache entry.
- `scan_open_files`: a test process opens a file on a tmp filesystem, unlinks it and keeps it open → found with `nlink == 0` and the right size; processes exiting mid-scan (`ENOENT`/`ESRCH`) are skipped silently.
- Privileged: `set_current_thread_idle` succeeds as root; `ioprio_get` returns the idle class; `scan_open_files` sees other users' processes as root.

### Acceptance

- [ ] All tests pass; no `unsafe` outside `tidemark-sys::ioprio`.
- [ ] `AdaptiveGate::stop` joins the controller thread within one interval.

---

## W1-E — Snapshot store, catalog, time series, retention, space guard

**Read first:** `05-storage.md` (all), `02-architecture.md` §7.

### Interfaces

`Snapshot` (reader) exactly as `05` §3, plus:

```rust
pub fn root_slug(root: &RawPath) -> String;   // '/' → '_', '_' → "%5F", bytes outside [A-Za-z0-9.-] → %XX; "/" → "_"

pub struct SnapshotWriter;
impl SnapshotWriter {
    pub fn create(dir: &Path, meta: &SnapshotMeta) -> Result<Self, StoreError>;
    pub fn put_dirs(&mut self, rows: &[DirRow]) -> Result<(), StoreError>;          // ascending id (debug-asserted)
    pub fn put_files(&mut self, rows: &[FileRow]) -> Result<(), StoreError>;
    pub fn put_link_sets(&mut self, rows: &[LinkSetRow]) -> Result<(), StoreError>;
    pub fn put_link_instances(&mut self, rows: &[LinkInstanceRow]) -> Result<(), StoreError>;
    pub fn put_skipped(&mut self, rows: &[SkipRow]) -> Result<(), StoreError>;
    pub fn put_errors(&mut self, rows: &[ErrorRow]) -> Result<(), StoreError>;
    pub fn put_annotations(&mut self, rows: &[AnnotationRow]) -> Result<(), StoreError>;
    pub fn space_check_every(&mut self, rows: u64, check: Box<dyn FnMut() -> SpaceVerdict + Send>);
    pub fn finish(self, final_meta: &SnapshotMeta) -> Result<PathBuf, StoreError>;
    pub fn abort(self);                                                             // also on Drop
}

pub struct ScanEntry { pub scan_id: String, pub root: RawPath, pub mode: ScanMode, pub started_at: i64,
                       pub finished_at: Option<i64>, pub status: String, pub degraded: bool,
                       pub file: Option<PathBuf>, pub t_disk: Option<u64>, pub t_inodes: Option<u64>,
                       pub t_dirs: Option<u64>, pub errors: Option<u64>, pub duration_ms: Option<u64> }
pub struct Catalog;
impl Catalog {
    pub fn open(state_dir: &Path, rw: bool) -> Result<Self, StoreError>;
    pub fn list(&self, root: Option<&RawPath>) -> Result<Vec<ScanEntry>, StoreError>;
    pub fn latest(&self, root: &RawPath) -> Result<Option<ScanEntry>, StoreError>;   // status = complete
    pub fn nearest_before(&self, root: &RawPath, ts: i64) -> Result<Option<ScanEntry>, StoreError>;
    pub fn get(&self, scan_id: &str) -> Result<Option<ScanEntry>, StoreError>;
    pub fn record_start(&self, e: &ScanEntry) -> Result<(), StoreError>;
    pub fn record_finish(&self, e: &ScanEntry) -> Result<(), StoreError>;
    pub fn rebuild_from_files(&self) -> Result<usize, StoreError>;
}

pub struct TimeSeries;     // timeseries.db; WAL; migrations
impl TimeSeries {
    pub fn open(path: &Path, rw: bool) -> Result<Self, StoreError>;
    pub fn insert_fs(&self, s: &[FsSample]) -> Result<(), StoreError>;
    pub fn insert_lvm(&self, s: &[LvmSample]) -> Result<(), StoreError>;
    pub fn insert_dir_series(&self, root: &RawPath, scan_id: &str, ts: i64, rows: &[DirRow]) -> Result<(), StoreError>;
    pub fn insert_hot_minutes(&self, rows: &[HotMinute]) -> Result<(), StoreError>;
    pub fn insert_phantom(&self, rows: &[PhantomSample]) -> Result<(), StoreError>;
    pub fn insert_writers(&self, rows: &[WriterSample]) -> Result<(), StoreError>;
    pub fn insert_pg(&self, s: &[PgSample]) -> Result<(), StoreError>;
    pub fn insert_alert(&self, a: &AlertEvent) -> Result<(), StoreError>;
    pub fn query_fs(&self, mount: &str, from: i64, to: i64) -> Result<Vec<FsSample>, StoreError>;      // picks resolution
    pub fn query_lvm(&self, vg: &str, lv: &str, from: i64, to: i64) -> Result<Vec<LvmSample>, StoreError>;
    pub fn query_pg(&self, kind: &str, name: Option<&str>, from: i64, to: i64) -> Result<Vec<PgSample>, StoreError>;
    pub fn query_hot(&self, mount: &str, scope: &str, from: i64, to: i64, limit: usize) -> Result<Vec<HotMinute>, StoreError>;
    pub fn query_dir_series(&self, root: &RawPath, path: &RawPath, from: i64, to: i64) -> Result<Vec<(i64, u64)>, StoreError>;
    pub fn compact(&self, now: i64, policy: &SampleRetention) -> Result<CompactStats, StoreError>;
}

pub fn retention_plan(entries: &[ScanEntry], policy: &SnapshotRetention, now: i64) -> Vec<String>; // scan_ids to delete
pub enum SpaceVerdict { Ok, Refuse { reason: String } }
pub fn space_check(state_dir: &Path, estimate: u64, policy: &RetentionConfig) -> io::Result<SpaceVerdict>;
pub fn space_check_with(statvfs: impl Fn() -> io::Result<FsTotals>, used_by_state: u64, estimate: u64,
                        policy: &RetentionConfig) -> SpaceVerdict;   // pure, for tests
```

### Details

- Writer PRAGMAs and the finish sequence exactly as `05` §2. The file is created `0640` (umask-independent: `fchmod` after create).
- Reader opens `file:<path>?immutable=1&mode=ro` (rusqlite `OpenFlags::SQLITE_OPEN_URI | SQLITE_OPEN_READ_ONLY`).
- Query methods use the subtree range technique (`05` §1). `children` sorts in SQL by the requested key, with `name` as tie-break for determinism.
- Time-series schema and compaction exactly `05` §6. Migrations are numbered Rust functions; v1 is the initial schema.
- `retention_plan` implements recent / daily / weekly (ISO week) keep rules. Partial and failed entries older than 24 h are always deleted. Pure and table-tested.

### Tests

- Round-trip: write 100k synthetic `DirRow`s plus files and links → read back by id, path, children and subtree, all equal.
- A writer dropped without `finish` leaves no `.db` and removes the `.tmp`.
- `immutable` reader can open while another process holds the file (no locks).
- `root_slug` is injective over a test set (including `/data_reports` vs `/data/reports`).
- `retention_plan`: tables of dates covering DST changes and ISO week boundaries.
- Compaction is idempotent (run twice → same row counts); downsampled values equal the bucket's last raw value.
- `space_check_with` decision table.
- Performance: inserting 2M `DirRow`s takes under 60 s on the dev box (record the number).

### Acceptance

- [ ] All tests pass; schema SQL in the code matches `05` byte-for-byte for column names and types (a test compares `sqlite_master`).
- [ ] The reader API covers every query in `05` §3, with rustdoc examples.

---

## Wave 1 gate checklist

- [ ] All five branches approved and merged in order; `cargo xtask ci` green.
- [ ] Privileged suite green on Rocky 8.8 and 8.10 VMs (W1-A, W1-B, W1-C, W1-D privileged tests).
- [ ] Recorded findings: v5 bulkstat present? `bs_blocks` unit confirmed? Default scheduler on the VM's virtual disks? Captured in `docs/handoff/W1-gate.md`.
- [ ] Gate review agent finds no duplicated helpers across `tidemark-fs` modules and confirms no `unsafe` outside `tidemark-sys`.
- [ ] Tag `wave-1-done`.
