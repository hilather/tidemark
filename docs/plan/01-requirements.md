# 01 — Requirements

## 1. Problem statement

The servers run Rocky Linux 8 and a multi-language product suite. The workload is part filesystem, part PostgreSQL.

- **Storage stack:** XFS on LVM on RAID 5, with large (4 TB+), largely consumed disks.
- **Partitions:** a root filesystem (`/`) and a data filesystem (`/data`). PostgreSQL's data directory lives on `/data`.
- **Data shape:** report directories are large and often hold tens of thousands of small files each. Hard links and symlinks are used everywhere.

When a disk fills, operators need to know **immediately** where the space is, what is growing, and who is writing it. They also need **history** to answer "what changed since yesterday or last week". Today that means running `du` over millions of inodes on a busy RAID 5 array. That is slow, it competes with Postgres, and its answers are misleading when hard links are involved.

## 2. Environment assumptions

`tidemark probe` (FR-1) reports most of these automatically.

| Item | Assumption | Status |
|------|------------|--------|
| OS | Rocky Linux 8.8 (kernel `4.18.0-477.x`). Rocky only supports the latest minor release; 8.10 (kernel `4.18.0-553.x`) is the final 8.x minor, so a regularly updated box should already be on 8.10. | Check `cat /etc/rocky-release; uname -r` |
| Kernel features | 4.18 base with Red Hat backports. Never infer features from the version string; probe at runtime. | Design rule |
| Filesystems | Two XFS filesystems, `/` and `/data`. Both are v5 (`crc=1`, `ftype=1`), with reflink probably enabled (RHEL 8 `mkfs.xfs` default). | Check `xfs_info / ; xfs_info /data` |
| Volume management | **LVM**: both filesystems are logical volumes, possibly in one volume group on the RAID 5 array. Thin provisioning unknown. | Check `lvs -o vg_name,lv_name,segtype,lv_size,data_percent,pool_lv; vgs` |
| RAID | RAID 5, either Linux md or a hardware controller LUN | Unknown — the design handles both |
| PostgreSQL | Data directory on `/data`, sharing the disks with scans; scanned like any other directory | Confirmed on `/data` |
| SELinux | Enforcing, Permissive or Disabled | No policy shipped; 8.10 package lifecycle/reboot measured Enforcing with no denials (R4-15) |
| cgroups | v1 (RHEL 8 default) | Assumed |
| Scale | 5–50 M inodes on `/data`, 0.5–5 M directories, widest directories 10k–100k+ entries; `/` is much smaller | Estimate (`df -i`) |
| Links | Many hard-link sets, some spanning report trees; many symlinks, some to directories | Measured in the pilot (`links` report) |
| Privileges | Installed files are root-only; daemon runs as root in production; administrators use `sudo` | Assumed |
| Deployment | Installed through the existing **hotfix pipeline**, staged at deploy time, enabled automatically on the next reboot | Requirement (FR-25) |

### Open questions for the team

| # | Question | Why it matters | Default if unanswered |
|---|----------|----------------|-----------------------|
| Q1 | Is `/data` a thin LV? Is there free space in the volume group? | Thin pools can fill before `df` shows it; VG headroom tells you how far you can grow | Monitor both if present (FR-24) |
| Q2 | Linux md RAID or hardware RAID? How many spindles? | Throttle device discovery and scan parallelism | Auto-discover through LVM → md/disks; 8 threads |
| Q3 | How are hard links created (app code, rsync `--link-dest`, `cp -al`, a dedup tool)? | Interpreting the reclaimable/shared numbers | Measured by the `links` report |
| Q4 | Is there an existing Prometheus/Alertmanager/Grafana stack? | Metrics delivery | node_exporter textfile metrics |
| Q5 | Which hours are quiet enough for the scheduled scans? | Scan windows | `/data` 00:00–06:00; `/` daily at 03:30 |
| Q6 | What does the hotfix pipeline run (shell steps, Ansible, …) and can it run a post-reboot check? | Shape of `deploy stage` / `deploy verify` | Shell commands with clear exit codes |
| Q7 | *(Deferred with FR-16.)* | — | — |

## 3. Goals

- **G1 — Live answers.** Within about a minute, show which directories are receiving writes and which processes or systemd units are writing them. Show free space and growth rate per filesystem every 30 seconds.
- **G2 — Full picture, gently.** A complete per-directory index of `/` and `/data`, built by a scanner that is several times faster than `du` and backs off when the array is busy.
- **G3 — Truthful numbers with links.** Every directory gets a `du`-compatible size, a **reclaimable** size and a **shared** size. Symlinks are never followed.
- **G4 — History.** Snapshots are kept, and "what grew between A and B" can be explained in one command.
- **G5 — Hidden consumers.** Surface deleted-but-open files, growing open files and LVM thin-pool usage.
- **G6 — Early warning.** Growth-rate, time-to-full and thin-pool alerts, delivered through existing monitoring.
- **G7 — Strictly read-only.** Tidemark performs no intentional scanned-data mutation or regular-file payload reads, and makes no changes to mounts, fstab, LVM or kernel command line. Symbolic-link text inspection can update link atime under mount policy; directory O_NOATIME fallback and administrator-supplied path resolution are documented limits (R5-23/25).
- **G8 — Hands-off deployment.** Deployable by the hotfix pipeline, active after the next reboot, with a verifiable result.

