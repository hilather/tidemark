# Wave 6 — Daemon, API, metrics, packaging

**Goal:** `tidemarkd` runs everything continuously under systemd, the CLI's live commands work through the API, metrics flow to Prometheus, and the whole thing installs as an EL8 RPM.

**Agents:** 3. **Entry:** `wave-3-done`, `wave-4-done`, `wave-5-done`.

| Task | Title | Owned paths | Depends on |
|------|-------|-------------|------------|
| W6-A1 | Lifecycle, state, sampler, LVM, ts-writer | `tidemarkd/src/{lib,main,state}.rs`, `tidemarkd/src/services/{sampler,lvm,tswriter}.rs`, `tidemarkd/tests/w6a1.rs`, `tidemarkd/tests/w6a1/**` | W2–W5, W6-prep |
| W6-A2a | Scheduler, scan control, post-scan, firstboot report | `tidemarkd/src/scheduler.rs`, `tidemarkd/tests/w6a2a.rs`, `tidemarkd/tests/w6a2a/**` | W6-A1, W2-C; W6-B cache |
| W6-A2b | Watcher, process-scan, alert services | `tidemarkd/src/services/{watcher,procscan,alerts}.rs`, `tidemarkd/tests/w6a2b.rs`, `tidemarkd/tests/w6a2b/**` | W6-A1, W5-A/B/D |
| W6-A3 | API server and CLI live commands | `tidemarkd/src/api.rs`, `tidemark-cli/src/cmd/{now,hot,alerts,status}.rs`, `tidemark-cli/src/render/{now,hot,alerts,status}.rs`, daemon branches in `cmd/{scan,lvm,phantom,writers}.rs`, both crates’ `tests/w6a3.rs` and `tests/w6a3/**` | W6-A1/A2a/A2b, W4-B client |
| W6-B | Metrics exporter, dashboards, alert rules | `tidemarkd/src/metrics.rs`, `packaging/grafana/**`, `packaging/prometheus/**` | W6-A1 `state` API (skeleton first) |
| W6-C | Packaging, systemd, RPM, install tests | `packaging/{systemd,tmpfiles.d,config}/**`, `packaging/rpm/**`, `xtask/src/rpm.rs`, `tests/install/**` | W6-A1/A2a/A2b/A3 binary |

**Note on shared CLI files:** `scan.rs` (W2-C), `lvm.rs` (W4-A) and `phantom.rs`/`writers.rs` (W5-B) already exist. W6-A3 adds a "daemon first, direct fallback" branch at the top of each `run()` and **must not change their direct-mode code**. The reviewer checks the diff stays within that branch.

W6-A1 lands the `state` API skeleton (`LiveStateView` read methods) first, so W6-B can compile against it.

**Merge order:** W6-prep → W6-A1 state/services → W6-A2a and W6-A2b → W6-A3; W6-B builds against A1 and supplies A2a’s metrics cache; W6-C consumes the composed binary.

**Ownership of the collective W6-A section:** lifecycle/startup/reload/shutdown, LiveState/View and sampler/LVM/ts-writer belong to A1; schedules, ScanControl, recorder/post-scan and firstboot composition to A2a; watcher/procscan/alerts to A2b; API and CLI live commands/renderers to A3. A2a calls existing W4-B firstboot helpers without owning firstboot.rs. Only A3’s daemon branches own the four shared CLI files; direct implementations remain intact. Test scopes follow the table; W6-B additionally owns `tidemarkd/tests/w6b.rs` and `tests/w6b/**`, W6-C `xtask/tests/w6c.rs` and `tests/w6c/**` (W6-prep/W6 implementation handoffs).

---

## W6-A — Daemon core, services, scheduler, API

**Read first:** `02-architecture.md` §5–9, `07-live-monitoring.md` (all), `08-interfaces.md` §2, §5, §6, `05-storage.md` §6–8, W4-B `firstboot` and `client` rustdoc.

### Structure

