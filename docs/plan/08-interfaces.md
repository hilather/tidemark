# 08 — Interfaces: CLI, daemon API, configuration, metrics, systemd

Everything here is a contract. Changing it after Wave 0 requires a spec-change request (`10-agent-playbook.md` §6).

---

## 1. CLI (`tidemark`)

`diff` first canonicalizes the original live path. On ENOENT, it canonicalizes the longest existing ancestor and appends the unresolved byte names. Refuse `..` in the unresolved suffix; existing symlink/parent semantics are handled by realpath before lexical normalization. This exception applies to diff so removed snapshot paths remain queryable.

```rust
pub struct WatchEvent { pub at: i64, pub path: RawPath, pub delta: i64, pub attribution: Attribution }
impl Watcher {
    pub fn enable_events(&self);
    pub fn drain_events(&self, limit: usize) -> Vec<WatchEvent>;
}
```

Only the foreground consumer enables the 1,000-event buffer; it drops oldest events and counts drops. Daemon construction leaves it absent and uses completed-minute drains. Foreground output shares a 50-lines/s limit across mounts, reports omissions, and emits JSON lines with schema `tidemark.watch.v1` and path/path_b64. MOVED_FROM/MOVED_TO are independent directional events with signed deltas; v1 uses no FAN_RENAME and reports no from-to pairs.

Retry EMFILE/ENFILE/ENOMEM/ENOBUFS with shutdown-aware exponential 50–800 ms waits, retaining the group and marking affected/recovery minutes lossy. Fatal transport failures persist as `WatchMode::Failed`; foreground CLI exits rather than appearing alive. `WatchStats` adds `dropped_events: u64`, `recovering: bool`, `read_errors: u64`. Clear unsafe path caches after loss; clock rollback discards affected ring history and marks the new minute lossy.

`deploy stage --from` validates its source independently of an invalid installed config and installs at `Context.config_path` (global `-c`). Deployment pre-flight and compensation are specified in §5.

### 1.1 Global options

| Option | Meaning |
|--------|---------|
| `-c, --config <FILE>` | Config file (default `/etc/tidemark/tidemark.toml`; missing file → built-in defaults) |
| `--state-dir <DIR>` | Override `general.state_dir` |
| `--json` | Machine output. Every command supports it; each document carries `"schema": "tidemark.<command>.v1"` |
| `--color auto\|always\|never` | Terminal colour |
| `-v / -q` | Log verbosity to stderr |

Paths given on the command line are canonicalised (`realpath`) before construction of `RawPath` and before lookup; lexical normalization never precedes realpath, so a symlinked path finds the real directory. Output always shows canonical paths.

Machine output uses `JsonDoc<T> { schema: Cow<'static, str>, data: T }`, with owned deserialization (no `#[serde(borrow)]`). Shared report DTO adapters emit a UTF-8 `path` string or mutually exclusive `path_b64` (padded standard base64) for arbitrary bytes, never integer arrays. Apply this to enclosing/nested reports, phantom holders, snapshot paths and directory hot entries; preserve internal raw byte types. Whole-envelope tests cover ASCII, Unicode and invalid UTF-8.

### 1.2 Commands

