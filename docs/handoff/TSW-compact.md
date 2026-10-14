# Handoff — TSW-compact: hourly writer compaction

## Needs from coordinator

Rerun fault 04 in the disposable Rocky 8.10 guest and run the socket-enabled uid 1000/umask 022 gate. The exact historical guest miss remains unresolved: the demonstrated 44-second drift alone does not explain the supplied deadline miss. This is an evidence candidate, not a verified guest fix.

- Branch: precreated attempt branch `hp-e38588288852a3b54201f8aa00f8a51f587ad6fb9e1b7b0d2eb2a1f17f85cac3`.
- Session: `attempt-e38588288852a3b54201f8aa00f8a51f587ad6fb9e1b7b0d2eb2a1f17f85cac3`.
- Date: 2026-10-14.
- Status: implementation and local evidence submitted for independent review; guest confirmation pending.

## Cause: established versus inferred

Established from the original writer and regression: re-arming at completion adds compaction duration to the next deadline. With an injected 44 seconds elapsing between the first due check and completion, the second witness still has 600 raw rows at elapsed second 7200. The committed reproduction (`ee0fdc4`) ran two tests: one passed, one failed at the second witness. The same tests pass after the change and drive a third compaction at 10800.

Established from the original supervisor: worker replacement initialized a fresh `compact_at` inside `writer_loop`; the supervisor published component health but emitted no tracing WARN. A replacement therefore silently postponed the next compaction by up to another hour. The new panic/restart test checks that a replacement at elapsed 3005 still compacts at 3600, not 6605. A silent restart is a plausible explanation for the guest observation, **not an observed cause** of that run. Its component health and thread history were not supplied.

Established by exact-row comparisons: a long-lived writer sees external SQLite commits on subsequent compactions. The tests copy the oracle's SQL statement, 600 successive second timestamps aligned 72 hours ago, TEXT mount parameter, and gauge values (2000000, n, 700000, 2000, 200). Each becomes exactly the two last-value five-minute rows. The `INSERT OR IGNORE ... NOT EXISTS` SQL handles those rows correctly; no store/schema change is justified.

Operational deadlines use one shared monotonic clock instance; wall time supplies retention cutoffs only. `monotonic_clock()` retains an Instant origin in its shared closure; the supervisor clones that instance. Wall-time changes before a deadline do not trigger maintenance in the tests. A deliberately invalid wall cutoff produces one ERROR and the writer successfully compacts the next interval. Compaction errors previously logged through the existing writer failure path and re-armed at completion; they did not themselves restart the writer. A backward wall correction can leave recently expired rows ineligible until wall time catches up; it does not suppress scheduling.

Other delay paths remain: pending SQLite work/locks, filesystem I/O, state-lock contention, thread scheduling, and supervisor reopen/backoff can delay the due check. Queue work does not skip it (one message plus bounded pending batch is processed per iteration). A disconnected queue ends the worker and is supervised; shutdown exits without a restart. No local evidence proves a permanent SQLite snapshot, SQL filtering error, or clock reset caused the guest failure. With the supplied first duration of at most 44 seconds, cadence drift alone should still have fit the old guest margin.

## What changed

The starter/supervisor retains an atomic next deadline shared with successive workers. Each due attempt records its start plus 3600 **before** compaction, preserving it even through a panic during maintenance. Overdue intervals coalesce into one attempt rather than an unbounded catch-up loop. Successful commits emit one INFO with `duration_ms`, `inserted`, and `deleted`. Failures retain the existing ERROR path. Each unexpected worker termination emits one WARN containing its cause before reopen/backoff.

Both oracle verification waits now allow 4200 seconds, and both corresponding shell helper deadlines allow 4300 seconds. Kill-phase helper timing remains 3800 seconds; the displayed aggregate deadline budget is conservatively 4300 per phase. The cadence fix removes duration drift but cannot guarantee completion inside a 100-second margin under I/O stalls or restart backoff, so the wait was widened as requested.

## Tests

New tests live in `crates/tidemarkd/tests/tscompact/mod.rs`, included as a private unit module by the writer so they can use the existing private supervisor seam without expanding the production API.

- `two_compactions_see_external_witnesses`: two compactions at 3600 and 7200; verifies 600 rows remain raw one second before each deadline, then exact bucket values at the deadline. Passes on the original code with zero simulated duration.
- `three_compactions_do_not_drift_by_duration`: same checks at 3600, 7200, 10800, with 44 seconds injected after the first due clock read. Fails on original code at the second witness; passes with start-based re-arming.
- `restart_preserves_deadline_and_logs_compaction_results`: isolated tracing subscriber (as existing daemon tests do), panic at 3000, clock-read acknowledgment for the five-second backoff, restart at 3005, raw witness at 3599 and compaction at 3600. Checks one cause-bearing WARN, two INFO completions with duration and exact 2 inserted/600 deleted counts, and one ERROR for cutoff overflow at 7200 followed by successful compaction at 10800.