```
tidemarkd/src/
  main.rs          config load/validate, logging (journald when INVOCATION_ID is set, else stderr),
                   signals (SIGTERM/SIGINT → shutdown, SIGHUP → reload), sd_notify READY/STOPPING/STATUS
  state.rs         LiveState + LiveStateView (read API for API handlers and metrics)
  scheduler.rs     schedules, windows, one scan at a time, catch-up, post-scan hooks
  api.rs           Unix-socket JSON-lines server
  services/
    sampler.rs     statvfs for sampler.mounts every sampler.interval → rings + ts-writer
    lvm.rs         tidemark_fs::lvm::collect every lvm.interval → LiveState.lvm + ts-writer (timeout, back-off)
    watcher.rs     one tidemark_live::watch::Watcher per watch mount (/ and /data); minute drain → ts-writer
    procscan.rs    ProcScanner every procscan.interval
    tswriter.rs    the single TimeSeries writer thread; batched inserts; hourly compact()
    alerts.rs      AlertEngine every 30 s; deliverers
```

`ComponentState` is defined in `tidemark_core::api` and re-exported by `tidemarkd::firstboot`; `VgInfo` is defined there too and re-exported by `tidemark_fs::lvm`. Use these shared payloads, never duplicate them. Report serialization uses the shared path DTO adapters in `08` §1.1/§2.

### Key behaviours

The daemon addresses its retained verified state directory as `/proc/self/fd/<n>/.`, so no-follow directory opens address the directory rather than the final procfs symlink. Daemon tests bind storage as production does.

Log one WARN per actual failed scan (root, mode, source, attempt and retry time), and INFO on admitted start and successful completion with scan id. Scheduler health remains degraded for each failed root until that root completes successfully or leaves configuration; waiting, another root’s success and reload do not establish recovery. Scheduled retries are 30, 60, 120, 240, 480, 960, 1920, then 3600 seconds, capped there. Completion and accepted reload reset retry delay/attempts. Busy and cancellation are not failures and use fixed 30-second deferral. Operator requests bypass backoff. Requested roots outside configuration retain failure health until recovery.

A normalized sampler.mounts/watch.mounts path must exactly equal a mount point in the daemon namespace; identity is topmost mount id plus device major/minor. Absent entries produce no samples/persistence or watcher and report `not mounted`. Presence checks pick up arrivals (sampler at most ten seconds, watcher every second); identity changes release the old watcher before replacement. Open sampling descriptor identity is checked before statvfs. Covered scheduled and requested scan roots are refused before run_scan, following existing error/backoff handling; unrelated roots remain eligible. Firstboot is unhealthy for absent entries, but startup readiness is prompt.

Superseded by R5-8/19: current samples/rate rings reset on identity changes, current missing-mount degradation clears on recovery, and retained watcher mark identity is reconciled. The residual check-to-mark race remains a limitation under trusted mount administration.

Invalid config.reload returns ok with `reloaded: false` and the error in warnings, keeping previous config/generation. Inventory/capacity/catalog/history/schedule preflight failures are contained the same way, without stopping scheduler. Reserve current configured roots transactionally within the 128-entry inventory, evict inactive manual history deterministically if needed, retire removed inactive roots and retain admitted/running roots until completion. Stale schedule generations cannot resurrect removed roots.

One shared monotonic shutdown deadline governs recorder queue/acknowledgement waits and startup-error cleanup; poll cancellation/deadline at most every 50 ms while preserving eight-second per-operation limits. Before admission work is uncommitted; interruption after admission reports persistence uncertain. Retain/join worker handles even after reporting deadline expiry; the ten-second overall bound is unchanged and arbitrary blocked kernel/SQLite operations remain a limitation. Parse exactly the bounded no-follow captured bytes that are hashed. Startup, reload and --check-config validate core/bounds, scheduler schedules/windows and alert predicates before publication. Runtime intervals (throttle sample, sampler, LVM interval/timeout, procscan, metrics, alert renotify/for/resolve_for and every schedules) are at most 366 days inclusive; historical retention horizons are exempt. (13 R4-1/2/9/10/11.)