| Command | Purpose | Key options | Needs |
|---------|---------|-------------|-------|
| `probe` | Capability and environment report (FR-1) | `--root PATH…` | root |
| `scan [ROOT]` | Run a scan and write a snapshot (FR-2/3) | `--mode auto\|walk\|bulk`, `--throttle gentle\|normal\|max`, `--threads N`, `--exclude GLOB…`, `--keep-partial` (walk mode only), `--cancel` (ask the daemon to cancel the running scan of ROOT) | root |
| `snapshots list\|show ID\|prune\|rebuild-catalog` | Snapshot catalog (FR-6) | `--root`, `--dry-run` | root |
| `top [PATH]` | Directory drill-down (FR-7) | `--depth N` (1), `--sort disk\|apparent\|reclaimable\|charged\|files\|inodes\|newest\|name\|growth`, `--limit N` (25), `--min-size SIZE`, `--snapshot ID`, `--wide` (adds NEWEST, INODES, APPARENT), `--live` (rescan PATH now, throttled, and show deltas vs snapshot) | root |
| `files [PATH]` | Largest files (FR-7) | `--limit`, `--min-size`, `--older-than DUR`, `--newer-than DUR`, `--snapshot` | root |
| `wide [PATH]` | Widest directories (FR-7) | `--min-entries N` (10000), `--limit` | root |
| `diff [PATH]` | Snapshot diff (FR-8) | `--since DUR` or `--from ID [--to ID]`, `--depth N` (3), `--metric disk\|reclaimable\|files`, `--explain`, `--explain-ratio F` (0.5), `--explain-min SIZE` (1GiB), `--limit` | root |
| `links [PATH]` | Hard-link topology (FR-9) | `--boundary GLOB…` or `--boundary-depth N`, `--incomplete`, `--top N`, `--show-paths` (live name resolution) | root |
| `symlinks [PATH]` | Live symlink analysis (FR-5) | `--check-targets` (stat targets: needed for dangling/loop/other-fs), `--dangling` (list only dangling), `--escaping` (list only links resolving outside PATH), `--limit` | root |
| `export ncdu [PATH]` | ncdu JSON export (FR-22) | `-o FILE`, `--snapshot`, `--utf8-lossy`, `--max-depth N` | root |
| `now` | Live situation (FR-18) | `--mount M` | root |
| `hot` | Hot directories/writers (FR-14) | `--window 1m\|5m\|15m\|60m`, `--by dir\|unit\|comm`, `--mount`, `--limit` | root |
| `lvm` | LV/VG/thin-pool status and trend (FR-24) | `--mount M` | root |
| `phantom` | Deleted-but-open files (FR-15) | `--mount`, `--direct` (scan `/proc` now) | root |
| `writers` | Growing open files (FR-15) | `--mount`, `--window`, `--direct` (no daemon: two `/proc` passes), `--interval DUR` (10s, with `--direct`) | root |
| `alerts` | Active alerts; `--all` for history | | root |
| `deploy stage` | Pre-flight checks, install config, enable the service for next boot (FR-25) | `--from FILE` (config to install; the global `-c` still names the installed path), `--start-now`, `--skip-check NAME…` (warning-level checks only) | root |
| `deploy verify` | Post-reboot pass/fail for the pipeline (FR-25) | `--wait DUR` (poll until healthy or timeout); machine output via the global `--json` | root |
| `deploy unstage` | Stop and disable the service; keep state (FR-25) | `--purge-state` (also delete `state_dir`), `--yes` | root |
| `watch` | Foreground event stream for debugging | `--mount`, `--events close_write,modify` | root |
| `status` | Daemon health, versions, component states | `--show-config` (print the CLI’s effective config, labelled, including default alert rules) | root |
| `tui [PATH]` | Interactive browser (FR-21) | `--snapshot` | root |
| `completions <SHELL>` | Shell completions | | — |

`scan --mode walk` reports in-memory apparent sizes exact at scan time. Bulk mode (including auto when it selects bulk) reports regular-file sizes recorded in XFS on-disk inodes: apparent sizes of files still being written or waiting for writeback may lag, down to zero for a new file, normally for about 30 seconds without a guaranteed deadline. Disk usage, counts, link accounting and charged bytes are unaffected by this lag. Tidemark never forces writeback or stats files to correct bulk sizes; choose walk when current apparent size matters (R3-16).

`scan --keep-partial` can retain collected walk rows on cancellation before publication; cancellation observed inside `finish_with` discards even that partial candidate; cancelled bulk scans produce no snapshot. `scan --help` explicitly says the option is walk-only. `snapshots prune` deletes retention-selected files, then rebuilds the catalog from remaining files for all roots; `--dry-run` only shows proposed deletions. `snapshots rebuild-catalog --root PATH` is refused before recovery or mutation; omit `--root` to select global rebuilding.

`--sort growth` uses a CLI sort wrapper with a `Growth` case, requiring two snapshots; it is not a `snapshot::SortKey` variant. W2-C compares the selected snapshot with the older complete snapshot used for Δ24h, sorts descending signed disk-byte growth with raw-byte path tie breaking, and errors when the older snapshot is unavailable. `writers --window` accepts any positive humantime duration (`2m`, `1h`, etc.); no additional maximum is imposed by this contract. `hot` retains only `1m|5m|15m|60m`. CLI/API throttle overrides accept only `gentle|normal|max`; configured root throttle may select custom profiles.

The privileges column describes an installed system, where all Tidemark files are root-only. The config, snapshots and API socket are `0600 root:root`; state and runtime directories are `0700 root:root`. Administrators use `sudo`; there is no `tidemark` group or `socket_group` key.