Tests use independent Python sqlite3 connections, matching the guest oracle's SQLite client and avoiding a new dependency or manifest edit. New configs clear roots and use private temp state; no sockets, ownership privilege, real scans, or privileged helpers are required. Log capture runs in a child test process to avoid tracing callsite-cache interference.

## Validation

Validation results and counts are recorded below after command completion. Full logs are in this attempt's `library/` output directory. All long commands use `timeout 1500`, pinned Rust 1.98.0, the host Cargo cache, and offline mode. Initial fmt/clippy invocations failed because rustup defaulted to the isolated read-only home; corrected runs set `RUSTUP_HOME=/home/brewerm/.rustup`.

| Check | Result |
|---|---|
| `cargo fmt --check` | exit 0 (`fmt-final.log`) |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 (`clippy-final.log`) |
| `cargo test --workspace --no-fail-fast` | exit 0; no failed tests (`tests.log`) |
| `cargo build --workspace --features privileged --tests` | exit 0; compile only (`privileged-build.log`) |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | exit 0 (`docs.log`) |
| `tools/ci.sh` | exit 0; all available checks passed (`ci-final.log`); cargo-deny not installed, skipped |
| Shell/Python helper syntax | `bash -n` and `ast.parse` passed; neither helper executed |

Counts: daemon library baseline had 122 tests; final has 125, all passing in the workspace run. The initial regression-only run had 1 pass/1 failure; final focused run had 3 passes/0 failures. Workspace output contains 955 printed passing results, 0 failures, 14 ignored across 86 summaries, **including nested child test-process summaries**, so this is not a count of unique workspace tests. No separate full baseline workspace run was performed. No expected sandbox failure needed a source change; the requested workspace suite and CI both returned zero here.

The wrapper's first run also failed on the rustup home setup; `ci-final.log` is the corrected result. `checks.json` retains the initial runner's results rather than overwriting environmental failures; final fmt/clippy/CI exits are recorded separately in `*-final.exit`.

- [x] Owned paths only; no dependencies or shared manifests changed.
- [x] Formatting, Clippy, workspace tests, documentation, and available CI checks passed locally.
- [x] Privileged-feature tests compile; execution and fault 04 remain pending VM.
- [x] Regression evidence and assumptions recorded.
- [ ] Exact historical guest cause and real hourly rerun independently confirmed.

## Guest rerun (pending VM)

In the disposable guest, build as its normal SSH user with `cargo build --offline --workspace --features privileged`. From that checkout, as root in the disposable guest:

```sh
TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_SLOW=1 TIDEMARK_FAULT_CYCLES=1 bash tests/fault/04-compaction-crash.sh
```

Keep the daemon logs and inspect the new INFO/WARN timestamps and counts. No privileged tests or fault scripts were executed by this worker. Existing privileged tests are only compiled; no new privileged test is needed for the injected-clock behavior.

## Proposed plan text

For `05` §6.1 and wave 6 ts-writer behavior: “The first compaction is due one hour after writer service startup; subsequent attempts are due one hour after the previous attempt started. The supervisor retains the deadline through worker replacement. Missed intervals coalesce into one attempt when the writer becomes available. Each successful compaction logs INFO with duration and inserted/deleted row counts; failure logs ERROR; unexpected writer termination logs WARN with its cause. Wall time controls retention cutoffs, while monotonic time controls the cadence.”

## Assumptions and deviations

- Retained scope/task overrides the older wave ownership and branch conventions. No branch creation, merge, push, contract change, manifest edit, or new dependency.
- Python 3 with sqlite3 is available for tests, as it is for the existing fault oracle. Separate mount keys prevent witness collisions; the restart test uses the exact oracle mount spelling.
- The injected first due clock read advances elapsed time by 44 seconds while returning the start instant. This models an operation consuming time before the old completion-time re-arm without sleeping or adding a production hook.
- Failed compactions consume their hourly attempt; they retry at the next start-based deadline, matching the former hourly error retry policy. A process restart still begins a fresh service schedule; only in-process writer replacement retains it.
- Historical causality and real-time guest completion remain uncertain until the requested rerun. No claim that broader blocking I/O is bounded by the new wait.

## Shared-file edits

None. The writer's existing owner header remains W6-A1; the new test source is tagged TSW-compact. Store code is unchanged.