- **Startup order:**
  1. Config.
  2. State dir (create, check permissions).
  3. Open shared storage (`TimeSeries::open` / `Catalog::open`) using the header preflight before SQLite (`05` §4), then migrate. Only a reported corrupt open triggers `tidemark_store::catalog::recover_catalog(state_dir)`: stop writers and close all catalog/sample connections first. Preserve DB and sidecars, rebuild from valid snapshots, and log every recovery file name (`Catalog::recovery_files`, `05` §4).
  4. Catalog and time-series connections on the healthy or recovered database.
  5. Namespace check (the watcher is disabled with a loud error if it fails; the daemon still runs).
  6. Services.
  7. API socket: remove a stale socket, bind, set mode `0600`, owner `root:root`.
  8. `READY=1`.
- **`LiveState`:**
  - Per-mount rings of the last 2 h of raw fs samples (and the latest LVM report), so rates are computed from memory without DB reads.
  - `Arc` handles to the watcher(s) and procscan.
  - The alert engine's active set.
  - Scan statuses.
  - Degraded reasons.
- **`LiveStateView`** implements W5-D's `AlertSource` (`07` §6, `13` R2-10), with bounded inventories and exact per-rule Duration windows subject to the watcher top-list/window exception in 07 §3.3 and 13 R3-1; unavailable data is None, measured empty is Some(empty). Supply ScanAge effective cadence (at least 24h for windowed roots), build optional NowInputs sections and unevaluable diagnostics, and expose diagnostics in alerts/now. Metrics derive firing gauges from active(), not a removed render_metrics method.
- `run_scan` owns the lock and `ScanGate`: pass gate() to I/O, observe stats(), cancel() wakes waiters within 250 ms, and stop_sampling() stops the sampler while retaining the handle. stop() is unchanged (04 §6.2/§8). Publication uses finish_with and cancellation publishes nothing.
- Watcher services follow the exact 1–60 whole-minute window and boundary-overflow contract in 07 §3.3; missing boundary facts leave rules not evaluable (W6-A2b). Services drain completed minutes only; enable_events/drain_events belong solely to foreground watch. Report WatchMode::Failed and transient recovery. State setup and firstboot obey the unsafe-state refusal/no-follow handle policy in `08` §5.
- **Scheduler:**
  - Parse `every <dur>` / `daily HH:MM` / `off` and windows `HH:MM-HH:MM` in local time (windows may wrap midnight).
  - At most **one scan at a time across all roots** (protects the array). `run_scan` alone holds the host-wide `state_dir/scan.lock` and creates the gate; pre-locking by a caller returns Busy. The scheduler passes a `ScanRecorder` whose callbacks wait for persistence acknowledgements, not queue admission (13 R3-8; series acknowledgement gap remains open in 12).
  - A due scan outside its window waits for the window to open.
  - After a daemon restart, a root whose last complete scan is older than its interval is due immediately (window permitting).
  - The space guard runs before each scan.
  - **Post-scan:**
    1. Inside `scan_finished`, compute `dir_growth` between the last two snapshots at depth ≤ `series_depth` (via `tidemark_store::diff`) before retention can remove the predecessor. Publish potentially truncated growth as unavailable. Successful metrics refresh/publication precedes that root’s dirty take.
    2. `dir_series` follows `scan_finished`; persistence callbacks preserve predecessor availability.
    3. `run_scan` then applies retention. Dirty loss coverage is acknowledged independently per root, only after its successful publication (13 R3-2/R3-9).
- **API:**
  - One thread per connection; max 32 concurrent connections.
  - 1 MiB request line limit; 10 s handler timeout.
  - `SO_PEERCRED` logged; every method accepts only peer uid 0, even for a nonroot daemon; other peers are closed before parsing, with zero response bytes (R5-20).
  - Methods and errors exactly as `08` §2.
- **Self-protection:** `state_dir` is always added to watch and procscan excludes and to scan excludes. The space guard emits the `state_space` alert.
- **Reload (SIGHUP / `config.reload`):** re-read config and validate. Apply changes to schedules, alert rules, thresholds, exclude lists and metrics settings live. Changes to mounts or the watch set restart only the affected service. Invalid config keeps the old one and reports the issues.
- **Shutdown:** stop the API, cancel the running scan (no partial snapshot kept), stop the watchers, flush the ts-writer, then exit within 10 s.
- **First-boot report:** after startup, if `tidemarkd::firstboot::needs_report(state_dir, config_sha)` is true, the daemon builds `FirstbootInputs` once all components have reported (or after 2 minutes), then calls `build` + `write` (W4-B). Open limitation: on each start with changed configuration, the report is rewritten (13 R4-16); do not claim once-per-staged-config behavior.
- **Strictly read-only** (ADR-008): the daemon has no code path that changes file metadata, mounts or LVM.

