# Wave 6 — Daemon, API, metrics, packaging

**Goal:** `tidemarkd` runs everything continuously under systemd, the CLI's live commands work through the API, metrics flow to Prometheus, and the whole thing installs as an EL8 RPM.

**Agents:** 3. **Entry:** `wave-3-done`, `wave-4-done`, `wave-5-done`.

| Task | Title | Owned paths | Depends on |
|------|-------|-------------|------------|
| W6-A | Daemon core, services, scheduler, API; CLI live commands | `tidemarkd/src/{main,api,state,scheduler}.rs`, `tidemarkd/src/services/**`, `tidemark-cli/src/cmd/{now,hot,alerts,status}.rs`, and the daemon-mode branches in `cmd/{scan,lvm,phantom,writers,pg}.rs` (see note) | W2–W5 |
| W6-B | Metrics exporter, dashboards, alert rules | `tidemarkd/src/metrics.rs`, `packaging/grafana/**`, `packaging/prometheus/**` | W6-A `state` API (skeleton first) |
| W6-C | Packaging, systemd, RPM, install tests | `packaging/{systemd,tmpfiles.d,config}/**`, `packaging/rpm/**`, `xtask/src/rpm.rs`, `tests/install/**` | W6-A binary |

**Note on shared CLI files:** `scan.rs` (W2-C), `lvm.rs` (W4-A), `phantom.rs`/`writers.rs` (W5-B) and `pg.rs` (W5-C) already exist. W6-A adds a "daemon first, direct fallback" branch at the top of each `run()` and **must not change their direct-mode code**. The reviewer checks the diff stays within that branch.

W6-A lands the `state` API skeleton (`LiveStateView` read methods) first, so W6-B can compile against it.

**Merge order:** W6-A → W6-B → W6-C.

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
    pg.rs          PgCollector fast/relations cadence; labels refreshed daily
    tswriter.rs    the single TimeSeries writer thread; batched inserts; hourly compact()
    alerts.rs      AlertEngine every 30 s; deliverers
```

### Key behaviours

- **Startup order:**
  1. Config.
  2. State dir (create, check permissions).
  3. `TimeSeries::open` + migrate.
  4. Catalog.
  5. Namespace check (the watcher is disabled with a loud error if it fails; the daemon still runs).
  6. Services.
  7. API socket: remove a stale socket, bind, set mode `0600`, owner `root:root`.
  8. `READY=1`.
- **`LiveState`:**
  - Per-mount rings of the last 2 h of raw fs samples (and the latest LVM report), so rates are computed from memory without DB reads.
  - `Arc` handles to the watcher(s), procscan and pg collector.
  - The alert engine's active set.
  - Scan statuses.
  - Degraded reasons.
- **`LiveStateView`** builds W5-D's `NowInputs` and `AlertInputs` from these, and answers the API methods.
- **Scheduler:**
  - Parse `every <dur>` / `daily HH:MM` / `off` and windows `HH:MM-HH:MM` in local time (windows may wrap midnight).
  - At most **one scan at a time across all roots** (protects the array). The scheduler holds the host-wide `state_dir/scan.lock` (W2-C) for each scan, so CLI scans are excluded too. It passes a `ScanRecorder` that forwards to the ts-writer thread.
  - A due scan outside its window waits for the window to open.
  - After a daemon restart, a root whose last complete scan is older than its interval is due immediately (window permitting).
  - The space guard runs before each scan.
  - **Post-scan:**
    1. Compute `dir_growth` between the last two snapshots at depth ≤ `series_depth` (via `tidemark_store::diff`), feeding the `dir_growth` rule.
    2. Clear the watcher dirty set for that root.
    3. Refresh labels from pg into the next scan's annotations.
    4. Apply retention.
- **API:**
  - One thread per connection; max 32 concurrent connections.
  - 1 MiB request line limit; 10 s handler timeout.
  - `SO_PEERCRED` logged; every method accepts root or the daemon's own effective uid; other peers receive `forbidden`.
  - Methods and errors exactly as `08` §2.
- **Self-protection:** `state_dir` is always added to watch and procscan excludes and to scan excludes. The space guard emits the `state_space` alert.
- **Reload (SIGHUP / `config.reload`):** re-read config and validate. Apply changes to schedules, alert rules, thresholds, exclude lists and metrics settings live. Changes to mounts, the watch set or the Postgres DSN restart only the affected service. Invalid config keeps the old one and reports the issues.
- **Shutdown:** stop the API, cancel the running scan (no partial snapshot kept), stop the watchers, flush the ts-writer, then exit within 10 s.
- **First-boot report:** after startup, if `tidemarkd::firstboot::needs_report(state_dir, config_sha)` is true, the daemon builds `FirstbootInputs` once all components have reported (or after 2 minutes), then calls `build` + `write` (W4-B). Never more than once per staged config.
- **Strictly read-only** (ADR-008): the daemon has no code path that changes file metadata, mounts or LVM.

### CLI live commands (using W4-B's `client.rs`)

- Use `tidemark_cli::client::Client` (W4-B) for every call. A connection failure maps to exit code 4 with "daemon unavailable — is tidemarkd running? (systemctl status tidemarkd)".
- `now`: renders `NowReport` via `render_now_text` (terminal width from `TIOCGWINSZ`, default 100). `--json` gives the raw report.
- `hot`, `alerts`, `status`: tables. `status` shows versions, uptime, each component's state and mode, last error per component, scan schedule and next run.
- **Daemon branches:**
  - `lvm`, `pg`, `writers`: try the daemon first, otherwise fall back to direct mode with a one-line notice.
  - `scan`: if the daemon is up, `scan.start` + follow `scan.status` (and `--cancel` → `scan.cancel`); otherwise W2-C's direct mode.
  - `phantom`: same, but `--direct` forces direct mode.

### Tests

- Unit: schedule/window parser and next-run computation (DST transitions, windows wrapping midnight); API framing (partial lines, oversize line, malformed JSON → `bad_request`); peer-uid check (root or the daemon's own uid accepted; others `forbidden`) for every method.
- Integration (unprivileged): start the daemon with a temp config (watcher/LVM/pg disabled, walk-mode root on a fixture, socket in a temp dir) → `tidemark status`, `scan.start` via the CLI, `scan.status` until complete, then `now` renders.
- Privileged `daemon_end_to_end` (`09` §5): start via `systemd-run` with the real unit's properties → write 50 MiB under a transient unit → within 60 s, `tidemark now` shows the hot directory and the unit; `tidemark now` shows both tracked mounts and, on a loop-LVM thin volume, the pool's data %.
- Privileged `daemon_reload_and_shutdown`: SIGHUP with a changed alert rule is applied; an invalid config is rejected; SIGTERM mid-scan exits within 10 s with no `.db.tmp` left.

### Acceptance

- [ ] End-to-end tests pass on both VMs.
- [ ] RSS after 1 h idle on the VM ≤ 150 MiB (recorded); after a scan it returns to within 20% of that.

---

## W6-B — Metrics exporter, dashboards, alert rules

**Read first:** `08-interfaces.md` §4, Prometheus text exposition format documentation.

### Deliverables

- `metrics.rs`:
  - Render all metrics in `08` §4 from `LiveStateView` plus the latest snapshots (dir metrics at depth ≤ `dir_metrics_depth`, cached and refreshed after each scan).
  - `# HELP` / `# TYPE` lines; label escaping (`\\`, `\"`, `\n`); stable ordering.
  - Cardinality guard per metric, plus `tidemark_metrics_dropped_series`.
