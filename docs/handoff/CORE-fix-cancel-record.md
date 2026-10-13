# Handoff — CORE-fix-cancel-record

## Needs from coordinator

Startup repair remains required and is outside this task's owned paths. In `crates/tidemarkd/src/lib.rs::run_controlled`, catalog recovery (around lines 424–439) is corruption-only; the history loop reads only `Catalog::latest` complete rows and publishes idle status. Neither `Catalog::open` nor `recover_catalog` transitions crash-left running rows. After the writer starts, and before any scheduler starts, enumerate all unowned `running` rows, set status to `failed`, set an injected startup finish timestamp, leave file absent, and send each through `Message::ScanFinished` with bounded acknowledgement. Emit WARN per repaired row with root and scan id. Fail startup or clearly report failed repair rather than starting scheduling with an unacknowledged repair. Include roots absent from current config. `Catalog::list(None)` is bounded and errors above MAX_QUERY_ROWS; for complete recovery of larger histories, a bounded paginated running-row query requires store-owner work. Preserve host scan ownership: coordinate with the host scan lock so a live direct scan is not falsely repaired.

The new shutdown test documents the open point by asserting the crash-left running row after shared shutdown interrupts final persistence. Once startup repair is supplied, add a restart test using production startup and assert failed status before scheduler admission. No out-of-scope file was edited.

## What changed and confirmed cause

The recorder previously tested the scan's own cancel flag in both queue and acknowledgement waits. Consequently the orchestrator's failed `ScanFinished` record could never enqueue after operator cancellation, even though `ScanStarted` had committed. The scheduler subsequently published idle status on the error path, leaving the catalog running indefinitely.

Recorder waits now observe daemon shutdown and its shared deadline only, retaining the existing eight-second bounds and 50 ms polling. `scan_started` explicitly skips admission when already cancelled. Public signatures and dependencies are unchanged. Existing recorder interruption tests now exercise daemon shutdown/deadline rather than scan cancellation; a unit test verifies cancellation permits queue and acknowledgement operations.

Audited all recorder callbacks: `dir_series` uses the same lifecycle-only waits and barrier. The orchestrator calls series only following successful snapshot publication; cancellation with no partial output returns through failed outcome recording before series/retention. No series or snapshot is written for the cancelled fixture. A cancellation arriving after a completed immutable publication must not suppress its outcome or series.

## Regression evidence

`w6a4.rs` copies a minimal version of the bound-storage harness from `w6a2/runtime.rs`, avoiding inclusion and duplication of its unrelated tests. It starts scheduler and writer directly, dispatches scan.start/scan.cancel/scan.status with injected peer uid 0, and opens no socket. Temp fixture/state directories are 0700.

- `cancelled_scan_records_failure_and_following_scan_completes`: 1,000 directories, gate capped at ten ops/second; acknowledged running row; cancel; idle status; failed row within 15 seconds, no file or series; following same-root max-throttle scan completes.
- `non_cancel_failure_records_failed_without_snapshot`: scanning the state directory triggers the orchestrator's post-admission precondition error without setting cancellation; failed row and no file.
- `shutdown_interrupted_outcome_exposes_startup_repair_requirement`: shared shutdown during the slow scan joins producers and drains writer; final outcome was interrupted and the row remains running. This is explicit evidence of the unresolved startup requirement, not an acceptance claim for repair.

Restored the exact baseline scheduler from `9152f843c633e7719e5d4ae9172c1f622987f25b` temporarily and ran the new cancellation test alone: exit 101, 0 passed / 1 failed, `state deadline expired` after 15.73 seconds. Restored the fixed scheduler; all three integration tests pass (1.00 seconds). Evidence logs are in the attempt output library.

## Assumptions

- The explicit task's ownership replaces the broad wave ownership; no manifests, lib.rs, store files, other tests, or plan documents changed.
- Shared daemon shutdown may interrupt outcome writes immediately, as permitted by the task; stale row repair is mandatory coordinator follow-up.
- Use populated `/home/brewerm/.cargo` cache, pinned Rust 1.98.0, offline builds, and direct toolchain PATH plus RUSTUP_HOME because cargo was initially absent from PATH. No dependencies added.
- No user questions, branch creation, push, privileged execution, or host configuration changes.

## Guest validation pending VM

As root on Rocky guests, run:

```sh
cargo test --offline -p tidemarkd --test w6a3 operator::real_operator_status_scan_top_now_reload_cancel_and_sigterm -- --nocapture
```

Operator assertions were left intact. Outcome acknowledgement precedes successful idle publication after this fix; no extra catalog polling should be needed on that path. Slow fsync exceeding the unchanged eight-second recorder budget can still produce uncertain persistence and require startup repair; no guest storage timing was measured here.

## Validation

Final command results and test counts will be recorded below. Independent verification and VM gate remain pending.

## Shared-file edits and deviations

None. New test target auto-discovered by Cargo; no dependencies or contracts changed. Existing source owner header retained; new test has task owner header and forbids unsafe code.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0; also rerun by CI after final source correction |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Final exit 0; initial lint attempt caught an edited unit-test variable mistake and test unwrap lint, both corrected |
| `cargo test --offline --workspace --no-fail-fast` | Exit 101: 646 passed, 5 failed, 13 ignored across 61 target summaries; only failed target w6a1 |
| New integration target w6a4 | 3 passed, 0 failed; 0.66 seconds in workspace run |
| Scheduler unit tests | All passed, including cancellation queue/ack and shutdown/deadline/series interruption |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; privileged tests compiled only, pending VM |
| `tools/ci.sh` with CARGO_NET_OFFLINE=true | Result recorded after completion below |

The five w6a1 failures are `private_and_missing_state_directories_start`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `startup_reads_last_complete_from_catalog`, and `binary_ready_and_stopping`. They fail readiness deadlines in the known Unix-socket-restricted sandbox. No assertions in those tests or the operator test changed. CI's cargo-deny stage follows workspace tests and cannot be reached here when these readiness tests fail.

Final CI wrapper: exit 1, after source checks (141 owner tags; forbidden-symbol scan passed), formatting and Clippy passed, then the same five w6a1 readiness failures. Independent workspace no-fail-fast run confirms all other targets, including w6a4, passed. No advisory audit result is claimed.

Owned deliverable is implemented; startup repair and VM/operator acceptance remain coordinator work. This is an evidence candidate awaiting independent verification.
