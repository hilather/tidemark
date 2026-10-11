# 00 — Does a tool for this already exist?

**Short answer: no single open-source tool covers what you need on Rocky 8.** Several tools cover one piece each. What's missing is the combination, running on a 4.18-based kernel without a heavy dependency stack:

1. Instant per-tree totals maintained by the kernel.
2. A historical index with hard-link-correct numbers, including "what would deleting this actually free".
3. Live attribution of growth to directories, processes and systemd units.
4. Postgres-internal breakdown (WAL, replication slots, relations).

The recommendation is to build **Tidemark** (placeholder name) in Rust, borrowing proven ideas from the tools below. Use the stopgaps in §4 while it is being built.

---

## 1. Requirements used for the comparison

| ID | Requirement |
|----|-------------|
| R1 | Per-tree totals in under a second, without scanning |
| R2 | Drill-down index with history and diffs ("what grew since yesterday") |
| R3 | Hard-link-correct sizing, including reclaimable vs shared bytes |
| R4 | Live "what is growing right now", attributed to process / systemd unit |
| R5 | Postgres internals: WAL, replication-slot retention, relation sizes |
| R6 | Low impact on a busy RAID 5 box: throttled, read-only, no Elasticsearch/MySQL on the host |
| R7 | Runs on Rocky/RHEL 8 (kernel 4.18 with Red Hat backports) |

## 2. Survey

Legend: ✓ meets · ◐ partial · ✗ no

| Tool | R1 | R2 | R3 | R4 | R5 | R6 | R7 | Notes |
|------|----|----|----|----|----|----|----|-------|
| GNU `du`, ncdu, gdu, dua, dust, pdu | ✗ | ◐ | ◐ | ✗ | ✗ | ✓ | ✓ | One-shot scans, some parallel. They count each hard-linked inode once, like `du`, but have no "reclaimable" view. ncdu can export/import a JSON snapshot but has no history or diff. |
| **duc** | ✗ | ◐ | ◐ | ✗ | ✗ | ✓ | ✓ | Keeps a database of accumulated directory sizes that you query instead of rescanning. Designed for hundreds of millions of files. It re-indexes periodically and has no live layer. |
| **Robinhood Policy Engine** (cea-hpc) | ✗ | ✓ | ◐ | ✗ | ✗ | ✗ | ✓ | Replicates filesystem metadata into MySQL using parallel scans, with reports and policies. On plain POSIX filesystems it relies on periodic scans; its changelog reader is Lustre-only. Latest release 3.2.0 (Nov 2024). HPC-oriented and heavy for one server. |
| **Diskover Community Edition** | ✗ | ✓ | ◐ | ✗ | ✗ | ✗ | ◐ | Python crawler with an Elasticsearch index and a PHP web app; strong search and analytics. Running Elasticsearch on the disk-heavy box competes for I/O and RAM. Crawls are periodic. v2.3.4 released Jan 2026. |
| XFS project quotas (`xfs_quota`) | ✓ | ✗ | ◐ | ✗ | ✗ | ✓ | ✓ | Kernel-maintained per-tree block and inode totals, read instantly. Only covers the trees you define, with no drill-down. Has a serious hard-link caveat (see `06-project-quotas-deferred.md`; deferred from v1 for this reason). |
| fatrace, `fsnotifywatch`, fsmon | ✗ | ✗ | ✗ | ◐ | ✗ | ✓ | ◐ | These produce fanotify event streams, not usage accounting. fsmon needs Linux 5.9+ fanotify FID mode, which is newer than the upstream 4.18 baseline. |
| bcc-tools (`filetop`, `biosnoop`) | ✗ | ✗ | ✗ | ◐ | ✗ | ✓ | ✓ | Interactive eBPF diagnosis, supported on RHEL 8 x86_64. Excellent as a microscope; it isn't an accounting system. |
| Prometheus node_exporter | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ | ✓ | Filesystem-level used/available only. |
| Commercial storage-analytics platforms | ◐ | ✓ | ? | ? | ✗ | ? | ? | Not evaluated. They are typically aimed at multi-petabyte estates. |

### Conclusion

> **Update after review:** instant per-tree totals (R1) were relaxed. Project quotas were deferred because of their hard-link side effects. Tidemark v1 covers R1 approximately: 30-second filesystem sampling plus live hot-directory attribution.

No tool reaches R1–R5 together. The closest combination is xfs_quota plus duc plus bcc-tools plus hand-written Postgres queries. That is four tools, and none of them answers the hard-link question, "how much would deleting this free?".

## 3. What Tidemark borrows

| From | Idea we reuse |
|------|---------------|
| duc | A persistent index of directory totals. Answer from the index; scan only on a schedule. |
| ncdu | Its documented JSON export format. Tidemark can emit it, so ncdu becomes a free interactive viewer before our own TUI exists. |
| Robinhood | "Scan sometimes, query always"; parallel scanning; database-backed reports. |
| fatrace | Classic fd-mode fanotify on a mount. It works on old kernels and gives the writer's PID. |
| xfsdump / xfs_fsr | XFS **bulkstat**: read inode attributes in large on-disk-ordered batches instead of one `stat` per file. This is the closest Linux analogue to how Windows tools read the NTFS MFT. |
| bcc-tools | Keep it installed as the operator's microscope for anything Tidemark doesn't cover. |

## 4. Stopgaps you can use this week

These need no new software beyond what Rocky 8 ships or EPEL carries. Run anything heavy at low priority.

```bash
# Who holds space in deleted-but-still-open files (a classic "df says full, du disagrees")
lsof +L1 2>/dev/null | awk 'NR==1 || $7 > 1073741824'      # entries over 1 GiB

# Which files are being written right now (bcc-tools, supported on RHEL 8 x86_64)
dnf install -y bcc-tools
/usr/share/bcc/tools/filetop -C 5            # refresh every 5 s; see --help for sort options

# Coarse top-level breakdown, gently
nice -n 19 du -xh --max-depth=2 /data 2>/dev/null | sort -h | tail -25

# Postgres: WAL on disk and WAL held back by replication slots (run as a superuser or pg_monitor member)
psql -XAtc "select pg_size_pretty(sum(size)) from pg_ls_waldir()"
psql -XAtc "select slot_name, active, pg_size_pretty(pg_wal_lsn_diff(pg_current_wal_lsn(), restart_lsn)) from pg_replication_slots"
psql -XAtc "select failed_count, last_failed_time from pg_stat_archiver"

# If project quotas are already enabled on the filesystem
xfs_quota -x -c 'report -pbih' /data
```

If you want a persistent index today, `duc` is the lightest option. Build it from source if your repos don't carry it, then run `duc index -x /data` nightly under `nice` and browse with `duc ui`.

## 5. References

- duc: https://duc.zevv.nl/
- Robinhood Policy Engine: https://github.com/cea-hpc/robinhood (wiki: https://github.com/cea-hpc/robinhood/wiki)
- Diskover Community Edition: https://github.com/diskoverdata/diskover-community
- ncdu JSON export format: https://dev.yorhel.nl/ncdu/jsonfmt
- fanotify_mark(2), for feature introduction versions: https://manpages.debian.org/testing/manpages-dev/fanotify_mark.2
- fsmon (fanotify FID-mode tool): https://docs.rs/crate/fsmon/latest
- RHEL 8.1 release notes (bcc-tools supported on x86_64): https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/8/html-single/8.1_release_notes/index
- Project-quota link/rename semantics (ext4 patch series stating XFS-consistent EXDEV behaviour): https://lwn.net/Articles/671627
- Rocky Linux release and support policy: https://docs.rockylinux.org/releases/