### CLI live commands (using W4-B's `client.rs`)

- Use `tidemark_cli::client::Client` (W4-B) for every call. For now/hot/alerts/status, a connection failure maps to exit code 4 with "daemon unavailable — is tidemarkd running? (systemctl status tidemarkd)".
- `now`: renders `NowReport` via `render_now_text` (terminal width from `TIOCGWINSZ`, default 100). `--json` gives the raw report.
- `hot`, `alerts`, `status`: tables. `status --show-config` labels the CLI’s effective config; there is no config.get API method. Alert history currently covers only this daemon run. `status` shows versions, uptime, each component's state and mode, last error per component, scan schedule and next run.
- **Daemon branches:**
  - `lvm`, `writers`: try the daemon first, otherwise fall back to direct mode with a one-line notice.
  - `scan`: if the daemon is up, `scan.start` + follow `scan.status` (and `--cancel` → `scan.cancel`); threads, extra excludes and keep-partial are refused while connected. Only absent/refused socket permits W2-C direct fallback. Follow only the started id; completed degraded/partial/unknown outcomes exit 3, and no automatic root-only cancellation of a replacement scan is allowed (13 R4-12).
  - `phantom`: same, but `--direct` forces direct mode.

Contract details: daemon LVM omits segment type and other LVs; CLI trends remain store-derived. Procscan publishes writers_full/writer_evictions with no permission-error counts. Config parsing/hashing uses Config::from_toml_bytes on the actual loaded bytes. Rustix net/termios enable SO_PEERCRED and terminal width. See 13 R3-4/R3-5/R3-6/R3-14 and 12 for unresolved implementation risks.

### Tests

- Unit: schedule/window parser and next-run computation (DST transitions, windows wrapping midnight); API framing (partial lines, oversize line, malformed JSON → `bad_request`); peer-uid check (only uid 0 accepted via kernel SO_PEERCRED; others closed before parsing with zero response bytes) for every method.
- Integration (unprivileged): start the daemon with a temp config (watcher/LVM disabled, walk-mode root on a fixture, socket in a temp dir) → `tidemark status`, `scan.start` via the CLI, `scan.status` until complete, then `now` renders.
- Privileged `daemon_end_to_end` (`09` §5): start via `systemd-run` with the real unit's properties → write 50 MiB under a transient unit → within 60 s, `tidemark now` shows the hot directory and the unit; `tidemark now` shows both tracked mounts and, on a loop-LVM thin volume, the pool's data %.
- Privileged `daemon_reload_and_shutdown`: SIGHUP with a changed alert rule is applied; an invalid config is rejected; SIGTERM mid-scan exits within 10 s with no `.db.tmp` left.

### Acceptance

- [ ] End-to-end tests pass on both VMs.
- [ ] RSS after 1 h idle on the VM ≤ 150 MiB (recorded); after a scan it returns to within 20% of that.

---

## W6-B — Metrics exporter, dashboards, alert rules

Independent global directory rankings retain at most the configured series cap per family and count omitted eligible rows exactly, with deterministic raw root/path ties, rather than refusing large inventories before selection. Startup and root/depth/capacity reload reconcile a read-only latest-complete snapshot cache. Crash-left owned single-link regular staging files are flocked, truncated and reused through a collector-directory handle; hostile or live staging files are refused. Nonrepresentable metrics interval reports component error until a new generation.

Superseded by R5-1: ADR-020 removed HTTP metrics and webhook delivery; only textfile metrics and journal delivery remain.

The former endpoint deliverable was removed by ADR-020 (R5-1).

**Read first:** `08-interfaces.md` §4, Prometheus text exposition format documentation.

### Deliverables

- `metrics.rs`:
  - Render all metrics in `08` §4 from `LiveStateView` plus the latest snapshots (dir metrics at depth ≤ `dir_metrics_depth`, cached and refreshed after each scan).
  - `# HELP` / `# TYPE` lines; label escaping (`\\`, `\"`, `\n`); stable ordering.
  - Cardinality guard per metric, plus `tidemark_metrics_dropped_series`.
