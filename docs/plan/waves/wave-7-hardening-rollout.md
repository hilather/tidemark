# Wave 7 — TUI, performance, hardening, docs and pilot

**Goal:** make Tidemark pleasant (TUI), prove it is fast and gentle on real hardware (performance gates), make it robust and safe (fuzzing, fault injection, security review), and ready it for production (operator docs, runbooks, pilot).

**Agents:** 4 in parallel. **Entry:** `wave-6-done`. W7-B and W7-D need staging access arranged by the human gatekeeper.

| Task | Title | Owned paths | Depends on |
|------|-------|-------------|------------|
| W7-A | TUI browser | `tidemark-cli/src/cmd/tui.rs`, `tidemark-cli/src/tui/**` | W2, W3 |
| W7-B | Performance, scale, tuning | `xtask/src/{gen_fixture,bench}.rs`, `docs/perf/**`, tuning changes via SCR | W6 |
| W7-C | Hardening, fuzzing, security review | `fuzz/**`, `tests/fault/**`, `docs/security.md`; fixes via small follow-up tasks | W6 |
| W7-D | Operator docs, runbooks, man pages, pilot kit | `docs/operator-guide.md`, `docs/runbooks/**`, `xtask/src/man.rs` (man pages), `docs/pilot/**` | W6 |

**Merge order:** any. Fixes found by W7-B or W7-C go into **separately reviewed follow-up branches** owned by the original module's path list (`W7-FIX-<n>`), never mixed into the investigation branches.

---

### Gate-review contracts to carry into hardening and rollout

Read 13 R4-1–R4-16 and the first-real-run risks in 12. Owner decision: fd-mode watcher budget is 5% of one core per 1,000 close-write events/s, or 50 microseconds/event, replacing 1%. EL8 kernels 4.18.0-477 and -553 offer no measured FID mode. Optimised 8.10 guest measured 45.6 microseconds/event at 2,000/s (9.1% of one core) with host under load. Owner will measure real servers; this is no universal performance guarantee. The 2,000/s scale test threshold is 10% of one core, replacing 2%. Idle CPU and write-throughput impact remain separate gates.

Open, not new accepted behavior: watcher-backed alert rules remain unevaluable until daemon publishes exact CompletedMinute windows (R4-7); W6-A2b alert reconfiguration continuity, component removal and cancellable process scan; mount rate-ring reset/current now.degraded (R4-9); one intermittent cancellation deadline miss on 8.8 in scan_lifecycle::interrupted_cancelled_and_failed_scans; firstboot report rewritten on every start with changed config; LVM schema/cancel-id gaps (R4-12).

Review findings without landed fix notes: core S1 writer rollover failure/supervision, S2 stale collector alert availability, S3 procscan published-lock contention, S4 LVM retry units/backpressure, S5 wall-clock operational deadlines, S7 late sampler generation resurrection, S8 internal alert capacity; edge F02 incomplete-request API deadline, F03 abandoned handler cancellation, F04 mutable API parent path operations. B1/B2/S6/S9 have CORE-fix-1; F05–F08 EDGE-cli; F09–F12 EDGE-metrics plus CORE-fix-1 validation; F13 INSTALL-assert. F01 has an EDGE-metrics fix note but HTTP removal/ADR-020 is separately assigned and not resolved by this plan task. Landed notes establish implementation evidence, not independent verification; unrestricted socket tests and guest gates remain required.

W7-D preserves byte-relative catalog portability, explicit invalid reload warnings and CLI scan-id ownership/exit 3. W7-C verifies shared recorder deadlines, exact hashed-byte parsing and 366-day validation, actual retained watcher mount identity and mount transition state. W7-B measures the revised watcher budget on real servers independently of idle CPU and writer impact.

Read 13 R3-1–R3-15 and 12’s open risks before implementing or claiming gate acceptance. Preserve uid-0-only SO_PEERCRED authorization, labelled CLI status configuration, reduced daemon LVM fields, direct fallback notices and daemon scan override refusal. Watcher rules evaluate only supported whole-minute 1–60 windows with boundary samples; hot data is per-minute top lists and lossy/truncated windows are unavailable. Current A2b producer/state seams leave all watcher-rule measurements unavailable. Growth is computed in scan_finished before dir_series and retention; potentially truncated growth is unavailable. Do not pre-lock run_scan or equate queueing with persistence.

W7-A/D preserve indexed notable-file ID paging and diff From/To headers plus the existing JSON notice, without schema or payload extensions. W7-C checks capability-based O_NOATIME fallback (uid 0 alone proves nothing), independently reviews syscall charging beyond counting-gate tests, and tests real peer credentials/listener behavior. W7-D explains stage-only local-state eligibility and blocking-preflight versus runtime compensation; unstage/purge remain usable on ineligible storage. W7-B/C/D must resolve or report 12’s open limitations, including finish timestamp, leaf no-follow, LVM cancellation, current-run alert history and duplicate deployment helper. R4-15 records first-run platform evidence; execute remaining Rocky/XFS/fanotify/service/reboot gates on disposable VMs and retain results rather than treating compile-only or socket-pair tests as acceptance.

