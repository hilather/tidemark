# Pilot checklist — fill in one copy per host type

Host/type: ______  Operator: ______  Change/ticket: ______
Package version/checksum/source: ______  Candidate config path/hash: ______
Dates/time zone: ______  Application/storage owners: ______
Baseline evidence root: ______  Monitoring dashboard: ______
Latency metric/units/percentile: ______  Source/query: ______
Latency SLO/abort threshold and workload interval: ______
Sample subtrees (same before/after): ______
Rollback owner/window: ______

Use `sudo` for all commands in the [operator guide](../operator-guide.md). Record evidence paths beside each checkbox; an unchecked gate needs an explicit no-go or accepted exception. Do not mark a test passed merely because it compiled or skipped.

## P0 — Preconditions and baseline (1–2 days)

Before capture: ______  After capture: ______  Date/reviewer: ______

- [ ] Record Rocky release/kernel and probe capabilities: ______
- [ ] Exact XFS mount paths/devices/options; geometry/reflink: ______
- [ ] LVM inventory: linear/thin LVs, shared pools, data/metadata %, VG free: ______
- [ ] Filesystem free bytes/inodes for root/data/state: ______
- [ ] RAID type/health, leaf devices, scheduler/rotational facts: ______
- [ ] Full business-day and batch-window iostat/workload latency/load graphs: ______
- [ ] Q1 thin layout/headroom answered: ______
- [ ] Q5 quiet scan windows answered: ______
- [ ] Q6 hotfix staging/reboot/post-reboot verification capability answered: ______
- [ ] Named latency figure and sample-subtree du captured: ______

Exit decision/baseline artifact: ______

## P1 — Staging validation (separate Wave 7 gates)

Before capture: ______  After capture: ______  Date/reviewer: ______

- [ ] Gate evidence for Rocky 8.8/8.10 XFS, fanotify, packaging/install/upgrade/erase and actual reboot: ______
- [ ] Bulk/walk allocation and link oracles; flushed fixture apparent-size comparison: ______
- [ ] Performance/memory/write-throughput impact and 72h soak evidence: ______
- [ ] Fault/security/fuzz gates and remaining open risks reviewed: ______
- [ ] Colleague who did not build it follows guide from RPM install to yesterday-growth answer without help: name/date/result ______
- [ ] Staging rehearsal of P0–P5 recorded; skipped/compile-only tests excluded from acceptance: ______

Exceptions/owner decision and evidence: ______

## P2 — Config and monitoring preparation

Before capture: ______  After capture: ______  Date/reviewer: ______

- [ ] Config under change control; exact roots/sampler/watch mounts match host: ______
- [ ] Local eligible state storage, root-only ownership/modes and capacity: ______
- [ ] Pilot throttle gentle; schedules/windows from Q5; scan budget below 2 GiB service limit: ______
- [ ] Excludes and link-topology coverage reviewed with data owner: ______
- [ ] Thresholds and full effective rule set reviewed; unavailable collectors understood: ______
- [ ] Textfile directory and node_exporter collector configured; scrape freshness verified: ______
- [ ] Matching Grafana dashboard/Prometheus rules loaded; non-paging routing: ______
- [ ] Candidate passes `tidemarkd --check-config`: evidence ______

<!-- verify after W6-watch-windows -->
- [ ] Whole-minute 1–60 watcher alert windows verified after implementation, or explicitly unavailable and recorded: ______

Exit decision: ______

## P3 — Pilot hotfix, reboot, verify

Before capture: ______  After capture: ______  Date/reviewer: ______

- [ ] Install approved RPM; confirm service disabled/stopped on fresh install: ______
- [ ] `deploy stage --from` result/exit; initiating and compensation errors if any: ______
- [ ] Config installed and service enabled for next boot: ______
- [ ] Maintenance reboot start/end and returned host identity: ______
- [ ] `deploy verify --wait 5m` exit 0 and archived JSON/staged/firstboot hashes: ______
- [ ] Status healthy, correct watcher mode, now filesystem/LVM view: ______
- [ ] Normal activity visible in hot; unknown short-lived writers understood: ______
- [ ] First-night root/data scans start within selected windows; IDs/modes/durations: ______
- [ ] Morning iostat/latency versus P0; small subtree du versus snapshot SIZE within 0.5% after accounting for live changes/scope: ______
- [ ] Bulk apparent-size lag understood; walk used where current length matters: ______

Abort criteria: sustained leaf utilisation/average wait above active profile targets after backoff; await >2× baseline for >2min; application alerts. Abort time/cancelled root/scan ID: ______
Rollback if needed: `deploy unstage` stops/disables and keeps evidence/state. Result/owner: ______
Exit decision: ______

## P4 — One-week observation and tuning

Before capture: ______  After capture: ______  Date/reviewer: ______

Fill daily rows; attach now/diff/journal and graphs rather than guessing missing measurements.

| Day/date | now/diff sensible + snapshot IDs | idle RSS/CPU; active events/CPU | overflow/loss | scan impact + latency | state size/headroom | alert noise/change |
|---|---|---|---|---|---|---|
| 1 | | | | | | |
| 2 | | | | | | |
| 3 | | | | | | |
| 4 | | | | | | |
| 5 | | | | | | |
| 6 | | | | | | |
| 7 | | | | | | |

- [ ] Idle RSS ≤300 MiB and CPU ≤2% of one core outside scans: evidence ______
- [ ] Active watcher CPU compared separately with 5% of one core per 1,000 close-writes/s; writer-throughput impact measured: ______
- [ ] Overflows near zero or explained; lossy periods excluded from exact accounting: ______
- [ ] State size versus cap/root headroom and retention reviewed: ______
- [ ] Alert thresholds/noise, including thin metadata/time-to-full, reviewed: ______
- [ ] Tuned throttle/threads/windows/schedules committed in deployment repository; reload acceptance checked in journal: ______

Week-end decision/sign-off reference: ______

## P5 — Fleet hotfix rollout

Before/after evidence per host: ______  Date/reviewer: ______

- [ ] Host layouts grouped and given matching configs; no nonexistent /data assumptions: ______
- [ ] P3 install/stage/reboot/verify steps run per host, exit/JSON archived: ______
- [ ] Failed preflight/verify hosts held out of rollout; compensation inspected: ______
- [ ] Upgrade restarts active daemon; stopped daemon stays stopped; enablement preserved: ______
- [ ] Paging/ticket route changes approved only after pilot evidence: ______

Hosts completed/held/rolled back with reasons: ______

## P6 — Steady state and 30-day review

Before capture: ______  After capture: ______  Date/reviewer: ______

- [ ] Approved paging for time-to-full, thin pool, phantom and down/freshness; other alerts ticketed: ______
- [ ] Alert precision, scan duration trend, state size reviewed: ______
- [ ] Questions still needing du/other tools recorded as feature gaps: ______
- [ ] Capacity/retention/quiet-window changes assigned: ______

Decision, open risks and next review date: ______

Complete [sign-off](sign-off.md) with the evidence above. These forms do not authorize data deletion, storage mutation or release tagging.
