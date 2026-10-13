# Handoff — EDGE-cli-fix: Bound the scan timeout test listener

- Branch: canonical attempt branch `hp-7911a1ecf2c3fe67eeda764cdaf557f79294867bb9ec38f9b3c8c9f70b06e385`.
- Date: 2026-10-12. Status: implemented; evidence candidate pending coordinator socket execution.

## What changed

`scan_full_backlog_expires_without_direct_state_writes` now creates its Unix listener with existing `rustix::net::{socket_with, bind, listen}` and requests backlog 1. It keeps the listener unaccepted and successful connection descriptors alive, makes at most 64 nonblocking connections, and requires EAGAIN before exercising the CLI. If the kernel does not bound that queue within 64 attempts, it emits one environment-limit line and returns.

The product assertions remain exit 4, stderr containing `timed out`, and no state directory. `cmd/scan.rs::daemon_connection` uses an 11-second client timeout; `client.rs::connect_deadline` bounds caller waiting with `recv_timeout`. The existing subprocess helper uses `timeout 20s`, so it exceeds that deadline and bounds a stalled CLI. Normal socket-enabled execution should take about 11 seconds plus process startup and bounded setup.

## Part 3 review

Reviewed EDGE-cli's test diffs and current test code, including all changed `w6a3.rs` cases, `w2c/main.rs` assertions, and three changed human snapshot files. Findings:

- Backlog size: the sole incorrect system-limit assumption was the std listener in the timeout test; fixed as above. Other listeners never rely on filling their queues.
- Timing: fake daemon acceptance allows 5 seconds and reads allow 3 seconds. Its 10ms sleep is a retry interval, not a subsecond completion assertion. The subprocess outer deadline already allows 20 seconds. No additional timing change needed.
- Terminal presence: width regressions inject 40/80/100/200 columns directly. Binary subprocess output is piped, using the default non-TTY width; assertions check short content substrings. No inherited terminal width assumption found.
- Absolute paths: `/fixture`, `/data`, and `/begin` are injected report/catalog values, not expected host files. Actual socket/config/state files and invalid/missing endpoints live under temporary directories. `scan /` exercises daemon/error handling and is intentionally guarded by state-write assertions. No host-path fixture change needed.
- Permissions: the mode-000 test probes actual denial and restores permissions/returns with a diagnostic when privilege permits traversal; it does not assume a non-root process. Socket tests already probe sandbox EPERM.
- Goldens: human UTC timestamps and machine mtime/newest_mtime fields come from fixed fixture values, independent of host timezone and clock. No additional changes needed.

Only `w6a3.rs` and this handoff changed. No shared-file, manifest, dependency, source, contract, or privileged-test changes. `rustix` is already a CLI dependency with the workspace `net` feature; clippy compiles the new calls without dependency changes.

## Validation

- `cargo fmt --check`: passed.
- `cargo clippy --offline -p tidemark-cli --all-targets -- -D warnings`: passed.
- `cargo test --offline -p tidemark-cli`: exit 0; 130 passed, 0 failed, 2 ignored (scale); w6a3 reported 15 passed.
- Targeted backlog `--nocapture` run: returned at the existing probe with `Unix socket bind denied by sandbox (EPERM)`; no product assertions ran.
- `git diff --check`: passed.
- Used the pinned 1.98.0 toolchain with `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo` and its toolchain bin directory on PATH. Initial invocation lacked RUSTUP_HOME and stopped before validation because the projected home was read-only; corrected invocation passed.
- Socket-dependent tests return early on sandbox EPERM; reported passes do not validate unrestricted socket behavior.

## Remaining uncertainty

This sandbox denies Unix socket binding. Compilation and sandbox test results cannot establish real backlog/CLI timeout behavior; coordinator execution with sockets available is still required. No privileged runtime or workspace-wide checks were requested for this narrow test-only fix.
