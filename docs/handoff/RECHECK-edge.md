# Handoff — RECHECK-edge

## Needs from coordinator

Run listener/readiness and guest gates in an unrestricted disposable runner, and investigate the out-of-scope CLI cancellation/lock-release failure observed in the full workspace run (isolated rerun passed). No implementation dependency or additional file ownership is needed. Real ShellCheck and cargo-deny checks still need equipped runners.

- **Branch:** precreated attempt branch hp-39aa21f9865c59cbd96e9b0c62ad97e8a135d8e783435b4890c2f62f6e9453bf
- **Date:** 2026-10-13
- **Status:** implementation evidence candidate; independent verification pending

## What changed and regression evidence

**R8:** Overflow availability now uses exact start/end cumulative totals, accepting a start supplied by the previous minute's end. Missing interior minutes leave hot unavailable but preserve overflow counts. Conflicting duplicate boundaries, decreasing totals, and inconsistent adjacent totals remain unavailable. Hot still needs every healthy, untruncated, overflow-free minute. Histories remain capped at 60 facts and reset on watcher replacement.

`services::watcher::tests::exact_consecutive_minutes_loss_gaps_overflows_and_cap` now checks the review's missing-minute example: minutes at 60 and 180, totals 10→13 and 13→15, yield two-minute overflow count 2 and three-minute count 5, both with hot unavailable. The original implementation returns None for both counts. `restart_and_duplicate_boundaries_do_not_supply_old_counts` covers replacement history, duplicate timestamps, and decreasing/inconsistent boundaries. Existing publication, loss, cap and freshness tests remain intact. Seven watcher tests passed.

**R7:** Both ShellCheck advisory runs write through `std::io::stderr().lock()` / `Write::write_all`, bypassing successful-test capture. Warning severity gates, existing source following, working directories, shell syntax and guest guards are preserved.

Each test binary has `shellcheck_advisories_survive_passing_harness_and_warnings_fail`. It runs its actual gate test as a child, with exact filtering and no `--nocapture`, and a temporary PATH stub. Default severity prints markers on both streams and exits 1; warning severity exits 0 or 1. The regression requires advisory markers on process stderr on success and failure, and verifies warnings fail the child. With `eprintln!` the passing child hides markers and the regression fails. W6-C: 10 passed; W7-C: 4 passed. Real ShellCheck is not available on this workstation; stub evidence establishes capture and severity handling, not an actual shell lint audit.

**R2:** `ScanControl::start` keeps its existing signature and non-request behavior. API dispatch calls a new crate-internal `start_for_request`, which checks cancellation/deadline before and after admission locks and state reads, after inventory reservation, and immediately before enqueue. Expired reservations release their pending slot. An atomic request phase arbitrates scan admission against explicit cancellation; unlike configuration commit, admitted scans remain cancellable. The acknowledgment loop polls the request every 50 ms and checks again on receipt. Expiration after enqueue sets the scan cancel flag and returns an unavailable error stating the outcome is uncertain. Transport-level scan-start timeout also states uncertainty. No wire type, method, flag, dependency, manifest, state or lifecycle file changed.

`request_expiring_behind_admission_locks_never_enqueues` dispatches the actual API method while either the active-scan or failure-history lock is held beyond a 100 ms request deadline; after unlock there is no command, active scan or inventory entry. Without propagation the old path enqueues or waits for its independent eight-second acknowledgment timeout. `short_request_deadline_cancels_admitted_scan_while_ack_is_held` uses a 150 ms API deadline and holds the actual command reply sender, then checks cancellation, uncertain error, bounded completion and rejection of a late reply. Without propagation cancellation takes eight seconds and the error lacks uncertainty. Existing atomic config-commit test also verifies admitted scan cancellation and rejection when cancellation wins first. All 67 daemon library tests passed.

## Shared-file edits

None. No added dependency. Only the five owned source/test files and this handoff changed.

## Assumptions and deviations

- The canonical coordinator decisions override the earlier stricter overflow policy. No plan files were edited.
- A scan-start transport timeout conservatively reports uncertainty even if it may have occurred before admission; it never implies the scan definitely ran or definitely did not run.
- Blocking kernel/state-lock calls remain cooperative cancellation boundaries: they cannot be interrupted in place, but their return cannot admit expired work. No new state exposure was needed.
- Regression coverage uses source-local tests and actual API dispatch because these permit deterministic lock/ack control without binding a Unix listener.
- Offline builds use the existing host cache (`CARGO_HOME=/home/brewerm/.cargo`), pinned 1.98.0 toolchain and `CARGO_NET_OFFLINE=true`; no dependencies were fetched. No privileged test binary was executed.

