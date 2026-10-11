# 07 — Live monitoring (L0 sampling + L2)

Goal: within about a minute of something starting to fill the disk, `tidemark now` names the filesystem and its growth rate, the hot directories, and the process or systemd unit doing it. It also shows hidden consumers (deleted-but-open files, Postgres WAL held by slots).

All live components run inside `tidemarkd` and write to `LiveState` (in memory) and `timeseries.db` (persisted).

---

## 1. Filesystem and LVM samplers

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

## 2. Rates and time-to-full (`tidemark-live::rates`, W5-D)

- **Rate** = least-squares slope of `(ts, used)` over a window. Windows are 5 min, 15 min and 1 h, and need at least 3 samples. A slope is robust to the sawtooth of temp files being created and deleted, unlike first-minus-last.
- **Time-to-full** = `avail / rate` when `rate > 0`, otherwise "not filling". Shown from the 1 h window, with the 15 min value alongside.
- The same functions apply to thin-pool usage, Postgres WAL, and `dir_series` (scan-to-scan growth).

```rust
pub fn slope_bytes_per_sec(samples: &[(i64 /*unix s*/, i64 /*bytes*/)]) -> Option<f64>;
pub fn time_to_full(avail: u64, rate: f64) -> Option<Duration>;
```

## 3. fanotify watcher (`tidemark-live::watch`, W5-A)

### 3.1 Modes and probing

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
- **PostgreSQL on `/data`:** when `[postgres]` is enabled, the watcher excludes the data directory by default (`watch.exclude_postgres = true`). Postgres growth is measured by the collector (§5) instead, which is more accurate and avoids event noise.
- `dir_key(path)` is the parent directory, truncated to `watch.hot_depth` components below the mount (default 6) to bound cardinality. Keys are raw bytes.
- The LRU of last-seen sizes holds 200,000 entries (about 10 MB). For a file **not** in the LRU, count its full size only if it is new: birth time (`statx` btime, which XFS v5 provides) or, failing that, ctime is after the watcher started and within the last 2 minutes. Otherwise count 0 and seed the LRU. `CLOSE_WRITE` fires even when nothing was written, and this rule avoids false spikes after restarts and evictions.
- Mount-mark limitation: in fd mode, writes made through a *different* mount of the same filesystem (bind mounts, other mount namespaces) are not seen. The probe lists bind mounts of watched filesystems so operators know.
- **Mount namespace:** the daemon must share the host mount namespace (`02` §8, ADR-013). Before marking, the watcher compares `/proc/self/ns/mnt` with `/proc/1/ns/mnt` and refuses to start, with a loud error, if they differ.

### 3.3 Aggregation (`LiveState.hot`)

- 60 one-minute buckets (a ring). Each bucket has three bounded maps:
  - `dir → {events, files, bytes_delta}`
  - `unit → {…}`
  - `comm → {…}`
- Map caps: the **current** minute's bucket holds up to 20,000 keys per scope (the smallest half by `bytes_delta` is evicted when full, marking it `approx`). When a minute completes it is **shrunk to its top 1,000 keys per scope**. Worst case is about 20k + 59 × 3k entries, which is well under 100 MB; typical use is far less.
- Windows of 1, 5, 15 and 60 minutes are computed by summing buckets.
- Each minute, the top 50 keys per scope are persisted to `hot_minutes`.
- `dirty` is a set of directories with events since the last completed scan of their root, capped at 200,000 and cleared per root when a scan finishes. It drives `tidemark top --live` hints and future incremental refresh.

### 3.4 FID+name mode (only if the probe allows)

Mask: `FAN_CREATE | FAN_DELETE | FAN_MOVED_FROM | FAN_MOVED_TO | FAN_CLOSE_WRITE | FAN_ONDIR`. Each event carries `fsid + file_handle(dir) + name`.

- Resolve the directory with `open_by_handle_at(mount_fd, handle, O_PATH)` (needs `CAP_DAC_READ_SEARCH`), then `readlink`. Cache handle→path in an LRU of 100,000.
- `CLOSE_WRITE`: `fstatat(dirfd, name)` for size.
- `DELETE`: count it, and estimate freed bytes from the LRU if the inode was seen before.
- Renames are recorded as `from → to` pairs. They're useful for spotting bulk moves between trees.

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

This catches log files and long-running writers, including runaway Postgres temp files, that close-write events never see.

