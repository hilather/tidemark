# 08 — Interfaces: CLI, daemon API, configuration, metrics, systemd

Everything here is a contract. Changing it after Wave 0 requires a spec-change request (`10-agent-playbook.md` §6).

---

## 1. CLI (`tidemark`)

### 1.1 Global options

| Option | Meaning |
|--------|---------|
| `-c, --config <FILE>` | Config file (default `/etc/tidemark/tidemark.toml`; missing file → built-in defaults) |
| `--state-dir <DIR>` | Override `general.state_dir` |
| `--json` | Machine output. Every command supports it; each document carries `"schema": "tidemark.<command>.v1"` |
| `--color auto\|always\|never` | Terminal colour |
| `-v / -q` | Log verbosity to stderr |

Paths given on the command line are canonicalised (`realpath`) before lookup, so a symlinked path finds the real directory. Output always shows canonical paths.

### 1.2 Commands

| Command | Purpose | Key options | Needs |
|---------|---------|-------------|-------|
| `probe` | Capability and environment report (FR-1) | `--root PATH…` | root |
| `scan [ROOT]` | Run a scan and write a snapshot (FR-2/3) | `--mode auto\|walk\|bulk`, `--throttle gentle\|normal\|max`, `--threads N`, `--exclude GLOB…`, `--keep-partial`, `--cancel` (ask the daemon to cancel the running scan of ROOT) | root |
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
| `pg` | Postgres breakdown (FR-16) | `--relations`, `--db NAME` | root |
| `alerts` | Active alerts; `--all` for history | | root |
| `deploy stage` | Pre-flight checks, install config, enable the service for next boot (FR-25) | `--from FILE` (config to install; the global `-c` still names the installed path), `--start-now`, `--skip-check NAME…` (warning-level checks only) | root |
| `deploy verify` | Post-reboot pass/fail for the pipeline (FR-25) | `--wait DUR` (poll until healthy or timeout); machine output via the global `--json` | root |
| `deploy unstage` | Stop and disable the service; keep state (FR-25) | `--purge-state` (also delete `state_dir`), `--yes` | root |
| `watch` | Foreground event stream for debugging | `--mount`, `--events close_write,modify` | root |
| `status` | Daemon health, versions, component states | `--show-config` (print the effective config including default alert rules) | root |
| `tui [PATH]` | Interactive browser (FR-21) | `--snapshot` | root |
| `completions <SHELL>` | Shell completions | | — |

All commands require effective uid 0, except `completions` and `--help` (including command help). A non-root invocation fails early, before reading config or state or connecting to the daemon, with exit code **5** and a clear message: `Tidemark requires root; run this command with sudo.`

### 1.3 Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime error |
| 2 | Usage or configuration error |
| 3 | Completed with errors (degraded or partial scan, some queries failed) |
| 4 | Daemon unavailable (live commands) |
| 5 | Precondition not met (not root, not XFS, unsupported kernel feature, `deploy stage` pre-flight failed) |
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

- **Transport:** Unix stream socket `/run/tidemark/api.sock`, mode `0600 root:root`.
- **Framing:** newline-delimited JSON. One request per line, one response per line; the connection may carry several requests.
- **Authentication:** socket permissions. Every request is logged with the peer's `SO_PEERCRED` uid/pid. Every method requires peer uid 0; other peers receive `forbidden`.

```json
{"v":1,"id":"c1","method":"now","params":{"mount":"/data"}}
{"v":1,"id":"c1","ok":true,"result":{ "...": "NowReport (07 §7)" }}
{"v":1,"id":"c2","ok":false,"error":{"code":"unavailable","message":"watcher disabled"}}
```

