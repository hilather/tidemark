# 04 — Scanner design (L1)

The scanner turns a directory tree into per-directory records with the aggregator inputs defined in `03-link-semantics.md`. It has two modes with an identical output contract.

| Mode | How | Needs | Best when |
|------|-----|-------|-----------|
| **walk** | Parallel `getdents64` + `statx(AT_SYMLINK_NOFOLLOW)` per entry | Read access (root for full coverage) | Any filesystem; scan root is a small part of the filesystem; reference implementation |
| **bulk** | Phase 1: parallel `getdents64` only (names → inode numbers). Phase 2: XFS bulkstat per allocation group, merge-joined by inode number. Phase 3: name lookup for notable files only. | XFS, `CAP_SYS_ADMIN`, memory for the join | The scan root covers most of an XFS filesystem; cold caches on HDD RAID |
| **auto** (default) | Chooses using §7 | — | — |

Walk mode is the **oracle** for bulk mode: on a quiescent tree, both must produce byte-identical snapshot rows.

---

## 1. Output contract (defined in `tidemark-core::scan`, frozen in Wave 0)

```rust
pub type Ino = u64;
pub type DevId = u64;

/// Discovery-order id. Root = DirId(0). Renumbered to preorder when writing the snapshot.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DirId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FileKind { Regular, Dir, Symlink, Other }

/// Bytes by mtime age relative to scan start: <1d, 1–7d, 7–30d, 30–90d, 90–365d, >365d.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgeBuckets(pub [u64; 6]);

/// Stats for direct entries whose inode has nlink == 1.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwnStats {
    pub files: u64,          // non-directory entries (includes symlinks and "other")
    pub symlinks: u64,       // subset of `files`
    pub disk: u64,
    pub apparent: u64,
    pub age: AgeBuckets,     // disk bytes
    pub newest_mtime: i64,
}

/// One direct entry whose inode has nlink > 1.
#[derive(Clone, Debug)]
pub struct LinkRef {
    pub ino: Ino, pub nlink: u32, pub kind: FileKind,
    pub disk: u64, pub apparent: u64, pub mtime: i64,
    pub name: Option<RawName>,   // Some in walk mode; None in bulk mode
}

/// A single-link file that may belong in the snapshot's `files` table.
#[derive(Clone, Debug)]
pub struct NotableFile {
    pub ino: Ino, pub name: Option<RawName>,
    pub disk: u64, pub apparent: u64, pub mtime: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason { Mount, OtherFs, Excluded }
#[derive(Clone, Debug)]
pub struct Skip { pub name: RawName, pub reason: SkipReason }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ErrOp { Open, Getdents, Statx, Fstat, Bulkstat, Readlink }
#[derive(Clone, Debug)]
pub struct ScanErrorRec { pub path: RawPath, pub op: ErrOp, pub errno: i32 }

/// Walk mode: one per directory.
#[derive(Clone, Debug)]
pub struct DirRecord {
    pub id: DirId,
    pub parent: Option<DirId>,
    pub name: Option<RawName>,     // None only for the root record (the root path is AggConfig.root / WalkConfig.root)
    pub ino: Ino, pub dev: DevId, pub mtime: i64,
    pub dir_disk: u64, pub dir_apparent: u64,
    pub projid: Option<u32>,       // walk mode: None
    pub entries: u32,              // all direct entries incl. subdirs and skipped
    pub own: OwnStats,
    pub links: Vec<LinkRef>,
    pub notable: Vec<NotableFile>,
    pub skipped: Vec<Skip>,
    pub errors: Vec<ScanErrorRec>,
    pub races: u32,                // entries that vanished or changed type mid-scan
}

/// Bulk mode phase 1: one per directory, no per-file stat.
#[derive(Clone, Debug)]
pub struct NsRecord {
    pub id: DirId, pub parent: Option<DirId>, pub name: Option<RawName>,   // None only for the root
    pub ino: Ino, pub dev: DevId,
    pub entries: u32,
    pub children: Vec<Ino>,        // non-directory entries' inode numbers (from d_ino)
    pub skipped: Vec<Skip>, pub errors: Vec<ScanErrorRec>, pub races: u32,
}

/// A child directory that was queued (so its DirId exists) but turned out not to be scannable.
#[derive(Clone, Debug)]
pub struct GoneRecord { pub id: DirId, pub parent: DirId, pub name: RawName, pub reason: GoneReason }
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GoneReason { Vanished, Replaced /* no longer a dir, or inode changed */, OtherFs }

pub enum WalkOutput { Dir(DirRecord), Ns(NsRecord), Gone(GoneRecord) }

/// Inode attributes as delivered by the bulk join (and used by the aggregator's bulk inputs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InodeAttrs {
    pub ino: Ino, pub kind: FileKind, pub nlink: u32,
    pub disk: u64, pub apparent: u64, pub mtime: i64,
    pub projid: Option<u32>,
}

/// Throttle seam. `cost` is in "metadata I/O units" (see §6).
pub trait Gate: Send + Sync {
    fn acquire(&self, cost: u32);
    fn current_rate(&self) -> f64;   // units/s currently allowed (for progress output)
}
pub struct NoopGate;
```