## 4. Non-goals (v1)

- Deleting or moving data. No "clean up" buttons anywhere.
- **XFS project quotas.** They are deferred to a possible future add-on; see `06-project-quotas-deferred.md` for why. They would need mount changes and stamping every file, and they make cross-tree hard links and renames fail.
- Content search or full filename search. `plocate` already does names.
- Multi-host fleet aggregation. Each host is standalone; aggregate in Prometheus.
- Non-XFS filesystems as a first-class target. Walk mode works on any local filesystem; only XFS gets bulk mode.
- eBPF programs inside Tidemark. bcc-tools stays the ad-hoc microscope.

## 5. Functional requirements

Wave files reference these IDs. FR-10 to FR-13 (project quotas) and FR-16 (collector) are **deferred** and kept unused so the numbering stays stable.

| ID | Requirement | Wave |
|----|-------------|------|
| FR-1 | **Probe**: report OS, kernel, cgroup version, per-mount filesystem facts (type, device, LVM LV/VG, mount options, XFS geometry, ftype, reflink), leaf block devices (through LVM and md) with scheduler and rotational flag, RAID level if md, fanotify capabilities, bulkstat availability, privileges. Human-readable text and JSON. | W1 |
| FR-2 | **Walk-mode scan**: parallel `getdents64` + `statx` traversal of a root, one filesystem only, producing per-directory records. | W1/W2 |
| FR-3 | **Bulk-mode scan** (XFS, root): namespace walk without per-file stat, joined with XFS bulkstat. Results identical to walk mode on a quiescent tree: no changes during scans and no data waiting for writeback. Regular-file apparent size follows the on-disk inode and may lag walk until writeback (R3-16). | W2 |
| FR-4 | **Link-correct metrics** per directory: `t_disk` (du-compatible), `t_apparent`, `t_charged` (additive), `t_reclaimable`, `shared`, `t_files`, `t_inodes`, `t_dirs`, age buckets, newest mtime, widest directory. Defined in `03-link-semantics.md`. | W2 |
| FR-5 | **Symlinks and mounts**: never follow symlinks; never cross filesystems (so scanning `/` never enters `/data`, `/proc`, tmpfs, …); skip mount points, including same-device bind mounts; optional live symlink analysis. | W1/W3 |
| FR-6 | **Snapshots**: a crash-safe, immutable SQLite file per scan; catalog; retention; space guard. | W1/W2/W3 |
| FR-7 | **Drill-down**: `top`, `files`, `wide` from any snapshot, sortable by size, reclaimable, files, age, growth. Paths given through symlinks resolve to canonical paths. | W2/W3 |
| FR-8 | **Diff and explain**: compare two snapshots; walk down the largest-growth path. | W3 |
| FR-9 | **Link topology report**: hard-link sets crossing chosen boundaries, sets pinned by links outside the root, largest shared sets, per-directory shared vs reclaimable. | W3 |
| FR-10 … FR-13 | *(Deferred: project quotas.)* | — |
| FR-14 | **Live write attribution**: fanotify watcher on `/` and `/data` (fd mode baseline, FID mode when available); per-minute hot directories, units and processes. | W5 |
| FR-15 | **Hidden consumers**: deleted-but-open files and growing open-for-write files, attributed to process and unit. | W5 |
| FR-16 | *(Deferred: PostgreSQL collector.)* | — |
| FR-17 | **Rates and alerts**: least-squares growth rates, time-to-full, thin-pool thresholds, nine configurable rule kinds with hysteresis, delivery to journald and textfile metrics. | W5 |
| FR-18 | **`now` view**: a one-screen live report of filesystems, LVM, hot directories, writers, phantom files, alerts and scan status in under 1 s. | W5/W6 |
| FR-19 | **Daemon**: filesystem/LVM samplers, watcher, process scanner, scan scheduler with windows, Unix-socket JSON API, self-protection. | W6 |
| FR-20 | **Metrics**: node_exporter textfile only (ADR-020); cardinality caps. | W6 |
| FR-21 | **TUI**: ncdu-style browser over snapshots, with diff overlay and on-demand live refresh of a subtree. Read-only. | W7 |
| FR-22 | **ncdu export**: snapshot → ncdu JSON. | W3 |
| FR-23 | **Packaging**: RPM for EL8, systemd unit, tmpfiles, default config for `/` + `/data`, SELinux sanity. | W6 |
| FR-24 | **LVM awareness**: map each mount to its LV/VG. Sample LV size, VG free space, and thin-pool data/metadata usage (when thin) every 5 minutes. Show it in `now`, `tidemark lvm`, metrics and alerts. | W4 |
| FR-25 | **Staged deployment**: `tidemark deploy stage` validates everything and enables the service to start on the next boot, changing nothing if a check fails. The daemon writes a first-boot health report. `tidemark deploy verify` gives the hotfix pipeline a pass/fail after the reboot. `deploy unstage` backs it out. | W4/W6 |

