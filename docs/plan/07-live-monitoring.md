# 07 — Live monitoring (L0 sampling + L2)

Goal: within about a minute of something starting to fill the disk, `tidemark now` names the filesystem and its growth rate, the hot directories, and the process or systemd unit doing it. It also shows hidden consumers (deleted-but-open files).

All live components run inside `tidemarkd` and write to `LiveState` (in memory) and `timeseries.db` (persisted).

---

## 1. Filesystem and LVM samplers

A normalized sampler.mounts/watch.mounts path must exactly equal a mount point in the daemon namespace; identity is topmost mount id plus device major/minor. Absent entries produce no samples/persistence or watcher and report `not mounted`. Presence checks pick up arrivals (sampler at most ten seconds, watcher every second); identity changes release the old watcher before replacement. Open sampling descriptor identity is checked before statvfs. Covered scheduled and requested scan roots are refused before run_scan, following existing error/backoff handling; unrelated roots remain eligible. Firstboot is unhealthy for absent entries, but startup readiness is prompt.

Open: clearing in-memory filesystem rate series/removing stale samples across appearance/disappearance/remount; current removable missing-mount reasons in `now.degraded`; daemon consumption of retained watcher mark identity and the residual check-to-mark race. Service persistence deduplication reset is not a rate-ring reset. The proposed future state behavior in W6-mount-presence is not claimed implemented. (13 R4-9.)

| Source | Call | Interval | Stored as |
|--------|------|----------|-----------|
| Filesystem totals for each tracked mount (`/`, `/data`) | `statvfs(mount)`: size, used, avail (`f_bavail`), inodes, inodes used | 30 s | `fs_samples`, `LiveState.fs[mount]` |
| LVM | `lvs --reportformat json --units b --nosuffix -o vg_name,lv_name,lv_attr,segtype,lv_size,data_percent,metadata_percent,pool_lv,lv_dm_path,lv_kernel_major,lv_kernel_minor` and `vgs --reportformat json --units b --nosuffix -o vg_name,vg_size,vg_free` | 5 min | `lvm_samples`, `LiveState.lvm` |

LVM details:

- Each tracked mount is mapped to its LV once at startup (and on reload): the mount's device number (major:minor from `/proc/self/mountinfo`) is matched against `lv_kernel_major`/`lv_kernel_minor`. This is robust whether the mount source is spelled `/dev/mapper/vg00-data`, `/dev/vg00/data` or `/dev/dm-3`.
- If the LV is **thin** (`segtype = thin`), its pool (`pool_lv`) is tracked as well. Its `data_percent` and `metadata_percent` are the numbers that matter: a thin pool can run out while `df` still shows free space on the filesystem, and an exhausted pool is far worse than a full filesystem.
- `vg_free` is reported as **growth headroom**: how much `lvextend` + `xfs_growfs` could add. Tidemark only reports it and never changes LVM.
- **Do not use `--readonly`.** It stops LVM from querying the device-mapper kernel driver, so `data_percent`, `metadata_percent` and the kernel major/minor come back empty. Plain `lvs`/`vgs` are reporting commands: they take only a brief shared VG lock and never write metadata. The collector runs `lvs`/`vgs` with a 30 s timeout, and failures back off (5, 10, 20 … up to 60 minutes) without affecting other samplers.
- Not on LVM, or `lvm2` not installed: the section reports `n/a`.

The daemon-served `lvm` report keeps `LvmResult { mounts: Vec<(String, LvmNow)>, vgs: Vec<VgInfo>, sampled_at: i64 }`. Compared with the direct command it omits segment type and the other/unmounted LV inventory and columns. No wire extension or new flag is added. `W6-A3.md` specifies that the CLI still reads 24-hour trends read-only from the store with the direct command's query/slope/window; missing segment/other-volume columns are omitted and a notice explains the reduced view.

## 2. Rates and time-to-full (`tidemark-live::rates`, W5-D)

- **Rate** = least-squares slope of `(ts, used)` over a window. Windows are 5 min, 15 min and 1 h, and need at least 3 samples. A slope is robust to the sawtooth of temp files being created and deleted, unlike first-minus-last.
- **Time-to-full** = `avail / rate` when `rate > 0`, otherwise "not filling". Shown from the 1 h window, with the 15 min value alongside.
- The same functions apply to thin-pool usage and `dir_series` (scan-to-scan growth).