## 2. Walk mode

### 2.1 Work distribution

- `threads` workers (default 8, configurable 1–64), each with a `crossbeam_deque::Worker::new_lifo()`, plus a global `Injector` and a `Stealer` per worker.
- **LIFO locally** keeps traversal depth-first per worker, so the pending frontier stays small even in very wide trees. Idle workers steal from others' FIFO end, which takes large subtrees and gives good balance.
- Termination: an atomic count of in-flight tasks. A worker exits when its local deque, the injector and all stealers are empty and in-flight is 0.
- Task: `{ id: DirId, parent: Option<DirId>, name: Option<RawName>, path: Vec<u8>, expect_ino: Option<Ino> }`, carrying the full path bytes and the `d_ino` the parent saw. `DirId`s come from an `AtomicU32` counter.

### 2.2 Per-directory procedure

```
gate.acquire(COST_OPEN_GETDENTS)
fd = openat(AT_FDCWD, path, O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC|O_NOATIME)
     EPERM with O_NOATIME → retry without it and count `noatime_fallbacks` (non-root runs)
     ENOENT/ENOTDIR/ELOOP → race: emit WalkOutput::Gone{reason: Vanished|Replaced}; return
     EACCES/EIO          → error: statx(AT_FDCWD, path, AT_SYMLINK_NOFOLLOW) for ino/mtime/blocks (stat needs only
                           search permission on the parent); emit a DirRecord with entries = 0 and the error, so
                           the tree stays connected and NOT NULL columns are filled
st = fstat(fd)
if st.dev != root_dev → emit Gone{reason: OtherFs} (the aggregator records Skip(OtherFs) on the parent); return
if expect_ino.is_some() && st.ino != expect_ino → an ancestor was swapped mid-scan (possibly for a symlink, which
                       O_NOFOLLOW only checks on the last component): emit Gone{reason: Replaced}; return
rec = DirRecord { ino: st.ino, dev: st.dev, mtime, dir_disk: st.blocks*512, dir_apparent: st.size, .. }
loop getdents64(fd, buf[getdents_buf = 256 KiB]):
  for each entry (skip "." and ".."):
    rec.entries += 1
    child = path + "/" + name
    kind = d_type, or statx-classify when DT_UNKNOWN (ftype=0 filesystems)
    if kind == Dir:
        if child ∈ mount_points → rec.skipped.push(Mount); continue
        if exclude.matches(child) → rec.skipped.push(Excluded); continue
        push Task{ id: next_id(), parent: rec.id, name, path: child, expect_ino: Some(d_ino) } to the local deque
    else:
        if exclude.matches(child) → rec.skipped.push(Excluded); continue
        gate.acquire(COST_STATX)
        sx = statx(fd, name, AT_SYMLINK_NOFOLLOW|AT_NO_AUTOMOUNT,
                   STATX_TYPE|STATX_MODE|STATX_NLINK|STATX_INO|STATX_SIZE|STATX_BLOCKS|STATX_MTIME)
             ENOENT → rec.races += 1; continue
        if sx is a directory (raced) → rec.races += 1; continue
        if sx.nlink > 1 → rec.links.push(LinkRef{ name: Some(name), .. })
        else → rec.own.add(sx, age_bucket(scan_start, sx.mtime)); maybe rec.notable.push(..)
close(fd)
out.send(WalkOutput::Dir(rec))
```

Notes:

- **`O_NOATIME` matters on RAID 5.** `getdents` counts as a read access. Under `relatime`, the first scan touching a directory more than 24 h after its last access updates its atime: a metadata **write** per directory, which turns a read-only scan into millions of small random writes on parity RAID. Root (`CAP_FOWNER`) can always use `O_NOATIME`. `statx` never updates atime.
- Only one directory fd per worker is open at a time, so fd usage is bounded by `threads + small constant`.
- `mount_points` is built once from `/proc/self/mountinfo`. It is the set of mount targets strictly under the root; mountinfo octal escapes such as `\040` must be decoded. It catches same-device bind mounts, which `st_dev` cannot.
- Notable-file admission: emit if `disk ≥ big_file_threshold` or if the worker-local min-heap of size `top_files` would admit it. The aggregator keeps the global top-N plus everything at or above the threshold, capped at `max_notable` (default 200,000).
- Each worker calls `ioprio_set(IOPRIO_CLASS_IDLE)` for itself (I/O priority is per thread) and the process runs at `nice 10`. These are best-effort only; see §6.

The aggregator tolerates `DirId` holes: a `Gone` record removes that id from the tree and, for `OtherFs`, adds a `Skip` to the parent. On a quiescent tree no `Gone` records occur, so bulk/walk equality is unaffected.

### 2.3 Output ordering

Records arrive in nondeterministic order. The aggregator stores them by `DirId` in a `Vec` and only depends on parent links. Determinism is restored by the preorder renumbering (`03` §4 Step F).

## 3. Bulk mode

### 3.1 Phase 1 — namespace walk (`getdents64` only)

The same walker with `stat_entries = false` emits `NsRecord`s:

- For each subdirectory, push a task exactly as in walk mode.
- For each non-directory entry, record `d_ino` only; no `statx`.
- The directory itself still gets `open` + `fstat`. The inode was read for `open` anyway, and this yields `dev` for the filesystem check.

A collector thread turns `NsRecord`s into:

- **Directory skeleton:** `Vec<DirSkel { parent, name, ino, entries, skipped, errors, races }>` indexed by `DirId`.
- **Dent list:** `Vec<DentRef>`, where `DentRef = { ino: u64, slot: u32 }` packed into 12 bytes (for example `[u32; 3]`).
  - `slot & 0x8000_0000 == 0` means "entry in directory `slot`".
  - `slot & 0x8000_0000 != 0` means "this inode **is** directory `slot & 0x7FFF_FFFF`". One self-reference is added per directory so the join also delivers each directory's own blocks, mtime and project ID.

### 3.2 Phase 2 — bulkstat join

1. `par_sort_unstable_by_key(|d| d.ino)` on the dent list. It sorts in place with no extra buffer.
2. Read the XFS geometry. Inode numbers encode the allocation group: `agino_bits = agblklog + inopblog`, where `agblklog = ceil(log2(agblocks))` and `inopblog = log2(blocksize / inodesize)`. AG *k* covers inode numbers `[k << agino_bits, (k+1) << agino_bits)`.
3. For each AG, run in parallel up to `threads` workers:
   - Locate the dent slice for the AG's range with `partition_point`.
   - Stream bulkstat over the AG range in batches (default 4,096 inodes per call). Bulkstat returns inodes in ascending order.
   - **Merge-join** the two ascending sequences:
     - Dents with no matching inode → the inode was deleted after phase 1; `races += 1`.
     - Inodes with no dents → not under the root, or created after phase 1; ignore.
     - Equal keys → gather all dents for this inode (they are adjacent) into a `JoinedInode { attrs, dirs: SmallVec<[DirId; 2]>, self_dir: Option<DirId> }`.
   - Send batches of 4,096 `JoinedInode`s to the aggregator.
4. The aggregator consumes them:
   - `self_dir` sets the directory's `dir_disk`, `dir_apparent`, `mtime` and `projid`.
   - A regular/symlink/other inode with `nlink == 1` and one dir adds to that directory's `OwnStats`.
   - `nlink > 1` becomes a `LinkSet` whose `dirs` are every link inside the root, since they arrive together. Completeness is still `found ≥ nlink` (`03` §2); links outside the root make it incomplete. No hash map is needed.
   - A kind mismatch (dent said file, bulkstat says directory) counts as a race and is ignored.

Inodes that changed between phase 1 and phase 2 make the snapshot fuzzy. That is acceptable and bounded by the recorded start and end times.

### 3.3 Phase 3 — names for notable files

After aggregation, the global top-N and over-threshold files are known by `(DirId, ino)` but have no names. Group them by directory. For each directory, `open` + `getdents64` and pick the wanted inode numbers. This touches at most `max_notable` directories and is throttled.