- **Textfile writer:** write to `<textfile>.tmp` in the same directory, `fsync`, `chmod 0644` (node_exporter typically runs as its own user), then `rename`. If the directory doesn't exist or isn't writable, skip with one log line per hour, and report `metrics` as `degraded` in status. The daemon's capability set has no `CAP_DAC_OVERRIDE`, so the directory must be writable by root through normal permissions; the usual root-owned `textfile_collector` directory is. Tidemark does **not** create it.
- **Textfile only:** no HTTP endpoint or network listener (ADR-020, R5-1).
- `packaging/grafana/tidemark.json`: one dashboard with:
  - Filesystem used/avail with time-to-full;
  - LVM: thin-pool data/metadata % and VG free;
  - top-level directory sizes over time (from `tidemark_dir_used_bytes`);
  - top directories (table from `tidemark_dir_used_bytes`);
  - phantom bytes;
  - scan freshness;
  - watcher health.
- `packaging/prometheus/tidemark-alerts.yml`: `TidemarkTimeToFull`, `TidemarkThinPool`, `TidemarkPhantomBytes`, `TidemarkScanStale`, `TidemarkWatcherLossy`, `TidemarkDown` (absent `tidemark_up`), and `TidemarkAlertFiring` (pass-through of `tidemark_alert_firing`).

### Tests

- Exposition-format validation: parse the output with a small parser (or `promtool check metrics` if available in the VM; skip otherwise).
- Cardinality guard with 10,000 fake directory paths.
- Label escaping with hostile path names.
- An atomic-write test: a concurrent reader never sees a partial file.

### Acceptance

- [ ] `promtool check rules packaging/prometheus/tidemark-alerts.yml` passes (run where promtool is available; record it).
- [ ] The dashboard JSON imports into Grafana (manual check recorded, or a schema-lint).

---

## W6-C — Packaging, systemd, RPM, install tests

The CLI manifest contains an empty `[package.metadata.generate-rpm]` table for external metadata. `cargo xtask rpm --native` builds on EL8; the changed-default test package requires `--native --version 0.1.1 --upgrade-fixture`. Native mode installs nothing, defaults offline and rejects foreign hosts/missing generator. `tools/check-rpm.sh` checks package identity, exact assets/modes/owners/config flags, scriptlets and extracted ELF binaries. `tools/el8-build-env.sh` prepares disposable EL8 build environments, pinning 8.8 repositories to the vault. GitHub Actions has a `ci` job and separate Rocky 8.8/8.10 `rpm` matrix, locked fetching, package/install verification, checksums and artifacts. ShellCheck gates at warning severity; informational notes are printed on stderr (superseded by R5-17). Owner decision: push main to GitHub after each green local gate; this task does not push.

Unit values have no trailing comments; the existing normative unit remains unchanged. Install neither starts nor enables. Upgrade/reinstall uses guarded `try-restart`: restart a running daemon and leave a stopped daemon stopped, preserving enablement. Erase stops/disables and retains state. The install gate requires effective memory limit 2147483648, notify/root, NoNewPrivileges and exact capability names, and rejects installed-unit parser warnings. Active upgrade must change a nonzero PID and regain API readiness; stopped reinstall stays inactive. Edited config survives, and the changed-default fixture must produce root:root 0600 `.rpmnew` with its marker. Enable and prove active/enabled before erase, then assert inactive/disabled and no wants entry even as a dangling symlink; retained `.rpmsave` matches edited bytes and mode 0600. Require SELinux Enforcing unless an explicit permissive override labels SELinux acceptance untested. Post-reboot verification requires an external writer’s hot directory on restored disposable XFS; any post-mount restart is explicit. (13 R4-4/5.)

**Read first:** `08-interfaces.md` §5–6, `02-architecture.md` §7–8 and §10, ADR-013 in `12-decisions-and-risks.md`.

### Deliverables