- Exclude prefixes from `procscan.exclude_prefixes` (default: PGDATA's `base/` and `global/`, which Postgres keeps open read-write constantly).
- Cost: one `readlink` + `stat` per fd and one small read per candidate. Expect well under a second for tens of thousands of fds. Ignore `ENOENT`/`ESRCH` from exiting processes.

### 4.3 Attribution helper (shared with the watcher)

`pid → { comm, unit, uid }`, read from:

- `/proc/<pid>/comm`
- `/proc/<pid>/cgroup`: on cgroup v1, the `name=systemd` line; on v2, the `0::` line. The unit is the last path component ending in `.service`, `.scope` or `.slice`. User sessions map to `user-<uid>`.
- `/proc/<pid>/status` for `Uid`.

Cache with a 60 s TTL, keyed by `(pid, starttime)` from `/proc/<pid>/stat` field 22, to survive PID reuse.

## 5. Postgres collector (`tidemark-pg`, W5-C)

### 5.1 Access

```sql
CREATE ROLE tidemark LOGIN;
GRANT pg_monitor TO tidemark;      -- PG10+: read all stats/settings, pg_ls_waldir()
```

The daemon runs as root and connects over the Unix socket. Either:

- **peer auth with an ident map** (`pg_ident.conf`: `tidemark  root  tidemark`; `pg_hba.conf`: `local all tidemark peer map=tidemark`), or
- **a password file** at `/etc/tidemark/pg.pass` (0600), used via `passfile=` in the DSN.

### 5.2 Queries (version-adaptive; probe `server_version_num` first)

| Every | Query | Notes |
|-------|-------|-------|
| 5 min | `SELECT datname, pg_database_size(oid) FROM pg_database WHERE datallowconn AND NOT datistemplate` | `kind=database` |
| 5 min | `SELECT COALESCE(sum(size),0) FROM pg_ls_waldir()` | `kind=wal_dir`; PG ≥ 10 |
| 5 min | `SELECT slot_name, slot_type, active, pg_wal_lsn_diff(pg_current_wal_lsn(), restart_lsn) FROM pg_replication_slots` | `kind=slot_retained`; skip when `pg_is_in_recovery()`; add `wal_status` on PG ≥ 13 |
| 5 min | `SELECT archived_count, failed_count, last_failed_time, last_archived_time FROM pg_stat_archiver` | `kind=archiver_failed_total`; a failing archiver makes WAL pile up |
| 5 min | `SELECT datname, temp_bytes, temp_files FROM pg_stat_database WHERE datname IS NOT NULL` | `kind=temp_bytes_total` (cumulative counter, so take the rate) |
| 1 h | Per database: `SELECT n.nspname, c.relname, c.relkind, pg_total_relation_size(c.oid) AS b FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind IN ('r','m') ORDER BY b DESC NULLS LAST LIMIT $top` | `kind=relation`, `name = <db>.<schema>.<relation>` (unique across databases; total size already includes TOAST and indexes, so TOAST tables aren't listed separately); one connection per database, sequential, with `statement_timeout = 30s` |
| 1 h | Per database: `SELECT schemaname, relname, n_live_tup, n_dead_tup, last_autovacuum FROM pg_stat_user_tables ORDER BY n_dead_tup DESC LIMIT 20` | `kind=dead_tuples`, a bloat hint |
| On start + daily | `SHOW data_directory`; `SELECT spcname, pg_tablespace_location(oid) FROM pg_tablespace`; `SELECT oid, datname FROM pg_database` | Path labels: annotate `<data_directory>`, `base/<oid>` → `pg.database=<name>`, tablespace dirs, `pg_wal` |

Every query sets `application_name=tidemark` and a statement timeout. Failures back off 1, 2, 4 … up to 30 minutes, and the status shows "pg: unreachable since …".

## 6. Alert engine (`tidemark-live::alerts`, W5-D)

### 6.1 Rule kinds

| Kind | Parameters | Fires when |
|------|------------|-----------|
| `fs_used_pct` | mount, above | Used % > above |
| `time_to_full` | mount, below, window | TTF(window) < below |
| `hot_dir_growth` | mount, path glob, above (bytes/s or "GiB/h"), window | Watcher-attributed growth into a matching directory exceeds the threshold |
| `thin_pool_pct` | vg/pool (or `*`), data_above, meta_above | Thin-pool data % or metadata % exceeds the threshold |
| `vg_free_low` | vg, below | VG free space < below (informational: headroom for growing filesystems) |
| `dir_growth` | root, depth, above | Between the last two snapshots, a directory at ≤ depth grew > above |
| `phantom_bytes` | mount, above | Phantom total > above |
| `pg_wal_bytes` / `pg_slot_retained` | above | WAL dir > above / any slot retains > above |
| `pg_archiver_failing` | window | `failed_count` increased within window |
| `scan_stale` | root, older_than | No complete scan within older_than |
| `watch_lossy` | mount, overflows, window | Overflows > N in window |
| `state_space` | (internal) | Space guard refused a snapshot |

### 6.2 Semantics

- Each rule has `for` (condition must hold continuously before firing; default 5 m) and `resolve_for` (must be false continuously before resolving; default 10 m).
- State is per `(rule, subject)`, for example `(time_to_full, /data)` or `(thin_pool_pct, vg00/pool0)`.
- Re-notify every `renotify` (default 4 h) while firing.

### 6.3 Delivery

- **Always:** journald, with structured fields `TIDEMARK_RULE`, `TIDEMARK_SUBJECT`, `TIDEMARK_STATE` and `TIDEMARK_VALUE`.
- **Always:** the metric `tidemark_alert_firing{rule,subject} 1`, so Alertmanager can route it.
- **Optional:** `alerts.webhook`, a JSON POST (`{rule, subject, state, value, message, host, ts}` plus a `text` field so Slack/Teams-style incoming webhooks render it). 5 s timeout, 3 retries, never blocking the engine.
- No arbitrary command hooks; that would be a root code-execution path.

### 6.4 Default rules

Shipped enabled; thresholds editable:

- `fs_used_pct > 90%` for 5 m
- `time_to_full < 12h` (1 h window) for 10 m
- `phantom_bytes > 10 GiB` for 10 m
- `pg_slot_retained > 50 GiB`
- `pg_archiver_failing` (30 m window)
- `scan_stale` with `older_than = "auto"`: 2 × the effective cadence, where a windowed schedule counts as at least 24 h (so a `00:00–06:00` window doesn't fire every afternoon)
- `watch_lossy > 10` per 15 m
- `thin_pool_pct`: data > 80% or metadata > 80% (only when a tracked LV is thin)

## 7. The `now` view (FR-18)

The daemon builds a `NowReport` from `LiveState` in under 50 ms; the CLI renders it.

```rust
pub struct NowReport {
    pub generated_at: i64, pub host: String,
    pub mounts: Vec<MountNow>,
    pub pg: Option<PgNow>,
    pub alerts_firing: Vec<AlertSummary>,
    pub scans: Vec<ScanStatus>,          // last success per root, running scan progress
    pub degraded: Vec<String>,           // e.g. "watcher: fd mode only", "pg: unreachable since …"
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

Example render:

```
tidemark now — db-report-01 — 2026-10-10 14:02:11 EDT
/data   used 3.41 TiB / 3.64 TiB (93.7%)   +38.2 GiB/h (15m)   full in ~6h (1h trend)   ⚠ time_to_full
  lvm                     vg00/data 3.64 TiB · thin pool vg00/pool0 data 81% meta 12% · VG free 0 B ⚠ thin_pool_pct
  hot dirs (5m)           /data/reports/acme/2026-10/raw   +2.6 GiB  14,212 files  reportgen.service
                          /data/reports/globex/daily       +310 MiB     402 files  etl-nightly.service
  writers (5m)            reportgen.service +2.7 GiB · etl-nightly.service +0.3 GiB · postgresql.service +0.1 GiB
  deleted-but-open        41.2 GiB — java (pid 4121, app-api.service) /data/logs/api.log
postgres                  WAL 18.4 GiB · slot replica_b retains 12.9 GiB (inactive) · archiver ok
/       used 31.2 GiB / 50.0 GiB (62.4%)   +0.1 GiB/h (15m)   not filling
  hot dirs (5m)           /var/log/journal  +42 MiB  systemd-journald.service
last scan /data           2026-10-10 02:30 (11h ago, bulk, 9m14s) · /  03:30 (10h ago, walk, 48s) — run `tidemark diff --since 24h --explain`
```

## 7a. Core live types (contract for `tidemark-core::live`)

These field lists are normative for W0-B. Paths are raw bytes (`Vec<u8>`), rendered lossily.

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
pub struct PgSample      { pub ts: i64, pub kind: String /* 05 §6 kinds */, pub name: String, pub value: i64 }
pub struct PgSlot        { pub name: String, pub slot_type: String, pub active: bool,
                           pub retained_bytes: Option<u64>, pub wal_status: Option<String> }
pub struct PgNow         { pub sampled_at: Option<i64>, pub unreachable_since: Option<i64>, pub server_version_num: Option<u32>,
                           pub databases: Vec<(String, u64)>, pub wal_dir_bytes: Option<u64>, pub slots: Vec<PgSlot>,
                           pub archiver_failing: bool, pub archiver_last_failed: Option<i64>,
                           pub temp_bytes_rate: Option<f64>, pub top_relations: Vec<(String /* db.schema.rel */, u64)> }
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

| Component | Budget | Safeguard |
|-----------|--------|-----------|
| Samplers | < 0.1% CPU | `statvfs` every 30 s; `lvs`/`vgs` every 5 min with a timeout |
| Watcher | ≤ 1% of one core at 1,000 events/s | Close-write only; bounded queue; capped maps |
| Procscan | < 1 s per pass | 60 s interval; skips non-matching devices early |
| Postgres | Sub-second queries, except hourly relation sizes | Statement timeout; sequential per DB; back-off |
| Memory | ≤ 300 MiB steady (NFR-4) | Fixed rings; capped LRUs and maps |