```rust
pub fn slope_bytes_per_sec(samples: &[(i64 /*unix s*/, i64 /*bytes*/)]) -> Option<f64>;
pub fn time_to_full(avail: u64, rate: f64) -> Option<Duration>;
```

## 3. fanotify watcher (`tidemark-live::watch`, W5-A)

### 3.1 Modes and probing

Measured on Rocky 8.8 (4.18.0-477) and 8.10 (-553): fd mode, no FID mode on either. Filesystem marks work on 8.10; 8.8 uses mount marks, so writes through another mount of the same filesystem are invisible (13 R4-15). Continue probing on other kernels.

| Mode | Kernel need (upstream) | What we get | Default |
|------|------------------------|-------------|---------|
| **fd mode** | fanotify 2.6.37+ | `FAN_CLOSE_WRITE` (and optionally `FAN_MODIFY`) with an open fd to the file and the writer's PID. Path via `readlink(/proc/self/fd/N)`, size via `fstat`. | **Yes**: works on any EL8 kernel |
| **FID+name mode** | `FAN_REPORT_FID` 5.1, `FAN_REPORT_DFID_NAME` 5.9, `FAN_MARK_FILESYSTEM` 4.20 | Adds create / delete / rename events with directory handle + name, and whole-filesystem marks (sees writes through every mount of the filesystem) | Only if the probe succeeds |

RHEL 8 is 4.18-based with selective backports, and whether these FID features were backported isn't documented anywhere we could confirm. So **probe, don't assume**:

1. Try `fanotify_init(FAN_CLASS_NOTIF | FAN_REPORT_DFID_NAME, …)`. On `EINVAL`, use fd mode. FID-only reporting (no names) is probed and reported, but not used: it would need a handle→path lookup per event.
2. With a FID mode, try `FAN_MARK_FILESYSTEM`, falling back to `FAN_MARK_MOUNT`.

`tidemark probe` prints the result, and the watcher logs the mode it chose.

### 3.2 fd mode in detail

```
fd = fanotify_init(FAN_CLASS_NOTIF | FAN_CLOEXEC, O_RDONLY | O_LARGEFILE | O_CLOEXEC)
fanotify_mark(fd, FAN_MARK_ADD | FAN_MARK_MOUNT, FAN_CLOSE_WRITE [| FAN_MODIFY], AT_FDCWD, mount)
loop:
  n = read(fd, buf[64 KiB])                       # blocking; wakes on events or the shutdown pipe (poll)
  for each fanotify_event_metadata m in buf:
     check m.vers == FANOTIFY_METADATA_VERSION
     if m.mask & FAN_Q_OVERFLOW: lossy_minute = true; overflow_total += 1; continue
     if m.pid == self_pid: close(m.fd); continue
     path  = readlink("/proc/self/fd/{m.fd}")     # canonical path, as seen from our mount namespace
     st    = fstat(m.fd)                          # dev, ino, size, blocks
     close(m.fd)                                  # ALWAYS, even on errors
     if excluded(path): continue
     delta = st.blocks*512 − lru.get((dev, ino)).unwrap_or(0); lru.put((dev, ino), st.blocks*512)
     who   = attribution.lookup(m.pid)            # comm + systemd unit, cached with TTL 60 s
     rings.current_minute().add(dir_key(path), who, delta, (dev, ino))
     dirty.insert(parent(path))
```

Rules:

- **Default mask is `FAN_CLOSE_WRITE` only.** `FAN_MODIFY` fires on every `write()`, opening an fd for us each time, which is too expensive for Postgres and log writers. Long-lived writers that never close are covered by the open-writer scanner (§4.2) and by the 30-second filesystem rates.
- **Bounded queue.** Don't use `FAN_UNLIMITED_QUEUE`. On overflow, mark the minute `lossy` and increment `tidemark_watch_overflows_total`. Rates from L0 remain correct.
- `dir_key(path)` is the parent directory, truncated to `watch.hot_depth` components below the mount (default 6) to bound cardinality. Keys are raw bytes.
- The LRU of last-seen sizes holds 200,000 entries (about 10 MB). For a file **not** in the LRU, count its full size only if it is new: birth time (`statx` btime, which XFS v5 provides) or, failing that, ctime is after the watcher started and within the last 2 minutes. Otherwise count 0 and seed the LRU. `CLOSE_WRITE` fires even when nothing was written, and this rule avoids false spikes after restarts and evictions.
- Mount-mark limitation: in fd mode, writes made through a *different* mount of the same filesystem (bind mounts, other mount namespaces) are not seen. The probe lists bind mounts of watched filesystems so operators know.
- **Mount namespace:** the daemon must share the host mount namespace (`02` §8, ADR-013). Before marking, the watcher compares `/proc/self/ns/mnt` with `/proc/1/ns/mnt` and refuses to start, with a loud error, if they differ.

