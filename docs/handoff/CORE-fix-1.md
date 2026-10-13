# Handoff — CORE-fix-1: daemon shutdown, reload admission and validation

## Needs from coordinator

Run the socket-dependent lifecycle suite in unrestricted CI and the existing privileged shutdown gate in a disposable Rocky guest. This is an evidence candidate, not a verified integration decision. Detailed final command results and regression evidence are recorded below after validation.

- Attempt: `attempt-441bd1e8a4380ea4ea5c3a17f100e2ca73c7784643278f40be1c927d380fb164`
- Base: `c7100a2f1d2fd8cf45ac990540a2c29b31505dba`
- Branch: the supplied `hp-441bd1e8a4380ea4ea5c3a17f100e2ca73c7784643278f40be1c927d380fb164`; no branch creation, merge or push.
- Status: implemented candidate; final checks recorded below.

## Per-finding changes

### B1

The lifecycle allocates one shared monotonic deadline, including startup-error cleanup, and passes it into the scheduler/recorder through a crate-private constructor. Existing public constructors/signatures remain unchanged. Recorder queue and acknowledgement waits poll in at most 50 ms slices, preserving the existing eight-second operation limits. They observe scan cancellation, daemon shutdown and the shared deadline. A message that never entered the queue is uncommitted; interruption after admission reports `persistence uncertain`. This includes DirSeries admission, Flush admission after series admission, and Flush acknowledgement. The API scan-start reply wait also observes shutdown/deadline.

Lifecycle joins every retained service handle even when its deadline expires, reporting a deadline error after joining rather than detaching. A scheduler job guard cancels/joins its scan on scheduler errors and unwinding as well as normal shutdown. Producer order, writer barrier, nine-second cleanup budget and the plan's ten-second overall bound are unchanged. These changes remove the recorder-specific 8+8/8+8+8 wait defect; they cannot interrupt an arbitrary blocked kernel/SQLite operation or fix the already-known service cancellation seams in other owned tasks. The unit test for expired cleanup deliberately proves no early return while a worker is alive; it is not evidence that uninterruptible workers meet ten seconds.

Tests: existing `callbacks_wait_for_persistence_and_propagate_writer_error` remains; new `recorder_interrupts_stalled_queue_and_uncertain_ack` covers both local cancel and global shutdown, `recorder_shared_deadline_interrupts_series_queue_and_barrier_ack` covers deadline expiry, `series_cancellation_interrupts_each_queue_and_ack_wait` covers all actual series waits, and `expired_shutdown_deadline_still_joins_worker` proves handle retention. Existing runtime cancellation and VM SIGTERM mid-scan gate are retained. The VM test's deadline/exit assertions now include daemon stderr.

### B2

`replace_config` computes a bounded candidate scan inventory before changing config/generation. It retires removed inactive configured roots, keeps running or admitted-but-not-yet-acknowledged scans, reserves every current root, and evicts inactive nonconfigured manual history deterministically when needed. If active reservations prevent admission, it returns an error without changing configuration or facts. Removed active roots retire on completion/release. All inventories remain at most 128 entries; the pending reservation is one slot, and retired keys are bounded by retained statuses.

The API and scheduled paths reserve admission before launch and release under the same active-scan mutex. Scheduled status publication and admission check configuration generation atomically, so an old pass cannot resurrect/launch a removed root. Capacity publication/admission errors are contained and reported without terminating the scheduler. Reload preflights catalog/history/schedule reads before replacement; errors flow through the existing `config.reload` warnings and `config` health. Rejection logs once (`reloaded=false`) in the common callback; SIGHUP no longer duplicates that log.

Tests: `exact_capacity_root_replacement_and_cumulative_churn_keep_scheduler_alive` replaces 128 roots and then churns through 140 one-root configurations with a live scheduler. `reload_inventory_failure_is_contained_and_active_removed_root_is_retired`, `configured_roots_reserve_capacity_against_inactive_manual_history`, and `pending_scan_reservation_survives_reload_and_stale_schedule_cannot_reappear` cover failed preflight, active-capacity rejection, history eviction, pending admission and stale-generation publication/admission. Existing `failure_health_survives_idle_other_root_and_reload_until_recovery` now asserts removed inactive statuses disappear, while preserving its failed-root health behavior.