The CLI has no effective-uid check. A non-root user can run it against a config, state directory and socket they own, as unprivileged tests do. If a non-root command fails because the config, state directory, a snapshot or the socket is not readable, it retains the exit code for that failure and adds the hint: `Tidemark's files are root-only; run this command with sudo.`

### 1.3 Exit codes

Scan falls back directly only when socket is absent/refuses, with an absolute connection deadline rechecked under scan lock. Other connection failures exit 4 before direct state mutation. Follow only the started scan id and read its persisted outcome; degraded, partial or unknown outcome exits 3. Never follow a replacement scan or automatically cancel another id. Root-only scan.cancel remains an explicit root operation. Tables use positive stdout TTY width or 100 otherwise, shortening cells in the middle while preserving escaped tokens; prose wraps at whitespace. Human absolute times are UTC with Z; JSON integers remain integers. now displays provided rate_15m in hourly units and provided one-hour ttf_secs without recomputation.

Open: `tidemark.lvm.v1` names both direct and reduced daemon shapes; scan.cancel cannot atomically condition on scan id under the frozen API. (13 R4-12.)

`now.hot_dirs_5m` uses the ranked five-minute watcher list, not an exact completed window; top_unit and FILES follow 07 §3.3 (R4-6).

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime error |
| 2 | Usage or configuration error |
| 3 | Completed with errors (degraded or partial scan, some queries failed) |
| 4 | Daemon unavailable (live commands) |
| 5 | Precondition not met (destinations not writable/ownable as required, not XFS, unsupported kernel feature, `deploy stage` pre-flight failed) |
| 6 | `deploy verify`: not healthy yet / timed out (details in output) |

### 1.4 Example: `tidemark top /data --depth 1`

```
/data   snapshot 20261010T023000Z-4f1c9a2e · bulk · 11h ago                SIZE   RECLAIM    SHARED    FILES     Δ24h
├─ reports/                                                              2.81T     2.64T  H 170.2G    18.2M   +118.0G
├─ pgsql/                                                                 611G      611G         0     4.1K     +9.4G
├─ archive/                                                               402G      231G  H 171.0G     3.0M         0
└─ tmp/                                                                  12.3G     12.3G         0     210K     -2.1G
H = contains hard links shared with directories outside this one (see `tidemark links`)
```

The `Δ24h` column appears when a snapshot about 24 h older exists.

## 2. Daemon API

Invalid config.reload returns ok with `reloaded: false` and the error in warnings, keeping previous config/generation. Inventory/capacity/catalog/history/schedule preflight failures are contained the same way, without stopping scheduler. Reserve current configured roots transactionally within the 128-entry inventory, evict inactive manual history deterministically if needed, retire removed inactive roots and retain admitted/running roots until completion. Stale schedule generations cannot resurrect removed roots. (13 R4-10.)

- **Transport:** Unix stream socket `/run/tidemark/api.sock`, mode `0600 root:root`.
- **Framing:** newline-delimited JSON. One request per line, one response per line; the connection may carry several requests.
- **Authentication:** socket permissions. Every request is logged with the peer's `SO_PEERCRED` uid/pid. Every method accepts only peer uid 0, identified by kernel SO_PEERCRED; other peers receive `forbidden`, even when the daemon runs as another uid.

```json
{"v":1,"id":"c1","method":"now","params":{"mount":"/data"}}
{"v":1,"id":"c1","ok":true,"result":{ "...": "NowReport (07 §7)" }}
{"v":1,"id":"c2","ok":false,"error":{"code":"unavailable","message":"watcher disabled"}}
```

| Method | Params | Result (types from `07` §7a unless stated) |
|--------|--------|--------|
| `status` | — | `StatusResult { version, uptime_s, components: Vec<ComponentState>, watch_modes: Vec<(mount, mode)>, last_errors: Vec<(component, ts, message)> }` (`ComponentState` defined in core API, re-exported by W4-B `firstboot`; names per §5) |
| `now` | `mount?` | `NowReport` (`07` §7) |
| `hot` | `mount?`, `window` (1m/5m/15m/60m), `by` (dir/unit/comm), `limit` | `Vec<HotEntry>` |
| `lvm` | `mount?` | `LvmResult { mounts: Vec<(mount, LvmNow)>, vgs: Vec<VgInfo>, sampled_at }` (`VgInfo` defined in core API, re-exported by W4-A `lvm`) |
| `phantom` | `mount?` | `Vec<PhantomSummary>` (one per mount) |
| `writers` | `mount?`, `window` (positive humantime duration) | `Vec<OpenWriter>` |
| `alerts.active` / `alerts.history` | `limit?` | `Vec<AlertSummary>` / `Vec<AlertEvent>` |
| `scan.start` | exactly one of `root`/`root_b64`, `mode?`, `throttle?` | `{scan_id}`, or error `busy` (another scan holds the host lock) |
| `scan.status` | at most one of `root`/`root_b64` | `Vec<ScanStatus>` |
| `scan.cancel` | exactly one of `root`/`root_b64` | `{cancelled: bool}` |
| `config.reload` | — | `{reloaded: bool, warnings: Vec<String>}` |