```rust
pub struct WatchEvent { pub at: i64, pub path: RawPath, pub delta: i64, pub attribution: Attribution }
impl Watcher {
    pub fn enable_events(&self);
    pub fn drain_events(&self, limit: usize) -> Vec<WatchEvent>;
}
```

Only the foreground consumer enables the 1,000-event buffer; it drops oldest events and counts drops. Daemon construction leaves it absent and uses completed-minute drains. Foreground output shares a 50-lines/s limit across mounts, reports omissions, and emits JSON lines with schema `tidemark.watch.v1` and path/path_b64. MOVED_FROM/MOVED_TO are independent directional events with signed deltas; v1 uses no FAN_RENAME and reports no from-to pairs.

Retry EMFILE/ENFILE/ENOMEM/ENOBUFS with shutdown-aware exponential 50–800 ms waits, retaining the group and marking affected/recovery minutes lossy. Fatal transport failures persist as `WatchMode::Failed`; foreground CLI exits rather than appearing alive. `WatchStats` adds `dropped_events: u64`, `recovering: bool`, `read_errors: u64`. Clear unsafe path caches after loss; clock rollback discards affected ring history and marks the new minute lossy.

### 3.3 Aggregation (`LiveState.hot`)

`now.hot_dirs_5m` is the watcher’s ranked five-minute directory list including the current minute, bounded by writers row limit and preserving lossy flags; it does not establish exact alert windows. Missing/failed watchers are unmeasured; an empty running list is measured. `top_unit` is the unit with greatest sum of absolute byte deltas, ties to the lexicographically smaller name, using at most eight private space-saving candidates per row/minute (approximate beyond eight). Unit rows identify themselves; unattributed rows have None. A process gone before `/proc` lookup stays unattributed without retry/guessing. Persisted `HotMinute` has no `top_unit`. Multi-minute `files` sums per-minute distinct counts, not distinct files across the window. (13 R4-6.)

`tidemark_live::watch::CompletedMinute` (Clone, Debug) has `started_at: i64`, `overflow_total_at_start: u64`, `overflow_total_at_end: u64`, `lossy: bool`, `truncated: bool`, `dirs: Vec<HotEntry>`, `persisted: Vec<HotMinute>`. `Watcher::drain_minutes(&self, top_k: usize) -> Vec<CompletedMinute>` shares the persistence-row cursor with legacy drains, while minute facts have an independent monotonic watermark. New drains exclude the startup partial minute, materialize idle minutes and remain bounded by 60 buckets; top_k is clamped to 1000 per scope, cuts mark truncation/loss. Retention gaps mark the first surviving minute lossy; rollback never replays emitted timestamps/rows and marks the replay interval lossy. Only actual queue overflow increments totals; contiguous boundaries chain. `Watcher::mount_identity(&self) -> std::io::Result<(u64, u32, u32)>` returns retained descriptor mount id, device major/minor from bounded fdinfo/fstat; injected watchers return Unsupported. Marks target the retained `/proc/self/fd/<fd>/.` directory.

The daemon consumer must retain at most 60 completed facts per watcher identity, discard on replacement and persist only `persisted`. Exact n-minute windows require 1 <= n <= 60, end E of latest completed minute, start S = E - n*60, and measured_at E (never refreshed artificially). Overflow differences need consistent nondecreasing exact S/E boundary totals; otherwise None. Known endpoints suffice even with interior loss. Hot requires n consecutive healthy, untruncated, overflow-free minutes; otherwise None. Sum raw-key rows with saturating counts/deltas, rank signed bytes descending then raw key ascending; healthy empty is Some(empty). A daemon union cap cut makes hot unavailable. Minute leaders cannot reconstruct exact cross-minute unit leadership. These consumer publications remain open.

