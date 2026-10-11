# 12 — Architecture decisions and risk register

## Architecture decision records (ADR)

Each ADR: context → decision → consequences. Status is "accepted" unless noted.

**ADR-001 — Rust as the implementation language.**
- *Context:* root daemon, hot metadata loops over tens of millions of entries, raw ioctls with C struct layouts, a single-host deployment on EL8.
- *Decision:* Rust (edition 2021, pinned stable).
- *Consequences:*
  - Memory safety for a root daemon.
  - Predictable memory (no GC heap growth during scans).
  - Mature syscall crates (`rustix`) and bundled SQLite.
  - Single binaries.
  - Go would be a viable alternative (simpler concurrency, GC acceptable), but its XFS ioctl structs need cgo or hand-written layouts, and its memory use is less predictable at this scale.
  - C/C++ are rejected for safety; Python for scan speed.

**ADR-002 — SQLite: one immutable file per snapshot plus one time-series DB.**
- *Decision:* as in `05`.
- *Consequences:*
  - Atomic creation, trivial retention, no vacuum churn, easy `ATTACH` diffs.
  - No server process competing with Postgres.
  - Cross-snapshot queries need explicit ATTACH, which is fine for the few we need.

**ADR-003 — Two scan modes, walk as the oracle for bulk.**
- *Decision:* implement both. `auto` selects bulk only when it's eligible and fits the memory budget.
- *Consequences:* about twice the scanner code, mitigated by shared aggregation and equivalence tests. Bulk gives the "read the metadata table" speed on cold HDD RAID.

**ADR-004 — fanotify fd mode as the baseline watcher; FID modes only via probe; no inotify; no eBPF in v1.**
- *Decision:* as stated.
- *Consequences:*
  - inotify needs a watch per directory, which is impractical at millions of directories.
  - eBPF on a 4.18-based kernel depends on backport details and adds verifier and BTF variability.
  - bcc-tools stays available for ad-hoc diagnosis.
  - fd mode doesn't see creates, deletes or renames; L0 sampling and scans cover net change.

**ADR-005 — No async runtime.**
- *Decision:* `std::thread` plus `crossbeam` channels. The daemon's concurrency is a dozen long-lived loops.
- *Consequences:* simpler code for agents to write and review; blocking syscalls are natural. Note: the sync `postgres` crate runs a small internal tokio runtime per client. That is acceptable, since it is confined to the `pg` thread.

**ADR-006 — Metrics through the node_exporter textfile collector by default.**
- *Decision:* textfile is the default; an optional HTTP listener binds to loopback only.
- *Consequences:* no new network surface; works with the most common EL8 monitoring setup.

