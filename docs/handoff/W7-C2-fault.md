# Handoff — W7-C2-fault: guest fault-injection suite

## Needs from coordinator

Run all six scripts on both disposable Rocky guests and review their transcripts;
all privileged execution is **pending VM**. Install/use shellcheck in CI: it is
absent here and the sandbox blocked downloading it. The real-compaction script
takes approximately four hours because the production writer's hourly timer has
no public trigger. Confirm that guest scratch storage has at least 10 GiB free.
**Likely product failure:** healthy daemon startup has no snapshot `.tmp` cleanup
in the inspected code; `02` deliberately retains the normative assertion and
may fail at its first killed active write. Route a product fix to its owner if
the guest reproduces this; this task does not alter product source.
This is an implementation/evidence candidate, not verified task or release success.

- Attempt: `attempt-88cae19a8c387e74d85e7337af0e6bba356792524750d7bb6188fb8ce53fd87c`.
- Task ID: `W7-C2-faultb`; owned deliverable handoff retains `W7-C2-fault.md`.
- Branch: precreated `hp-88cae19a8c387e74d85e7337af0e6bba356792524750d7bb6188fb8ce53fd87c`.
- Date: 2026-10-13. Status: implemented, guest validation pending.

## Existing coverage reviewed

| Fault row | Existing test | What it asserts | What it does not; disposition |
| --- | --- | --- | --- |
| Directory removed mid-scan | `tidemark-fs/tests/w1b/main.rs::deleted_queued_child_is_a_race`; scan `w2a::holes_errors_races_and_annotations` | Real removal after queued discovery becomes one race and zero errors; aggregator totals include injected races | No real CLI publication under background deletion. Added `05-directory-races.sh`. |
| Permission denied | CLI `e2e::scan_reports::scan_to_every_report`, fixture `e2e_scan.toml`, `e2e::support::Harness` | Real mode-000 directory; uses non-root uid 65534 when root can bypass DAC; real CLI exits 3, JSON complete/degraded/errors=1; persisted error path and catalog degradation asserted; no temporaries | Uses a temporary real filesystem rather than loop XFS, but meets the assigned real-binary/filesystem criterion. Good coverage; deliberately no duplicate script. |
| Disk full on state directory | store `storage::space_guard_decision_table_and_live_state_measurement`, `storage::drop_abort_collision_and_failed_write_never_publish`; daemon `w6a1` state-space tests | Guard decision table/live statvfs; injected writer refusal aborts; no publication; daemon state-space flag/alert unit paths | No real CLI+daemon ENOSPC fixture or full binary alert chain. Added `01-state-full.sh`. |
| Crash during snapshot write | store `storage::finish_cancels_at_every_publication_checkpoint`, `storage::catalog_selection_rebuild_and_nonutf8_file_paths`; CLI `e2e::scan_lifecycle::interrupted_cancelled_and_failed_scans` | Injected cancellation at durable boundaries; temporary cleanup/rebuild; real CLI SIGINT exits 3 and handles partial/failed catalog status | No SIGKILL with a real active writer and actual daemon startup recovery. Added `02-snapshot-crash.sh`. |
| Crash during time-series compaction | store `storage::compaction_last_values_idempotence_and_incomplete_buckets`; daemon `w6a1::compaction_uses_injected_clock` | Last value in completed buckets, zero changes on repeated compaction, retention, injected-clock scheduler barrier | No killed real daemon transaction, WAL recovery or second real hourly compaction. Added `04-compaction-crash.sh`. |
| fanotify overflow | live `watch::tests::kernel_overflow_worker_survives_and_resumes` (privileged), `w5a/privileged.rs` documents its location; rings/injected facts tests | Actual loop-XFS kernel queue, 100k distinct writes while worker lock held, overflow total/lossy flag, resumed events, worker healthy | Good real kernel/library test, but does not run product binaries or expose daemon JSON/metric publication. Added `03-fanotify-overflow.sh` to cover that boundary. |
| Huge directory | scan `w2b::vm::bulk_equals_walk_million_file_timing_smoke` (privileged scale), `w2a` simulated scale; store million-directory/link fixtures | Million files across 100 directories, scan equivalence, subprocess bulk RSS estimate; simulated aggregation/storage scale | No single million-entry directory through real CLI with configured RSS budget. Added `06-huge-directory.sh`. |

## What was built

Six guarded `tests/fault/*.sh` entrypoints run in the task's requested order.
`common.bash` supplies private config/state/API socket, an owned sparse 8-GiB loop
XFS image, bounded daemon readiness and child shutdown, diagnostic evidence and
EXIT/INT/TERM cleanup. Mounts are created with `mount -o loop`; cleanup unmounts
and explicitly detaches a loop if it has not already auto-detached. Cleanup
failure changes the final result to FAIL and retains the evidence directory.
No system daemon/service is queried or changed.

`check.py` checks public JSON, SQLite integrity, all required snapshot metadata,
complete status, finish ordering and absence of temporaries. The overflow total
is parsed from the documented Prometheus textfile because the frozen JSON DTO
has no cumulative counter field; lossy and recovery checks use JSON. Script
`03` pauses only its own daemon, creates 100,000 distinct files (avoids kernel
coalescing of writes to one inode), resumes it, and requires fresh activity in a
new directory in addition to overflow/lossy evidence.