Every API method serves only peer uid 0, identified by kernel `SO_PEERCRED`, even if the daemon runs with another uid; all other peers receive `forbidden`. The workspace `rustix` dependency enables `net` and `termios`. `status --show-config` prints the CLI's effective configuration, explicitly labelled; the running daemon may retain an older config until reload. There is no `config.get` method in v1. With no daemon, `lvm`, `writers` and `phantom` fall back to direct mode with one stderr notice (including quiet mode); `now`, `hot`, `alerts` and `status` exit 4. Explicit direct mode bypasses the daemon. A connected daemon scan refuses threads, extra excludes and keep-partial options; its request carries only the frozen root/mode/throttle overrides.

The daemon-served `lvm` report keeps `LvmResult { mounts: Vec<(String, LvmNow)>, vgs: Vec<VgInfo>, sampled_at: i64 }`. Compared with the direct command it omits segment type and the other/unmounted LV inventory and columns. No wire extension or new flag is added. `W6-A3.md` specifies that the CLI still reads 24-hour trends read-only from the store with the direct command's query/slope/window; missing segment/other-volume columns are omitted and a notice explains the reduced view.

`alerts.history` currently covers persisted events from the current daemon run only; the store has no history reader (12 R3-L3).

### Wire types and adapters (SCR-002)

`Request { v: u32, id: String, method: String, params: serde_json::Value }` requires params, with `{}` for no-argument methods. `Response` uses the same version/id, `ok: bool`, and optional `result: serde_json::Value` / `error: ApiError`, omitted when absent. `ApiError { code: ApiErrorCode, message: String }` has the six snake_case codes below. Every listed method has named `*Params`/`*Result` types; collection/optional results alias the exact live payloads above. `limit: usize`, `uptime_s: u64`, `sampled_at: i64`; mounts/grouping labels are `String`. `HotWindow` is only `1m|5m|15m|60m`, `HotBy` only `dir|unit|comm`. `WritersParams.window: Duration` uses humantime serde and must be positive, with no extra contract maximum. Scan mode overrides are `Auto|Walk|Bulk`, throttle overrides `Gentle|Normal|Max`.

Scan params carry `"root":"<utf-8 path>"` or `"root_b64":"<base64>"` (padded standard base64). Exactly one is required for start/cancel; status permits neither to query all roots, or one. Reject both, invalid base64, and invalid paths; deserialize through fallible `RawPath` validation. Internal serde may remain byte-oriented behind explicit DTO adapters. Report paths use the shared `path`/`path_b64` adapters in §1.1. Full DTO/envelope round-trip tests cover ASCII, Unicode, invalid UTF-8 and omitted optional roots. Explicit String mount/root display fields follow `07` §7a.

`tidemark_core::api` defines `ComponentState { name: String, state: String /* ok|degraded|disabled|error */, detail: String }` and `VgInfo { vg: String, size: u64, free: u64 }`; downstream re-exports are `tidemarkd::firstboot::ComponentState` and `tidemark_fs::lvm::VgInfo`.

Error codes: `bad_request`, `forbidden`, `not_found`, `unavailable`, `busy`, `internal`. Requests time out server-side after 10 s.

## 3. Configuration (`/etc/tidemark/tidemark.toml`)

Daemon validation (13 R4-11): One shared monotonic shutdown deadline governs recorder queue/acknowledgement waits and startup-error cleanup; poll cancellation/deadline at most every 50 ms while preserving eight-second per-operation limits. Before admission work is uncommitted; interruption after admission reports persistence uncertain. Retain/join worker handles even after reporting deadline expiry; the ten-second overall bound is unchanged and arbitrary blocked kernel/SQLite operations remain a limitation. Parse exactly the bounded no-follow captured bytes that are hashed. Startup, reload and --check-config validate core/bounds, scheduler schedules/windows and alert predicates before publication. Runtime intervals (throttle sample, sampler, LVM interval/timeout, procscan, metrics, alert renotify/for/resolve_for and every schedules) are at most 366 days inclusive; historical retention horizons are exempt.