- **Textfile writer:** write to `<textfile>.tmp` in the same directory, `fsync`, `chmod 0644` (node_exporter typically runs as its own user), then `rename`. If the directory doesn't exist or isn't writable, skip with one log line per hour, and report `metrics` as `degraded` in status. The daemon's capability set has no `CAP_DAC_OVERRIDE`, so the directory must be writable by root through normal permissions; the usual root-owned `textfile_collector` directory is. Tidemark does **not** create it.
- **Optional HTTP:** `tiny_http` on `metrics.listen`. Config validation **rejects non-loopback addresses**. It serves only `GET /metrics`, with 404 for everything else.
- `packaging/grafana/tidemark.json`: one dashboard with:
  - Filesystem used/avail with time-to-full;
  - LVM: thin-pool data/metadata % and VG free;
  - top-level directory sizes over time (from `tidemark_dir_used_bytes`);
  - top directories (table from `tidemark_dir_used_bytes`);
  - phantom bytes;
  - Postgres WAL/slots;
  - scan freshness;
  - watcher health.
- `packaging/prometheus/tidemark-alerts.yml`: `TidemarkTimeToFull`, `TidemarkThinPool`, `TidemarkPhantomBytes`, `TidemarkPgSlotRetention`, `TidemarkScanStale`, `TidemarkWatcherLossy`, `TidemarkDown` (absent `tidemark_up`), and `TidemarkAlertFiring` (pass-through of `tidemark_alert_firing`).

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

**Read first:** `08-interfaces.md` §5–6, `02-architecture.md` §7–8 and §10, ADR-013 in `12-decisions-and-risks.md`.

### Deliverables

- `packaging/systemd/tidemarkd.service`: **exactly** `08` §6. Do not add mount-namespace options.
- `packaging/tmpfiles.d/tidemark.conf`: `d /run/tidemark 0700 root root -`.
- `packaging/config/tidemark.toml`: the `08` §3 example with explanatory comments. Postgres, webhook and HTTP metrics are disabled by default. Roots, sampler and watch mounts default to `/` and `/data` (matching the fleet layout); hosts without `/data` are caught by `deploy stage` pre-flight.
- `xtask rpm [--container]`:
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
    - **%postun:** `daemon-reload`.
- `tests/install/`: a VM script run by `xtask vm-test --filter install`:
  1. Install the RPM.
  2. `tidemarkd --check-config`.
  3. Write a minimal config (watch the loop-XFS mount).
  4. `systemctl start tidemarkd`.
  5. `tidemark status`.
  6. **Namespace sanity:** write a file as another unit and assert `tidemark hot` sees it.
  7. `ausearch -m AVC -ts recent` returns nothing for `tidemarkd`.
  8. Upgrade to a rebuilt `0.1.1` RPM preserves an edited config.
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