`01` first reduces free space below the mandatory floor and requires daemon scan
exit 3 plus `state_space` alert. It then restores space, admits a direct scan,
observes a writer temporary, pauses its PID, fills the loop filesystem to ENOSPC,
and resumes; failed write must leave no `.tmp` or incomplete `.db`.

`02` creates 50,000 directories/files, observes writer startup before applying
12 fixed delays (20 ms–2 s), records which delays still observed `.tmp`, and
checks every kill independently. Each iteration starts/stops its private daemon
to exercise startup cleanup. At least one delay must hit a temporary; a complete
recovery scan must succeed.

`04` seeds 300,000 raw rows into the daemon-created schema while it is stopped.
An independent Python oracle expects exactly 1,000 five-minute rows, each with
the last raw value. It waits for the real hourly timer, observes WAL growth with
raw rows still uncommitted, delays 20/200 ms and rechecks uncommitted rows before
SIGKILL. Each killed DB must pass integrity checking. A restart must complete
compaction to the oracle; a subsequent actual hourly pass must leave the exact
historical set unchanged. It does not call a Python imitation of compaction or
change clocks. Each long phase has an external timeout and internal deadline.

`05` deletes subtrees in reverse name order while a one-worker scanner processes
the fixture; it requires positive races and complete publication. `06` creates
50,000 (or scale-marker 1,000,000) empty files, samples VmRSS/VmHWM on the direct
CLI PID, requires actual samples and peak <= configured 1280 MiB, and checks the
exact completed count. `xtask/tests/w7c.rs` validates every entrypoint's syntax,
VM/root markers and trap delegation, and conditionally runs `shellcheck -x` with
both stdout/stderr in failure messages.

## Contract checks and doubts

- All used global/command flags are implemented: `-c`/`--config`, `--json`,
  `scan ROOT --mode walk --throttle max`, `status`, `now`, `alerts`; daemon
  `--config` and `--check-config`. A real small unprivileged scan returned exit
  0 with `tidemark.scan.v1`, `data.status`, `data.summary.files/races`, and all
  snapshot metadata. Actual daemon config validation accepted watch enabled,
  watch disabled, and the 24-hour interval compaction configuration.
- `StatusResult.watch_modes` uses `fd_mount`, `dfid_name_mount`,
  `dfid_name_filesystem` (daemon state conversion, not WatchMode Display).
  `HotEntry` encodes absolute directory keys as top-level `path`; its `events`
  count proves fresh watcher recovery activity. `NowReport.mounts` exposes
  `lossy_minutes_15m`. Alert summaries expose `rule`.
- The free-space guard has a mandatory **2-GiB floor** independent of
  `min_free_pct`; a 512-MiB image would prevent every successful scan. All scripts
  therefore use a sparse 8-GiB filesystem and `01` deliberately fills it. This
  is a documented fixture-size choice, not a change to the product guard.
- A direct preflight NoSpace maps to exit 5, but a daemon scan's unavailable or
  failed persisted outcome maps to exit 3. The first state-space check uses the
  daemon and requires 3; mid-write direct ENOSPC can be runtime exit 1 or space
  precondition exit 5. Both must fail and clean up. Guest behavior still needs
  confirmation; the expected alert is asserted even if the current product
  fails to publish it.
- Inspected startup (`tidemarkd/src/lib.rs` catalog open/recovery) performs
  corruption-only recovery; `Catalog::open` does not remove orphan snapshot
  temporaries, and `rebuild_from_files` explicitly ignores them. No `.tmp`
  cleanup was found in daemon startup. Thus the next-start cleanup row appears
  unimplemented on this baseline. The script asserts it as specified and must
  not manually delete the evidence or force corrupt-catalog recovery to pass.
- Snapshot `.tmp` timing and WAL spill timing depend on guest speed. Scripts
  fail if they cannot demonstrate a write hit; a scheduling miss must not be
  reported as crash acceptance. A commit can still race the final uncommitted
  observation and SIGKILL; this unavoidable external-observation gap is not a
  proof of instruction-level transaction interruption.
- Compaction has no CLI/API trigger and begins 3600 monotonic seconds after
  writer startup (`services/tswriter.rs`). The test costs about four hours and
  may fail if the fixture completes before the delayed kill. Do not shorten it
  by changing the guest system clock or product source. An eventual reviewed
  product test seam could shorten this gate, outside this task's ownership.
- The daemon watcher consumer still documents exact CompletedMinute window
  unavailability despite `LIVE-minutes` library support. This suite checks the
  existing now lossy count/cumulative metric; it does not claim exact alert
  window integration is implemented or verified.
- Directory removal is inherently schedule-sensitive. Positive races are a
  mandatory assertion; if a guest finishes discovery/deletion without them,
  the script fails instead of silently accepting a no-fault scan.
- RSS sampling includes VmHWM (kernel lifetime peak), mitigating sample misses;
  `/proc` disappearance at exit is allowed. No RSS samples is a failure.