## W7-A — TUI browser

**Read first:** `03-link-semantics.md` §7, `05-storage.md` §3, `08-interfaces.md` (`tui`).

### Behaviour

- Opens `tidemark tui [PATH] [--snapshot ID]` on the latest snapshot of the root containing PATH.
- **Main list:** children of the current directory, plus an `[own files]` row. Columns:
  - Name (`/` suffix for directories);
  - Share bar (by `t_charged`, so bars add up);
  - SIZE (`t_disk`), RECLAIM, SHARED with an `H` marker;
  - FILES, NEWEST;
  - Δ against the previous snapshot (when one exists; computed per visible row via path lookups).
- **Keys:**

  | Key | Action |
  |-----|--------|
  | `↑↓ PgUp PgDn Home End` | Move |
  | `Enter / →` | Descend |
  | `Backspace / ←` | Ascend |
  | `s` | Cycle sort (size, reclaim, files, newest, growth, name) |
  | `i` | Info pane: path, counts, age histogram, link totals, annotations, errors |
  | `l` | Top link sets in the subtree, with an instance-paths drill-in |
  | `f` | Top files in the subtree |
  | `R` | **Live refresh** of the current directory: gated walk-mode rescan into a `VecSink`; shows live values with Δ against the snapshot, marked `live` |
  | `p` | Snapshot picker (switches snapshot, keeping the path) |
  | `/` | Filter children by substring |
  | `?` | Help |
  | `q` | Quit |

- **Read-only.** There are no delete, move or open actions, by design (`01` non-goals).
- Large directories page their children 1,000 at a time from SQL. Terminal resize, 80×24 minimum, no colour when `NO_COLOR` is set, works over SSH.

### Tests

- State-machine unit tests (navigation, sort cycling, filter).
- Render snapshots via ratatui's `TestBackend` for a fixture snapshot, at 80×24 and 160×48.
- A live-refresh test on a fixture directory mutated after the snapshot.

### Acceptance

- [ ] Browsing a 1M-directory synthetic snapshot stays responsive (< 100 ms per navigation step, recorded).

---

## W7-B — Performance, scale, tuning

**Read first:** `04-scanner.md` §6–7 and §10, `09-testing.md` §7, `01-requirements.md` NFR-2 and NFR-4.

### Deliverables

- **`xtask gen-fixture --profile reports --inodes N --root DIR [--seed S]`**:
  - Report directories with 10k–60k files.
  - Log-normal sizes (median 32 KiB, long tail to 2 GiB).
  - 20% of files hard-linked into 2–4 other report directories.
  - 10% symlinks (half to directories).
  - A few 100k-entry directories.
  - A mutate mode (`--mutate-rate`) that creates, appends, links and deletes continuously, for the soak.
- **`xtask bench`**: as in `09` §7. Results go to `docs/perf/results-<date>-<host>.md` with raw JSON alongside.
- **Profiling:** `perf record` + flamegraphs of walk, bulk and aggregation at 5M and 20M inodes, plus a short write-up of the top three hotspots and what was done about them.
- **Tuning proposals** (via SCR, then applied):
  - Default `threads`;
  - `getdents_buf`, `bulk_batch`;
  - Cost weights;
  - `gentle`/`normal` profile values;
  - `auto` mode thresholds;
  - Whether to use `mimalloc` (measure before and after).
- **Memory verification** at 20M inodes for both modes against `memory_budget`. If bulk mode doesn't fit the default budget at the measured production scale, implement the **AG-partitioned spill join** (`04` §3.2 variant: write `DentRef`s per AG to temp files, then process one AG at a time) as `W7-FIX-spill`.

### Gates (NFR-2, NFR-4)

- [ ] Cold-cache scan ≥ 3× faster than `du -sx` with the throttle off (or a documented, agreed revised target).
- [ ] With the `normal` profile during a scan, the leaf block devices' utilisation and average wait (from `/sys/block/*/stat`, using the adaptive gate's samples) stay within the profile's configured targets, apart from short excursions the controller corrects.
- [ ] Watcher overhead measured by comparing fixture-generator throughput in mutate mode with and without the watcher.
- [ ] Peak RSS ≤ `memory_budget` at the target scale.
- [ ] Daemon steady-state RSS ≤ 300 MiB and idle CPU ≤ 2% of one core outside scans; active watcher CPU ≤ 5% per 1,000 close-write events/s (R4-8) (72 h soak, below).

### 72-hour soak (with W7-D)

On staging: daemon at defaults, generator in mutate mode, hourly scans. Pass criteria are in `09` §8. Graphs and a summary go in `docs/perf/soak-<date>.md`.

---

## W7-C — Hardening, fuzzing, security review

**Read first:** `02-architecture.md` §8, `09-testing.md` §6, all of `tidemark-sys`.

### Deliverables