| Method | Params | Result (types from `07` §7a unless stated) |
|--------|--------|--------|
| `status` | — | `StatusResult { version, uptime_s, components: Vec<ComponentState>, watch_modes: Vec<(mount, mode)>, last_errors: Vec<(component, ts, message)> }` (`ComponentState` from W4-B `firstboot`; names per §5) |
| `now` | `mount?` | `NowReport` (`07` §7) |
| `hot` | `mount?`, `window` (1m/5m/15m/60m), `by` (dir/unit/comm), `limit` | `Vec<HotEntry>` |
| `lvm` | `mount?` | `LvmResult { mounts: Vec<(mount, LvmNow)>, vgs: Vec<VgInfo>, sampled_at }` (`VgInfo` from W4-A) |
| `phantom` | `mount?` | `Vec<PhantomSummary>` (one per mount) |
| `writers` | `mount?`, `window` | `Vec<OpenWriter>` |
| `pg` | `relations?`, `db?` | `Option<PgNow>` |
| `alerts.active` / `alerts.history` | `limit?` | `Vec<AlertSummary>` / `Vec<AlertEvent>` |
| `scan.start` | `root`, `mode?`, `throttle?` | `{scan_id}`, or error `busy` (another scan holds the host lock) |
| `scan.status` | `root?` | `Vec<ScanStatus>` |
| `scan.cancel` | `root` | `{cancelled: bool}` |
| `config.reload` | — | `{reloaded: bool, warnings: Vec<String>}` |

Error codes: `bad_request`, `forbidden`, `not_found`, `unavailable`, `busy`, `internal`. Requests time out server-side after 10 s.

## 3. Configuration (`/etc/tidemark/tidemark.toml`)

Durations use humantime (`30s`, `6h`); sizes use binary units (`1GiB`). Unknown keys are rejected with a helpful error. `tidemark status` and `tidemarkd --check-config` validate the file.

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
throttle           = "normal"          # gentle|normal|max
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
exclude_postgres = true                # skip PGDATA when [postgres] is enabled (collector covers it)
lru_entries      = 200000

[procscan]
enabled          = true
interval         = "60s"
open_writers     = true
exclude_prefixes = []                  # PGDATA base/ and global/ are added automatically when [postgres] is enabled

[postgres]
enabled            = false
dsn                = "host=/var/run/postgresql user=tidemark dbname=postgres application_name=tidemark"
interval           = "5m"
relations_interval = "1h"
top_relations      = 20
statement_timeout  = "30s"

[metrics]
textfile              = "/var/lib/node_exporter/textfile_collector/tidemark.prom"   # "" = disabled
listen                = ""             # e.g. "127.0.0.1:9877"; "" = disabled
interval              = "60s"
dir_metrics_depth     = 2
max_series_per_metric = 500

[alerts]
webhook  = ""
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

### Alert rule fields (`AlertRule`, normative)

`AlertRule` is one flat struct with optional fields. `Config::validate` checks that each rule has exactly the fields its kind needs. Field names are as written in TOML; `for` maps to the Rust field `for_` via `#[serde(rename = "for")]`.

| Field | Type | Used by kinds |
|-------|------|---------------|
| `name` | string (unique) | all |
| `kind` | enum | all: `fs_used_pct`, `time_to_full`, `hot_dir_growth`, `thin_pool_pct`, `vg_free_low`, `dir_growth`, `phantom_bytes`, `pg_wal_bytes`, `pg_slot_retained`, `pg_archiver_failing`, `scan_stale`, `watch_lossy` |
| `enabled` | bool (default true) | all |
| `for`, `resolve_for` | duration (defaults 5m / 10m) | all |
| `mount` | path | fs_used_pct, time_to_full, hot_dir_growth, phantom_bytes, watch_lossy |
| `above` | number, size (`"10GiB"`) or rate (`"50GiB/h"`) | fs_used_pct (percent), hot_dir_growth (rate), dir_growth, phantom_bytes, pg_wal_bytes, pg_slot_retained (sizes), watch_lossy (count) |
| `below` | duration or size | time_to_full (duration), vg_free_low (size) |
| `window` | duration | time_to_full, hot_dir_growth, pg_archiver_failing, watch_lossy |
| `path` | glob | hot_dir_growth |
| `vg`, `pool` | glob (default `*`) | thin_pool_pct (`vg`, `pool`), vg_free_low (`vg`) |
| `data_above`, `meta_above` | percent | thin_pool_pct |
| `root`, `depth` | path, integer | dir_growth |
| `older_than` | duration, or `"auto"` | scan_stale (`auto` = 2 × the effective cadence, where a windowed schedule counts as at least 24 h) |

