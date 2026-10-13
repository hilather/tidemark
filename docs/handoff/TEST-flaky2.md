# Handoff — TEST-flaky2: deterministic mount-transition lock test

## Needs from coordinator

Run the five socket-dependent readiness tests on a host permitting Unix socket binds. The sibling blocked-resolver regression is actually `state::presence_lock_tests::blocked_resolution_and_logging_leave_readers_free_and_reject_stale_generation` in `crates/tidemarkd/src/state.rs:1485`, not in `w6a1.rs`. It still uses two-second entry and 200-ms reader deadlines. Updating that test requires separate authorization because this task explicitly forbids editing product/source files and owns only this integration test and handoff. Its existing bounded-channel ordering needs no sleeps; raise its deadlines to at least ten seconds and ensure failure cleanup releases the publisher before assertions (including reload during phase 2). No product state-lock race was identified in the inspected implementation.

- **Branch:** precreated `hp-f829d4bceecdc729c26f2e32bf50b48648089221135b2501dc0266defff2e407`
- **Attempt:** `attempt-f829d4bceecdc729c26f2e32bf50b48648089221135b2501dc0266defff2e407`
- **Date:** 2026-10-13
- **Status:** implementation and validation evidence candidate; independent verification pending.

## Cause and interleavings

The fault is test interference through tracing's process-wide callsite interest cache, despite the original thread-scoped subscriber. The installed tracing-core 0.1.36 has a single-dispatcher fast path: `Dispatchers::rebuilder` returns `Rebuilder::JustOne`, whose `for_each` consults the registering thread's default dispatcher. Other integration tests call the same `mounts::log` WARN callsite without a subscriber. After the logger subscriber's dispatch is registered, another test can first register that callsite under its no-op default, caching `Interest::never`. The publisher then skips the warning, returns normally, drops its scoped dispatch/subscriber/writer factory and thus the last `entered` sender. The waiting receiver gets `Disconnected`, rather than `Timeout`.

A controlled standalone probe uses actual `LiveState::publish_mount_presence`: construct the sole dispatch, publish absence on another thread without a default subscriber, then publish fresh absence with that dispatch scoped locally. Observed writer count: **0**. Rebuild the callsite cache under that same dispatch and publish fresh absence again: **1**. Source and transcript are `library/callsite-probe.rs` and `library/callsite-probe.log` in this attempt's worker-output. This reproduces the causal mechanism deterministically; the original busy-machine execution has no thread trace, so its precise scheduler order cannot be recovered.

Other sender-loss possibilities were inspected:

- A publisher panic before writing drops the sink during unwinding; the old entry `unwrap` could obscure that panic because it never joined the publisher on entry failure. The new test releases and joins it before reporting an entry failure.
- A normal early return for stale generation or no transition also drops the sink without writing. This fixture creates an independent fresh state, publishes generation 0, has no concurrent reload, configures `/`, and starts with an empty presence map. It necessarily produces an initial absence transition. The root path is valid; canonicalization errors use lexical fallback, and the empty mount table resolves absence. No fixture-dependent product early return explains this failure.
- The scoped default surrounds the whole synchronous publication and is dropped only afterward. No guard-lifetime gap exists in the original test. No other test in this file installs a subscriber. Daemon binary logging initialization runs in separate processes; there is no replaceable process-global subscriber installed by this integration binary. The shared callsite cache, however, is process-global.
- Dropping a sender after a successful send does not discard a buffered entry message. At the first entry receive, `Disconnected` means no message was sent. The writer's own entry send cannot lose its receiver before that receive fails; a disconnected release receive can panic only after entry was already sent.
- Scheduling delays alone produce `Timeout`, not `Disconnected`. The original two-second entry, 200-ms state-reader and 50-ms collector-reader deadlines were independently vulnerable under load.
- A formatter may perform multiple writes for one event. The old writer blocked on every write but received only one release token, potentially hanging joins after the first chunk. The revised sink blocks only its first write.