## 6. Non-functional requirements

| ID | Requirement | How it's verified |
|----|-------------|-------------------|
| NFR-1 | **Correctness**: `t_disk` and `t_apparent` of every fixture directory equal `du -sxB1` and `du -sxB1 --apparent-size`. Bulk mode equals walk mode on quiescent trees (no changes during scans and no data waiting for writeback); apparent-size oracle fixtures must be flushed. Walk reports in-memory sizes exact at scan time; bulk reports regular-file on-disk sizes (R3-16). See `09-testing.md` §3 for apparent-size oracle normalization on development hosts. | Oracle tests (W2), VM tests |
| NFR-2 | **Scan speed**: cold-cache full scan at least 3× faster than `du -sx` on the staging copy with throttling off. With the `normal` adaptive throttle profile, the leaf block devices' utilisation and average wait (from `/sys/block/*/stat`, using the adaptive gate's samples) stay within the profile's configured targets, apart from short excursions the controller corrects. Targets revisited after pilot measurements. | `xtask bench` (W7) |
| NFR-3 | **Live latency**: filesystem samples every 30 s; hot-directory attribution visible ≤ 60 s after a file is closed. | Integration test (W5/W6) |
| NFR-4 | **Footprint**: daemon steady-state RSS ≤ 300 MiB and ≤ 2% of one core when idle outside scans; active watcher budget is 5% of one core per 1,000 close-write events/s (50 µs/event; 13 R4-8). Scan memory within the configured budget (default 1.25 GiB, inside the service's 2 GiB cgroup limit). Watcher overhead on writers is measured with a generic write workload (the fixture generator in mutate mode), comparing throughput with and without the watcher. | Soak test (W7) |
| NFR-5 | **Safety**: scans never follow found symlinks or read regular-file payloads; no intentional scanned metadata writes. Request O_NOATIME for directories with charged EPERM fallback; symlink inspection/path-resolution atime limits are documented (R5-23/25). | Code review checklist, tests, strace audit (W7-C) |
| NFR-6 | **Crash safety**: a crash never leaves a corrupt "latest" snapshot. The time-series DB survives power loss (SQLite WAL). | Fault-injection tests |
| NFR-7 | **Self-protection**: Tidemark never fills a disk it monitors (state-size cap, free-space floor). It excludes its own state directory from scans and watches. | Tests (W6) |
| NFR-8 | **Security**: root daemon with a minimal capability set; no network listeners by default; API socket `0600 root:root`; config and snapshot files `0600 root:root`; state and runtime directories `0700 root:root`; installed files are root-only, so administrators use `sudo` (path names are sensitive); no uid gate in the CLI or daemon. | Review (W7) |
| NFR-9 | **Operability**: journald logs, metrics, runbooks, `tidemark status` self-check, `deploy verify` for automation. | W4/W6/W7 |
| NFR-10 | **Compatibility**: Rocky/RHEL 8.8–8.10 first-class; should also run on EL9; builds on a `rockylinux:8` container (glibc 2.28). | CI + VM matrix |

## 7. Success measures (after rollout)

- Operator time from "disk alert" to "named directory and writer" drops to under 2 minutes.
- No more ad-hoc `du` runs on production during business hours.
- Every disk-full incident in the first quarter is explained by `tidemark now` or `tidemark diff --explain` without other tools.
- Deployment to a new host is a pipeline step plus a reboot, with no manual follow-up.

## Read-only and visibility limits (13 R5)

The daemon has no network socket or HTTP/webhook transport; metrics use textfile and alerts journal. Read-only observation excludes regular-file payload reads and intentional scanned-data mutation. Reading found symbolic-link text is limited to symlinks and may update link atime under mount policy; resolving administrator-supplied paths may follow links. --check-targets stays inside the selected mount. Root capabilities do not provide a read-only sandbox.

Watcher fd/filesystem covers all mounts on supported 8.10; fd/mount fallback on 8.8 misses other bind/private namespaces and warns in status. TUI paging/index responsiveness and owner staging/soak/fuzz/final guest gates remain open; see 12’s current hardening risks.