`AttributionCache::lookup_watcher` may reuse PID/starttime validation for less than one second, separately from its 60-second attribution TTL. Older hits revalidate; failed/recycled identities invalidate and fresh loads recheck. Failed lookups are not cached. Exact process-scanner callers validate every lookup. (13 R4-7; LIVE-minutes is the precise source for drain and consumer semantics.)

- 60 one-minute buckets (a ring). Each bucket has three bounded maps:
  - `dir → {events, files, bytes_delta}`
  - `unit → {…}`
  - `comm → {…}`
- Map caps: the **current** minute's bucket holds up to 20,000 keys per scope (the smallest half by `bytes_delta` is evicted when full, marking it `approx`). When a minute completes it is **shrunk to its top 1,000 keys per scope**. Worst case is about 20k + 59 × 3k entries, which is well under 100 MB; typical use is far less.
- Windows of 1, 5, 15 and 60 minutes are computed by summing buckets.
- Persist the rows returned by the chosen completed-minute drain; requested top_k is capped at 1,000 per scope. Legacy and new drains share the persistence cursor. The new consumer should drain_minutes(1000) for the full completed-row cap (R4-7); requested cuts are reflected in truncation facts.
- `dirty` is a set of directories with events since the last completed scan of their root, capped at 200,000 and taken independently per root only after its successful scan publishes. It drives `tidemark top --live` hints and future incremental refresh.

Watcher-backed rules (`hot_dir_growth` and `watch_lossy`) accept only exact whole-minute windows from 1 through 60 minutes. Other durations are not evaluable, with a diagnostic; never round or clamp. Keep 60 whole-minute buckets and a cumulative overflow total. Sample that total at each completed-minute boundary; a window overflow count is the difference between its two boundary samples, unavailable if either sample is missing. Hot data consists of per-minute top lists, not a complete inventory. If any included minute overflowed or was truncated at its cap, hot data for the window is unavailable. The ordinary hot API retains its four ranked-list windows.

Updated by 13 R4-7: LIVE-minutes implements CompletedMinute, drain_minutes and mount_identity with empty-minute and boundary-loss facts. The daemon exact-window/freshness consumer remains open; watcher-backed rules are still not evaluable.

`Watcher::take_dirty(&self, under: &RawPath) -> Vec<RawPath>` returns `under` itself after a loss, independently for every root, until that root has been taken. A parent, child or sibling take does not acknowledge another root. Loss acknowledgements use a FIFO capped at 1,024 roots; eviction conservatively makes a forgotten root dirty again, repeated loss resets acknowledgements, and clock rollback invalidates them. Ordinary ancestor dirty entries are retained for other callers; descendants are drained, sorted and deduplicated. The scheduler takes dirtiness only after that root's successful scan has published; failed/cancelled scans clear nothing.

### 3.4 FID+name mode (only if the probe allows)

Mask: `FAN_CREATE | FAN_DELETE | FAN_MOVED_FROM | FAN_MOVED_TO | FAN_CLOSE_WRITE | FAN_ONDIR`. Each event carries `fsid + file_handle(dir) + name`.

- Resolve the directory with `open_by_handle_at(mount_fd, handle, O_PATH)` (needs `CAP_DAC_READ_SEARCH`), then `readlink`. Cache handle→path in an LRU of 100,000.
- `CLOSE_WRITE`: `fstatat(dirfd, name)` for size.
- `DELETE`: count it, and estimate freed bytes from the LRU if the inode was seen before.
- Renames are directional MOVED_FROM/MOVED_TO events only. V1 reports no from-to pairs and does not use FAN_RENAME.

## 4. Process scanner (`tidemark-live::procscan`, W5-B, every 60 s)

One pass over `/proc/[0-9]*/fd/*` feeds both detectors.

### 4.1 Deleted-but-open ("phantom") files

For each fd:

1. `readlink` the fd. Skip anything that isn't an absolute path (sockets, pipes, `anon_inode:`).
2. `stat("/proc/<pid>/fd/<n>")` follows the magic link to the open inode, even when it's deleted.
3. If `st_nlink == 0`, it's a regular file, and `st_dev` belongs to a watched mount, it's a **phantom**: `bytes = st_blocks × 512`.

