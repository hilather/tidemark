# Handoff — CORE-fix-2: writer supervision, LVM backoff and operational clocks

## Needs from coordinator

Run the socket-dependent daemon suite in unrestricted CI and the existing privileged daemon gate in disposable Rocky 8.8/8.10 guests. This is an implementation and evidence candidate, not an integration approval. Local workspace tests and tools/ci.sh remain non-green because the five known w6a1 readiness tests cannot bind Unix sockets here.

- Attempt: `attempt-e249ef5f972dff1de5ced575070176ae48e9c231ee5080965e1b752893010cc0`.
- Agent: codex-sol; date: 2026-10-12 (America/Toronto).
- Base: `7fe65f683652547d91f28ccc6a0ccf017b6e559f`.
- Branch: supplied attempt branch; no branch creation, merge, push or privileged execution.
- Findings not implemented: none among S1, S4 and S5. Independent verification and guest behavior remain pending.

## Per-finding changes and regression evidence

### S1

The lifecycle starts a writer supervisor which owns the same bounded receiver across worker restarts. It joins each writer worker immediately on exit, captures returned errors and panic payloads, reports `tswriter=error` with `persistence lost` and the cause, and reopens catalog/time-series connections after 5, 10, 20, 40, 80, 160, then 300 seconds. Reopening/spawn failures also back off. Successful persistence resets health and the next failure starts again at five seconds. Sampling and in-memory publication continue independently. There is one active writer at a time; the supervisor joins it during ordered shutdown and observes cancellation during backoff.

Rollover flushing now uses the same failure publication path as ordinary flushing instead of returning early. A later successful write restores health. The regression `rollover_error_is_published_and_next_batch_recovers` prequeues a 20,000-row batch plus one more, injects an out-of-range gauge, checks the storage cause in health, then writes a valid batch through the same live worker and observes recovery. Reintroducing the rollover `?` makes this test fail (exit 101; `library/regression-S1-rollover.log`). `supervisor_reports_panic_and_preserves_producer_queue` kills the actual writer thread with a test-only message, checks panic cause/unhealthy state while the supervisor stays alive, publishes an in-memory sample and persists the queued sample after restart. Existing batching/barrier and repeated-key/restart tests remain intact.

Persistence already removed from the queue or buffered inside a dead worker can be lost; the error explicitly reports lost persistence. Remaining queued messages are retained, bounded by the original capacity 64. No unbounded replay buffer or second concurrent writer is introduced.

### S4

Default collection failures now retry after 300/600/1200/2400/3600 seconds. For interval i the first delay is max(i,300 seconds), doubles with saturation and caps at max(i,3600 seconds). Successful collection resets attempts, including when its persistence enqueue fails. Queue congestion publishes degraded persistence health and waits the ordinary interval; it does not become a collection failure or recollect early.

`lvm_disabled_and_backoff` changes its old one-second/four-second assertions. New source-local `retry_sequence_custom_intervals_and_success_reset` covers the default sequence, shorter/longer intervals, cap and reset; restoring the old formula fails with 1 versus 300 (exit 101; `library/regression-S4-sequence.log`). `full_queue_does_not_recollect` uses injected collection, fills the queue, and checks one collection after 1.1 seconds (the old failure branch recollects after one second). No LVM command ran.

### S5

Wall and monotonic clocks are separate injected functions. Production monotonic clocks use Instant; existing public start/start_with_clock signatures are unchanged and new start_with_clocks constructors allow independent tests. Wall time supplies calendar windows/daily schedules, stored/report timestamps and startup history translation. Monotonic time supplies scheduler retries, Busy deferral, one-second polling, firstboot elapsed waiting, runtime every cadence, writer restart backoff and hourly compaction. Existing Instant-based shutdown/recorder bounds remain monotonic.

Runtime completion anchors are bounded by the configured root cap, seeded from already-published startup inventory before accepting commands, survive unchanged reloads, and are retired for removed roots. Seeding performs no additional storage read and prevents an early manual completion from being misclassified as restart history. A changed interval is measured from the same elapsed completion anchor. Next-due API timestamps project remaining elapsed delay onto current wall time. Stored catch-up history is translated using the startup clock pair. Compaction uses wall time for retention cutoffs, while its hourly trigger uses monotonic time.

`wall_steps_do_not_consume_retry_delay_or_suspend_polling` independently advances wall time forward/backward while preserving retry time, then advances elapsed time to permit the next attempt. Restoring shared wall timing makes it fail (exit 101; `library/regression-S5-retry.log`). `successful_cadence_survives_wall_steps` covers an early manual completion with a different startup wall clock, successful cadence, and reload/changed interval after corrections; restoring the earlier cache-reset implementation also fails (exit 101; `library/regression-S5-reload.log`). `compaction_uses_injected_clock` uses two persistence barriers to observe the preceding loop's compaction check, and advances independent clocks, proving both wall-step directions leave the interval intact before elapsed time reaches an hour; restoring the wall trigger fails (exit 101; `library/regression-S5-compaction.log`). Firstboot coverage proves forward/backward corrections cannot consume 119 seconds and a backward wall clock still permits reporting at elapsed 120.

## Tests updated and signature compatibility