### S9

Startup, reload and `--check-config` share daemon validation: core validation, daemon inventory bounds, scheduler schedule/window parsing and AlertEngine rule compilation. This validates downstream duration/rate/glob parsing before config publication without changing core contracts. The original oversized-schedule regression checks rejection through all three entry points, unchanged config/generation, warnings and config health.

### F12 (validation half)

Daemon validation rejects operational intervals greater than 366 days, inclusive at the boundary and rejecting even one additional nanosecond. Fields: throttle sample interval, sampler interval, LVM interval/timeout, procscan interval, metrics interval, alert renotify/for/resolve_for, and `every` scan intervals. Historical retention horizons are exempt: they are not runtime wait intervals, and the default hourly retention is 400 days. No exporter or core code changes.

Tests: `operational_intervals_have_inclusive_366_day_cap` checks all seven service interval fields plus scan schedules; `oversized_metrics_interval_is_rejected_before_startup_and_reload` checks the review's maximum-u64 scenario through check/startup/reload and unchanged generation/config.

### S6

`load_config` keeps its bounded, no-follow regular-file capture, then calls `Config::from_toml_bytes` on exactly the buffer it hashes. Reopen via `/proc/self/fd`, rewind, second read and comparison are removed. `--check-config` now uses the same bounded loader. `captured_config_bytes_determine_both_parse_and_hash` changes the inode contents after capture and proves both the parsed interval and digest come from the captured buffer; a later load sees the changed contents. This is deterministic captured-buffer evidence, not a claimed timed A→B→A exploit.

## Scope and compatibility

Only assigned daemon sources, permitted daemon tests and this note changed. No dependencies, manifests, module declarations, services, API/metrics source, frozen contracts or plan files changed. Public signatures remain unchanged; `run_controlled` alone uses the new crate-private scheduler deadline constructor. New state/scheduler helpers are crate-private. Preserved the neighboring daemon-scan storage-path/retry behavior, LIVE-hot publication interface and mount-presence guards. No privileged/destructive command or host system-directory write was performed.

## Tests updated and audit

Before each behavior change, searched workspace assertions for scheduling/reload/cancellation, extreme intervals, removed roots and shutdown/detachment. No existing test required old detachment, rereading or unlimited duration acceptance. Existing tests changed: `failure_health_survives_idle_other_root_and_reload_until_recovery` adds removal evidence; `daemon_reload_and_shutdown` adds stderr to deadline/exit assertions. The five known w6a1 readiness tests were read and retained; their scenarios still require real Unix sockets. All new integration tests avoid listening sockets.

## Assumptions and proposed plan clarification

- The task's explicit 366-day decision governs daemon operational intervals; historical retention remains governed by storage policy.
- Inactive manual status history may be evicted to reserve configured roots; active/admitted scans cannot be evicted.
- A cancelled or expired acknowledgement after queue admission is uncertain, not proof of rollback. Existing wire errors/payloads remain unchanged.
- The ten-second bound is not extended. Once a deadline expires, report an error but retain/join workers; known noninterruptible dependency waits still need their separately owned fixes/guest validation.

Proposed plan text (not applied): “Validate scheduler representations and alert predicates together with operational duration bounds before startup or config replacement. Daemon operational intervals must be at most 366 days. Reserve configured-root capacity transactionally against inactive manual history; removed admitted/running scans retain a slot until completion. Reject reload admission/inventory failures without changing configuration/generation or stopping the scheduler. Recorder waits observe cancellation and the shared shutdown deadline; acknowledgement interruption after admission reports uncertain persistence. Cleanup retains and joins worker handles.”

## Validation and regression evidence

Every Cargo/CI validation command below ran under `timeout 1500`, with pinned toolchain 1.98.0, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, and `CARGO_NET_OFFLINE=true`. No dependency was added. Artifact paths below are relative to this attempt's worker-output `library/` directory. Preliminary checks preceded the final race/capture refinements and are retained under `initial/`; the table refers to the final code/test tree.

