# Handoff — W7-A2-live: Shared gated live rescan

## Needs from coordinator

Wire the TUI refresh to this helper in its own task; no TUI files were changed here. Existing privileged mount-boundary execution remains pending a disposable VM. Install/run cargo-deny in coordinator CI; the wrapper skipped it because it is absent here. Review the observed intermittent daemon metrics test failure if it recurs. The standalone workspace run passed all daemon readiness tests in this attempt.

- **Branch:** precreated hp-58c23145e5644ea3a86472058296844945cfde8280e37b8ff66fc33b1b9e9dd2
- **Agent/session:** Codex; attempt-58c23145e5644ea3a86472058296844945cfde8280e37b8ff66fc33b1b9e9dd2
- **Date:** 2026-10-13
- **Status:** implementation and validation evidence candidate; independent verification pending

## What was built

Moved the live walk and aggregation from `cmd/top.rs` into private `live_rows.rs`. The crate-private function takes `&RawPath`, `&Config`, and `&AtomicBool`, returning `Vec<DirRow>` for browser callers. `top` retains SIGINT registration/unregistration and maps those rows to the same path/disk lookup used by its existing renderer. No public API, flags, config keys, dependencies, snapshot schema or renderer changed.

The helper retains scan locking, gentle adaptive/fixed gating, configured limits, exclusions, mount skips (including same-device bind mounts), state-directory exclusion, metadata-only walker settings, and VecSink aggregation. The walker retains its no-follow, device checks and O_NOATIME handling. Cancellation is checked before setup and around preparation/output. During walking, the receiver checks the caller flag every 20 ms when idle and between records, calls `ScanGate::cancel()` to wake throttled acquisitions, drains the bounded channel, and joins the walker. Cancellation returns the existing typed `WalkError::Cancelled`; sampler shutdown and scan-lock cleanup remain owned by their guards.

## Draft review

Kept the draft's extraction, configuration-based interface, VecSink result and cancellation checks around aggregation. Replaced its signal-owning shared wrapper and `&Arc<AtomicBool>` argument with the requested borrowed `&AtomicBool`; signal ownership belongs to `top`. Replaced string cancellation errors with the existing walker error. Added gate wakeup while receiving records: passing a flag alone cannot promptly release a blocked gate acquisition. Retained all original walk/aggregation parameters. No unverified draft files were present at the baseline, so the extraction was made from the checked-in original function.

## Shared-file edits

| File | Change | Why |
|------|--------|-----|
| crates/tidemark-cli/src/lib.rs | One private `mod live_rows;` line | Make the helper available inside the CLI crate |

No Cargo files or dependencies changed. No integration test files changed, including `tests/w7a*`.

## Assumptions

The requested private shared function may return complete directory rows (as the draft did) while `top` adapts them to its previous disk-only lookup. This lets the TUI reuse all live metrics without another scan. The fixture regression uses the same independent GNU `du` disk oracle used by existing top report tests, avoiding hard-coded filesystem allocation numbers. A separate baseline experiment compiled the original checked-in command and compared its complete `top --live` JSON with the final binary on the same three-directory fixture; the JSON was identical. Temporary source substitutions for this experiment were restored byte-for-byte before final CI. The existing scanner owns mount traversal guarantees; this extraction does not duplicate those low-level checks or add new privileged operations.

## Acceptance criteria evidence

| Criterion | Evidence |
|-----------|----------|
| Existing top --live tests unchanged | `rg -n -- '--live' crates/tidemark-cli/tests` found `e2e/scan_reports.rs`; unchanged E2E suite: 3 passed |
| Original top output retained | `baseline-parity.log`: original checked-in binary and extracted binary emitted identical complete JSON on the same three-directory fixture |
| Small fixture rows retain disk totals | `live_rows::tests::fixture_rows_match_top_disk_oracle`: three rows, each matches GNU `du` |
| Symlinks not followed | `symlink_to_outside_directory_is_not_followed`: outside 1 MiB file contributes no directory or file entry; the link itself counts as one entry; disk matches no-follow `du` |
| Caller cancellation returns cancelled error | `preset_cancellation_returns_cancelled_before_state_io`: typed `WalkError::Cancelled` without touching default state |
| Cancellation wakes gate and releases lock | `cancellation_wakes_throttled_scan_and_releases_lock`: rate fixed at one op/sec, flag set during scan, joins within two seconds and reacquires ScanLock |
| No crate public API added | Private module and `pub(crate)` helper only |

## Validation

All Cargo commands use Rust 1.98.0, `PATH=/home/brewerm/.cargo/bin:$PATH`, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_NET_OFFLINE=true`, and `timeout 1500`. Logs are in the attempt output `library/`.

- Focused live unit tests: 4 passed, 79 filtered out, 0.12 seconds.
- Existing CLI E2E suite: 3 passed, 52.26 seconds.
- `cargo fmt --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace --no-fail-fast`: passed; summed target summaries: 684 passed, 0 failed, 14 ignored.
- `cargo build --workspace --features privileged --tests`: passed; privileged execution pending VM.
- First `tools/ci.sh`: source checks (146 owner tags), fmt and Clippy passed; test stage failed on `tidemarkd/tests/w6b.rs::temporary_live_writer_and_nonregular_are_refused`, at line 424 with `WouldBlock` while recovering a temporary metrics writer. This test passed in the standalone workspace run and passed on targeted rerun (1 passed, 11 filtered). No out-of-scope daemon edit was made. Final `tools/ci.sh` repeat passed: 684 tests passed, 0 failed, 14 ignored; source checks, fmt, Clippy and privileged-feature compilation passed. Cargo-deny is not installed, so the wrapper explicitly skipped dependency-deny checking.
- Baseline JSON parity experiment: passed; three directories, complete JSON identical, live disk values `[8192, 8192, 0]` on this host. Allocation values are observations, not hard-coded assertions. Logs and both JSON documents retained.

Initial tool invocation could not find cargo in PATH; adding the existing host Cargo bin path resolved it. The initial symlink test incorrectly expected zero non-directory entries; corrected to one per normative link semantics. All four final tests passed afterward.

## Remaining uncertainty and limits

The receiver's 20 ms poll bounds detection while waiting for records; it does not promise a wall-clock bound for kernel I/O, mount/block-device setup, or aggregator preparation/write internals. Cancellation is checked at aggregation boundaries and cannot interrupt `Aggregator::prepare` internally without an out-of-scope API change. No new privileged test is needed for the move itself; existing privileged scanner/mount tests are compiled and remain pending VM execution. No mounts, LVM, services or system configuration were changed.

## Definition of Done evidence

Owned source paths only; the sole shared-file edit is listed. Source owner header and helper documentation are present. No unsafe code, dependencies, unexplained TODOs or production unwrap/expect calls added. Implementation committed after focused tests passed. All required commands ran; final CI passed with the documented cargo-deny skip. The full workspace command and final CI both passed all five retained daemon readiness tests. Privileged execution remains pending VM. Submission is an evidence candidate; verified integration success is not claimed.