- w6a1: `lvm_disabled_and_backoff`, `compaction_uses_injected_clock`.
- w6a2 runtime Harness uses separate clocks; `scheduled_failures_back_off_with_injected_clock_and_reload_resets` and `busy_keeps_fixed_deferral_and_does_not_increment_failure_attempts` advance monotonic time separately.
- Scheduler source-local `firstboot_waits_for_components_or_deadline_and_writes_once` extends clock correction assertions.
- `retry_sequence_uses_injected_time_and_caps_at_one_hour` remains unchanged: its pure seconds arithmetic applies equally to monotonic input; its production caller now passes monotonic seconds.
- Workspace search found no additional old LVM-delay assertions or injected scheduler/writer callers. Calendar/DST schedule tests remain unchanged.
- No existing public signature changed. lib.rs only changes lifecycle writer construction and adds monotonic clock wiring/docs. No module declarations, dependencies, manifests, core/store contracts, state or other services changed.

## Assumptions and proposed plan precision

No user questions were asked. No service-restart policy existed in the inspected lifecycle, so the authorized five-second-to-five-minute fallback is used. Firstboot's elapsed budget begins at scheduler coordinator startup. Runtime completion anchors capture the first idle observation of a new persisted scan ID; the scheduler has one-second cadence resolution. Empty Flush barriers alone do not claim persistence recovery. Initial storage opening still fails startup synchronously as before; runtime writer deaths restart without stopping the daemon. Full queue and disconnected queue both represent persistence loss rather than collector failure.

Proposed plan additions (not applied to docs/plan): writer supervision retains the bounded producer queue, reports exit/panic causes and retries 5 seconds doubling to 5 minutes, resetting after successful persistence; collection backoff starts at max(configured interval,5 minutes) and caps at max(interval,60 minutes), with persistence congestion excluded; operational intervals use monotonic time, calendar windows/daily schedules use wall time, and next_due is the wall projection of remaining elapsed time. Runtime completion anchors survive unchanged reloads and changed cadence is measured from the same completion anchor.

## Validation

All final commands run under `timeout 1500`, with `CARGO_NET_OFFLINE=true`, the pinned host 1.98.0 toolchain and writable registry copy `/tmp/core-fix-2-cargo`. `RUSTUP_HOME=/home/brewerm/.rustup` and the host toolchain bin path avoid the isolated read-only home. No new dependency was added.

| Command | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline --workspace --no-fail-fast` | exit 101; 618 passed, 5 failed, 13 ignored; only w6a1 fails |
| `cargo build --offline --workspace --features privileged --tests` | exit 0, compile only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 |
| `tools/ci.sh` | exit 1; source policy/fmt/Clippy pass, tests stop at the same five w6a1 readiness failures |

Final daemon results: library 42 passed; w6a1 19 passed/5 known readiness failures; w6a2 21 passed. Source policy, format and Clippy pass inside the CI wrapper before the test phase. Cargo-deny was not reached; no audit is claimed. The no-fail-fast workspace run covers the later targets that CI does not reach. Final code/test source is commit `82ada56`; the evidence-only handoff commit follows. Writer supervision and compaction clock plumbing share their constructor and were committed together; LVM and scheduler changes have separate commits.

Daemon test inventories before/after: baseline library 38, w6a1 24, w6a2 19 (81 total); candidate library 42, w6a1 24, w6a2 21 (87 total). Baseline enumeration compiled and listed tests without running them; no full baseline execution count is claimed. Candidate passes include tests which intentionally skip socket transport in this sandbox. Detailed command outputs and regression mutation logs are under the canonical attempt's worker-output library. Mutations were temporary, sequential and restored before final checks.

## Coordinator/guest commands and limits

In unrestricted CI, with the offline cache:

```sh
export CARGO_NET_OFFLINE=true
timeout 1500 cargo test --offline -p tidemarkd --test w6a1 -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a2 -- --nocapture
timeout 1500 tools/ci.sh
```

The five pending real readiness checks are binary_ready_and_stopping, corrupt_catalog_is_preserved_before_recovery, lifecycle_reload_and_shutdown, private_and_missing_state_directories_start and startup_reads_last_complete_from_catalog. Existing transport tests detect denied sockets and skip/fallback; those passes do not establish actual Unix transport behavior.

Only in disposable Rocky guests, compile and execute the existing gates:

```sh
export CARGO_NET_OFFLINE=true TIDEMARK_DEPLOY_TEST_VM=1
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture
```

No privileged test was executed locally. Real disk-full/SQLite I/O stalls and forced termination, actual LVM commands and long-running resource behavior need guest evidence. The change does not make blocking SQLite/kernel operations cancellable. No unrelated review findings were addressed. No dependency audit is claimed if CI fails before cargo-deny.

## Definition of Done evidence

- Owned source/test paths only, plus this owned handoff; no dependency/module/manifest edit.
- Public additions documented; existing ownership headers and unsafe prohibitions retained.
- Format, Clippy, privileged compilation and rustdoc pass.
- Workspace/CI execution is limited by the five known sandbox readiness failures; unrestricted CI and existing guest gates are pending.
- Regression evidence recorded for all three assigned findings; no unrelated finding addressed.
