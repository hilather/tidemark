# 06 — XFS project quotas: deferred, not part of v1

> **Status: deferred.** v1 does not use project quotas, and Tidemark makes no mount, fstab or file-metadata changes. This document is kept as the reference for a possible future add-on, and to record *why* it was left out.
>
> **Why it was left out:** the goal is fast disk-space debugging and history, not limits. The live watcher, 30-second sampling and scheduled scans cover that need. Project quotas would add three things the servers don't need:
> - a mount-option change and a slow first mount (quotacheck);
> - stamping a project ID onto every file, which changes every file's ctime;
> - **`EXDEV` failures for hard links and renames across tracked trees (§3), on servers that use hard links everywhere.**
>
> The cost is that per-tree totals between scans aren't instant. The watcher's hot-directory view (where growth is landing, minute by minute) fills most of that gap.
>
> The wave plans, CLI and config contain no quota features. If this add-on is ever revived, it becomes a new wave built on this document, with its audit-first approach.

---


Project quotas make the kernel keep a running block and inode total for every directory tree you tag. Reading those totals is one `quotactl` call per project and takes milliseconds, no matter how many files the tree has. That is the "immediate" answer, and Tidemark samples it every 30 seconds.

There is a **sharp edge that matters a lot on a server that uses hard links everywhere**. Read §3 before enabling anything.

---

## 1. How it works

- Every XFS inode has a 32-bit **project ID** (0 = no project).
- A directory with the `PROJINHERIT` flag passes its project ID to every file and subdirectory created inside it. New subdirectories also inherit the flag.
- When accounting is on, every block and inode allocation is charged to the inode's project ID. `xfs_quota -x -c 'report -p'` and `quotactl(Q_XGETNEXTQUOTA, PRJQUOTA)` return the totals. Project 0 accumulates everything untagged.
- Each inode has exactly one project ID, so **every inode is counted exactly once**, even with hard links. Totals are accurate, but a hard-linked inode is charged to only one tree.
- Deleted-but-open files keep being charged until the last handle closes, which matches what `df` sees.

## 2. Prerequisites

| Requirement | Detail |
|-------------|--------|
| Mount option | `pqnoenforce` = accounting only (**recommended**: limits can never cause `EDQUOT`). `prjquota`/`pquota` = accounting plus enforcement. |
| Enabling | Quota options **cannot be changed by remount**. A data filesystem needs a full `umount` + `mount`. The root filesystem needs `rootflags=pqnoenforce` on the kernel command line (`grubby --update-kernel=ALL --args=rootflags=pqnoenforce`) and a reboot. |
| First mount with quotas | XFS runs a **quotacheck** at mount, scanning all inodes to build initial usage. It takes time proportional to the inode count. Plan a maintenance window, and measure it on a staging copy of similar size. |
| User/group quotas | On v5 XFS (`crc=1`, the RHEL 8 default) project quotas coexist with group quotas. Check `xfs_info`. |
| Tools | `xfsprogs` provides `xfs_quota`. Projects are named in `/etc/projects` (`id:path`) and `/etc/projid` (`name:id`). |

## 3. The sharp edge: `EXDEV` on hard links and renames across projects

When a directory has `PROJINHERIT` set, the kernel refuses to **hard-link** or **rename** an inode with a *different* project ID into it. Both fail with `EXDEV` ("cross-device link"), even though source and target are on the same filesystem. ext4's project-quota patches state explicitly that they copy this XFS behaviour.

Why it exists: otherwise you could move or link data into a tree to dodge its quota.

What it breaks, concretely:

| Operation | Example | Result after tagging |
|-----------|---------|----------------------|
| `link()` / `ln` across trees | Report generator hard-links a shared asset from `/data/shared/` into `/data/reports/acme/` | `ln: failed to create hard link … Invalid cross-device link`. The application gets an error. |
| `rename()` into a tree | Write to `/data/tmp/x.partial`, then `rename()` into `/data/reports/acme/x` (the common "atomic publish" pattern) | The `rename()` syscall returns `EXDEV`. Applications that call `rename(2)` directly **fail**. |
| `mv` across trees | Ops moves a directory between report trees | coreutils `mv` silently falls back to **copy + delete**. That is slow, temporarily doubles the space, and breaks hard links (new inodes). |
| Moves *out of* a tree into an untagged directory | `mv /data/reports/acme/x /data/archive/` where archive has no project | Allowed. The file **keeps** project ID of acme and is still charged to acme while sitting in archive. This is *drift*, detected in §8. |
| Links/renames *within* one project | Anything inside `/data/reports/acme/**` | Unaffected. |
| Symlinks | Anywhere | Unaffected. A symlink is its own inode created in the target directory. |