**Hard-link instance names are not resolved in bulk mode.** `link_instances.name` stays `NULL`, and reports resolve names lazily by reading the live directory (`links --show-paths`). This is labelled "live" in output, because the directory may have changed since the snapshot.

## 4. Things both modes must do identically

- One filesystem only: skip mounts (mountinfo) and other devices (`st_dev`).
- Never follow symlinks: `O_NOFOLLOW` on directory open, `AT_SYMLINK_NOFOLLOW` on statx, `d_type`/mode-based classification.
- Apply `exclude` globs (`globset`) to full paths, for directories and entries alike.
- Count a directory's own inode blocks.
- Use the same age-bucket function (`tidemark_core::time::age_bucket(scan_start, mtime)`).
- Treat `ENOENT`/`ENOTDIR`/`ELOOP` on entries as races, not errors.
- Record `EACCES`/`EIO` and other errors in the record (capped at 10,000 stored errors per scan; counts are never capped).

## 5. XFS specifics

### 5.1 Interfaces (implemented in `tidemark-sys::xfs`, W1-C)

| Need | Interface | Notes |
|------|-----------|-------|
| Geometry | `XFS_IOC_FSGEOMETRY_V1` (ioctl nr 100, always present) → blocksize, agblocks, agcount, inodesize, uuid **and the `flags` word**, which already carries `XFS_FSOP_GEOM_FLAGS_V5SB`, `…_FTYPE` and `…_REFLINK`. Don't rely on the newer geometry ioctls: in EL8's xfsprogs headers `XFS_IOC_FSGEOMETRY` names the v5 struct (nr 126), which a 4.18-based kernel may not implement. | Read-only |
| Bulk inode stats | `XFS_IOC_FSBULKSTAT` with `struct xfs_fsop_bulkreq` / `struct xfs_bstat` (v1, present since early XFS). Probe for v5 `XFS_IOC_BULKSTAT` (`struct xfs_bulk_ireq`, per-AG flag) and prefer it when present. | **Requires `CAP_SYS_ADMIN`** |

**Generate the FFI *structs* with `bindgen` from `<xfs/xfs_fs.h>` (xfsprogs-devel).** Commit the generated code; don't regenerate at build time. bindgen does not expand function-like macros, so **hand-define the ioctl request numbers** (`_IOR`/`_IOWR` arithmetic in a `const fn`), each with a test that checks its size field against `size_of` of the generated struct. Add `size_of` and `offset_of` tests for every struct. In `xfs_bstat`, the 16-bit fields are `bs_mode`, `bs_projid_lo`/`bs_projid_hi` and `bs_forkoff` (`bs_nlink` is 32-bit), and the timestamp structs have padding, so hand-written layouts are a classic source of bugs.

### 5.2 Iteration ranges

v1 bulkstat returns inodes with number `> *lastip`. For AG *k*, start with `lastip = (k << agino_bits) − 1` (or 0 for k = 0) and stop at the first inode ≥ `(k+1) << agino_bits`.

### 5.3 What bulkstat gives us for free

`bs_projid_lo/hi` gives the project ID of every inode. v1 only records each directory's ID (`dirs.projid`, normally 0) so a future project-quota add-on (`06-project-quotas-deferred.md`) has the data; nothing in v1 depends on it.

### 5.4 Units validation (mandatory test)

`bs_blocks` is expected in filesystem blocks, not 512-byte units. **Do not trust that.** The privileged test `bulkstat_blocks_match_statx` picks 10,000 random inodes on a loop XFS image and asserts `bulk_disk_bytes(ino) == statx.blocks * 512` for each. The conversion constant lives in one function with this test next to it.

## 6. Throttling

`ionice -c3` (idle class) is honoured only by I/O schedulers that implement priorities, such as BFQ. RHEL 8 commonly runs `mq-deadline` on rotational disks and RAID LUNs, where the idle class may do nothing. The probe reports each leaf device's scheduler. **Tidemark's own throttle is the primary protection.**

### 6.1 Cost units (defaults; tuned in W7-B)

| Operation | Cost |
|-----------|------|
| Directory open + each `getdents64` call | 1 |
| `statx` | 1 |
| Bulkstat call returning *n* inodes | `ceil(n / inodes_per_cluster)`, where `inodes_per_cluster = 32` for 512-byte inodes |
| Phase 3 directory re-read | 1 |

### 6.2 Adaptive gate (`tidemark-fs::throttle`, W1-D)

