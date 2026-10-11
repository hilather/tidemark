# Tidemark — disk-space debugging and history for Rocky Linux 8 (XFS on LVM on RAID 5)

*Architecture, design and agent-wave implementation plan · revised 2026-10-10 · "Tidemark" is a placeholder name.*

## Build or buy?

**Build.** No single open-source tool covers what these servers need (survey in [`00-existing-tools.md`](00-existing-tools.md)):

- live attribution of disk growth to directories and systemd units;
- a historical index with hard-link-correct "what would deleting this free" numbers;
- LVM thin-pool and Postgres WAL/slot visibility;

all gentle enough for a busy RAID 5 array on a 4.18-based kernel. duc, Robinhood, Diskover and bcc-tools each cover a piece. Tidemark borrows their best ideas, and they serve as stopgaps meanwhile.

## What Tidemark does

It tracks **both filesystems, `/` and `/data`**, separately. It is **strictly read-only**: no fstab, LVM, kernel or file-metadata changes.

| Layer | Answers | How | Cost |
|-------|---------|-----|------|
| **L0 Sampling** | "How full is each filesystem/volume, and how fast is it filling?" | `statvfs` every 30 s; LVM (`lvs`/`vgs`, read-only) every 5 min, including **thin-pool usage** and VG headroom | Negligible |
| **L1 Index** | "Where exactly is the space, what grew since yesterday, what would deleting X free?" | Scheduled, self-throttling scans in quiet windows (`getdents64`+`statx`, or **XFS bulkstat**, the Linux analogue of reading the NTFS MFT) into immutable SQLite snapshots: the history | Moderate, controlled |
| **L2 Live** | "Who is writing what right now? What's hidden?" | fanotify close-write events with PID → systemd unit; `/proc` scan for deleted-but-open and growing files; Postgres WAL/slots/relations (PGDATA is on `/data`) | Light (≈1% of a core) |

Two commands do most of the work during an incident:

- `tidemark now`: both filesystems, growth rate, time-to-full, LVM/thin pool, hot directories, writers, phantom files, Postgres.
- `tidemark diff --since 24h --explain`: walks down the path the growth came from.

## Deployment: staged by the hotfix pipeline, live after the next reboot

```bash
dnf install -y ./tidemark-<ver>-1.el8.x86_64.rpm
tidemark deploy stage --from ./tidemark.toml     # pre-flight checks; all-or-nothing; enables the service
# … hotfix reboot …
tidemark deploy verify --wait 5m                   # pipeline pass/fail from the first-boot health report
```

Nothing boot-critical is touched, so a bad deploy can't affect the reboot itself. `deploy unstage` backs it out ([`08`](08-interfaces.md) §5, [`11`](11-rollout-runbook.md)).

## Decisions worth knowing

1. **No XFS project quotas in v1.** They would give instant per-tree totals, but need mount changes, stamp every file, and make hard links and renames *across* tracked trees fail with `EXDEV`, which is unacceptable on servers that use hard links everywhere. The live watcher fills most of that gap. The design is parked in [`06-project-quotas-deferred.md`](06-project-quotas-deferred.md).
2. **Rocky 8.8 is out of support.** Rocky supports only the latest minor; 8.10 (kernel 4.18.0-553.x) is the final 8.x release. The design targets 4.18-based EL8 kernels and **probes features at runtime** (bulkstat version, fanotify modes).
3. **`ionice` may do nothing on this box.** The idle class needs a priority-aware scheduler such as BFQ, and EL8 commonly runs `mq-deadline`. Tidemark throttles itself from live disk utilisation and latency (followed through LVM down to the RAID disks), and opens directories with `O_NOATIME`, so scans can't cause atime write storms ([`04`](04-scanner.md) §2.2 and §6).
4. **Thin pools.** If `/data` is a thin LV, the pool can fill before `df` shows it. Tidemark watches pool data and metadata percentages and alerts on them.

## Contents

| File | Purpose |
|------|---------|
| [`00-existing-tools.md`](00-existing-tools.md) | Survey of existing tools, what we borrow, stopgaps for this week |
| [`01-requirements.md`](01-requirements.md) | Environment (root + `/data`, LVM, Postgres on `/data`), open questions, goals, FR/NFR IDs |
| [`02-architecture.md`](02-architecture.md) | Layers, components, crates, threading, security, degradation, deployment model |
| [`03-link-semantics.md`](03-link-semantics.md) | **Normative** hard-link/symlink size semantics with a worked golden example |
| [`04-scanner.md`](04-scanner.md) | Walk mode, bulk mode (bulkstat join), throttling, mode selection |
| [`05-storage.md`](05-storage.md) | Snapshot and time-series schemas, diff/explain, retention, ncdu export |
| [`06-project-quotas-deferred.md`](06-project-quotas-deferred.md) | Why project quotas were left out; reference design for a future add-on |
| [`07-live-monitoring.md`](07-live-monitoring.md) | FS + LVM sampling, fanotify watcher, `/proc` scanner, Postgres, alerts, `now` |
| [`08-interfaces.md`](08-interfaces.md) | CLI, daemon API, config file, metrics, **staged deployment**, systemd unit (contracts) |
| [`09-testing.md`](09-testing.md) | Fixtures, oracles, property tests, VM suite (incl. loop LVM and reboot tests), performance, soak |
| [`10-agent-playbook.md`](10-agent-playbook.md) | How to run the waves: roles, worktrees, ownership, reviews, gates |
| [`11-rollout-runbook.md`](11-rollout-runbook.md) | Pilot via hotfix deploy → tuning → fleet rollout, with abort criteria and rollback |
| [`12-decisions-and-risks.md`](12-decisions-and-risks.md) | ADRs and risk register |
| [`waves/`](waves/) | **Wave 0–7 implementation plans**: per task, owned paths, interfaces, steps, tests, acceptance criteria |
| [`agents/`](agents/) | `AGENTS.md` for the repo root, prompt templates (implementer, reviewer, gate, coordinator, SCR), handoff template |

## How to use this plan with coding agents

1. Create the repo and copy this folder to `docs/plan/`. Copy `agents/AGENTS.md` to the repo root ([`10`](10-agent-playbook.md) §2).
2. Answer the open questions in [`01`](01-requirements.md) §2. Stand up Rocky 8.8 **and** 8.10 VMs laid out like production (XFS `/` and `/data` on LVM) for the privileged test suite.
3. Run **Wave 0** with a single agent, then personally review the frozen contracts (`tidemark-core`). This is the highest-leverage review in the project.
4. Run each later wave with one implementer agent per task, a fresh reviewer agent per task, and a gate review. A human approves each gate and runs the VM and staging suites.
5. Ship after Wave 7's release gate; roll out with [`11`](11-rollout-runbook.md).

```
W0 Foundation ─▶ W1 Primitives ─┬─▶ W2 Scan engine ─┬─▶ W3 Queries ────────┐
   (1 agent)        (5 agents)  │      (3 agents)   └─▶ W4 LVM + deploy ───┤
                                │                         (2 agents)       │
                                └───────────────────────▶ W5 Live ─────────┴─▶ W6 Daemon+RPM ─▶ W7 TUI/perf/hardening/pilot
                                                           (4 agents)             (3 agents)        (4 agents)
```

Language: **Rust**. It gives predictable memory at tens of millions of inodes, memory safety for a root daemon, good syscall crates (`rustix`), bundled SQLite, and single binaries built against EL8's glibc. The rationale is in ADR-001.