Because the server "uses hard and soft links everywhere", **assume something will break until an audit proves otherwise.** The plan below makes the decision data-driven.

## 4. Choosing project boundaries

Boundaries are a trade-off between granularity (more, smaller trees give more detailed instant totals) and safety (every boundary is a wall that hard links and renames cannot cross).

| Option | Granularity | EXDEV risk | When to use |
|--------|-------------|------------|-------------|
| A — one project per **product/data root** (`/data/reports`, `/data/archive`, `/data/pgsql`) | Coarse | Low, if each app keeps its links and temp files within its root | Default starting point |
| B — one project per **customer/report tree** (`/data/reports/<customer>`) | Medium | Medium–high (shared assets, common temp dirs) | Only if the audit is clean |
| C — one project per **report directory** | Fine | High | Rarely. Use L1 snapshots for this granularity instead. |

Tidemark's L1 index already provides fine-grained numbers on a schedule. Projects only need to be fine enough to answer "which area is growing right now". Option A is usually enough.

## 5. Audit before enabling (FR-11)

### 5.1 Static audit, from a snapshot

`tidemark quota plan --boundary '/data/reports/*' --boundary /data/archive --snapshot latest`

- Each link instance is mapped to the deepest boundary containing it, or to `none`.
- **Crossing link sets** are sets whose instances fall in more than one region (including `none`). They prove that the application or ops create links across the proposed walls.
- The output lists crossing sets by count and bytes, with example paths and the regions involved, plus an estimate of how long tagging would take (inode count ÷ apply rate).

### 5.2 Dynamic audit, by watching real syscalls (auditd)

A static snapshot cannot show rename-into-place patterns that leave no trace. `tidemark quota audit start --plan plan.toml --duration 7d` installs **temporary** audit rules (`auditctl`, not persisted to `/etc/audit/rules.d`):

```
-a always,exit -F arch=b64 -S rename,renameat,renameat2,link,linkat -F dir=/data -k tidemark_xdev
-a always,exit -F arch=b32 -S rename,renameat,renameat2,link,linkat -F dir=/data -k tidemark_xdev
```

- `tidemark quota audit report` parses `ausearch -k tidemark_xdev --raw`. It reconstructs source and destination paths from the PATH records (using CWD for relative names), maps both to regions, and lists every **cross-region** operation, grouped by executable, systemd unit and path pattern.
- `tidemark quota audit stop` removes the rules; the duration also expires them.
- Run the audit across a full business cycle, including **month-end/quarter-end report runs**, and back-fill jobs.
- Before starting, check the audit log has room (`/var/log/audit`, `max_log_file_action`); rename-heavy workloads produce many records. The command estimates the record rate during the first 10 minutes and aborts if the projected volume exceeds a configured cap.

### 5.3 Go / no-go

**GO** for a proposed boundary set only if:

1. The static audit shows no crossing link sets, or every crossing set is explained and acceptable (for example historical, and no longer created).
2. The dynamic audit shows **zero** cross-region `link`/`rename` operations over at least 7 days, including a period-end run.
3. App owners confirm that temp/staging directories live inside the same region as their destinations.

Otherwise: merge regions, move the boundary up a level, or ask the app team to move temp directories, then re-run the plan and audit.

## 6. Applying (FR-12)

### 6.1 One-time: enable accounting

```bash
findmnt -no SOURCE,TARGET,FSTYPE,OPTIONS /data
xfs_quota -x -c 'state -p' /data            # project accounting state before
# /etc/fstab:   /dev/mapper/vg-data  /data  xfs  defaults,pqnoenforce  0 0
systemctl stop <apps using /data>
umount /data && mount /data                  # quotacheck runs here
xfs_quota -x -c 'state -p' /data            # expect: Accounting: ON, Enforcement: OFF
```

### 6.2 Tag the trees with `tidemark quota apply`

```bash
tidemark quota apply --plan /var/lib/tidemark/quota/plans/plan-2026-10.toml --dry-run
tidemark quota apply --plan /var/lib/tidemark/quota/plans/plan-2026-10.toml --throttle gentle
```

- It writes `/etc/projects` and `/etc/projid` after taking timestamped backups. Project names are prefixed `tm_` to avoid clashing with existing entries; existing entries are preserved.
- It walks each tree with the throttled walker. For every directory and regular file:
  - Open with `O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_NOCTTY|O_NOATIME|O_CLOEXEC`. Symlinks, devices, FIFOs and sockets are skipped; they keep project 0, at negligible size.
  - `FS_IOC_FSGETXATTR`, then set `fsx_projid`, plus `FS_XFLAG_PROJINHERIT` on directories, then `FS_IOC_FSSETXATTR`.
