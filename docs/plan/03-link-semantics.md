# 03 — Link semantics and size metrics (normative spec)

This document is **normative**: the aggregator (W2-A), both scan modes, the reports and all tests must follow it exactly. The worked example in §6 is a golden test.

## 1. Why one number is not enough

With hard links, "the size of a directory" means different things for different questions:

- *How much does this directory hold?* That's what `du` reports. Sibling sizes can add up to **more** than their parent, because a hard-linked file inside two siblings is counted in each.
- *How much space would deleting this directory free?* Only inodes whose **every** link is inside the directory, and that aren't held open after deletion.
- *Split the disk among directories so the parts add up to the whole*, for treemaps and sorting. Each inode must be charged to exactly one place.

Tidemark computes all three for every directory.

## 2. Definitions

Let **R** be the scan root. Let **T(d)** be the subtree of directory *d*, including *d*. Directories form a tree: there are no hard links to directories, and mount points and bind mounts are skipped (§5).

For every non-directory inode *i* reached through one or more directory entries inside R:

| Symbol | Meaning |
|--------|---------|
| `disk(i)` | Allocated bytes: `st_blocks × 512` (walk mode), or the bulkstat equivalent (bulk mode; must be validated equal, see `04-scanner.md` §5.4) |
| `apparent(i)` | `st_size` |
| `nlink(i)` | Link count reported by the kernel |
| `L(i)` | Multiset of directories inside R holding an entry for *i*. Two links in the same directory count twice. |
| `found(i)` | `|L(i)|` |
| `complete(i)` | `found(i) ≥ nlink(i)`: every link is inside R (≥ rather than = to tolerate races where a link is removed mid-scan) |
| `LCA(i)` | The lowest common ancestor of all directories in `L(i)` |

Directory inodes contribute their own `disk`/`apparent` (directory blocks; small XFS directories are stored inside the inode and have 0 blocks) to themselves.

## 3. Metrics (per directory *d*)

| Column | Name in CLI | Definition | Additive across siblings? |
|--------|-------------|------------|---------------------------|
| `t_disk` | SIZE | Σ `disk(i)` over **distinct** inodes with at least one link in T(d), plus Σ disk of directory inodes in T(d). Equals `du -sxB1 d`. | No |
| `t_apparent` | APPARENT | Same as `t_disk` using `apparent`. Equals `du -sxB1 --apparent-size d` after the oracle normalization below (`09` §3). | No |
| `t_charged` | CHARGED | Every inode is charged to exactly one directory: its single directory if `found(i) = 1`, otherwise `LCA(i)`. `t_charged(d)` = Σ charged to directories in T(d) + directory inodes in T(d). | **Yes** |
| `t_reclaimable` | RECLAIM | Like `t_charged`, but only inodes with `complete(i)`. "What deleting T(d) would free", ignoring open file handles and reflink-shared extents. | Yes |
| `shared` | SHARED | `t_disk − t_reclaimable`. Bytes visible in T(d) but kept alive by links elsewhere. | — |
| `t_files` | FILES | Count of non-directory **entries** (link instances) in T(d), like `find d -not -type d \| wc -l` | Yes |
| `t_inodes` | INODES | Count of **distinct** non-directory inodes in T(d) (union semantics, like `t_disk`) | No |
| `t_dirs` | DIRS | Directories in T(d), including *d* | Yes |
| `age0..age5` | AGE | `t_charged` bytes split by inode mtime age, relative to scan start: `<1d`, `1–7d`, `7–30d`, `30–90d`, `90–365d`, `>365d`. Future mtimes count as `age0`. | Yes |
| `newest_mtime` | NEWEST | max mtime over the directory inodes in T(d) and every non-directory inode with at least one link in T(d). Computed as a rolled-up max, so a link set's mtime counts in every directory that holds one of its links. | — |
| `max_entries` | WIDEST | max direct-entry count of any directory in T(d) | — |
| `t_errors` | ERR | Errors encountered in T(d) (permission, I/O). Values for *d* may be incomplete if > 0. | Yes |