## Guest commands

From the workstation (dedicated guest checkout; repeat for rocky88):

```sh
rsync -a --exclude target --exclude .git ./ tester@rocky810:tidemark-vm-test/
ssh tester@rocky810 'cd tidemark-vm-test && cargo build --offline --workspace --features privileged'
cargo xtask vm-test --host tester@rocky810 --remote-dir tidemark-vm-test --suite fault --env TIDEMARK_DEPLOY_TEST_VM=1
cargo xtask vm-test --host tester@rocky810 --remote-dir tidemark-vm-test --suite fault --env TIDEMARK_DEPLOY_TEST_VM=1 --env TIDEMARK_FAULT_SCALE=1
```

For individual evidence inside that disposable guest checkout:

```sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 bash tests/fault/01-state-full.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 bash tests/fault/02-snapshot-crash.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 bash tests/fault/03-fanotify-overflow.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 bash tests/fault/04-compaction-crash.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 bash tests/fault/05-directory-races.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_SCALE=1 bash tests/fault/06-huge-directory.sh
cargo test --offline -p tidemark-cli --test e2e scan_reports::scan_to_every_report -- --exact --nocapture
```

Guest execution must occur in PID 1's mount namespace; require Bash, python3 with
SQLite, coreutils, util-linux, xfsprogs, loop devices and root capabilities.
The runner passes PATH through sudo and never builds script suites. Do not use
its cargo-only `--no-fail-fast`/`--nocapture` options for the fault suite.

## Assumptions

- Keep the precreated attempt branch; no branch creation, merge or push.
- No user questions; fixture sizes, fixed delays and timeout budgets chosen from
  code behavior and documented contracts. No product or plan changes authorized.
- Privileged scripts are compiled/static-reviewed here and run only in guests.
- A real temporary filesystem CLI test is enough to avoid duplicating permission
  denial; XFS-specific permission behavior is not newly claimed.
- Root-only API policy is preserved. Every config/socket/state is private; no
  `tidemark` group, default service/socket, PostgreSQL work or system clock change.

## Shared-file edits and dependencies

None. Only the assigned `tests/fault/**`, `xtask/tests/w7c.rs` and this handoff
are edited. No new dependencies or unsafe code.

## Validation

All Cargo commands used the pinned 1.98.0 toolchain, host offline cache and
`CARGO_NET_OFFLINE=true` for CI. Logs are retained in the attempt output
`library/` directory. No privileged script, mount, mkfs, losetup, sudo or service
operation ran on this workstation.

| Check | Result |
| --- | --- |
| `bash -n` on six entrypoints and `common.bash` | Passed (final source) |
| Python AST parse of both helpers | Passed |
| `cargo fmt --check` | Passed |
| `cargo clippy --offline -p xtask --all-targets -- -D warnings` | Passed |
| `cargo test -p xtask --offline` | Passed: 32 tests, including w7c |
| Final focused `cargo test --offline -p xtask --test w7c` | Passed after final shell changes |
| `cargo build --offline --workspace --features privileged` | Passed; binary build only |
| Real small unprivileged CLI scan | Exit 0; JSON/count, full metadata and no-temporary validators passed |
| Partial-publication mutation of that snapshot | Correctly rejected by validator |
| Actual daemon `--check-config` | Watch on/off and hourly-compaction test config passed |
| Compaction seed against actual product-created DB schema | Passed: 300,000 raw rows, 1,000 independent oracle buckets; integrity ok |
| `CARGO_NET_OFFLINE=true tools/ci.sh` (final Rust/test source) | Exit 1: source policy, fmt and workspace Clippy passed; 559 harness passes, 5 known failures, 13 ignored before stopping |
| Shellcheck default severity | Not executed: tool absent; attempted download blocked by sandbox domain allowlist. Conditional test remains pending CI |
| `cargo clean` | Passed after all builds/checks; build artifacts removed |
| All six guest fault scripts | **Pending VM**, no runtime acceptance claimed |

The five CI failures are `w6a1::binary_ready_and_stopping`,
`corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`,
`private_and_missing_state_directories_start`, and
`startup_reads_last_complete_from_catalog`. Four logs explicitly report the API
socket's EPERM and the binary readiness test hits its deadline. CI stopped at
this target, before its privileged test-build and cargo-deny stages; the
separate privileged binary build above passed. No new failing target appeared.
Final cosmetic shell changes (quiet recovery status, expected `/proc` exit race)
were subsequently checked by bash syntax and the focused w7c test.

## Definition of Done checkpoint

- [x] Only owned paths changed; no shared-file/dependency edits.
- [x] Format and xtask Clippy clean; all xtask tests pass.
- [x] Six guest scripts, runner README, coverage table and handoff provided.
- [x] Real flags/config/scan JSON and compaction fixture schema checked locally.
- [x] Build artifacts cleaned; committed and submitted through canonical spool.
- [ ] Workspace CI green: five known socket restrictions remain.
- [ ] Shellcheck acceptance: pending CI with tool installed.
- [ ] Fault suite green on both VMs: pending coordinator execution.

No release acceptance or verified task-success claim is made.