- It is **idempotent and resumable**. A checkpoint (completed top-level subtrees plus the current directory path) is flushed every 10 seconds, and `--resume` continues from it.
- It is rate limited by the gate (default 2,000 inodes/s, `--rate` to change). Each tag is a metadata **write** (an inode update through the XFS log), so on RAID 5 keep it gentle and run off-peak. ETA = inodes ÷ rate; for example, 20 M inodes at 2,000/s takes about 2.8 h.
- **Side effect: every tagged inode's ctime changes.** Backup software that uses ctime for incrementals will see every file as changed and take a near-full backup next run. Schedule tagging just before a planned full backup.

### 6.3 Verify

`tidemark quota verify --plan …` performs two independent checks:

1. `xfs_quota -x -c 'project -c <name>' <mount>` for each project. This is xfsprogs' own check mode, which reports inodes with the wrong ID or missing inherit flag.
2. A bulk-mode scan of the mount, checking that every inode under each tree has the expected ID (`t_drift_files = 0`) and that per-project quota totals match snapshot totals within tolerance (§8).

### 6.4 Rollback

- **One project:** `tidemark quota clear --project tm_reports_acme` resets `projid=0` and clears `PROJINHERIT` under the tree (throttled, resumable). It removes the entries from `/etc/projects` and `/etc/projid`. xfsprogs' equivalent is `xfs_quota -x -c 'project -C <name>' <mount>`.
- **Everything:** clear all projects, then remove `pqnoenforce` from fstab at the next maintenance window. Leaving accounting on with no projects is harmless: all usage goes to project 0.
- If an application hits `EXDEV` in production, `clear` on the affected tree restores normal semantics immediately for new operations, because clearing `PROJINHERIT` on the target directories removes the check.

## 7. Reading (FR-10)

- **`tidemark quota status [MOUNT]`** shows mount options, accounting/enforcement state (`Q_XGETQSTATV`), and configured projects with their ID, path, existence and `PROJINHERIT` on the root.
- **`tidemark projects [--window 15m]`** lists every project's bytes and inodes (`Q_XGETNEXTQUOTA` iteration), growth rate over the window from `quota_samples`, and share of the filesystem. Project 0 is shown as `(untagged)`.
- The daemon samples all projects on configured mounts every 30 s into `quota_samples`.

## 8. Drift and reconciliation (FR-13)

Drift sources: renames *out of* a tree (files keep the old ID); files created before tagging and missed by an interrupted apply; tools that reset attributes; pre-existing cross-tree hard links (one inode, one ID).

`tidemark quota reconcile [MOUNT]` compares, per project:

| Quantity | Source |
|----------|--------|
| `quota_bytes` | quotactl, now |
| `tree_bytes` | latest snapshot `t_disk` of the project root |
| `drift_in` | Bulk-mode snapshot: inodes under the tree whose ID ≠ project (`t_drift_*`, `drift_samples`) |
| `drift_out` | Bulk-mode snapshot: inodes outside the tree carrying this project's ID (aggregated from `drift_samples` and `dirs.projid`) |
| `open_deleted` | Live phantom bytes on that tree's files (charged to the project but invisible to scans) |

It reports the unexplained difference and its likely causes. With `--fix`, it re-tags drifted inodes (the `apply` code path, limited to listed inodes). Tolerance defaults to 1% or 1 GiB, whichever is larger, since snapshots are fuzzy while quota is live.

## 9. Runbook commands (reference)

```bash
xfs_quota -x -c 'state -p' /data                 # accounting/enforcement on?
xfs_quota -x -c 'report -pbih' /data             # per-project usage (human)
xfs_quota -x -c 'project -c tm_reports' /data    # check tagging
xfs_quota -x -c 'project -C tm_reports' /data    # clear tagging (rollback)
xfs_io -c 'lsproj' /data/reports/acme            # project ID of one path
xfs_io -c 'stat' /data/reports/acme | grep -i xflags   # look for the inherit flag
```

## 10. References

- ext4 project-quota patch series, stating XFS-consistent semantics: hard-linking or renaming an inode with a different project ID into a project-inherit directory fails with `EXDEV`. https://lwn.net/Articles/671627
- `xfs_quota(8)`, `xfs_io(8)`, `quotactl(2)` man pages on the target host.
