# Tidemark operator guide

The packaged service permits only Unix sockets (`RestrictAddressFamilies=AF_UNIX`).

For administrators on Rocky Linux 8 with access through `sudo`. Examples use `/data`; substitute your actual mount and subtree names. Tidemark reads metadata, keeps snapshots, and observes growth. It does not delete data or resize storage. Scans skip symlinks and all nested mounts, including same-device bind mounts. Scans request O_NOATIME on directories (EPERM fallback is a limit). `tidemark symlinks`, even without --check-targets, reads link text and may update that link’s access time under the mount’s atime policy. See the [security model](security.md).

## Install and inspect the host

The RPM includes command manuals: `man tidemark` and `man tidemarkd`.

Obtain the approved EL8 RPM and its checksum from your release pipeline. Replace the filename below with that artifact.

```sh
sudo dnf install ./tidemark-0.1.0-1.el8.x86_64.rpm
sudo tidemark probe --root / /data
sudo findmnt -t xfs -o TARGET,SOURCE,OPTIONS
sudo df -h / /data
sudo df -i / /data
```

Installation leaves `tidemarkd` **disabled and stopped**. Read the probe before staging: check filesystem type, XFS geometry/reflink, bulkstat and fanotify capabilities, leaf devices, LVM mapping and privileges. Probe actual capabilities; a kernel version alone is not enough. Rocky 8.8 and 8.10 support bulkstat v5 and normal fanotify fd mode. Tidemark ships no SELinux policy and supports Enforcing, Permissive and Disabled modes; do not change SELinux mode to install it.

Config and snapshots are root-owned `0600`; state and runtime directories are `0700`, with no group access. There is no Tidemark access group. Run every operator command through `sudo`.

## Prepare the configuration

Copy the installed `/etc/tidemark/tidemark.toml` into your controlled deployment repository and edit that copy, here `./tidemark.toml`. Start with the packaged configuration, keeping only roots, sampler mounts and watcher mounts that exist on this host. A sampler/watch path must be an **exact mount point**, not a directory somewhere below one. Scan roots can be subtrees.

This is a configuration fragment: replace the matching sections in the full file, rather than appending duplicate tables.

```toml
[[roots]]
path = "/data"
mode = "auto"
schedule = "every 6h"
window = "00:00-06:00"
throttle = "gentle"
exclude = []
series_depth = 2
big_file_threshold = "1GiB"
top_files = 10000

[scan]
threads = 8
memory_budget = "1280MiB"
link_instances = "all"

[sampler]
mounts = ["/", "/data"]
interval = "30s"

[watch]
enabled = true
mounts = ["/", "/data"]
events = ["close_write"]

[metrics]
textfile = "/var/lib/node_exporter/textfile_collector/tidemark.prom"
interval = "60s"
dir_metrics_depth = 2
max_series_per_metric = 500
```