- Token bucket with rate *r* (units/s), bounded by `[min_ops_per_sec, max_ops_per_sec]`.
- A controller thread samples `/sys/dev/block/<maj:min>/stat` for the **leaf devices** every `sample_interval` (500 ms). Leaf devices come from `st_dev` → resolve partitions to their parent disk → recurse `slaves/` (dm/LVM/md). Hardware RAID appears as one leaf such as `sdb`.
- Per device, it computes:
  - `util = Δio_ticks / Δt`
  - `await = Δ(read_ticks + write_ticks) / Δ(read_ios + write_ios)`
- It takes the **worst** device.
- **`await` is the primary signal.** `%util` is only meaningful for single-queue rotational disks (md members, plain SATA/SAS). For a hardware-RAID LUN, or any blk-mq device that serves requests in parallel, `%util` reads 100% while capacity remains, so it is ignored for leaves that are non-rotational or report more than one hardware queue (`/sys/block/<dev>/mq/` has more than one entry).
- AIMD:
  - If `util > max_util_pct` or `await > target_latency_ms`, then `r = max(min, r × 0.5)`.
  - Otherwise `r = min(max, r + 0.05 × max)`.
- Profiles:

  | Profile | Behaviour |
  |---------|-----------|
  | `gentle` | `max_util_pct = 50`, `target_latency_ms = 15` |
  | `normal` | `max_util_pct = 70`, `target_latency_ms = 25` (default) |
  | `max` | Throttle off |

  Real targets come from the pilot baseline.
- Our own I/O counts toward `util` on purpose: the goal is to keep the array below the cap, whoever is causing the load.

## 7. Mode selection (`auto`)

Choose **bulk** if all of the following hold, otherwise choose **walk** and log the reason:

1. The filesystem is XFS, bulkstat works (probe), and the effective UID has `CAP_SYS_ADMIN`.
2. `ftype = 1` (phase 1 needs `d_type` to find directories without stat).
3. The root covers ≥ 50% of the filesystem's used inodes, estimated from the previous snapshot's `t_inodes` or assumed true when the root is the mount point. Bulkstat cost scales with the **whole filesystem**, not the root.
4. The memory estimate fits `memory_budget`:
   ```
   used_inodes = f_files − f_ffree                 (statvfs)
   dents      ≈ used_inodes × 1.1                  (hard links add entries)
   dirs       ≈ prev_snapshot.t_dirs or used_inodes × 0.05
   bytes      ≈ dents × 12 + dirs × 320 + 64 MiB
   ```

The walk-mode estimate is `dirs × 320 + link_entries × 72 + 64 MiB`. If it exceeds the budget, the scan still runs but logs a warning; the cap is enforced by the systemd `MemoryLimit`.

## 8. Cancellation, failures and partial snapshots

- SIGINT/SIGTERM or an API `scan.cancel` sets an `AtomicBool`. Workers stop taking tasks, finish their current directory and exit.
- A cancelled or failed scan deletes its `.db.tmp`, unless `--keep-partial` is given, in which case the snapshot is written with `meta.status = partial` and is never chosen as "latest" by default.
- A scan whose error count is > 0 is `status = complete` with `meta.degraded = true`. Reports show this.

## 9. Progress and observability

- Every 2 s, emit progress to stderr (CLI) or tracing (daemon): directories done, entries, errors, races, current gate rate, elapsed time, and an ETA based on the previous snapshot's counts when available.
- At the end, write a summary into `meta`: mode, threads, durations per phase, gate statistics (mean rate, time throttled), `noatime_fallbacks`, races and errors.

## 10. What performance to expect, and how we'll know

There are deliberately no promised numbers until the pilot measures the real array. The cost model behind the design:

- **Cold walk mode** reads every directory's blocks plus the inode cluster holding each inode. XFS allocates inodes in chunks near their parent directory, so `statx` in `getdents` order often hits clusters already read. Parallelism (`threads`) keeps several spindles busy.
- **Cold bulk mode** reads inode clusters in on-disk order per AG (large, mostly sequential reads), plus directory blocks. This is the closest thing to "read the MFT". It should win most on HDD RAID with cold caches.
- **Warm caches**: both are CPU-bound. Bulk avoids millions of path lookups and syscalls.

`xtask bench` (W7-B) measures `du -sx`, walk and bulk, each cold (staging only: `echo 3 > /proc/sys/vm/drop_caches`) and warm, with and without a concurrent `pgbench`. Defaults (threads, batch sizes, costs, profiles) are set from those results.