| Command | Exit / outcome | Artifact |
|---|---|---|
| `cargo fmt --check` | 0 | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | 101: 579 passed, 5 failed, 13 existing scale ignores | `workspace-tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | 0; compiled only | `privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 | `rustdoc.log` |
| `tools/ci.sh` with offline environment | 1: source policy checks, fmt and Clippy passed; stopped at the same five w6a1 readiness failures; deny stage not reached | `ci.log` |

The only failing workspace target is `w6a1`: `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. These are the task's documented Unix-socket-denied readiness category; confirmation on an unrestricted host remains necessary. Existing socket/root-dependent tests that return early count as passed in Rust's summaries, not as executed operator scenarios. No privileged test ran.

Final daemon test counts: library 34/34 passed, w6a1 19 passed/5 failed, w6a2 19/19 passed. Baseline inventories are library 27, w6a1 20, w6a2 18, inferred from source/test declarations rather than an unmodified full baseline rerun. Twelve tests were added (seven library, four w6a1, one w6a2). Whole-workspace final inventory is 597 tests including 13 ignored scale tests; baseline inventory is inferred as 585 by subtracting those additions. `final-test-counts.json` records the final summaries.

### Counterfactual regression checks

`counterfactual.py` temporarily changed only the owned production source, ran the named test, and restored the exact saved contents in `finally` after each case. All six runs exited 101 with one expected test failure. `counterfactual-summary.log` records exits and restored SHA-256 hashes. The final required checks ran after restoration; no counterfactual source is committed.

| Finding | Removed behavior / failing test | Evidence |
|---|---|---|
| B1 queue/ACK | Disabled recorder stop observation; `recorder_interrupts_stalled_queue_and_uncertain_ack` fails its 500 ms completion bound | `B1-without-interruption.log` |
| B1 joining | Restored the baseline `join_until` detach-on-expiry implementation; `expired_shutdown_deadline_still_joins_worker` observes the worker still incomplete on return | `B1-without-join.log` |
| B2 | Removed the retirement/reservation transaction; the exact-128-startup replacement/churn test reaches its state deadline because the new configured root never gets a complete inventory slot | `B2-without-reservation.log`; positive `B2-positive-exact-start.log` |
| S9 | Restored the original daemon validator (before both consumer validation and the interval cap); `scheduler_consumer_validation_is_shared_by_check_startup_and_reload` sees check-config accept the original oversized schedule | `S9-without-consumer-validation.log` |
| F12 | Removed only interval-cap enforcement; `oversized_metrics_interval_is_rejected_before_startup_and_reload` sees check-config accept maximum-u64 metrics interval | `F12-without-cap.log` |
| S6 | Restored the actual old reopen/rewind/reread tail behind the loader's capture-boundary test hook; `captured_config_bytes_determine_both_parse_and_hash` gets `config changed during load; retry` instead of a coherent captured config/hash | `S6-without-captured-parse.log` |

S9's reviewed huge-duration scenario is also prevented by F12's later cap, so its counterfactual intentionally restores the original common validator. S6's mutation is deterministic at the real loader boundary and does not claim a timed A→B→A exploit. The new source-local series tests separately verify DirSeries queue, Flush queue, and Flush acknowledgement interruption. The capture hook is private; production supplies a no-op closure.

`test-audit.txt` records the workspace assertion search. Scope and whitespace checks passed; final clean/status and submission receipt are recorded in the worker report.

## Pending coordinator/guest validation

- Unrestricted host: `timeout 1500 cargo test --offline -p tidemarkd --test w6a1 --test w6a2 --test w6a3 -- --nocapture` (the five readiness tests plus socket-dependent API/operator paths).
- Disposable Rocky guest only: `TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture`.
- No new privileged fixture is needed for these metadata-independent queue/capacity/validation cases; the existing VM shutdown gate remains pending VM. Feature-enabled tests are compiled only here.

## Findings not done / remaining uncertainty

All five assigned findings have code and regression coverage in this candidate. F12 exporter overflow handling is explicitly excluded. Other core/edge review findings are excluded. Socket execution, real guest shutdown timing, arbitrary stalled syscall behavior and coordinator verification remain unestablished here.