Deduplicate by `(dev, ino)`, since many fds and processes can hold one file. Attribute to every holder.

Output: total phantom bytes per mount, and the top holders as `pid, comm, unit, path` (the readlink text without the ` (deleted)` suffix, plus bytes). This is the classic "`df` says full, `du` disagrees" explanation. The usual fix is restarting or HUP-ing the holder; Tidemark only reports.

### 4.2 Growing open-for-write files

For fds whose target is a regular file on a watched mount, read `/proc/<pid>/fdinfo/<n>` and parse `flags:` (octal). If `flags & O_ACCMODE ∈ {O_WRONLY, O_RDWR}`, track `(dev, ino) → (size, disk, path, pid, comm, unit)` across passes. "Growing" = `disk` increased since the previous pass. Report the top growers per window.

This catches log files and long-running writers, including runaway database temp files, that close-write events never see.

- Exclude prefixes from `procscan.exclude_prefixes` (default: none).
- Cost: one `readlink` + `stat` per fd and one small read per candidate. Expect well under a second for tens of thousands of fds. Ignore `ENOENT`/`ESRCH` from exiting processes.

`ProcScanResult { at: i64, fds_seen: u64, writers_full: bool, writer_evictions: u64, duration: Duration }` describes a completed pass. `writers_full` says writer candidates exceeded the history cap; `writer_evictions` counts histories evicted for admission during that pass. The daemon publishes these diagnostics with `fds_seen` and persistence drops in component health. Permission-error counts are not reported in v1; do not fabricate a zero or extend frozen report DTOs to add them.

### 4.3 Attribution helper (shared with the watcher)

Watcher lookup may reuse PID/starttime validation for less than one second via `lookup_watcher`; older entries revalidate identity, failures invalidate without negative caching. Exact process-scanner lookup validates every time. A writer already gone from /proc stays unattributed (13 R4-6/7).

`OpenFile.starttime: u64` is the process identity validated around enumeration. Compare it with later `Attribution.starttime`; mismatch produces comm `?` and no unit for both phantom and writers. `/proc` reads do no disk I/O and are not charged to Gate; O_NOATIME does not apply. Existing bounded scanner caps remain. Writer reports use the actual baseline-to-latest timestamp span, not the requested window; positive growth with zero measured span is omitted.


`pid → { comm, unit, uid }`, read from:

- `/proc/<pid>/comm`
- `/proc/<pid>/cgroup`: on cgroup v1, the `name=systemd` line; on v2, the `0::` line. The unit is the last path component ending in `.service`, `.scope` or `.slice`. User sessions map to `user-<uid>`.
- `/proc/<pid>/status` for `Uid`.

Cache with a 60 s TTL, keyed by `(pid, starttime)` from `/proc/<pid>/stat` field 22, to survive PID reuse.

## 5. *(Deferred: PostgreSQL collector, see `14`.)*

## 6. Alert engine (`tidemark-live::alerts`, W5-D)

### 6.1 Rule kinds

Nine configurable rule kinds remain; `state_space` is an internal alert.

| Kind | Parameters | Fires when |
|------|------------|-----------|
| `fs_used_pct` | mount, above | Used % > above |
| `time_to_full` | mount, below, window | TTF(window) < below |
| `hot_dir_growth` | mount, path glob, above (bytes/s or "GiB/h"), window | Watcher-attributed growth into a matching directory exceeds the threshold |
| `thin_pool_pct` | vg/pool (or `*`), data_above, meta_above | Thin-pool data % or metadata % exceeds the threshold |
| `vg_free_low` | vg, below | VG free space < below (informational: headroom for growing filesystems) |
| `dir_growth` | root, depth, above | Between the last two snapshots, a directory at ≤ depth grew > above |
| `phantom_bytes` | mount, above | Phantom total > above |
| `scan_stale` | root, older_than | No complete scan within older_than |
| `watch_lossy` | mount, above (overflow count), window | Overflows > N in window |
| `state_space` | (internal) | Space guard refused a snapshot |

### 6.2 Semantics

