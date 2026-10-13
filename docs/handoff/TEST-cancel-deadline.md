# Handoff — TEST-cancel-deadline

- Branch: hp-019446f7830edc8d0a0c904f7c2511cdffe917eb26c2ad2b3355831b83b44aea
- Agent/session: Codex, canonical attempt-019446f7830edc8d0a0c904f7c2511cdffe917eb26c2ad2b3355831b83b44aea
- Date: 2026-10-12
- Status: implemented; evidence candidate awaiting independent verification

## What changed

Changed the SIGINT-to-exit deadline from 2 seconds to 120 seconds for both cancellation variants. Existing exit-code-3, failed catalog, retained partial snapshot, growth-baseline exclusion and temporary-file assertions remain. The CLI emits `scan cancelled` only after `run_scan` returns: the failed lifecycle calls `scan_finished` in `recorded`, and partial publication completes snapshot, catalog, series and retention bookkeeping before returning. The retained partial diagnostic is `scan completed with errors or partial results`. Therefore this test uses the explicitly authorized exit-only fallback; no product source changed.

Added shared `tests/support/config.rs::direct_config`, used by the E2E harness, interruption setup and follow-up commands, rules, W2-C, W3-A, W3-C, W4-A and W5-B helpers. Each writes an absent `absent-api.sock` under its temporary owner. Added private sockets to W1-A configured probe fixtures, W4-B deployment fixtures (including privileged ones), and the daemon check-config example in surface tests. W3-B and W6-A3 already use private sockets, including intentional listener/error fixtures, and were preserved. Deliberately unparsable deployment fixture text remains unparsable; it never reaches a daemon connection. Missing-config probe and deployment-preflight scenarios that do not use daemon dispatch remain intact.

Added `unrelated_listener_does_not_redirect_direct_scan`: bind an unrelated temporary Unix listener, scan a small fixture with the harness configuration, require code 0 and a complete catalog entry, and verify the listener has no queued connection. It uses nonblocking accept after completion, with no listener-backlog size or sub-second timing assumption. PermissionDenied/EPERM prints one skip line and returns.

## Shared-file edits and dependencies

None. Only CLI test paths and this handoff changed. No dependencies added.

## Deadline audit

Only the E2E cancellation exit deadline changed (2 → 120 seconds). Searched every CLI test for `from_secs`, `from_millis`, `deadline`, `timeout`, and elapsed checks:

- E2E startup/catalog/progress readiness stays 60 seconds; lock-holder readiness stays 30 seconds. The 20 ms sleeps are polling intervals, not completion deadlines.
- W6-A3 fake-daemon accept (5 seconds), read timeout (3 seconds), and CLI wrapper (20 seconds) cover API transport/unavailable endpoints; the scan cases either dispatch to a mock daemon or reject connection/preflight before durable direct scans.
- W3-B's 10-second scale query measurement starts after snapshot finish and catalog rebuild; it measures a read-only links query.
- W4-B privileged tool timeout (10 seconds) and reboot wait (300 seconds) are VM/tool lifecycle checks, not short direct-scan publication bounds.
- W2-C's 2-second duration is synthetic outcome data. W4-A alert durations, W5-B writer/poll intervals and rules timestamps are data/intervals, not process-exit deadlines.

## Validation

All builds used Rust 1.98.0 binaries, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and offline mode (`CARGO_NET_OFFLINE=true`). Existing populated cache was sufficient; no dependency fetch was attempted.

- `cargo test -p tidemark-cli --test e2e --offline`: exit 0; 3 passed, 53.04 seconds.
- Same command with `-- --nocapture`: exit 0; 3 passed, 53.35 seconds. New listener regression prints the expected sandbox EPERM skip.
- `cargo fmt --check`: passed.
- `cargo clippy -p tidemark-cli --all-targets -- -D warnings`: passed with offline environment.
- `cargo test -p tidemark-cli --offline`: exit 0; all unprivileged CLI targets and doc tests passed (one existing scale test ignored).
- `cargo test -p tidemark-cli --offline --features privileged --no-run`: passed. Privileged execution: pending VM; no privileged/destructive operation executed.
- `git diff --check`: passed.

The first formatting invocation lacked RUSTUP_HOME and failed before formatting; corrected environment and reran. Initial Clippy found the new shared test helper needed a local test-only unwrap lint allowance, which was added; final Clippy passed.

## Socket audit outside scope

No matching CLI system-daemon dispatch fault found in `crates/tidemarkd/tests`, `crates/tidemark-scan/tests`, or `xtask/tests`. Daemon operator CLI calls in `tidemarkd/tests/w6a3/operator.rs::cli` receive the explicitly private config; daemon listener fixtures use temporary socket paths. Scan tests invoke the orchestrator directly (default config socket is unused); xtask tests inspect packaging/VM plans, not daemon-dispatched CLI scans. No paths outside scope edited.

## Needs from coordinator

The current CLI has no pre-bookkeeping cancellation diagnostic, so these tests cannot establish a 5-second scan-work stop separately from durable I/O. If that observable assertion is desired, coordinate a separate product task; this task intentionally changes tests only. Run the new listener regression on unrestricted CI; its body cannot execute in this sandbox. Guest busy-storage behavior remains pending coordinator VM validation.

Suggested plan text for R3-7 / scanner cancellation: “The cancellation bound applies to stopping scan work and waking userspace gate waiters. Durable snapshot/catalog/series bookkeeping after cancellation has no fixed wall-clock bound. End-to-end tests distinguish a pre-bookkeeping cancellation/progress signal, when available, from process exit; allow up to 120 seconds for exit and verify durable outcome and retained partial state. A post-bookkeeping diagnostic is not evidence of prompt work cancellation.” This is a proposal, not an applied plan change.