**ADR-007 — No project quotas in v1** (supersedes the earlier "accounting-only, audit-gated quotas" decision).
- *Context:* the need is fast disk-space debugging and history, not limits. Project quotas would need a mount-option change (with a slow first mount), stamping every file (changing every file's ctime), and would make hard links and renames across tracked trees fail with `EXDEV`, on servers that use hard links everywhere.
- *Decision:* defer. Live attribution (fanotify), 30-second sampling and scheduled scans cover the need. `06-project-quotas-deferred.md` keeps the design for a possible add-on.
- *Consequences:* no instant per-tree totals between scans; the hot-directory view fills most of that gap. Far lower deployment risk: no filesystem or boot changes at all.

**ADR-008 — Tidemark is strictly read-only.**
- *Consequences:* no code path changes file metadata, mounts, LVM or boot configuration, so a bug or compromise can't corrupt data. The only writes are to its own state directory, its config (`deploy stage`), and the service's enablement.

**ADR-009 — Build on a `rockylinux:8` container (glibc 2.28), not musl static.**
- *Consequences:* the native allocator performs well under multithreading; normal debugging tools work. Binaries target EL8+ (they also run on EL9).

**ADR-010 — No deletion or cleanup features in v1.**
- *Consequences:* Tidemark tells you what deleting would free; humans or app tooling do the deleting. This removes the most dangerous class of bugs.

**ADR-011 — Snapshots store per-directory aggregates, notable files and link sets, not every file.**
- *Consequences:*
  - Snapshot size scales with directories, not files.
  - Per-file questions beyond "largest files" are answered live (`top --live`, `links --show-paths`, TUI `R`).
  - A future option could store all files for selected roots.

**ADR-012 — Three size semantics: du-compatible, charged (LCA) and reclaimable.**
- *Consequences:* correct answers to three different questions. UI and docs must explain them; `03` §6 is the teaching example.

**ADR-013 — The daemon runs in the host mount namespace.**
- *Context:* fanotify mount marks attach to the specific mount object. systemd's `ProtectSystem=`/`ProtectHome=`/`PrivateTmp=`/`ReadWritePaths=` create a private mount namespace containing *copies* of the mounts, so marks would never fire for writes made through the host's mounts.
- *Decision:* omit those options. Compensate with a capability bounding set, `NoNewPrivileges`, `RestrictNamespaces`, `SystemCallArchitectures`, `LockPersonality`, `RestrictAddressFamilies`, plus a startup namespace check and an install-time end-to-end test.
- *Consequences:* weaker filesystem sandboxing than ideal. Revisit if `FAN_MARK_FILESYSTEM` proves available on the target kernels, since filesystem marks are per superblock. A watcher-only helper process outside the sandbox is a possible future split.

**ADR-014 — Self-throttling instead of relying on `ionice`.**
- *Context:* the idle I/O class is only honoured by priority-aware schedulers such as BFQ; EL8 commonly uses `mq-deadline` for these devices.
- *Decision:* an AIMD token bucket driven by leaf-device utilisation and await. Per-thread idle priority stays as best-effort extra.
- *Consequences:* works on any scheduler, md or hardware RAID; tunable from pilot data.

**ADR-015 — At most one scan at a time per host.**
- *Consequences:* the array is never hit by two metadata sweeps at once; with `/` and `/data` on the same RAID, their schedules queue.

**ADR-016 — Staged deployment for the hotfix pipeline.**
- *Decision:*
  - `deploy stage` does all validation up front and is all-or-nothing. It enables the service but doesn't start it.
  - The daemon writes a first-boot report.
  - `deploy verify` gives a machine-checkable result.
- *Consequences:* Tidemark rides existing reboot cycles with no manual steps. Since nothing touches boot-critical config, a bad deploy can't affect the reboot itself.

**ADR-017 — LVM via `lvs`/`vgs --reportformat json` every 5 minutes.**
- *Context:* reading LVM metadata directly is complex. The lvm2 tools are the supported interface, and JSON reports are available in EL8's lvm2.
- *Decision:* shell out to the plain reporting commands (shared lock only; `--readonly` is avoided because it suppresses device-mapper status such as thin-pool usage), with a timeout and a 5-minute interval.
- *Consequences:* a cheap, robust read; thin-pool exhaustion becomes visible even when `df` looks fine.

**ADR-018 — Root-only access; no `tidemark` group.**
- *Context:* path names in snapshots and API responses are sensitive. The owner wants no group granting access without sudo.
- *Decision:* no `tidemark` group. All installed Tidemark files are `root:root`: the API socket, config, snapshots and their `.db.tmp` files are `0600`; state and runtime directories are `0700`. Access control is by ownership, file modes and socket peer uid: every API method accepts uid 0 or the daemon's own effective uid; other peers receive `forbidden`. In production the daemon is root, so only root is accepted. Neither the CLI nor the daemon has a uid gate. Remove the socket-group configuration key and group creation from packaging; no group-specific service setting or account-definition file is shipped.
- *Consequences:* operators use `sudo`; one less packaging step and no group membership to manage. A non-root command that cannot read the root-only config, state directory, snapshot or socket retains the exit code for that failure and adds the hint: `Tidemark's files are root-only; run this command with sudo.` No new exit code is added; exit code 5 retains its precondition-failure meaning. Unprivileged tests keep working with their own temporary config, state directory and socket. Non-root daemon and scan runs use the existing degraded mode (readable-subset walk mode, no watcher or procscan of other users).

---

## Risk register

L = likelihood, I = impact (H/M/L).

| ID | Risk | L | I | Mitigation | Where |
|----|------|---|---|------------|-------|
| R1 | *(removed: project quotas deferred, ADR-007)* | — | — | — | — |
| R2 | Scans degrade Postgres or app latency on RAID 5 | M | H | Adaptive throttle; windows; `gentle` profile in pilot; one scan at a time; pgbench gate | `04` §6, W1-D, W7-B, P3 |
| R3 | Scans cause atime write storms | L | H | `O_NOATIME` on every directory open; `noatime_no_writes` test | W1-B |
| R4 | Misleading numbers with hard links lead to wrong deletions | M | H | Normative spec; oracle, property and golden tests; reclaimable column; `H` markers; v5-only bulk metadata (32-bit nlink), never truncated v1 link counts | `03`, W2-A |
| R5 | Watcher silently blind (namespace, bind mounts, overflow) | M | M | Namespace guard; install E2E test; probe warnings; lossy/overflow metrics and alert | W5-A, W6-C |
| R6 | Memory blow-up on very large trees | M | M | Memory estimates; budget-driven mode choice; cgroup memory limit; scale tests; spill join if needed | `04` §7, W2, W7-B |
| R7 | EL8 kernel behaviour differs from upstream (bulkstat units, v5, fanotify FID) | M | M | EL8 5.0.0 headers lack v5; module inspection found v5 handlers in 477.27.1.el8_8 and 553.172.1.el8_10, not runtime ioctl evidence. Hand-transcribed ABI pinned to gcc layouts (`04` §5.1); runtime probes and units tests on both VMs remain gates | W1-C, W5-A |
| R8 | Tidemark's own state fills the disk or adds random writes to RAID 5 | L | M | Space guard; retention; state dir on another device; excluded from scans and watch | `05` §8, W1-E |
| R11 | Postgres collector adds load or locks | L | L–M | Statement and lock timeouts; read-only session; low frequency; per-DB sequential | W5-C |
| R12 | Path names exposed through metrics or webhooks | M | M | Root-only file and API permissions (ADR-018); loopback-only HTTP; cardinality caps; redaction decision in `security.md` | W6-B, W7-C |
| R13 | Inconsistent or low-quality code from parallel agents | M | M | Frozen contracts; owned paths; independent reviewers; gate reviews | `10` |
| R14 | Host on unsupported Rocky 8.8 | H | M | Probe warns; P0 recommends updating to 8.10 | P0 |
| R15 | SELinux denials in enforcing mode | L | M | Install test checks AVCs; runs as `unconfined_service_t` | W6-C |
| R17 | Reflinked files overstate physical usage | L | L | Documented; probe flags reflink; FIEMAP-based option deferred | `03` §5 |
| R18 | LVM thin pool fills before the filesystem looks full | M if thin | H | LVM sampler; `thin_pool_pct` alert; shown in `now` | `07` §1, W4-A |
| R19 | `/data` snapshots consume root-filesystem space (state dir on `/`) | M | M | Space guard; retention; `state_dir` relocatable; snapshot size measured in pilot | `05` §8, P4 |
| R20 | Scans of `/` and `/data` share the same RAID and both hit it | M | M | One scan at a time per host; separate windows; adaptive throttle | ADR-015 |
| R21 | Fleet hosts differ from the pilot layout (no `/data`, not LVM) | M | L | `deploy stage` pre-flight blocks or warns per host; per-host-type configs | `08` §5, P5 |
| R9, R10, R16 | *(removed: quota tagging ctime, quotacheck boot delay, auditd volume — no longer applicable)* | — | — | — | — |