- Each rule has `for` (condition must hold continuously before firing; default 5 m) and `resolve_for` (must be false continuously before resolving; default 10 m).
- State is per `(rule, subject)`, for example `(time_to_full, /data)` or `(thin_pool_pct, vg00/pool0)`.
- Re-notify every `renotify` (default 4 h) while firing.

Numeric `hot_dir_growth.above` must be finite and nonnegative and means bytes/s; equivalent rate strings evaluate identically. Rules whose collector/input is disabled or unavailable remain visible in effective config but do not fire. Thin-pool rules evaluate only tracked thin LVs.

```rust
pub trait AlertSource {
    fn fs_sample(&self, mount: &str) -> Option<FsSample>;
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64>;
    fn hot_dirs(&self, mount: &str, window: Duration) -> Option<Vec<HotEntry>>;
    fn watch_overflows(&self, mount: &str, window: Duration) -> Option<u64>;
    fn phantom_bytes(&self, mount: &str) -> Option<u64>;
    fn lvm(&self) -> Option<Vec<LvmNow>>;
    fn dir_growth(&self, root: &str, depth: u32) -> Option<Vec<(RawPath, i64)>>;
    fn scan_age(&self, root: &str) -> Option<ScanAge>;
}
pub struct ScanAge { pub last_complete_at: Option<i64>, pub effective_cadence: Option<Duration> }
pub struct AlertInputs<'a> { pub source: &'a dyn AlertSource, pub state_space_refused: bool }
pub struct AlertDiagnostic { pub rule: String, pub reason: String }
impl AlertEngine { pub fn diagnostics(&self) -> &[AlertDiagnostic]; }
```

Query the source per rule and exact Duration window; dir_growth compares snapshots and has no window. Inventories other than watcher hot top lists must be complete and bounded (watcher inputs follow R3-1 and §3.3): `Some(empty)` confirms absence, `None` means unavailable. A rule without data is not evaluable: preserve state, values and timers without starting, resolving or renotifying; pause continuity and restart pending/clearing evidence on the next known observation. Show diagnostic reasons in alerts and `NowInputs.unevaluable`, propagated through `NowReport.degraded`. Unknown thin-pool percentages cannot resolve an active alert. Healthy unseen subjects allocate no state; the private 10,000-pair cap reports admission failures and protects unavailable active states.

`older_than = "auto"` is twice effective cadence; windowed schedules supply at least 24h cadence, non-windowed short cadences remain valid. Unknown cadence is diagnostic; a known root with no complete scan breaches staleness. Measured NowInputs sections use Option: None is “not measured”, Some(empty) is measured empty. Preserve the frozen core report via degraded reasons and never display an unavailable phantom placeholder as measured zero.

### 6.3 Delivery

Journal delivery is the only alert delivery sink in v1 (ADR-020). Active alerts and history remain available through the Unix API and CLI.

- **Always:** journald, with structured fields `TIDEMARK_RULE`, `TIDEMARK_SUBJECT`, `TIDEMARK_STATE` and `TIDEMARK_VALUE`.
- **Always:** the metric `tidemark_alert_firing{rule,subject} 1`, so Alertmanager can route it.
- No arbitrary command hooks; that would be a root code-execution path.

### 6.4 Default rules

Shipped enabled; thresholds editable:

- `fs_used_pct > 90%` for 5 m
- `time_to_full < 12h` (1 h window) for 10 m
- `phantom_bytes > 10 GiB` for 10 m
- `scan_stale` with `older_than = "auto"`: 2 × the effective cadence, where a windowed schedule counts as at least 24 h (so a `00:00–06:00` window doesn't fire every afternoon)
- `watch_lossy > 10` per 15 m
- `thin_pool_pct`: data > 80% or metadata > 80% (only when a tracked LV is thin)

Pure `tidemark_core::config::default_rules(cfg)` owns generation; W5-D wraps/re-exports it. Filesystem and phantom defaults expand over `sampler.mounts`, watcher defaults over `watch.mounts`. Generate one `scan_stale` rule per configured root with schedule other than `off`; explicit stale rules may monitor manually scanned roots. Base names are `fs_used_pct`, `fs_full_soon`, `phantom_bytes`, `watch_lossy`, `scan_stale`, `thin_pool_pct`. Mount/root-scoped names are unsuffixed only for the exact `/data` subject; all others append `:<mount>` or `:<root>`, independent of list order. The suffix preserves UTF-8 bytes: retain `/` and ASCII `[A-Za-z0-9._-]`, percent-encode every other byte as uppercase `%XX` (including `%` and `:`). This escape choice settles S3's unspecified format. Same-name user rules override defaults; duplicate user names remain for validation. Generation/merge is idempotent; defaults use 5m/10m firing/resolution unless listed otherwise above.

## 7. The `now` view (FR-18)

The ranked watcher five-minute list includes the current minute and is not an exact alert window. `files` sums minute-distinct counts. Human output uses UTC and terminal-width middle shortening, default width 100; JSON retains integer timestamps. Present rate_15m in hourly units and supplied one-hour ttf_secs without recomputation (13 R4-6/12).

The daemon builds a `NowReport` from `LiveState` in under 50 ms; the CLI renders it.

```rust
pub struct NowReport {
    pub generated_at: i64, pub host: String,
    pub mounts: Vec<MountNow>,
    pub alerts_firing: Vec<AlertSummary>,
    pub scans: Vec<ScanStatus>,          // last success per root, running scan progress
    pub degraded: Vec<String>,           // e.g. "watcher: fd mode only"
}
pub struct MountNow {
    pub mount: String, pub size: u64, pub used: u64, pub avail: u64,
    pub rate_15m: Option<f64>, pub rate_1h: Option<f64>, pub ttf_secs: Option<u64>,
    pub lvm: Option<LvmNow>,             // LV size, VG free, thin-pool data/meta % (if thin)
    pub hot_dirs_5m: Vec<HotEntry>, pub writers_5m: Vec<HotEntry>,
    pub phantom: PhantomSummary, pub open_writers_top: Vec<OpenWriter>,
    pub watch_mode: String, pub lossy_minutes_15m: u32,
}
```

Measured `NowInputs` sections are optional: None is not measured, Some(empty) is measured empty. `NowInputs.unevaluable: &[AlertDiagnostic]` adds rule diagnostics to `NowReport.degraded`. Missing phantom and LVM sections render “not measured”; required core placeholders never imply measured zero.

Example render:

```
tidemark now — db-report-01 — 2026-10-10 14:02:11 EDT
/data   used 3.41 TiB / 3.64 TiB (93.7%)   +38.2 GiB/h (15m)   full in ~6h (1h trend)   ⚠ time_to_full
  lvm                     vg00/data 3.64 TiB · thin pool vg00/pool0 data 81% meta 12% · VG free 0 B ⚠ thin_pool_pct
  hot dirs (5m)           /data/reports/acme/2026-10/raw   +2.6 GiB  14,212 files  reportgen.service
                          /data/reports/globex/daily       +310 MiB     402 files  etl-nightly.service
  writers (5m)            reportgen.service +2.7 GiB · etl-nightly.service +0.3 GiB · app-api.service +0.1 GiB
  deleted-but-open        41.2 GiB — java (pid 4121, app-api.service) /data/logs/api.log
/       used 31.2 GiB / 50.0 GiB (62.4%)   +0.1 GiB/h (15m)   not filling
  hot dirs (5m)           /var/log/journal  +42 MiB  systemd-journald.service
last scan /data           2026-10-10 02:30 (11h ago, bulk, 9m14s) · /  03:30 (10h ago, walk, 48s) — run `tidemark diff --since 24h --explain`
```

## 7a. Core live types (contract for `tidemark-core::live`)

These field lists are normative for W0-B. Path fields are raw bytes (`Vec<u8>`); human display uses escaping, JSON report DTOs use `path`/`path_b64` (`08` §1.1). Explicit `mount: String` and `ScanStatus.root: String` override this general byte-path rule: tracked mounts/roots come from UTF-8 TOML, so these strings are exact config paths. Arbitrary non-UTF-8 scan request roots use the API DTO; for the String-only status root use `RawPath::display_lossy()` byte escaping, not replacement characters. This display string is not a round-trip request encoding.

```rust
pub struct FsSample      { pub ts: i64, pub mount: String, pub size: u64, pub used: u64, pub avail: u64,
                           pub inodes: u64, pub inodes_used: u64 }
pub struct LvmSample     { pub ts: i64, pub vg: String, pub lv: String, pub lv_size: u64, pub vg_free: u64,
                           pub data_pct: Option<f64>, pub meta_pct: Option<f64> }
pub struct LvmNow        { pub vg: String, pub lv: String, pub lv_size: u64, pub vg_free: u64,
                           pub thin_pool: Option<String>, pub data_pct: Option<f64>, pub meta_pct: Option<f64>,
                           pub sampled_at: i64 }
pub struct Attribution   { pub pid: u32, pub starttime: u64, pub comm: String, pub unit: Option<String>, pub uid: Option<u32> }
pub struct HotEntry      { pub key: Vec<u8> /* dir path, unit or comm */, pub events: u64, pub files: u64,
                           pub bytes_delta: i64, pub top_unit: Option<String>, pub lossy: bool }
pub struct HotMinute     { pub mount: String, pub scope: String /* dir|unit|comm */, pub key: Vec<u8>, pub ts: i64,
                           pub events: u64, pub files: u64, pub bytes_delta: i64, pub lossy: bool }
pub struct PhantomHolder { pub pid: u32, pub comm: String, pub unit: Option<String>, pub path: Vec<u8>,
                           pub dev: u64, pub ino: u64, pub bytes: u64 }
pub struct PhantomSummary{ pub mount: String, pub total_bytes: u64, pub files: u64, pub holders: Vec<PhantomHolder> }
pub struct PhantomSample { pub ts: i64, pub mount: String, pub pid: u32, pub comm: String, pub unit: Option<String>,
                           pub path: Vec<u8>, pub dev: u64, pub ino: u64, pub bytes: u64 }
pub struct OpenWriter    { pub mount: String, pub pid: u32, pub comm: String, pub unit: Option<String>, pub path: Vec<u8>,
                           pub dev: u64, pub ino: u64, pub size: u64, pub disk: u64, pub growth: i64, pub window_secs: u64 }
pub struct WriterSample  { pub ts: i64, pub mount: String, pub pid: u32, pub comm: String, pub unit: Option<String>,
                           pub path: Vec<u8>, pub dev: u64, pub ino: u64, pub size: u64, pub disk: u64 }
pub enum   AlertState    { Firing, Resolved, Renotify }
pub struct AlertEvent    { pub ts: i64, pub rule: String, pub subject: String, pub state: AlertState,
                           pub value: Option<f64>, pub message: String }
pub struct AlertSummary  { pub rule: String, pub subject: String, pub since: i64, pub value: Option<f64>, pub message: String }
pub struct ScanProgress  { pub scan_id: String, pub phase: String, pub dirs: u64, pub entries: u64,
                           pub started_at: i64, pub rate: f64 }
pub struct ScanStatus    { pub root: String, pub running: Option<ScanProgress>,
                           pub last_complete: Option<LastScan>, pub next_due: Option<i64> }
pub struct LastScan      { pub scan_id: String, pub finished_at: i64, pub mode: String, pub duration_ms: u64 }
```

There is no separate `Hot` or `Growth` type: hot lists use `HotEntry`, and scan-to-scan growth uses W5-D's `DirGrowth`.

## 8. Overhead budget and safeguards

Owner decision: fd-mode watcher budget is 5% of one core per 1,000 close-write events/s, or 50 microseconds/event, replacing 1%. EL8 kernels 4.18.0-477 and -553 offer no measured FID mode. Optimised 8.10 guest measured 45.6 microseconds/event at 2,000/s (9.1% of one core) with host under load. Owner will measure real servers; this is no universal performance guarantee. The 2,000/s scale test threshold is 10% of one core, replacing 2%. Idle CPU and write-throughput impact remain separate gates.

| Component | Budget | Safeguard |
|-----------|--------|-----------|
| Samplers | < 0.1% CPU | `statvfs` every 30 s; `lvs`/`vgs` every 5 min with a timeout |
| Watcher | ≤ 5% of one core per 1,000 close-write events/s (50 µs/event; R4-8) | Close-write only; bounded queue; capped maps |
| Procscan | < 1 s per pass | 60 s interval; skips non-matching devices early |
| Memory | ≤ 300 MiB steady (NFR-4) | Fixed rings; capped LRUs and maps |