**Apparent-size oracle:** the definition includes directory `st_size` (§2) unchanged. GNU coreutils 8.30 (Rocky Linux 8) counts directory sizes in apparent mode, so direct equality holds on the target. Coreutils 9.2 and later count only regular files and symbolic links; their output differs by the subtree's directory sizes (and any other non-regular, non-symlink inode with non-zero `st_size`). Tests probe the local `du` with an empty directory, never branch on a version string, and when it omits directory sizes add those measured apparent sizes to the `du` figure before comparing, counting distinct inodes consistently with the metric. `t_disk` is compared directly with `du -sxB1` on every version. See `09-testing.md` §3.

**Invariants** (checked by property tests and asserted in debug builds):

1. For the root: `t_charged(R) = t_disk(R)`.
2. `t_reclaimable(d) ≤ t_charged(d)` and `t_reclaimable(d) ≤ t_disk(d)` for every *d*.
3. `t_charged(d) = own_charged(d) + Σ t_charged(children)`; the same holds for `t_reclaimable`, `t_files`, `t_dirs`, `age*` and `t_errors`.
4. `t_disk(d) ≥ max over children c of t_disk(c)` and `t_disk(d) ≤ own_disk(d) + Σ t_disk(children)`, where `own_disk(d)` = *d*'s directory-inode blocks + Σ disk of distinct inodes with an entry directly in *d*.
5. `Σ age* (d) = t_charged(d)`.
6. Running the scan twice on an unchanged tree yields identical rows (excluding timestamps).

## 4. Algorithms (aggregator)

Inputs come from the scanner (see `04-scanner.md`):

- Per directory: "own" stats for its direct entries whose inode has `nlink = 1`. The scanner accumulates these; they are always reclaimable.
- Per directory: its own directory inode's disk/apparent.
- For every direct entry whose inode has `nlink > 1`: one `LinkRef` (one per link instance found).

**Step A — group link instances by inode.** Build `FxHashMap<Ino, LinkSet>` from all `LinkRef`s, where `LinkSet { nlink, disk, apparent, mtime, dirs: SmallVec<[DirId; 2]> }`. If `LinkRef`s for one inode disagree on `nlink` or size (races), take the **max** `nlink` (conservative for `complete`) and the values from the last-seen stat. In bulk mode the join already produces complete sets, so no hash map is needed.

**Step B — classify each link set.**

- `found = dirs.len()`.
- If `found = 1`: add to that directory's own `disk`, `apparent`, `charged`, `files`, `inodes` and age bucket, exactly like a single-link file, **but not** to `reclaimable`. Its kernel link count is above 1 while only one link was found, so the other links are outside R (or in excluded or unreadable areas). Deleting this directory would not free it.
- If `found ≥ 2`:
  - `lca = LCA(dirs)`.
  - `charged_at[lca] += disk`; `age_at[lca][bucket] += disk`.
  - If `complete`: `reclaim_at[lca] += disk`.
  - Union contributions (Step C).
  - `files_at[d] += 1` for each `d` in `dirs` (link instances count everywhere they appear).

**Step C — union contributions (`t_disk`, `t_apparent`, `t_inodes`) by stamping.** Keep a per-directory `stamp: u64`, initialised to 0, and a global counter `seq`. For each link set with `found ≥ 2`:

```
seq += 1
for d in set.dirs:
    v = d
    while v != NONE and stamp[v] != seq:
        stamp[v] = seq
        union_disk[v]     += set.disk
        union_apparent[v] += set.apparent
        union_inodes[v]   += 1
        v = parent[v]
```

This is correct because when the climb meets a node already stamped for this inode, every ancestor of that node was stamped on an earlier climb. These `union_*` values are **already cumulative** and must not be rolled up again. The cost is O(Σ path lengths), about `found × depth` per set.

