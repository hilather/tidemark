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
- *Consequences:* simpler code for agents to write and review; blocking syscalls are natural.

**ADR-006 — Metrics through the node_exporter textfile collector by default.**
- *Decision:* textfile is the only v1 metrics transport (ADR-020; R5-1 supersedes the optional listener).
- *Consequences:* no new network surface; works with the most common EL8 monitoring setup.

**ADR-007 — No project quotas in v1** (supersedes the earlier "accounting-only, audit-gated quotas" decision).
- *Context:* the need is fast disk-space debugging and history, not limits. Project quotas would need a mount-option change (with a slow first mount), stamping every file (changing every file's ctime), and would make hard links and renames across tracked trees fail with `EXDEV`, on servers that use hard links everywhere.
- *Decision:* defer. Live attribution (fanotify), 30-second sampling and scheduled scans cover the need. `06-project-quotas-deferred.md` keeps the design for a possible add-on.
- *Consequences:* no instant per-tree totals between scans; the hot-directory view fills most of that gap. Far lower deployment risk: no filesystem or boot changes at all.

**ADR-008 — Tidemark is strictly read-only.**
- *Consequences:* observation performs no intentional scanned-data mutation, mount/LVM/boot changes or regular-file payload reads. Writes are Tidemark state/config and explicit deployment/service operations. Link-text inspection may update symlink atime under mount policy; directory NOATIME fallback is also a limit (R5-23/25). Root capabilities are not a read-only sandbox; see `../security.md`.

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
- *Consequences:* weaker filesystem sandboxing than ideal. Revisit completed by R5-26: fd mode prefers filesystem marks on measured 8.10 and retains mount marks on 8.8; host namespace checks remain for scanning/sampling/path interpretation. A watcher-only helper process outside the sandbox is a possible future split.

**ADR-014 — Self-throttling instead of relying on `ionice`.**
- *Context:* the idle I/O class is only honoured by priority-aware schedulers such as BFQ; EL8 commonly uses `mq-deadline` for these devices.
- *Decision:* an AIMD token bucket driven by leaf-device utilisation and await. Per-thread idle priority stays as best-effort extra.
- *Consequences:* works on any scheduler, md or hardware RAID; tunable from pilot data.

**ADR-015 — At most one scan at a time per host.**
- *Consequences:* the array is never hit by two metadata sweeps at once; with `/` and `/data` on the same RAID, their schedules queue.

**ADR-016 — Staged deployment for the hotfix pipeline.**
- *Decision:*
  - `deploy stage` validates before mutation: blocking pre-flight failures change nothing. Later failures attempt best-effort compensation and report residual errors; it is not a transaction (13 R2-7). It enables the service and starts it only with `--start-now`.
  - The daemon writes a first-boot report.
  - `deploy verify` gives a machine-checkable result.
- *Consequences:* Tidemark rides existing reboot cycles with no manual steps. Since nothing touches boot-critical config, a bad deploy can't affect the reboot itself.

**ADR-017 — LVM via `lvs`/`vgs --reportformat json` every 5 minutes.**
- *Context:* reading LVM metadata directly is complex. The lvm2 tools are the supported interface, and JSON reports are available in EL8's lvm2.
- *Decision:* shell out to the plain reporting commands (shared lock only; `--readonly` is avoided because it suppresses device-mapper status such as thin-pool usage), with a timeout and a 5-minute interval.
- *Consequences:* a cheap, robust read; thin-pool exhaustion becomes visible even when `df` looks fine.

**ADR-018 — Root-only access; no `tidemark` group.**
- *Context:* path names in snapshots and API responses are sensitive. The owner wants no group granting access without sudo.
- *Decision:* no `tidemark` group. All installed Tidemark files are `root:root`: the API socket, config, snapshots and their `.db.tmp` files are `0600`; state and runtime directories are `0700`. Access control is by ownership, file modes and socket peer uid: every API method accepts only peer uid 0, identified by kernel SO_PEERCRED; other peers are closed before parsing, with zero response bytes (R5-20), even with a nonroot daemon (13 R3-14). Neither binary has a startup uid gate; API dispatch does. Remove the socket-group configuration key and group creation from packaging; no group-specific service setting or account-definition file is shipped.
- *Consequences:* operators use `sudo`; one less packaging step and no group membership to manage. A non-root command that cannot read the root-only config, state directory, snapshot or socket retains the exit code for that failure and adds the hint: `Tidemark's files are root-only; run this command with sudo.` No new exit code is added; exit code 5 retains its precondition-failure meaning. Unprivileged tests keep working with their own temporary config, state directory and socket. Non-root daemon and scan runs use the existing degraded mode (readable-subset walk mode, no watcher or procscan of other users).

**ADR-019 — No PostgreSQL tracking in v1.**
- *Context:* owner decision on 2026-10-11: keep disk-usage intelligence simple; database-specific tracking may return later as an add-on.
- *Decision:* remove the collector and `tidemark-pg`, `tidemark pg`, probe integration, `[postgres]` and PostgreSQL-only keys, `pg.pass`, the three alert kinds and defaults, core types/fields/API, sample table/store functions, service thread, first-boot component, metrics, deploy checks and workspace `postgres` dependency. `[postgres]` is rejected as unknown. The reference design and former wave tasks live in `14-postgres-deferred.md`. The database benchmark gate was removed with PostgreSQL tracking and replaced by a device utilisation and average-wait gate using the adaptive throttle samples.
- *Consequences:* no migration because nothing has been released. Generic annotations and path exclusions remain. A PostgreSQL data directory is scanned normally. Adaptive throttling, scan windows, one scan at a time, `O_NOATIME`, R2 and the device utilisation and wait gate remain to protect the shared-disk workload. R11 no longer applies; ADR-005 needs no client-runtime exception.

**ADR-020 — No HTTP in v1.**
- *Decision:* Tidemark v1 exposes and requires no HTTP. Metrics use only the node_exporter textfile collector; alerts use the journal, with active alerts and history through the Unix API and CLI.
- *Date:* 2026-10-12.
- *Reason:* A root daemon on database servers should expose and need no network. Textfile and journal cover the need, and removing HTTP gives a smaller dependency tree.
- *Removed:* inbound Prometheus endpoint and `metrics.listen`; outbound alert webhook and `alerts.webhook`; HTTP client/server code, tests and TLS dependencies. Old keys are rejected with an ADR-020 removal message. The unit permits only `AF_UNIX`: Rocky 8.8 and 8.10 guest measurements confirmed readiness, status/now/lvm/hot, daemon scans and LVM reporting with no address-family errors over three minutes, and strace saw no socket() calls from lvs/vgs children.
- *Consequences:* no replacement or hidden HTTP flag; the daemon opens only its Unix API socket.
- *Revisit if:* a concrete requirement cannot be met through the textfile collector, journal or Unix API, with a new owner-approved ADR and security review.

---

## Risk register

### First real runs: measured platform and resource facts (13 R4-8/15)

First real runs on Rocky 8.8/8.10: bulkstat v5 works on both; statx supplies no mount id on either. fanotify fd mode works on both; filesystem marks work on 8.10, while 8.8 uses mount marks and cannot observe writes through another mount of that filesystem. On 8.10 the package installs/upgrades/erases and survives reboot under SELinux Enforcing without denials. Tidemark ships no SELinux policy and supports Enforcing, Permissive or Disabled; enforcing is a test posture, not a product prerequisite. Do not infer remaining VM/soak gates passed from these observations.

Owner decision: fd-mode watcher budget is 5% of one core per 1,000 close-write events/s, or 50 microseconds/event, replacing 1%. EL8 kernels 4.18.0-477 and -553 offer no measured FID mode. Optimised 8.10 guest measured 45.6 microseconds/event at 2,000/s (9.1% of one core) with host under load. Owner will measure real servers; this is no universal performance guarantee. The 2,000/s scale test threshold is 10% of one core, replacing 2%. Idle CPU and write-throughput impact remain separate gates.

### First real runs: open risks (13 R4-16)

Superseded by R5-2/8/9/11/19/21: writer supervision, collector availability, immutable process reports, monotonic scheduling, exact watcher windows, reload continuity, mount-rate reset/current degradation, API ownership/deadlines, crash cleanup and state trust have landed fix notes. Cancellation’s work bound excludes durable outcome sync (R5-3). Open: scan.cancel id conditioning, million-child paging/index decision, staging scale/soak, nightly fuzz execution/cumulative gate and R5-27 core/guest decisions. Implementation notes do not establish independent guest acceptance.

Historical findings are superseded by R5-2/4/19/21/24. Unrestricted listener, final Rocky guest and owner sign-off gates remain required; see R5-18/27.

Mount-state publication follow-ups are implemented (R5-8/19); privileged remount/race acceptance remains separate.


L = likelihood, I = impact (H/M/L).

| ID | Risk | L | I | Mitigation | Where |
|----|------|---|---|------------|-------|
| R1 | *(removed: project quotas deferred, ADR-007)* | — | — | — | — |
| R2 | Scans degrade Postgres or app latency on RAID 5 | M | H | Adaptive throttle; windows; `gentle` profile in pilot; one scan at a time; device utilisation and wait gate | `04` §6, W1-D, W7-B, P3 |
| R3 | Scans cause atime write storms | L | H | `O_NOATIME` on every directory open; `noatime_no_writes` test | W1-B |
| R4 | Misleading numbers with hard links lead to wrong deletions | M | H | Normative spec; oracle, property and golden tests; reclaimable column; `H` markers; v5-only bulk metadata (32-bit nlink), never truncated v1 link counts | `03`, W2-A |
| R5 | Watcher silently blind (namespace, bind mounts, overflow) | M | M | Namespace guard; install E2E test; probe warnings; lossy/overflow metrics and alert | W5-A, W6-C |
| R6 | Memory blow-up on very large trees | M | M | Memory estimates; budget-driven mode choice; cgroup memory limit; scale tests; spill join if needed | `04` §7, W2, W7-B |
| R7 | EL8 kernel behaviour differs from upstream (bulkstat units, v5, fanotify FID) | M | M | EL8 5.0.0 headers lack v5; module inspection found v5 handlers in 477.27.1.el8_8 and 553.172.1.el8_10, followed by successful v5 first-run runtime probes on both releases. No statx mount ID or FID mode measured; filesystem marks only on 8.10. Hand-transcribed ABI pinned to gcc layouts (`04` §5.1); units and scale tests remain gates | W1-C, W5-A |
| R8 | Tidemark's own state fills the disk or adds random writes to RAID 5 | L | M | Space guard; retention; state dir on another device; excluded from scans and watch | `05` §8, W1-E |
| R11 | *(removed: collector deferred, ADR-019)* | — | — | — | — |
| R12 | Sensitive names exposed through state, logs or metrics | M | M | Root-only installed access; trusted collector ancestry; metrics 0644 is deliberate; protect downstream exporter/journal; no v1 redaction or HTTP/webhook (R5-1/21) | W6-B, W7-C |
| R13 | Inconsistent or low-quality code from parallel agents | M | M | Frozen contracts; owned paths; independent reviewers; gate reviews | `10` |
| R14 | Host on unsupported Rocky 8.8 | H | M | Probe warns; P0 recommends updating to 8.10 | P0 |
| R15 | SELinux denials in enforcing mode | L | M | Install test checks AVCs; runs as `unconfined_service_t` | W6-C |
| R17 | Reflinked files overstate physical usage | L | L | Documented; probe flags reflink; FIEMAP-based option deferred | `03` §5 |
| R18 | LVM thin pool fills before the filesystem looks full | M if thin | H | LVM sampler; `thin_pool_pct` alert; shown in `now` | `07` §1, W4-A |
| R19 | `/data` snapshots consume root-filesystem space (state dir on `/`) | M | M | Space guard; retention; `state_dir` relocatable; snapshot size measured in pilot | `05` §8, P4 |
| R20 | Scans of `/` and `/data` share the same RAID and both hit it | M | M | One scan at a time per host; separate windows; adaptive throttle | ADR-015 |
| R21 | Fleet hosts differ from the pilot layout (no `/data`, not LVM) | M | L | `deploy stage` pre-flight blocks or warns per host; per-host-type configs | `08` §5, P5 |
| R9, R10, R16 | *(removed: quota tagging ctime, quotacheck boot delay, auditd volume — no longer applicable)* | — | — | — | — |

### Gate review / Wave 6 open risks (R3-15)

These are open risks, not accepted implementation guarantees. Historical handoff test results do not establish VM or release acceptance.

| Risk | Remaining limitation and follow-up | Evidence / owners |
|------|------------------------------------|-------------------|
| N1 | Snapshot finish timestamp is after rows but before index build and sync because `SnapshotWriter::finish_with` takes `&SnapshotMeta`; needs a storage-owned finalization clock/metadata seam. | W23-fix-scan; store/scan owners |
| R3-L1 | `deploy/files.rs` is compiled through `#[path]` from both daemon `lib.rs` and `firstboot.rs`; consolidate into one crate-visible module. | CI-fix-daemon-state, W6-A1; daemon/deploy owners |
| R3-L2 | `tidemark_fs::lvm::collect` has no cancellation hook; an in-flight LVM worker can exceed the shared join deadline; CORE-fix-1 retains/joins handles rather than abandoning them. | W6-A1; FS/lifecycle owners |
| R3-L3 | Store has no alert-history reader; `LiveStateView::alerts_history` covers only successfully persisted events in the current daemon run. | W6-A1/W6-A3; store/daemon owners |
| R3-L4 | Unprivileged state alias trust gap addressed by R5-21/24; SQLite pathname opens still assume no hostile root/same-euid leaf replacement. Real second-user guest acceptance remains pending. | STATE-trust-A/A2/B |
| R3-L5 | First real guest runs establish R4-15 platform facts; remaining privileged gates are not established by compile-only or sandbox skips. | All supplied test/implementation handoffs; W7 gate |
| R3-L6 | A counting gate detects removed charges for existing syscalls, but cannot detect a newly added uncharged syscall. | TEST-rules mutation evidence; W7-C review |
| R3-L7 | Deployment scenarios use socket pairs rather than a real listener; production bind/connect/credential/service/reboot behavior needs VM execution. | TEST-e2e-deploy, W6-A3-fix; W6/W7 gates |
| R3-L8 | Consumer/freshness implementation superseded by R5-9/19; exact available windows now evaluate watcher rules. Real guest hot-growth/overflow gates remain separate. | W6-watch-windows, RECHECK-edge |
| R3-L9 | Series Flush unit acknowledgement cannot report a particular write error. R4-3 resolves catalog portability with relative bytes; the particular-write error seam remains open. | W6-A2a; writer/store/scan owners |

## Current hardening open risks (13 R5)

The first direct-state audit F01 and second-audit state/metrics trust findings have implementation fix notes; real second-user acceptance is pending. F04 symlink atime is an owner-accepted documented read-only limit, not an unresolved proposal; F05 deny is now required. See [security](../security.md) for retained root/same-euid, mount race and dependency limits.

| Risk / gate | Remaining work |
|---|---|
| Million-child TUI page >100 ms | Measure sort indexes versus snapshot size/write time before schema decision |
| Rocky 8.8 namespace blind spots | fd/mount misses database units with PrivateTmp=/ProtectSystem= and bind writes; status warns, 8.10 prefers filesystem marks |
| Descriptor pressure | Historical 64 KiB/NOFILE 1,024 exhausted fds and API ended daemon in 3/13 Rocky 8.8 runs; capped reads/retry landed, final stress acceptance pending |
| Aggregation cap health | Decide whether reached cap alone makes component degraded; facts already truncated/lossy |
| Frozen core wire key | HotEntryWire non-path keys are byte arrays, paths use path/path_b64; owner/core sign-off pending |
| Frozen cancellation method | scan.cancel cannot atomically condition on scan id; operator must avoid cancelling replacements |
| Window semantics | HotMinute has no top_unit; multi-minute files sum per-minute distinct counts |
| Performance / release | Owner staging 5M/20M, 72-hour soak, first nightly execution/cumulative 24-hour crash-free fuzz, final Rocky 8.8/8.10 guest gates |
| Guest security behavior | Real second-user state rules, filesystem marks under packaged restrictions, symlink boundary and external-link atime preservation |

W6-recheck and W7-C1-audit2 results are recorded only through their supplied reviews/fix notes, not anticipated or certified. Wave tags and tidemark-core sign-off are the owner’s.

## Release-candidate decisions and unresolved gates (13 R6)

Earlier scan-lock-only recovery and ancestry exception descriptions are superseded by R6-1/2/3: packages force strict-ancestry, refuse state-path symbolic links and both namespace/read-only ownership exceptions. Every Catalog/TimeSeries write handle retains a shared lease; recovery/replacement requires exclusive writer then scan ownership. Purge verifies every directory before descent and deletes lock names last; every successful flock checks named identity. These remain cooperative guarantees inside trusted root-only state.

R6-4/5 add shared 64 MiB watcher pathname/key admission, conservative fallback with reasons/exact measured event-byte totals, sixty-minute query history independent of persistence delivery, recoverable dirty caps, explicit shutdown before replacement and generic watcher health recovery. R6-6 makes unmappable filesystem events incomplete directory attribution rather than complete-looking root rows; mount-mode namespace/bind blindness remains on 8.8. Watches pin mounts until teardown (R6-7). R6-8 requires complete current-generation sampler health and invalidation before changed mount-set hash publication. No snapshot schema, core wire or normative unit/config block changes follow from these decisions.

| Open item | Evidence and next decision |
|---|---|
| Non-path HotEntryWire key encoding | Byte-number arrays remain frozen pending owner/tidemark-core sign-off (R6-12) |
| Reached aggregation-cap component policy | Truncation/loss facts exist; whether cap alone should degrade the component remains undecided |
| Quiet-host fd/filesystem CPU | One supplied 2,000-events/s run used 8.65% of one core; loaded host exceeded 10%; repeat on owner hardware |
| Owner hardware release runs | 72-hour soak and 5M/20M inode runs still outstanding |
| Slow guest compaction-crash cycle | No completed cycle; second witness/live-daemon oracle is landed, one cycle plus verification about 183 minutes |
| Independent review | Four rounds and per-finding fix notes in R6-11; no fifth report, so fourth-round fixes have not been independently re-reviewed |
| Remaining implementation evidence | RC-fix-tests/product notes absent; SAMPLER-reload/PURGE-lease report stale component-count assertion, CLI-format retains normalized Debug-helper guard exception, PKG-build-id leaves status identity/RPM VCS metadata follow-ups |

Retain the existing paging-index, cancellation, window-shape, cumulative fuzz, real second-user and restricted-unit guest gates. Supplied eight-of-eight Rocky 8.10 attribution is narrower evidence than performance/soak acceptance. Wave tags and core sign-off remain owner gates.