Retained state storage is `/proc/self/fd/<n>/.`; tests bind storage as production does (R4-1).

Durations use humantime (`30s`, `6h`); sizes use binary units (`1GiB`) or explicit SI suffixes, with checked u64 arithmetic and fractional-byte truncation. Unknown keys are rejected with a helpful error. Removed keys `metrics.listen` and `alerts.webhook` are rejected with a message naming the key and stating that the feature was removed in v1 (ADR-020). `tidemark status` and `tidemarkd --check-config` validate the file.

```toml
[general]
state_dir    = "/var/lib/tidemark"     # root filesystem by default; keeps Tidemark writes off /data
socket       = "/run/tidemark/api.sock"
log_level    = "info"                  # error|warn|info|debug|trace

[[roots]]                              # one entry per tracked filesystem
path               = "/data"
mode               = "auto"            # auto|walk|bulk
schedule           = "every 6h"        # "every <dur>" | "daily HH:MM" | "off"
window             = "00:00-06:00"     # optional; scans only START inside this local-time window
throttle           = "normal"          # gentle|normal|max or a user-defined profile
exclude            = []                # full-path globs, e.g. "/data/tmp/cache/**"
series_depth       = 2                 # dirs at <= depth (relative to root) recorded in dir_series (max 20,000 rows per scan)
big_file_threshold = "1GiB"
top_files          = 10000

[[roots]]
path               = "/"               # never crosses into /data, /proc, /sys, /run, tmpfs (other filesystems)
mode               = "walk"            # small tree: walk is simplest
schedule           = "daily 03:30"
throttle           = "normal"
exclude            = []                # state_dir is always excluded
series_depth       = 2
big_file_threshold = "256MiB"
top_files          = 2000

[scan]
threads           = 8
memory_budget     = "1280MiB"            # scan working set; fits the unit's 2G cgroup limit (see §6)
getdents_buf      = "256KiB"
bulk_batch        = 4096
link_instances    = "all"              # all|none
max_notable       = 200000
max_stored_errors = 10000

[throttle]
max_ops_per_sec = 50000
min_ops_per_sec = 200
sample_interval = "500ms"
  [throttle.profiles.gentle]
  max_util_pct      = 50
  target_latency_ms = 15
  [throttle.profiles.normal]
  max_util_pct      = 70
  target_latency_ms = 25

[sampler]
mounts   = ["/", "/data"]              # statvfs sampling
interval = "30s"

[lvm]
enabled  = true                        # auto "n/a" if not on LVM or lvm2 missing
interval = "5m"
timeout  = "30s"

[watch]
enabled          = true
mounts           = ["/", "/data"]
events           = ["close_write"]     # add "modify" only for short investigations
hot_depth        = 6
exclude_prefixes = []                  # state_dir is always excluded
lru_entries      = 200000

[procscan]
enabled          = true
interval         = "60s"
open_writers     = true
exclude_prefixes = []

[metrics]
textfile              = "/var/lib/node_exporter/textfile_collector/tidemark.prom"   # "" = disabled
interval              = "60s"
dir_metrics_depth     = 2
max_series_per_metric = 500

[alerts]
renotify = "4h"

  [[alerts.rule]]
  name   = "fs_full_soon"
  kind   = "time_to_full"
  mount  = "/data"
  below  = "12h"
  window = "1h"
  for    = "10m"
  # Other default rules are listed in 07 §6.4; `tidemark status --show-config` prints the effective set.

[retention]
max_state_size = "20GiB"
min_free_pct   = 5
  [retention.snapshots]
  recent = 4
  daily  = 14
  weekly = 8
  [retention.samples]
  raw         = "48h"
  five_min    = "35d"
  hourly      = "400d"
  hot_minutes = "7d"
```

Add `Config::from_toml_bytes(bytes: &[u8]) -> Result<Config, CoreError>` in `tidemark-core`. It shares `Config::load`'s bounded UTF-8/TOML parsing, default merge and error precedence (`MAX_CONFIG_BYTES = 1 MiB`). The daemon hashes the bytes it actually loaded, and stage parses, installs and hashes one bounded no-follow read rather than reopening the pathname (`W4-B-fix2.md`).

### Effective config contracts (SCR-003)

