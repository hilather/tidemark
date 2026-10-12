# 11 — Production rollout runbook

Tidemark is read-only and makes no fstab, LVM, kernel or file-metadata changes. A rollout is therefore a package, a config file and a service. It still goes in phases, because the scheduled scans put real (throttled) load on the RAID 5 array, and their windows and throttle need tuning against real data.

| Phase | What | Changes on prod | Typical duration |
|-------|------|-----------------|------------------|
| P0 | Preconditions and baseline | none | 1–2 days of observation |
| P1 | Staging validation (Wave 7 gates) | none | done in W7 |
| P2 | Preparation: config and monitoring | config and monitoring setup | 1 hour |
| P3 | Pilot host: hotfix deploy → reboot → verify | RPM, config, enabled service | 1 hotfix cycle |
| P4 | Pilot observation and tuning | config tweaks | 1 week |
| P5 | Fleet rollout via the hotfix pipeline | same as P3, per host | per hotfix schedule |
| P6 | Steady state and review | — | 30-day review |

---

Gate-review contracts: stage requires allowlisted local state storage (or its longest existing ancestor); refusal exits 5 without changes. Unstage/purge remain available on an ineligible filesystem. Blocking pre-flight failure is change-free; later failure has best-effort compensation and may leave downtime, timestamps or empty directories. See 08 §5 and 13 R3-3/R3-12.

Run live commands with sudo: API peers must be uid 0. `status --show-config` labels CLI configuration; `lvm` omits segment type/other LVs in daemon mode, while absent daemon causes lvm/writers/phantom direct fallback with one notice. now/hot/alerts/status exit 4. Watcher alerts require complete supported 1–60 whole-minute observations and otherwise report not evaluable; top-list truncation or overflow makes hot data unavailable. Review the open risks in 12, and require real VM/listener/credential/reboot tests before rollout; no supplied privileged runtime test has run.

## P0 — Preconditions and baseline

1. **OS currency:** `cat /etc/rocky-release; uname -r`. Rocky supports only the latest minor; 8.10 (kernel `4.18.0-553.x`) is the final 8.x release. If the hosts are on 8.8, it's worth bundling the 8.10 update into the same hotfix cycle. Tidemark works on both.
2. **Inventory, per host type:**
   - `findmnt -t xfs -o TARGET,SOURCE,OPTIONS` (expect `/` and `/data`)
   - `lvs -o vg_name,lv_name,segtype,lv_size,data_percent,pool_lv` and `vgs`
   - `xfs_info / ; xfs_info /data`
   - `df -h / /data ; df -i / /data`
   - `cat /proc/mdstat` (or the hardware RAID tool)
   - `lsblk -o NAME,TYPE,ROTA,SCHED,SIZE`
3. **Baseline load** across a full business day and a batch window: `iostat -x 5` (await and %util on the array's disks), Postgres latency (existing monitoring or `pg_stat_statements`), and application SLOs.
4. Answer the open questions in `01-requirements.md` §2: Q1 (thin LVs), Q5 (quiet windows), Q6 (pipeline capabilities).

**Exit:** baseline recorded in `docs/pilot/baseline-<host>.md`.

## P2 — Preparation

1. **Config:** start from the packaged default (roots `/` and `/data`, watch and sampler on both, LVM on). Set:
   - `[[roots]]` windows from Q5;
   - `throttle = "gentle"` for the pilot;
   - alert thresholds;
   - the metrics textfile path.

   Keep the file in the hotfix repository. Validate it anywhere the RPM is installed: `tidemarkd --check-config --config tidemark.toml`.
2. **Monitoring:** load `packaging/prometheus/tidemark-alerts.yml` and the Grafana dashboard. Route Tidemark alerts to a **non-paging** channel for now.

## P3 — Pilot host: hotfix deploy, reboot, verify

Add these steps to the pilot host's hotfix:

```bash
dnf install -y ./tidemark-<ver>-1.el8.x86_64.rpm
tidemark deploy stage --from ./tidemark.toml        # pre-flight failure changes nothing; later failure requires inspection
# … hotfix reboot …
tidemark deploy verify --wait 5m                      # non-zero → see output; Tidemark is not healthy
```

On stage failure, read the initiating and compensation errors. A blocking pre-flight failure changes nothing; a failure after pre-flight attempts best-effort compensation and may leave changes or observed service downtime. Inspect state, then retry stage or unstage; staging is not a transaction.

After verify:

- `tidemark status` shows each component with its mode. Expected watcher mode on EL8: `fd/mount`.
- `tidemark now` shows both filesystems, LVM.
- `tidemark hot --window 5m` during normal activity shows writing services.

Choose walk when current apparent sizes matter: bulk (including auto-selected bulk) can show smaller regular-file apparent sizes until XFS writes data back; Tidemark never forces writeback.

**First night:** both roots scan in their windows (`/data` in `bulk` if eligible, `/` in walk). Next morning:

- Check `tidemark snapshots list`, scan durations (`tidemark status`), and the `iostat`/Postgres graphs for the scan window against the P0 baseline.
- Cross-check one large subtree: `nice -n19 du -sxB1 /data/reports/<big>` vs `tidemark top /data/reports/<big> --json` (`t_disk`). They should agree within 0.5%, allowing for live changes.

**Abort criteria** (any of these during a scan):

- leaf block device utilisation or average wait remains above the active profile's configured targets after the controller backs off;
- array await > 2 × baseline for more than 2 minutes;
- application alerts.

Action: `tidemark scan --cancel` (or let the adaptive throttle settle if it's already backing off), then tighten the profile, lower `scan.threads` or move the window, and reload (`systemctl reload tidemarkd`).

**Rollback:** `tidemark deploy unstage` (service stopped and disabled; data kept) or `dnf remove tidemark`.

## P4 — Pilot observation and tuning (1 week)

Daily:

- `tidemark now` and `tidemark diff --since 24h --explain`. Are the answers sensible?
- Daemon RSS and CPU (`systemctl status tidemarkd`): the targets are ≤ 300 MiB and ≤ 2% of a core outside scans.
- `tidemark_watch_overflows_total` should stay near 0.
- Snapshot store size (`du -sh /var/lib/tidemark`) against root filesystem headroom; adjust `retention` if needed.
- Alert noise: tune thresholds, especially `time_to_full` and `thin_pool_pct`.

At the end of the week, decide on `throttle` (`gentle` or `normal`), `threads`, windows and schedules, and update the config in the hotfix repository.

## P5 — Fleet rollout

Use the same steps as P3 in each host's next hotfix cycle, with the tuned config. For the pipeline:

- `deploy stage` is safe to run on every host. It changes nothing unless all blocking checks pass.
- `deploy verify` is the pass/fail gate after the reboot. Its `--json` output can be archived per host.
- Hosts with a different layout (no `/data`, not LVM) are caught by pre-flight. Either give them a matching config or let the warnings stand (for example no LVM section).

## P6 — Steady state

- Route `TidemarkTimeToFull`, `TidemarkThinPool`, `TidemarkPhantomBytes` and `TidemarkDown` to paging, and the rest to tickets. Link the runbooks from the alerts.
- 30-day review:
  - alert precision;
  - scan durations trend;
  - snapshot store size;
  - which questions people still answer with `du` (each one is a feature gap to log).
- If instant per-tree totals ever become a requirement, revisit `06-project-quotas-deferred.md`, with its audit-first approach.