*Optimisation (only if profiling demands it):* the Euler-tour trick. Sort `dirs` by preorder index, add `+disk` at each, add `−disk` at the LCA of each consecutive pair, then roll up normally. This gives identical results.

**Step D — LCA.** Directories carry `parent` and `depth`. `LCA(a, b)`: lift the deeper node until depths match, then lift both until equal. For a set, fold pairwise. Depth is typically under 30, so this is cheap.

**Step E — roll-up.** Bucket directories by depth. From the deepest level up, add each directory's subtree totals of the additive metrics into its parent. Then compute:

```
t_disk(d)     = rolled_single_disk(d)     + rolled_dir_inode_disk(d)     + union_disk(d)
t_apparent(d) = rolled_single_apparent(d) + rolled_dir_inode_apparent(d) + union_apparent(d)
t_inodes(d)   = rolled_single_inodes(d)   + union_inodes(d)
t_charged(d)  = rolled(own_single_disk + charged_at + dir_inode_disk)(d)
t_reclaimable(d) = rolled(own_single_reclaimable + reclaim_at + dir_inode_disk)(d)
```

Here `rolled_single_*` covers inodes whose found count is 1, which includes `nlink = 1`. Directory inode blocks count as reclaimable, because deleting the subtree frees them.

**Age buckets:** single-found inodes go into their directory's bucket by their own mtime; link sets go into the LCA's bucket by the inode's mtime; **directory-inode blocks go into the directory's own bucket by the directory's mtime**. Then roll up, so invariant 5 (`Σ age* = t_charged`) holds exactly.

**Step F — renumber.** Assign final IDs in preorder with children sorted by name bytes (deterministic), and compute `subtree_end` (the last preorder ID in the subtree). Snapshot IDs equal these preorder IDs.

**Step G — notable files (deterministic).** Candidates are single-link files and link sets. A link set's **representative instance** is the one with the smallest `(final dir id, name bytes)`; in bulk mode, phase 3 picks the smallest name among that directory's entries with that inode. Selection order is `(disk desc, final dir id asc, ino asc)`; names are not used, so walk and bulk mode select identically. Keep every candidate at or above `big_file_threshold`, plus the first `top_files` in that order, capped at `max_notable` in the same order.

## 5. Symlinks, mounts, special cases

| Case | Rule |
|------|------|
| **Symlink** | Never followed, by scanners or reports. It counts as a non-directory entry with its own inode (`lstat` semantics): usually 0 blocks on XFS ("fast" symlinks live in the inode). It counts in `t_files` and `t_inodes`. |
| **Symlink to a directory** | Not traversed. This prevents double counting and loops. `tidemark symlinks --check-targets` (FR-5) can report targets, dangling links, and links that escape the root. |
| **Query path through a symlink** | The CLI canonicalises the query path (`realpath`) on the live system before looking it up, and prints the canonical path. |
| **Mount points under R** | Skipped and recorded in `skipped` with reason `mount` or `other_fs`. Detected from `/proc/self/mountinfo` (catches same-device bind mounts) **and** by `st_dev` comparison on each opened directory. |
| **Bind mount of part of the same filesystem inside R** | Skipped as above, so the bound inodes are counted once, under their real location. |
| **Excluded paths** (config globs) | Skipped and recorded with reason `excluded`. Links found under excluded paths aren't counted, so affected sets look incomplete, which is conservative for `t_reclaimable`. |
| **Hard link race** (link created/removed during scan) | `complete` uses `≥`. Snapshots are "fuzzy" by nature; `meta.started_at` and `meta.finished_at` bound the fuzziness. |
| **Sparse files** | `disk < apparent`; both are reported. |
| **XFS speculative preallocation** | Files being appended can briefly show extra allocated blocks beyond EOF. That is real allocated space at that moment. Document it; don't correct for it. |
| **Reflinked files** (`cp --reflink`) | Separate inodes that share extents. `t_disk` overstates physical usage, as `du` does. Exact detection needs per-file FIEMAP (`FIEMAP_EXTENT_SHARED`), which is out of scope for v1. The probe reports whether reflink is enabled so operators know this caveat applies. |
| **Deleted-but-open files** | Not in any directory, so invisible to scans. Covered by the live layer (`07-live-monitoring.md` §4.1); they still count in `df`/`statvfs` totals. This is the usual cause of a `df` vs `du` mismatch. |
| **Postgres data directory** | Scanned like any other tree (few, large files). Its sizes in a snapshot are a moment-in-time view. |
| **Non-UTF-8 names** | Stored and compared as raw bytes (`RawName`/`RawPath`). Displayed with lossy conversion plus an escape marker; JSON output uses `"path_b64"` when not valid UTF-8. |