`ConfigIssue { field: String, message: String }` reports validation errors at dotted field locations. Config/table structs deny unknown fields; paths use `PathBuf`, sizes `ByteSize`, ordinary durations `Duration` with humantime serde, bounded counts `u32`/`usize`, percentages/latencies `f64`. Root mode/link_instances use enums. Root `throttle: String` may name `gentle`, `normal`, `max` or a configured custom profile. `throttle.profiles: BTreeMap<String, ThrottleProfile>` merges overrides with gentle/normal defaults; reject a profile definition named `max` with an actionable error (reserved `NoopGate` selector).

Absent roots use both example roots; absent per-root window is `None`, while built-in `/data` explicitly has its example window. `Config::load` reads one file capped at `MAX_CONFIG_BYTES = 1 MiB`; missing files return the original I/O error so CLI defaults apply. `Config::default` and deserialization produce effective defaults; default rule merge is idempotent and uses pure core `default_rules(cfg)` (`07` §6.4).

Require `1 <= scan.max_notable <= u32::MAX`, with checked zero-based file-id assignment and SQL conversions.

### Alert rule fields (`AlertRule`, normative)

`AlertRule` is one flat struct with optional storage fields. `name`/`kind` are required. Every rule-specific field in a kind’s usage set below is required except applicable `vg`/`pool` globs (effective `*`); reject unused supplied fields and missing required fields even on disabled rules. `above: Option<AlertThreshold>` uses untagged `Number(f64)|Text(String)`; `below`, `window`, `older_than` are `Option<String>` interpreted during validation. `AlertKind` is an enum. `Config::validate` checks that each rule has exactly the fields its kind needs. Field names are as written in TOML; `for` maps to the Rust field `for_` via `#[serde(rename = "for")]`.

| Field | Type | Used by kinds |
|-------|------|---------------|
| `name` | string (unique) | all |
| `kind` | enum | nine kinds: `fs_used_pct`, `time_to_full`, `hot_dir_growth`, `thin_pool_pct`, `vg_free_low`, `dir_growth`, `phantom_bytes`, `scan_stale`, `watch_lossy` |
| `enabled` | bool (default true) | all |
| `for`, `resolve_for` | duration (defaults 5m / 10m) | all |
| `mount` | path | fs_used_pct, time_to_full, hot_dir_growth, phantom_bytes, watch_lossy |
| `above` | number, size (`"10GiB"`) or rate (`"50GiB/h"`) | fs_used_pct (percent), hot_dir_growth (finite nonnegative number in bytes/s or rate string), dir_growth, phantom_bytes (sizes), watch_lossy (count) |
| `below` | duration or size | time_to_full (duration), vg_free_low (size) |
| `window` | duration | time_to_full, hot_dir_growth, watch_lossy |
| `path` | glob | hot_dir_growth |
| `vg`, `pool` | glob (default `*`) | thin_pool_pct (`vg`, `pool`), vg_free_low (`vg`) |
| `data_above`, `meta_above` | percent | thin_pool_pct |
| `root` | path | dir_growth, scan_stale (required) |
| `depth` | integer | dir_growth |
| `older_than` | duration, or `"auto"` | scan_stale (`auto` = 2 × the effective cadence, where a windowed schedule counts as at least 24 h) |

Explicit `scan_stale` rules may target manually scanned roots; generated defaults exclude schedule `off`. Component-dependent rules remain visible but do not fire without available collector/input (`07` §6).

## 4. Metrics

All metrics are written to the node_exporter textfile collector (atomic write-and-rename every `metrics.interval`) as the only metrics output (ADR-020). Label values are escaped per the Prometheus text format; non-UTF-8 path bytes are converted lossily.

| Metric | Type | Labels |
|--------|------|--------|
| `tidemark_up` | gauge | — |
| `tidemark_build_info` | gauge | `version`, `commit` |
| `tidemark_fs_size_bytes`, `tidemark_fs_used_bytes`, `tidemark_fs_avail_bytes` | gauge | `mount` |
| `tidemark_fs_inodes_used` | gauge | `mount` |
| `tidemark_fs_growth_bytes_per_second` | gauge | `mount`, `window` (`15m`, `1h`) |
| `tidemark_fs_time_to_full_seconds` | gauge | `mount` (absent when not filling) |
| `tidemark_lvm_lv_size_bytes` | gauge | `vg`, `lv` |
| `tidemark_lvm_vg_free_bytes` | gauge | `vg` |
| `tidemark_lvm_thin_data_percent`, `tidemark_lvm_thin_metadata_percent` | gauge | `vg`, `pool` |
| `tidemark_dir_used_bytes`, `tidemark_dir_reclaimable_bytes` | gauge | `root`, `path` (depth ≤ `dir_metrics_depth`) |
| `tidemark_hot_bytes` | gauge | `mount`, `scope`, `key` (top 10, 5 m window) |
| `tidemark_phantom_bytes` | gauge | `mount` |
| `tidemark_open_writer_growth_bytes` | gauge | `mount`, `unit` (top 10) |
| `tidemark_scan_last_success_timestamp_seconds` | gauge | `root` |
| `tidemark_scan_duration_seconds` | gauge | `root`, `mode` |
| `tidemark_scan_errors` | gauge | `root` |
| `tidemark_watch_events_total`, `tidemark_watch_overflows_total` | counter | `mount` |
| `tidemark_alert_firing` | gauge | `rule`, `subject` |