- `packaging/systemd/tidemarkd.service`: **exactly** `08` §6. Do not add mount-namespace options.
- `packaging/tmpfiles.d/tidemark.conf`: `d /run/tidemark 0700 root root -`.
- `packaging/config/tidemark.toml`: the `08` §3 example with explanatory comments. HTTP metrics and webhooks are absent in v1 (ADR-020). Roots, sampler and watch mounts default to `/` and `/data` (matching the fleet layout); hosts without `/data` are caught by `deploy stage` pre-flight.
- `xtask rpm [--container | --native]` (`--native --version 0.1.1 --upgrade-fixture` for the changed-default install fixture):
  - Build release binaries in a `rockylinux:8` container (podman, or docker if present). The toolchain is installed via rustup in the container image; cache it.
  - Then `cargo generate-rpm` with metadata in `packaging/rpm/rpm.toml`.
  - RPM layout:
    - `/usr/bin/{tidemark,tidemarkd}`
    - `/usr/lib/systemd/system/tidemarkd.service`
    - `/usr/lib/tmpfiles.d/tidemark.conf`
    - `/etc/tidemark/tidemark.toml` (`config(noreplace)`, `0600 root:root`)
    - `/usr/share/doc/tidemark/` (README plus the plan's `07`/`08`/`11`)
    - Bash completion
  - Scriptlets:
    - **%post:** `systemd-tmpfiles --create /usr/lib/tmpfiles.d/tidemark.conf`, `systemctl daemon-reload`. **Do not enable or start** the service; `tidemark deploy stage` does that.
    - **%preun on erase:** stop and disable.
    - **%postun:** `daemon-reload`; on upgrade argument >=1, guarded `try-restart tidemarkd.service`, preserving stopped state and enablement.
- `tests/install/`: a VM script run by `xtask vm-test --filter install`:
  1. Install the RPM.
  2. `tidemarkd --check-config`.
  3. Write a minimal config (watch the loop-XFS mount).
  4. `systemctl start tidemarkd`.
  5. `tidemark status`.
  6. **Namespace sanity:** write a file as another unit and assert `tidemark hot` sees it.
  7. `ausearch -m AVC -ts recent` returns nothing for `tidemarkd`.
  8. Upgrade to a rebuilt `0.1.1` RPM preserves an edited config, restarts a running daemon with a new main PID and API readiness within 60 seconds, and leaves a stopped daemon stopped on reinstall.
  9. Erase stops the service and leaves `/var/lib/tidemark`.
  10. **`deploy_stage_reboot_verify`** (written by W4-B): `deploy stage` → reboot the VM → `deploy verify --wait 5m` exits 0 with a healthy `firstboot.json`.

### Acceptance

- [ ] `xtask rpm --container` produces an installable RPM; the install test script passes on Rocky 8.8 and 8.10.
- [ ] `rpm -qlp` output matches the layout above (test).
- [ ] `systemd-analyze security tidemarkd` output is attached to the handoff note for the record. The score will be modest by design, since mount-namespace hardening is excluded; see ADR-013.

---

## Wave 6 gate checklist

- [ ] RPM installs cleanly on both VMs; `daemon_end_to_end` and the install tests are green.
- [ ] A 24 h mini-soak on a VM, with the generator mutating a fixture and hourly scans: RSS flat, no fd growth, metrics continuous, no unexpected log errors.
- [ ] Gate review: every API method and metric in `08` is implemented with the documented names, and nothing extra is undocumented.
- [ ] Tag `wave-6-done`. This is the first deployable build.

## Post-review implemented contracts (13 R5)

R5-2/3/6/8/9/19 supersede historical open lifecycle/window/mount findings: supervised writer, immutable process reports, failed-collector availability, monotonic operational timing, durable cancellation outcomes, crash cleanup under scan lock, firstboot follow-up and exact completed windows. Request deadlines reach scan admission; unauthorized peers close before parse; per-frame/handler/write budgets are separate. API/exporter use socket.lock/textfile.tmp locks and output collisions are rejected. NoSpace is Unavailable scan: NoSpace and CLI 5.

R5-10/21/24 bound descriptor reads/resource retry and process bytes, verify state/metrics ancestry and leaves, retain stricter private daemon/deploy modes with checked marker read/execute normalization only, and refuse ownership repair. Unit families are AF_UNIX. R5-11/24 specify wrapping/error context and distinct daemon LVM schema. Final guest acceptance is separate from implementation closure.