## 4. Metrics

All metrics are written to the node_exporter textfile collector (atomic write-and-rename every `metrics.interval`) and served on `metrics.listen` if set. Label values are escaped per the Prometheus text format; non-UTF-8 path bytes are converted lossily.

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
| `tidemark_pg_database_bytes` | gauge | `database` |
| `tidemark_pg_wal_dir_bytes` | gauge | — |
| `tidemark_pg_slot_retained_bytes` | gauge | `slot` |
| `tidemark_pg_archiver_failed_total` | counter | — |
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
tidemark deploy stage --from ./tidemark.toml      # exit 0 = staged; non-zero = nothing changed
# … the hotfix's normal reboot …
tidemark deploy verify --wait 5m                    # exit 0 = healthy; 6 = not healthy (details printed)
```

**`deploy stage` pre-flight checks.** Blocking checks fail the stage with no changes made; warnings are printed and recorded.

| Check | Blocking? |
|-------|-----------|
| Running as root; binaries and unit file present (RPM installed) | yes |
| Config parses and validates (`Config::validate`) | yes |
| Every `[[roots]]` path exists, is a directory, and sits on a local filesystem (subtree roots such as `/data/reports` are allowed) | yes |
| Every `sampler.mounts` and `watch.mounts` entry is a mount point | yes |
| `state_dir` filesystem has ≥ 2 GiB free (plus the `min_free_pct` floor) | yes |
| fanotify available (`probe`), mount namespace OK | warning (watcher would be disabled) |
| XFS + bulkstat available on `/data` | warning (walk mode would be used) |
| LVM visible (`lvs` succeeds) | warning |
| Postgres reachable with `pg_monitor` (if enabled) | warning |
| A previous version staged/running | info (config backed up, service restarted only with `--start-now`) |

On success, `deploy stage`:

1. installs the config to `/etc/tidemark/tidemark.toml` (`0600 root:root`), backing up any existing file as `tidemark.toml.<ts>.bak`;
2. runs `systemctl enable tidemarkd`, and also starts it with `--start-now`;
3. writes `/var/lib/tidemark/deploy/staged.json`, recording the version, time, check results and config hash.

**On first start after staging**, `tidemarkd` writes `/var/lib/tidemark/deploy/firstboot.json` once it has been running for 2 minutes or once every component has reported, whichever is first. The report holds each component's state (`ok`/`degraded`/`disabled` + reason), the watch mode, the first samples' timestamps and the next scheduled scan per root. A one-line summary also goes to journald.

**Component names** are canonical and used in `firstboot.json`, `tidemark status` and the API:

| Component | Blocks `deploy verify` if not `ok`? |
|-----------|--------------------------------------|
| `sampler`, `scheduler`, `api`, `tswriter` | **yes** (the core) |
| `lvm`, `watcher:<mount>`, `procscan`, `pg`, `alerts`, `metrics` | no: reported as `ok`/`degraded`/`disabled` with a reason. A host without LVM or Postgres still verifies. |

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
After=local-fs.target network-online.target postgresql.service
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
MemoryLimit=2G                   # cgroup v1 (EL8 default). Budget: ~300 MiB steady + scan memory_budget 1.25 GiB
                                 # + SQLite build cache 256 MiB ≈ 1.8 GiB. Raise both together if you raise memory_budget.
MemoryMax=2G                     # cgroup v2
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
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
# DO NOT add ProtectSystem=, ProtectHome=, PrivateTmp=, ReadWritePaths=, ProtectKernel*= or
# other options that create a private mount namespace: fanotify mount marks would then attach
# to the namespace's copies and the watcher would silently see nothing (ADR-013).

[Install]
WantedBy=multi-user.target
```

Also shipped:

- `tmpfiles.d/tidemark.conf` creates `/run/tidemark` as `0700 root:root` (it does not create the node_exporter textfile directory).
- Default config goes to `/etc/tidemark/tidemark.toml` as `%config(noreplace)`, mode `0600 root:root`.