**Cardinality guard:** no metric emits more than `max_series_per_metric` series. Excess series are dropped largest-first-kept, and `tidemark_metrics_dropped_series{metric}` is emitted.

## 5. Staged deployment (FR-25)

Designed for the hotfix pipeline. Nothing here touches fstab, LVM or the kernel command line.

```bash
# During the hotfix deploy (no reboot needed yet)
dnf install -y ./tidemark-<ver>-1.el8.x86_64.rpm
tidemark deploy stage --from ./tidemark.toml      # exit 0 = staged; blocking pre-flight failure = nothing changed; later failure = best-effort compensation
# … the hotfix's normal reboot …
tidemark deploy verify --wait 5m                    # exit 0 = healthy; 6 = not healthy (details printed)
```

**`deploy stage` pre-flight checks.** Blocking checks fail the stage with no changes made; warnings are printed and recorded.

| Check | Blocking? |
|-------|-----------|
| Binaries/unit present; destination ancestors writable/searchable and required ownership achievable (no effective-uid gate; ADR-018) | yes |
| Safe state_dir and no-follow state artifacts, as specified below | yes |
| Config parses and validates (`Config::validate`) | yes |
| Every `[[roots]]` path exists, is a directory, and sits on an allowlisted local filesystem (xfs, ext2, ext3, ext4, btrfs, tmpfs, zfs, f2fs, jfs, reiserfs; unknown types block; subtree roots allowed) | yes |
| The filesystem holding `general.state_dir`, or its longest existing ancestor when the directory does not exist yet, is an allowlisted local filesystem (xfs, ext2, ext3, ext4, btrfs, tmpfs, zfs, f2fs, jfs, reiserfs; unknown types block) | yes |
| Every `sampler.mounts` and `watch.mounts` entry is a mount point | yes |
| `state_dir` filesystem has ≥ 2 GiB free (plus the `min_free_pct` floor) | yes |
| fanotify available (`probe`), mount namespace OK | warning (watcher would be disabled) |
| XFS + v5 bulkstat available on `/data` | warning (walk mode would be used) |
| LVM visible (`lvs` succeeds) | warning |
| A previous version staged/running | info (config backed up, service restarted only with `--start-now`) |

A failed state-filesystem eligibility check reports the configured state path and filesystem type, exits with status 5, and makes no host changes. This check applies only to `deploy stage`; `deploy unstage` and `deploy purge` remain available for removing an installation whose state directory is on an ineligible filesystem. It does not add a daemon or first-boot check.

On success, `deploy stage`:

1. installs the config to the global `-c` destination (default `/etc/tidemark/tidemark.toml`) (`0600 root:root`), backing up any existing file as `tidemark.toml.<ts>.bak`;
2. runs `systemctl enable tidemarkd`, and also starts it with `--start-now`;
3. writes `/var/lib/tidemark/deploy/staged.json`, recording the version, time, check results and config hash.

The CLI `Context` carries `pub config_path: PathBuf`; global `-c` names the installed destination. `deploy stage --from` loads and validates its source independently, even when the installed config is missing or invalid. Blocking pre-flight failures change nothing. After pre-flight, publication/service failures trigger best-effort restoration of previous config, marker and staged-record bytes, removal of new backup files, and independent attempts to restore enabled/active service state. Report initiating and compensation errors with instructions to retry stage or unstage. This is not a transaction: downtime, timestamps and newly created empty directories may remain.

Local filesystem eligibility is exactly `xfs`, `ext2`, `ext3`, `ext4`, `btrfs`, `tmpfs`, `zfs`, `f2fs`, `jfs`, `reiserfs`; unknown types block. ADR-018 requires destination writability/searchability and required ownership, with no running-as-root check. Production files are root:root, 0600; state directories 0700.