## 6. Worked example (golden test `link_semantics_basic`)

Fixture (directory inode blocks are 0 here, because small XFS directories are inline; tests on other filesystems must subtract measured directory blocks):

```
/r
├── a/
│   ├── x        inode 100, nlink 2, disk 4096        (link 1 of 2)
│   └── big      inode 101, nlink 1, disk 1048576
├── b/
│   ├── y        inode 100                            (link 2 of 2)
│   └── z        inode 102, nlink 3, disk 8192        (only 1 of 3 links inside /r)
└── c/
    ├── s  ->  ../a   symlink, inode 104, disk 0
    ├── d/
    │   └── w    inode 103, nlink 2, disk 12288       (link 1 of 2)
    └── e/
        └── v    inode 103                            (link 2 of 2)
```

Link sets: 100 → {a, b}, complete, LCA `/r` · 102 → {b}, found 1 < nlink 3, incomplete · 103 → {c/d, c/e}, complete, LCA `/r/c`.

| Directory | t_disk | t_charged | t_reclaimable | shared | t_files | t_inodes | t_dirs |
|-----------|-------:|----------:|--------------:|-------:|--------:|---------:|-------:|
| `/r/a` | 1,052,672 | 1,048,576 | 1,048,576 | 4,096 | 2 | 2 | 1 |
| `/r/b` | 12,288 | 8,192 | 0 | 12,288 | 2 | 2 | 1 |
| `/r/c/d` | 12,288 | 0 | 0 | 12,288 | 1 | 1 | 1 |
| `/r/c/e` | 12,288 | 0 | 0 | 12,288 | 1 | 1 | 1 |
| `/r/c` | 12,288 | 12,288 | 12,288 | 0 | 3 | 2 | 3 |
| `/r` | 1,073,152 | 1,073,152 | 1,064,960 | 8,192 | 7 | 5 | 6 |

Read the table as follows:

- Siblings `a + b + c` by `t_disk` = 1,077,248 > 1,073,152 for `/r`. That's the double-counted inode 100, which is why `t_disk` is not additive.
- By `t_charged`: `own(/r) 4,096 + a 1,048,576 + b 8,192 + c 12,288 = 1,073,152`. It's additive and equals `t_disk(/r)`.
- Deleting `/r/b` frees nothing: inode 100 lives on in `a`, and inode 102 is pinned by links outside `/r`.
- Deleting `/r/c` frees 12,288, even though neither `c/d` nor `c/e` alone frees anything.
- `/r`'s `shared` of 8,192 is exactly inode 102, kept alive from outside the root.
- `du -sxB1 /r/a` prints 1,052,672, and `du -sxB1 /r` prints 1,073,152. The oracle test asserts both.

## 7. Display conventions

- Default sort is `t_disk` (what people expect from `du`). `--sort reclaimable` answers "what should I delete".
- In tables, a directory with `shared > 0` gets an `H` marker; one with `t_errors > 0` gets `!`.
- Treemaps and the TUI percentage bar use `t_charged`, so the parts add up to the whole.
- Every report header states the snapshot ID, mode, age and whether the snapshot is `degraded`.