Keep `general.state_dir` on local storage, normally `/var/lib/tidemark` on the root filesystem, away from the busy data array where possible. Tidemark excludes its own state automatically. Stage requires an allowlisted local state filesystem (or the longest existing ancestor if the directory is missing); network or unsupported storage is refused. The stage allowlist is xfs, ext2, ext3, ext4, btrfs, tmpfs, zfs, f2fs, jfs and reiserfs; unknown types block. Use persistent storage for retained production history. For a custom `--state-dir`, every ancestor must be a real directory reached without symbolic links, owned by root or the invoking effective user, with no group/world writes unless sticky. Sticky ancestors require an owned child without group/world write; writable sticky chains propagate that rule. Storage state/used subdirectories and regular single-link leaves must be owned by the effective user and not group/world writable. Storage accepts readability with a warning, but production state should stay root-owned 0700 with 0600 leaves. Daemon/deploy admission requires private state; a checked deployment marker can authorize normalizing read/execute bits, never foreign ownership or write bits. Preexisting unsafe paths are refused; Tidemark does not repair ownership. Packaged builds accept no user-namespace or read-only-mount ownership exception. A symbolic link anywhere in the state path is refused, even if its target is trusted; configure the real path (the link refusal says `Not a directory`). Development builds allow warning-bearing overflow-uid and read-only-mount ancestry exceptions, announce them once per process, and can enforce the packaged rules with `TIDEMARK_STRICT_ANCESTRY=1`. Metrics collector ancestry must also be trusted. See [security](security.md#state-directory-and-leaf-trust) before choosing custom paths.

Validate the full candidate:

```sh
sudo tidemarkd --check-config --config ./tidemark.toml
```

There is no HTTP service or alert webhook in v1. `metrics.listen` and `alerts.webhook` do not exist in the v1 configuration: omit them.
<!-- gap: this checkout's packaged config and core parser still contain metrics.listen and alerts.webhook; ADR-020 removal is assigned separately. Do not enable either legacy setting. -->

## Stage, reboot, verify

Use the [pilot checklist](pilot/checklist.md) for the first host. Staging installs the candidate config and enables the service for the **next boot**. It does not normally start it now. A blocking pre-flight refusal (exit 5) changes nothing; failures after pre-flight attempt compensation and can leave changes or downtime. Read both the initiating and compensation errors before retrying.

```sh
sudo tidemark deploy stage --from ./tidemark.toml
```

Reboot through the approved maintenance pipeline. After the host returns:

```sh
sudo tidemark deploy verify --wait 5m
sudo tidemark status
sudo tidemark now
sudo tidemark hot --mount /data --window 5m
```

Verify requires an active service, matching staged/firstboot config hashes, healthy sampler/scheduler/api/tswriter components and a working API ping. It returns 0 when these checks pass, and 6 when not healthy yet or timed out. Other collectors can still be degraded or disabled: inspect status before relying on their reports. Archive machine results with `sudo tidemark --json deploy verify --wait 5m`. `--start-now` on stage is available for an approved immediate start. The global `--config` names the installed destination; `--from` names the independent source candidate. See [failed verification](runbooks/deploy-verify-failed.md).

## What is filling the filesystem now?

```sh
sudo tidemark now --mount /data
sudo tidemark hot --mount /data --window 5m --by dir
sudo tidemark hot --mount /data --window 5m --by unit
sudo tidemark writers --mount /data --window 5m
sudo tidemark phantom --mount /data
sudo tidemark lvm --mount /data
```

`now` combines filesystem free space, growth, time to full, hot directories, writers, deleted-open files, LVM and scan health. The displayed rate is the 15-minute slope in bytes/hour; time to full uses the one-hour slope. Different windows can disagree during a burst. Missing or insufficient measurements are unknown, not zero. These estimates do not predict sudden workload changes.

<!-- Sample rendered by the real CLI renderer from example_now() in crates/tidemark-cli/tests/w6a3.rs, width 100; not a production host. -->
```text
tidemark now - fixture - 2026-10-12 22:52:56Z
/data used 47.7 MiB / 954 MiB (5.0%)  7.92 GiB/h (15m)  full in ~3m 9s (1h trend)
  lvm n/a
  hot dirs (5m) /data/hot  +47.7 MiB  2 files  writer.service
  hot dirs (5m) /bad\xFF  +1 B  1 files  ?
  writers (5m) other.service +100 B
  deleted-but-open 0 B (0 files)
last scan /data 2026-10-12 22:52:56Z (0s ago, walk, 5ms)
```

`hot` reports observed allocated-byte changes at close-write events, not all bytes written or exact directory totals. `LOSSY=true` means observations were lost; use a scan for totals. Windows are `1m`, `5m`, `15m`, `60m`. Across minutes, FILES sums per-minute distinct counts: one file touched in two minutes can count twice. TOP UNIT is an approximate leader when many units write the same directory; `?` means attribution is unavailable. Processes that write and exit within a fraction of a second commonly appear as `?`.

<!-- Sample rendered from example_now() hot rows in crates/tidemark-cli/tests/w6a3.rs, width 100. -->
```text
KEY        EVENTS  FILES  DELTA BYTES  TOP UNIT        LOSSY
/data/hot  3       2      +50000000    writer.service  false
/bad\xFF   1       1      +1           ?               false
```

`writers` compares allocated sizes of still-open write descriptors; it catches appenders that have not closed. `phantom` finds deleted files still held open. Neither is a cleanup command. With no daemon, writers/phantom/lvm announce direct fallback. For an explicit fresh `/proc` investigation:

```sh
sudo tidemark phantom --mount /data --direct
sudo tidemark writers --mount /data --direct --interval 10s
```

The two writer passes measure growth over their actual elapsed interval. Limited inventories cannot prove there are no other holders. See [disk filling](runbooks/disk-filling.md) and [df/du mismatch](runbooks/df-du-mismatch.md).

## Make a snapshot and find the largest trees

```sh
sudo tidemark scan /data --mode auto --throttle gentle
sudo tidemark snapshots list --root /data
sudo tidemark top /data --depth 1
sudo tidemark top /data/reports --wide --sort reclaimable
sudo tidemark files /data/reports --limit 25 --min-size 1GiB
sudo tidemark wide /data --min-entries 10000
```

`scan` uses the running daemon. When its socket is absent or refuses connection, it scans directly. Other socket errors, including permission or transport failure, return 4 rather than scanning behind the daemon's back. A forwarded scan accepts mode/throttle overrides, but refuses `--threads`, extra `--exclude` and `--keep-partial`; change daemon configuration for those settings.

Bulk mode is faster on eligible XFS roots. Disk allocation is exact, but apparent sizes of files being written or awaiting writeback can lag, even to zero for a new file. Walk mode reads current in-memory metadata for both. Scans are a time-bounded, changing-tree view, not a filesystem transaction. Tidemark never forces writeback to correct bulk apparent sizes.

`top` reads the chosen snapshot, not live totals. Check its ID, mode, age, status and degraded marker. `--snapshot ID` fixes a report to an older snapshot. `!` indicates recorded errors: totals may be incomplete. `--wide` adds apparent size, inode count and newest modification time (UTC). `files` lists retained notable files, not every filename; raise configured thresholds/caps deliberately if necessary. Shared files carry a marker and a representative path. `wide` identifies directories with many direct entries, which can make listing and sorting costly.

`tidemark files PATH --direct` shows retained notable files directly in PATH, excluding subdirectories. In the TUI, Enter on `.` shows these files and the remaining file count and charged bytes not individually retained.

The following real direct-mode fixture has one 1.5 MiB file hard-linked into two siblings and an 8 MiB sparse file. It ran unprivileged with a private config/socket and state directory; production commands above use sudo.
<!-- Real CLI stdout captured on 2026-10-13; /tmp fixture on workstation, not the normative XFS example. -->
```text
Snapshot 20261013T051206Z-704e53ea · Walk · 0s ago · Complete
PATH                                      SIZE      RECLAIM   SHARED      FILES  NEWEST                INODES  APPARENT
/tmp/tidemark-w7-fixture/root             1.50 MiB  1.50 MiB  0 B         4      2026-10-13 05:12:06Z  3       9.50 MiB
├─ /tmp/tidemark-w7-fixture/root/archive  1.50 MiB  0 B       H 1.50 MiB  2      2026-10-13 05:12:06Z  2       1.50 MiB
├─ /tmp/tidemark-w7-fixture/root/reports  1.50 MiB  0 B       H 1.50 MiB  1      2026-10-13 05:12:06Z  1       1.50 MiB
└─ /tmp/tidemark-w7-fixture/root/tmp      0 B       0 B       0 B         1      2026-10-13 05:12:06Z  1       8.00 MiB
H = hard links shared outside this directory (see tidemark links)
```

## What grew since yesterday?

```sh
sudo tidemark diff /data --since 24h --explain
sudo tidemark snapshots list --root /data
```

Two complete snapshots are needed. Read the From/To headers: `--since 24h` selects available history, not necessarily an exact 24-hour boundary. For repeatable incident evidence, use `--from ID --to ID`, substituting actual catalog IDs. `--explain` follows the largest-growth branch until growth is spread or below the explanation threshold. `--depth`, `--explain-min` and `--explain-ratio` control that drill-down. Warnings about age or incomplete coverage matter; failed and partial scans are not growth baselines.

<!-- Real CLI direct fixture diff; 512 KiB appended between the captured scan IDs. Command: sudo tidemark --config /tmp/tidemark-w7-fixture/config.toml --state-dir /tmp/tidemark-w7-fixture/state diff /tmp/tidemark-w7-fixture/root --from 20261013T051206Z-56791a34 --to 20261013T051206Z-704e53ea --explain --explain-min 1KiB (capture omitted sudo in sandbox). -->
```text
From: Snapshot 20261013T051206Z-56791a34 · Walk · 7s ago · complete
To: Snapshot 20261013T051206Z-704e53ea · Walk · 7s ago · complete
CLASS  DELTA  PATH
+  +512 KiB  /tmp/tidemark-w7-fixture/root
+  +512 KiB    /tmp/tidemark-w7-fixture/root/archive
+  +512 KiB    /tmp/tidemark-w7-fixture/root/reports
=  +0 B    /tmp/tidemark-w7-fixture/root/tmp
Explanation: /tmp/tidemark-w7-fixture/root +512 KiB
└─ /tmp/tidemark-w7-fixture/root/archive +512 KiB (100%)
Change is spread across 0 subdirectories; top 0:
Own charged bytes: +0 B
```

Both linked siblings show the same growth. Do not add their SIZE deltas: they refer to the same inode. `top --sort growth` also needs older complete history.

## Read the size columns before planning cleanup

| Column | Meaning |
|---|---|
| SIZE | Allocated bytes of distinct inodes visible in this subtree, including directory blocks; comparable to a separate `du -sxB1` invocation. |
| APPARENT | Logical lengths, including directory sizes; sparse files can be much larger here. Bulk regular-file values can lag writeback. |
| CHARGED | Allocated bytes assigned once at the lowest common ancestor of the found links; this view adds up. |
| RECLAIM | Bytes whose links are all inside the subtree; an estimate of deletion benefit, ignoring open handles and reflink sharing. |
| SHARED / H | SIZE minus RECLAIM: links elsewhere keep these bytes alive. |
| FILES / INODES | Non-directory entries versus distinct non-directory inodes; two hard links are two entries and one inode. |

This is the **normative model example**, reproduced from [link semantics §6](plan/03-link-semantics.md#6-worked-example-golden-test-link_semantics_basic), not command output. Directory blocks are zero in this small XFS model. `a/x` and `b/y` share a 4,096-byte inode; `a/big` is 1,048,576 bytes; `b/z` is 8,192 bytes with two additional links outside `/r`; `c/d/w` and `c/e/v` share 12,288 bytes; `c/s` is a zero-block symlink.

| Directory | SIZE | CHARGED | RECLAIM | SHARED | FILES | INODES | DIRS |
|---|---:|---:|---:|---:|---:|---:|---:|
| `/r/a` | 1,052,672 | 1,048,576 | 1,048,576 | 4,096 | 2 | 2 | 1 |
| `/r/b` | 12,288 | 8,192 | 0 | 12,288 | 2 | 2 | 1 |
| `/r/c/d` | 12,288 | 0 | 0 | 12,288 | 1 | 1 | 1 |
| `/r/c/e` | 12,288 | 0 | 0 | 12,288 | 1 | 1 | 1 |
| `/r/c` | 12,288 | 12,288 | 12,288 | 0 | 3 | 2 | 3 |
| `/r` | 1,073,152 | 1,073,152 | 1,064,960 | 8,192 | 7 | 5 | 6 |

Sibling SIZE totals exceed `/r` by 4,096 bytes. CHARGED adds up with 4,096 charged directly to `/r`. Removing `b` would free nothing; removing all of `c` would free 12,288 even though neither `c/d` nor `c/e` alone frees any. RECLAIM is evidence to discuss with the data owner, never authorization to delete.

```sh
sudo tidemark links /data --boundary-depth 1
sudo tidemark links /data/reports --incomplete --top 20
```

`links` shows crossing sets, found/kernel link counts, the lowest common ancestor and pinned bytes. Incomplete sets mean links lie outside the scanned coverage, including exclusions or unreadable areas. `--show-paths` resolves live names; files may have changed since the snapshot. Reflinks are separate inodes sharing extents: SIZE can overstate physical consumption, as `du` does. Tidemark v1 does not measure exact shared reflink extents.

## Check LVM separately from filesystem fullness

```sh
sudo tidemark lvm --mount /data
sudo tidemark now --mount /data
```

Thin-pool data and metadata percentages can become critical while `df` still shows free space. VG FREE is unallocated volume-group headroom, not filesystem free space. An unknown percentage is not zero. Trends need history (at least three usable samples); percentages/day are percentage points/day. LVM disabled, missing tools or an unmapped filesystem can show n/a.

Daemon LVM reports omit segment types and other LVs. Direct fallback includes them. Machine consumers must check the schema: this checkout emits `tidemark.lvm.daemon.v1` for daemon data and `tidemark.lvm.v1` for direct data.
<!-- gap: 08 and R4-12 still describe the two LVM shapes under one schema; current renderers distinguish them. Recheck after the LVM contract follow-up. -->
<!-- Sample rendered from the empty daemon LVM fixture in crates/tidemark-cli/tests/w6a3.rs; no LVM host command was run. -->
```text
View from daemon; segment types and other volumes are shown only when the daemon is not running.
MOUNT  VG/LV  SIZE  VG FREE  THIN POOL  DATA %  META %  DATA pp/day  META pp/day
```

Use the [thin-pool runbook](runbooks/thin-pool-filling.md) and a storage administrator's approved change procedure for capacity actions.

## Alerts

```sh
sudo tidemark alerts
sudo tidemark alerts --all
sudo journalctl -u tidemarkd --since '-1 hour' --no-pager
```

Alerts appear in the journal, `alerts`, and `now`. `alerts --all` covers only the current daemon run; retain the journal externally for incident history.
<!-- gap: persisted historical alerts are not exposed by alerts --all; it is current-run history only. -->
<!-- Sample rendered from the active-alert fixture in crates/tidemark-cli/tests/w6a3.rs, width 100. -->
```text
RULE  SUBJECT  SINCE                 VALUE  MESSAGE
full  /data    1970-01-01 00:00:01Z  90     full
```

Rules use `for` before firing (default 5m), `resolve_for` before resolving (default 10m), and `[alerts].renotify` (default 4h). If a collector is disabled or its data is unavailable, a rule is **not evaluable**: it must not be treated as healthy or zero. Review degraded reasons and journal diagnostics. Rules are merged by name with built-in defaults. Override a default using its exact name (or set its `enabled = false`); a differently named rule is an additional rule. Review the full effective set.

| Kind | Use |
|---|---|
| `fs_used_pct` | `mount`, numeric `above` percent. |
| `time_to_full` | `mount`, `below` duration, `window` duration. |
| `hot_dir_growth` | `mount`, `path` glob, `above` rate, `window`. |
| `thin_pool_pct` | `vg`, `pool`, `data_above`, `meta_above` percentages. |
| `vg_free_low` | `vg`, `below` byte size. |
| `dir_growth` | `root`, `depth`, `above` byte size between snapshots. |
| `phantom_bytes` | `mount`, `above` byte size. |
| `scan_stale` | `root`, `older_than` duration or `auto` (twice effective cadence). |
| `watch_lossy` | `mount`, numeric `above` overflow count, `window`. |

Example additional rule (review default rules too):

```toml
[[alerts.rule]]
name = "data_full_soon"
kind = "time_to_full"
mount = "/data"
below = "12h"
window = "1h"
for = "10m"
resolve_for = "10m"
```

Watcher rules `hot_dir_growth` and `watch_lossy` require whole-minute windows from 1 to 60 minutes. Exact hot-growth evaluation requires consecutive completed, healthy minutes without loss or truncation; overflow evaluation needs consistent boundary counts. `now`'s ranked five-minute list is not proof of an exact alert window. For hot growth, numeric `above` means bytes/second, or use a rate such as `"50GiB/h"`.
Exact watcher-window publication is implemented. The newest completed boundary must be no more than 90 seconds old; startup’s partial minute is excluded, so warmup needs the next boundary plus the full configured minutes. Missing/lossy/truncated hot history is unavailable, while overflow uses consistent exact boundary totals. Files are summed per-minute counts, not distinct files across the whole window.

## Tune scans against workload impact

Start the pilot with `gentle`: targets 50% leaf-device utilisation and 15ms average latency. `normal` targets 70% and 25ms. These are adaptive controller targets, not guarantees against bursts. `max` disables throttling; reserve it for controlled measurements. The gate backs off when the device is busy. Measure workload latency alongside `iostat`; a faster scan is not useful if it violates the application SLO.

Set each root's `schedule` to `every 6h`, `daily 03:30` or `off`, and an optional local-time `window` such as `00:00-06:00`. Windows restrict **start** times; a running scan can finish outside them. Reduce `scan.threads` or tighten `[throttle.profiles.gentle]` targets if impact persists. Custom profiles may be selected in root config; CLI overrides accept only gentle/normal/max.

`scan.memory_budget` is the scan working-set budget, not total daemon RSS. The packaged service limit is 2 GiB; the default scan budget is 1280 MiB. Leave room for watchers and other daemon components. Budget/cap failures require smaller scans or coordinated tuning, not blindly raising the service limit. Keep `scan.link_instances = "all"` for complete link topology. Root `exclude` entries are full-path globs, for example `"/data/tmp/cache/**"`; watcher/procscan `exclude_prefixes` are separate path prefixes. Exclusions make link sets conservative and can explain smaller totals than `du`.

Validate edits before reload:

```sh
sudo tidemarkd --check-config --config /etc/tidemark/tidemark.toml
sudo systemctl reload tidemarkd
sudo tidemark status --show-config
sudo journalctl -u tidemarkd --since '-5 minutes' --no-pager
```

Invalid reload preserves prior daemon configuration and reports warnings. `status --show-config` labels the **CLI** configuration; it is not confirmation that the daemon accepted that generation. Check the journal and component health.

## Metrics and monitoring

Tidemark writes Prometheus text exposition to `[metrics].textfile`, normally `/var/lib/node_exporter/textfile_collector/tidemark.prom`, every 60s. Provision that collector directory using your node_exporter deployment, and enable its textfile collector for the same directory. Confirm exporter access without loosening Tidemark's root-only config, socket or snapshot permissions. Prometheus scrapes **node_exporter**, never Tidemark.

Import the supplied [Grafana dashboard](../packaging/grafana/tidemark.json) and load [Prometheus rules](../packaging/prometheus/tidemark-alerts.yml) through your monitoring change process. They cover time-to-full, thin-pool data/metadata, phantom bytes, stale scans, watcher overflows, availability and active Tidemark alerts. Start with a non-paging destination during the pilot.
<!-- gap: dashboard and rules exist in packaging/ but this checkout's RPM asset list does not install them; obtain them from the matching release source bundle. -->

Check collector output and monitoring freshness:

```sh
sudo tidemark status
sudo stat /var/lib/node_exporter/textfile_collector/tidemark.prom
sudo cat /var/lib/node_exporter/textfile_collector/tidemark.prom
```

Inspect `tidemark_up`, `tidemark_scan_last_success_timestamp_seconds`, `tidemark_watch_overflows_total`, and the configured directory-series caps. A stale textfile can leave old gauges visible: the packaged `TidemarkDown` absent-series rule alone cannot detect this. Add exporter textfile-mtime freshness monitoring for this file through the monitoring team. Lower `dir_metrics_depth` or `max_series_per_metric` to limit cardinality; caps omit eligible rows and should not be mistaken for full inventories.

## Health and troubleshooting

```sh
sudo tidemark status
sudo systemctl status tidemarkd --no-pager
sudo journalctl -u tidemarkd --since '-30 minutes' --no-pager
```

<!-- Sample rendered from the status fixture in crates/tidemark-cli/tests/w6a3.rs, width 100. Empty schedules/watch modes are fixture inputs. -->
```text
tidemarkd fixture · uptime 42s
COMPONENT  STATE  DETAIL
api        ok     ready
ROOT  RUNNING  NEXT RUN  LAST COMPLETE
```

| Symptom | Next step |
|---|---|
| Daemon will not start | Run `tidemarkd --check-config` with the installed path; inspect journal for config, state-storage permissions/free space, unit or collector failure. Do not infer enablement from RPM installation. |
| `deploy verify` fails | Read JSON details, staged versus firstboot hashes, component health and mount presence; follow the [verification runbook](runbooks/deploy-verify-failed.md). |
| `not mounted` | Confirm exact mount point in the daemon namespace with `findmnt`; an existing `/data` directory does not qualify. Sampling/watching wait and pick up arrival automatically; covered scans are refused. |
| Watcher fd mode | Normal on EL8. 8.8 uses mount marks and misses writes through another mount of the filesystem; 8.10 uses filesystem marks. Check loss/failed state separately. |
| Watched filesystem refuses unmount (`target is busy`) | Stop the daemon/foreground watcher or remove the watch and reload; see [planned unmount](runbooks/watcher-degraded.md#planned-unmount). |
| Watcher degraded | Check namespace, overflows, transport errors and excluded prefixes; follow [watcher degraded](runbooks/watcher-degraded.md). |
| `daemon unavailable` | Check sudo, selected config/socket and service health. `now`, `hot`, `alerts`, `status` require the daemon and return 4. lvm/phantom/writers have direct fallback. |
| Scan slow | Check adaptive throttling, busy leaf devices, entry counts, mode choice, errors and state-disk I/O. Use [scan impact](runbooks/scan-impact.md); do not switch a busy server to max. |
| Scan seems stuck after Ctrl-C | Direct scan work stops within a second, then durable snapshot/catalog writes can delay exit (code 3). Do not repeatedly kill it during record publication. A forwarded scan continues in the daemon after Ctrl-C; cancel explicitly by root if intended. |
| `df` and `du` disagree | Compare fresh, equal coverage and times; inspect deleted-open files, open writers, exclusions, mounts, filesystem metadata and reflinks using the [mismatch runbook](runbooks/df-du-mismatch.md). |

```sh
sudo tidemark scan /data --cancel
```

Explicit cancellation targets the current scan of that root, not an atomic scan ID. Confirm the root/activity first; a replacement scan can race the request. Failed scans log retry details and leave scheduler health degraded until that root succeeds or leaves configuration. Scheduled retries back off from 30s to 1h; manual requests bypass backoff.
<!-- gap: missing-mount rate-ring reset and reversible now degraded reasons remain open; after remount, confirm fresh samples before trusting rates or time-to-full. -->

Exit codes: **0** success; **1** runtime error; **2** usage/config error; **3** errors, degraded/partial or cancelled scan (including uncertain forwarded outcome); **4** daemon unavailable; **5** precondition failure; **6** verify unhealthy/timeout. Archive stderr as well as JSON. Paths with unusual bytes are escaped for text and use `path_b64` in JSON; do not parse human tables as filenames.

## Upgrade or remove

```sh
sudo dnf upgrade ./tidemark-0.1.1-1.el8.x86_64.rpm
sudo tidemark status
sudo journalctl -u tidemarkd --since '-5 minutes' --no-pager
```

Substitute the approved newer RPM. An upgrade restarts a running daemon, leaves a stopped one stopped and preserves enablement. Edited config is retained; compare any `.rpmnew` with your controlled config, validate changes and reload. Take [before/after baselines](pilot/README.md) around the upgrade.

```sh
sudo tidemark deploy unstage
sudo dnf remove tidemark
```

Unstage stops and disables the service, keeping state. RPM erase also stops/disables and keeps `/var/lib/tidemark`; edited config may remain as `.rpmsave`. For deliberately approved deletion of Tidemark's own retained state, `sudo tidemark deploy unstage --purge-state --yes` is available. Unstage/purge remain available when storage would be ineligible for stage. No monitored application data is removed. Keep incident evidence before purging.

## Watcher visibility and symbolic-link limits

Run `sudo tidemark status` and inspect the mode and watcher component detail. `fd_filesystem` in status (`fd/filesystem` in foreground/detail) uses a filesystem mark where supported, measured on Rocky 8.10, and sees writes through bind mounts and private mount namespaces. Foreign-mount events are mapped by file handle to the watched mount. Unmappable paths preserve totals at the watched root, but explicitly mark directory attribution incomplete with a reason; affected directory alert windows are unavailable and component health degrades for the affected fifteen-minute window, then recovers. The lifetime `unmapped_events` count is informational (13 R6-6). Hard-link names and directory attribution remain best effort. A watched subdirectory bind can receive outside activity attributed at its root.

`fd/mount` is the fallback on Rocky 8.8 or a failed filesystem mark. It sees only writes through the selected mount and explicitly warns that other mount namespaces and bind mounts are not seen. Database services using `PrivateTmp=` or `ProtectSystem=` may be invisible on 8.8. Both fd modes observe close-write, not FID create/delete/rename; capability probing and status are the way to tell. Do not add namespace settings to the Tidemark unit: host-namespace checks still protect scans, sampling and path interpretation.

An active daemon or foreground `tidemark watch` keeps a directory descriptor open on the watched mount, so ordinary `umount` fails with `target is busy`. Before a planned unmount, run `sudo systemctl stop tidemarkd` and wait for the daemon to exit (which releases the mount), or remove the mount from `watch.mounts`, validate and reload with `sudo systemctl reload tidemarkd`, and confirm watcher teardown before unmounting. Stop any foreground watcher and wait for it to exit too. `umount -l` also detaches the mount, but the old filesystem stays busy until the daemon notices, reports `not mounted` and releases it; the daemon picks the configured mount up again when it returns. See [unmounting a watched filesystem](runbooks/watcher-degraded.md#planned-unmount).

`sudo tidemark symlinks /data --check-targets` resolves components only within the selected mount, including same-device bind boundaries. `other_fs` means the external target was not checked: it does not tell you whether it exists, dangles or loops. Reading link text may update that link’s atime under mount policy. No scan and no other command reads the text of symbolic links it finds; resolving an administrator-supplied path on the command line follows links in that path as ordinary tools do. The 40-expansion bound also treats overlong chains as loops. Tidemark never restores timestamps or changes mount options to hide this limit.

`man tidemark` and `man tidemarkd` provide the packaged command references. Final guest acceptance for real second-user trust, filesystem marks under packaged restrictions and cross-mount link atime remains pending; implemented checks and earlier capability measurements do not replace those gates.