Stage, unstage and firstboot refuse unsafe `state_dir` before mutations: `/`, nonabsolute paths, `..` components, symlink/nondirectory ancestry, mount points, and paths equal to or containing tracked roots/sampler/watch mounts (including existing roots' canonical locations). Also refuse paths equal to or containing `/etc`, `/usr`, `/var`, `/var/lib`, `/run`, `/home`, `/root`, `/boot`, `/dev`, `/proc`, `/sys`, `/tmp`. This is containment by state_dir: `/var/lib/tidemark` and private descendants of `/tmp` are eligible. Unsafe/mounted deploy subdirectories, symlink/nonregular artifacts and malformed/path-mismatched markers block too. Existing unmarked production directories must already have 0700 root:root; change ownership/mode only for new directories or a valid marker identifying that directory. Marker-backed unstage/firstboot recover tracked roots from the staged config; path-only calls without a marker protect default roots, and CLI unstage also checks loaded roots.

Every operation under state_dir uses no-follow directory handles: component-relative opens, regular-file reads, exclusive temporary writes, renameat/unlinkat and descriptor fsync. Retain handles through service actions and compensation; the installed config is also inspected/read through a verified parent and O_NOFOLLOW.

**On first start after staging**, `tidemarkd` writes `/var/lib/tidemark/deploy/firstboot.json` once it has been running for 2 minutes or once every component has reported, whichever is first. The report holds each component's state (`ok`/`degraded`/`disabled` + reason), the watch mode, the first samples' timestamps and the next scheduled scan per root. A one-line summary also goes to journald.

**Component names** are canonical and used in `firstboot.json`, `tidemark status` and the API:

| Component | Blocks `deploy verify` if not `ok`? |
|-----------|--------------------------------------|
| `sampler`, `scheduler`, `api`, `tswriter` | **yes** (the core) |
| `lvm`, `watcher:<mount>`, `procscan`, `alerts`, `metrics` | no: reported as `ok`/`degraded`/`disabled` with a reason. A host without LVM still verifies. |

**`deploy verify`** is healthy when all of these hold:

- the service is active;
- `firstboot.json` matches the staged config hash;
- every core component is `ok`;
- an API `status` ping succeeds.

`--wait` polls every 5 s. Output is a human summary, or JSON with the global `--json`.

## 6. systemd unit (`packaging/tidemarkd.service`)

```ini
[Unit]
Description=Tidemark disk usage monitor
Documentation=file:///usr/share/doc/tidemark/README.md
After=local-fs.target network-online.target
Wants=network-online.target

[Service]
Type=notify
ExecStart=/usr/bin/tidemarkd --config /etc/tidemark/tidemark.toml
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5s
User=root
UMask=0077
Nice=10
# No IOSchedulingClass here: scan worker threads set idle priority for themselves
# (04 §2.2); the samplers and DB writer must not be starved.
# cgroup v1 (EL8 default). Budget: ~300 MiB steady + scan memory_budget 1.25 GiB
# + SQLite build cache 256 MiB ≈ 1.8 GiB. Raise both together if you raise memory_budget.
MemoryLimit=2G
# cgroup v2
MemoryMax=2G
CPUQuota=200%
LimitNOFILE=65536
StateDirectory=tidemark
StateDirectoryMode=0700
RuntimeDirectory=tidemark
RuntimeDirectoryMode=0700
NoNewPrivileges=yes
CapabilityBoundingSet=CAP_SYS_ADMIN CAP_DAC_READ_SEARCH CAP_FOWNER CAP_SYS_PTRACE
RestrictNamespaces=yes
RestrictRealtime=yes
LockPersonality=yes
SystemCallArchitectures=native
RestrictAddressFamilies=AF_UNIX AF_NETLINK
# DO NOT add ProtectSystem=, ProtectHome=, PrivateTmp=, ReadWritePaths=, ProtectKernel*= or
# other options that create a private mount namespace: fanotify mount marks would then attach
# to the namespace's copies and the watcher would silently see nothing (ADR-013).

[Install]
WantedBy=multi-user.target
```

Also shipped:

- `tmpfiles.d/tidemark.conf` creates `/run/tidemark` as `0700 root:root` (it does not create the node_exporter textfile directory).
- Default config goes to `/etc/tidemark/tidemark.toml` as `%config(noreplace)`, mode `0600 root:root`.