- **`cargo-fuzz` targets:**
  - `parse_mountinfo`, `parse_disk_stat`, `parse_cgroup_unit`, `parse_fdinfo_flags`, `parse_stat_starttime`;
  - the `lvs`/`vgs` JSON parsers (W4-A);
  - `staged.json`/`firstboot.json` readers (W4-B);
  - the fanotify event-buffer parser (W5-A);
  - the API request parser;
  - `Config` TOML loading.

  Seed corpora come from the test samples. Nightly CI runs 10 minutes each; there must be **zero crashes over 24 h cumulative** before v1.0.
- **Fault-injection suite** (`tests/fault/`): every row of `09` §6 not already covered, run in the VM suite.
- **`unsafe` audit:** review every `unsafe` block in `tidemark-sys` against its `SAFETY` comment. Run the pure parsers under Miri where possible. Findings go in `docs/security.md`.
- **Content-read audit:** prove that no code path reads regular-file contents.
  - A test wraps the walker, bulk, procscan and watcher in an `strace -f -e trace=read,pread64,readv,openat` run in the VM.
  - It asserts that `read*` calls on fds opened for regular files are absent. Fanotify event fds are closed unread.
- **`docs/security.md`, a threat model covering:**
  - The root daemon and its capability set.
  - Root-only CLI/API access, sudo guidance, and root-owned file and directory modes (ADR-018).
  - Sensitivity of path names in snapshots and metrics.
  - Data sent in webhooks (escaped paths/subjects and documented fields; no separate v1 redaction key or rule, 13 R2-10).
  - The read-only guarantee (no metadata, mount or LVM writes) and the `xtask ci` forbidden-call check.
  - `deploy stage`/`unstage` safety (pre-flight failures change nothing; later failures attempt compensation; unsafe-state refusal and no-follow handles; purge guard, 13 R2-7).
  - The SELinux posture (no shipped policy or required SELinux mode; 8.10 enforcing first-run evidence with no denials, R4-15).
  - Supply chain (`cargo-deny`, pinned toolchain, reproducible container build).
- **Dependency review:** `cargo deny check` clean; an explanation for each crate with `unsafe` in its dependency tree that touches our data paths.

Review the implemented contracts in `13` R2-1–10: record/resource refusals, checked live name reads, removed-path diff, publication cancellation, foreground-only event drains, PID reuse and not-evaluable alerts. Corruption fault tests must call recover_catalog only after reported corruption with connections closed; healthy startup is non-destructive.

### Acceptance

- [ ] Fuzzing gate met; fault suite green on both VMs; `security.md` reviewed by a human.

---

## W7-D — Operator docs, runbooks, man pages, pilot kit

**Read first:** everything in `docs/plan/`, especially `07`, `08` §5 and `11`.

### Deliverables

- **`docs/operator-guide.md`:**
  - Install;
  - First run (`probe`, then editing config from its recommendations);
  - Reading `now`, `top`, `diff --explain`, `links`, `lvm` and `phantom`;
  - What the size columns mean (with the `03` §6 example);
  - Alerts and tuning;
  - Troubleshooting (daemon won't start; watcher in fd mode; "daemon unavailable"; scans slow).
- **`docs/runbooks/`**, each with symptoms → commands → interpretation → actions → escalation:
  - `disk-filling.md` (`now` → `hot` → `diff --explain` → `lvm`)
  - `df-du-mismatch.md` (`phantom` for deleted-but-open files, `writers` for growing open files, then compare `now` with the latest snapshot)
  - `scan-impact.md` (throttle profiles, windows, cancelling)
  - `watcher-degraded.md` (namespace, overflow, fd mode)
  - `thin-pool-filling.md` (`lvm` → which thin LVs grow → `now`/`hot` on their mounts → extend the pool or free space)
  - `deploy-verify-failed.md` (read `deploy verify --json` → `tidemark status` → journal → fix config → re-stage or unstage)
- **Man pages:** `tidemark(1)` and `tidemarkd(8)` generated by `cargo xtask man`, which calls the `clap` command builders exported from the `tidemark-cli`/`tidemarkd` lib targets and renders them with `clap_mangen`. Shipped in the RPM (a W6-C follow-up adds the files to `rpm.toml`).
- **`docs/pilot/`:** the pilot checklist from `11` as a fill-in form; a sign-off template; and a baseline-capture script (`iostat -x`, existing workload latency, `df`, `du` on sample subtrees) to run before and after each phase.
- **Rehearse `11-rollout-runbook.md`** phases P0–P5 on staging and correct the runbook where reality differs.

### Acceptance

- [ ] A colleague who didn't build it follows the operator guide on staging from RPM install to an answered "what grew yesterday" question without help (recorded).

---

## Wave 7 gate checklist (release gate for v1.0.0)

- [ ] W7-B performance and soak gates met, or revised targets agreed in writing.
- [ ] W7-C fuzzing and fault gates met; security review signed off.
- [ ] W7-D docs reviewed; runbook rehearsed on staging.
- [ ] All `W7-FIX-*` branches merged and reviewed.
- [ ] Tag `v1.0.0`; build the RPM; hand off to the rollout runbook (`11`).