## Proposed plan text (coordinator review only)

- 07 §3.3 / 13 R4-7: “Known exact S/E overflow boundary totals for one watcher identity suffice despite missing interior minutes; hot data still requires all consecutive healthy minutes. Watcher replacement discards prior boundary history.” Existing normative text already agrees; update implementation-status claims only after review.
- Wave 6 packaging / 13 R4-4: “ShellCheck informational/style diagnostics are advisory and emitted directly to process stderr, visible on passing ordinary CI runs; warnings/errors fail the gate.”
- Wave 6 API / 08 §2: “scan.start admission observes request cancellation and its absolute handler deadline before and after blocking admission operations and before queue publication. Request expiry during acknowledgment cancels admitted work; timeout responses describe the outcome as uncertain. Public/wire signatures remain unchanged.”

## Validation

All required commands ran under `timeout 1500`, with offline mode and the pinned toolchain. Full logs are retained in this attempt's worker-output `library/`.

| Command | Observed result | Log |
|---|---|---|
| `cargo fmt --check` | Exit 0 | `fmt.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 | `clippy.log` |
| `cargo clippy --workspace --all-targets --features privileged -- -D warnings` | Exit 0, compile only | `clippy-privileged.log` |
| `cargo test --workspace --no-fail-fast` | Exit 101; aggregate reported summaries: 749 passed, 1 failed, 14 ignored | `tests.log` |
| `cargo build --workspace --features privileged --tests` | Exit 0, no privileged execution | `build-privileged.log` |
| `tools/ci.sh` | Exit 0; 146 owner tags and forbidden-symbol checks passed; fmt/Clippy/tests passed; cargo-deny unavailable and skipped | `ci.log` |

The first workspace run's sole failure was out of scope: `tidemark-cli`'s `live_rows::tests::cancellation_wakes_throttled_scan_and_releases_lock`, final reacquisition at live_rows.rs:224 returned Busy. The same compiled binary passed an isolated exact-test rerun (1 passed, 0 failed in 0.25 s; `cli-cancellation-rerun.log`). The subsequent CI workspace run passed that test and all available tests (750 passed, 0 failed, 14 ignored across reported summaries). Treat this as an observed timing/concurrency-sensitive failure, not a demonstrated fix. No CLI code changed. Counts aggregate libtest summaries; successful early returns for unavailable platform facilities are not guest evidence.

`binary_ready_and_stopping --nocapture` passed by printing its socket-unavailable skip (`readiness-probe.log`). W6-A1 overall reported 29 passed; the earlier retained environment note about five failures is superseded by the observed skip behavior in this baseline. W6-A2 reported 21 passed and W6-A3 13 passed. All task source-local regressions passed in both workspace/CI runs. Final rustdoc-only additions document the internal admission and dispatch seams; CI rebuilt/checked those sources.

## Guest and equipped-runner checks — pending

These changes add no privileged operations or privileged-only test. Real listener, credentials, fanotify, deployment/reboot and remaining lifecycle checks require disposable Rocky guests. The workstation's Unix-bind restriction causes listener-dependent tests to return early. A focused `binary_ready_and_stopping --nocapture` run printed “Unix sockets unavailable in sandbox; readiness test deferred to CI”. The retained note predicted five readiness failures; this baseline instead reports those tests as passing after the existing skip. Those results do not establish real readiness behavior. Run these on equipped runners:

```sh
timeout 1500 cargo test --offline -p tidemarkd --lib
timeout 1500 cargo test --offline -p tidemarkd --test w6a2 --test w6a3
timeout 1500 cargo test --offline -p xtask --test w6c --test w7c
timeout 1500 cargo xtask vm-test --host rocky88 --no-fail-fast
timeout 1500 cargo xtask vm-test --host rocky810 --no-fail-fast
timeout 1500 tools/ci.sh --require-deny
```

The final xtask tests must also run with real ShellCheck installed; ordinary successful output should visibly contain its advisory section. Guest aliases above are assumptions for the coordinator to replace with available SSH aliases. No VM was contacted, and no service/storage-management command was run.

## Definition of Done evidence

- Owned paths only; no branch creation, merge or push; one implementation commit per finding in required order.
- Existing public signatures, frozen contracts, owner headers, read-only policy and bounds retained.
- Focused regressions passed locally; final workspace validation detailed above.
- Privileged builds and equipped-runner/guest results distinguished from execution evidence.
- Submission is an evidence candidate, not task/release acceptance.