Product inspection confirms configuration capture and path resolution happen before acquiring the write lock; application rechecks generation under that lock; transition logging happens after releasing it. This task changes no production code or contracts.

## What changed

The blocked-log test launches its own exact-test subprocess with a child-only environment marker, excluding parallel tests from the tracing callsite registry. Inside the child, the subscriber remains scoped to the publisher thread. A mutex-owned writer retains one sink and blocks only its first write. A bounded entry channel proves that logging is blocked before the reader is spawned; the bounded result channel proves the reader finishes before release. Entry and reader deadlines are ten seconds. Dropping the release sender unblocks logging on both success and failure, and both threads are joined before asserting the reader result.

The immutable-process-report test now gives its concurrent reader ten seconds while retaining the collector mutex until the receive completes. Its existing lock-before-spawn/result-before-release ordering is explicit and unchanged. The remaining blocked-worker test, `late_sampler_pass_cannot_resurrect_removed_mount`, already captures generation before its bounded ready signal and waits on a bounded resume channel until configuration replacement completes; it has no sleeps or short deadlines. No other logger installation or sub-ten-second receive deadline remains in this file.

## Shared-file edits and deviations

None. Only `crates/tidemarkd/tests/w6a1.rs` and this note changed. No dependencies, module declarations, APIs, config keys or schemas changed. Exact-test process isolation was chosen because serializing only subscriber installers would not serialize other tests using the shared warning callsite. The out-of-scope resolver follow-up is identified above rather than silently editing `state.rs`.

## Validation and measurements

All Cargo commands used the pinned Rust 1.98.0 toolchain, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup` and offline dependencies. No privileged commands were run.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline -p tidemarkd --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline -p tidemarkd --test w6a1` | exit 0; 30 reported passed, 0 failed; five readiness bodies return early on denied socket binds |
| Same command with `-- --nocapture` | exit 0; confirms five sandbox-deferred readiness bodies |
| Normal binary loop | 100 runs passed, 0 failed; 25 tests/run, five filtered out; 32.00 seconds total |
| Build-loaded binary loop | 100 runs passed, 0 failed; 25 tests/run, five filtered out; 54.06 seconds total |
| Concurrent `cargo build --offline --workspace --target-dir target-flaky2-load` | exit 0; fresh independent target directory; alive before and after all 100 loaded runs |
| Actual mount-warning cache probe | exit 0; suppressed count 0, repaired count 1 |
| `git diff --check` | exit 0 |

The loop directly executes the compiled integration binary with default test parallelism and `--nocapture`, invoking the isolated blocked-log child on every run. Exactly these five tests were excluded using individual `--skip` arguments:

- `lifecycle_reload_and_shutdown`
- `private_and_missing_state_directories_start`
- `binary_ready_and_stopping`
- `corrupt_catalog_is_preserved_before_recovery`
- `startup_reads_last_complete_from_catalog`

Total loop evidence: **200/200 binaries passed; 5,000 parent-test results passed; 200 isolated blocked-log executions passed**. Host: x86_64 Linux 7.2.5-3-omarchy, glibc 2.44, 12 logical CPUs. The build process's liveness is measured, not a claim of constant CPU saturation. Logs for every run, the repeat script, JSON records, full tests, Clippy and formatting are under this attempt's worker-output `library/`. No new privileged tests are needed for this test-only synchronization correction; VM and socket lifecycle acceptance remain external.

## Remaining uncertainty and completion evidence

No original-run scheduler trace exists. The controlled probe establishes the matching disconnection mechanism; the 200 successful repetitions provide stress evidence, not proof against all possible environments. The ten-second deadlines are deliberate liveness bounds and could still fail on a completely starved machine. Workspace-wide CI and VM acceptance were not requested or performed. Memory brief/input and receipts were read through the canonical API; no pending updates were reported and no memory projections were edited. No branches were created, merged or pushed.

The requested commit, cleanup of both target directories and clean-status confirmation are recorded in the external evidence report so this committed note need not be amended after cleanup. Independent acceptance and integration belong to the coordinator.
