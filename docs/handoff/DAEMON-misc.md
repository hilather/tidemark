# Handoff — DAEMON-misc: isolated locks and scan refusal semantics

- Attempt: `attempt-9a2a07777c982ada8ae9ade18acc4e7c4fac3d9a96e5e67da812985146bf6c84`.
- Branch: supplied `hp-9a2a07777c982ada8ae9ade18acc4e7c4fac3d9a96e5e67da812985146bf6c84`; base `5fe9a1cec5bbb45c22c32105fb838edb8567ab89`.
- Date: 2026-10-13. Status: implementation/evidence candidate; independent review and unrestricted/guest acceptance pending.

## What changed and acceptance evidence

### 1. API and metrics in the same directory

The exporter locks only its owned `<textfile>.tmp`, retaining no-follow, nonblocking opening, owned regular single-link validation, truncation after locking, file sync, 0644 and handle-relative atomic rename. After obtaining the lock it compares the named temporary's device/inode with its opened handle. A writer that opened the old inode before another writer renamed it returns `WouldBlock` instead of truncating the published textfile; a missing temporary has the same retry result. This identity check replaces the removed directory lock's open/rename serialization guarantee.

The API holds an exclusive nonblocking flock on `<socket>.lock`, created/normalized to 0600. Existing owned single-link regular crash files are reused; symlinks, hard links and nonregular/other-owned files are refused. The locked handle and parent handle survive until socket cleanup. Lock identity is checked after acquisition and before cleanup; a substituted lock leaf is preserved. Trusted no-follow ancestry, procfd-derived socket addressing, active/stale probes, socket 0600, socket identity checks and replacement-safe cleanup remain. Cooperating instances exclude the same socket; different socket names may share a parent. No directory flock remains in either component.

`api::tests::socket_lock_excludes_same_socket_reuses_crash_file_and_preserves_replacement` executes without listening and covers exclusion, crash-file reuse/mode, simultaneous metrics publication, different socket names, replacement preservation, symlinks and hard links. `w6a3::api_and_metrics_share_a_private_directory` starts the actual API and exporter services with the socket and textfile in one 0700 directory, checks successful exposition and API status, then joins both and checks socket/lock cleanup. Its socket probe prints one line and returns under sandbox EPERM; real listening acceptance is pending unrestricted CI/guest.

### 2. NoSpace through the daemon

The frozen API enum is unchanged. `DaemonError::Scan("NoSpace")` maps to `Unavailable` and retains `scan: NoSpace` wording. The CLI's scan-start branch maps only the shared Unavailable error with that message prefix to the existing direct `ScanError::NoSpace` mapping: precondition exit 5 and wording `NoSpace`. Other Unavailable and Internal replies retain their previous mappings; direct scan code is unchanged.

The scheduler publishes `StateSpaceRefused(true)` together with `state_space=degraded`, detail `scan: NoSpace`, and retains scheduler failure health before sending the refusal acknowledgement. Successful guard admission publishes `state_space=ok` before its acknowledgement, clearing stale refusal health. Internal alert evaluation retains its existing cadence; this task makes status/component publication immediate without changing the unowned state or alert services.

`w6a3::space_refusal_reply_and_status_are_published_before_acknowledgement` starts the real scheduler/writer with an empty root and a 100% free-space floor. In-process API dispatch asserts Unavailable, exact message, refusal evidence and the immediate API status component. This runs without sockets. CLI `daemon_scan_space_refusal_matches_direct_exit_and_wording` supplies fake-daemon responses for exact/prefixed NoSpace, ordinary Unavailable and Internal/NoSpace, asserting exits 5/5/4/1 and wording. Its real socket exchange is pending unrestricted CI because the sandbox probe returns early. CLI fixtures explicitly set a private 0700 directory and keep `general.socket` inside it.

### 3. Offline dependency guard

`resolved_workspace_has_no_http_or_tls_packages` reads the workspace Cargo.lock directly and parses generated `[[package]]` name lines. It checks all platforms' locked names, rejects ureq, tiny_http, rustls, ring, hyper, reqwest, webpki-roots, native-tls and openssl, and asserts at least one name was found. It starts no process and requires no cross-platform registry downloads. No TOML dependency was available in this crate; no dependency was added.

### 4. Transient inherited locks

Successful textfile test writes, including the recovery immediately after dropping a lock and the repeated atomic-publication loop, use a bounded two-second retry helper. Only WouldBlock is retried; other errors fail immediately. Workspace search in the owned daemon tests found the explicit staging lock/drop/recovery case in w6b; no other owned test releases a metrics lock and requires immediate success.

The product exporter already attempted publication on every interval and cleared degradation on a later successful export; the hourly warning limiter did not gate retries. WouldBlock is now explicitly reported as `writer busy; retry on next tick`, with no hourly WARN budget consumption. It schedules the ordinary next tick and clears metrics health when that export succeeds. Repeated real contention remains degraded while it persists; permanent I/O/cache errors retain their existing diagnostics and warning behavior. `exporter_retries_busy_temporary_and_clears_health_on_next_success` holds an actual staging flock, observes transient health and unchanged staging data, releases it, then verifies real exporter publication and recovered health. The suspected fork/exec retention cause was not independently reproduced; eliminating the Cargo child and tolerating bounded transient contention address the observed failure mechanism.

## Scope, assumptions and deviations

- Only task-owned paths changed; no shared manifest/lib.rs/mod lines, dependencies, frozen types/schema/flags/config/metric names, packaging, services, fault scripts or plan files changed. No SCR is required for the explicitly assigned mappings and lock changes.
- Existing source owner headers are retained. No unsafe code or production unwrap/expect was introduced.
- This task's explicit scope/workflow takes precedence over the broad historical wave instructions: supplied attempt branch, no branch creation/merge/push, no privileged test execution, no host service/storage/config changes.
- Existing trusted-parent checks exclude unprivileged directory replacement. A hostile process with daemon/root credentials can interfere with leaves; inode-check/unlink is not an atomic kernel compare-and-unlink operation. The prior EDGE-api trust boundary remains.
- No parent flock is required for identity checks: parent descriptors pin directories, per-socket lock files serialize cooperating instances for that leaf, and both locked/named identities are checked. Existing bind and procfd security checks are retained.
- Cache errors still prevent healthy exporter status after an otherwise successful write. Contention is retried at the configured interval, not in an unbounded loop or a new thread/queue.
- Host `/home/brewerm/.cargo` and `/home/brewerm/.rustup`, pinned Rust 1.98.0, offline Cargo and `timeout 1500` are used. No network/dependency installation was attempted. Canonical memory receipts were empty; no projected memory file was edited.

## Coordinator commands and remaining uncertainty

On an unrestricted host, execute the real-socket regressions and all existing identity/startup regressions:

```sh
timeout 1500 cargo test --offline -p tidemarkd --lib api::tests -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a3 --test w6b -- --nocapture
timeout 1500 cargo test --offline -p tidemark-cli --test w6a3 -- --nocapture
CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh
```

On disposable Rocky guests only, execute the existing privileged daemon end-to-end test and textfile/ownership fixture through the VM gate. Pending VM command:

```sh
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture
```

Also reproduce the original installed-daemon configuration with both outputs in one private runtime directory; check successful textfile publication, API binding and shutdown `.lock` cleanup. Configure a refusing state-space floor and confirm `tidemark --json scan` exits 5 with `NoSpace`, and status shows the refusal immediately. No privileged runtime or installed-unit acceptance was performed here. Dependency/advisory auditing depends on cargo-deny availability. Existing privileged tests are compiled only; no new privileged mechanism is needed for these ordinary file-lock/API mappings.

## Validation

Final-source required check outcomes are appended below. Complete logs and exact exits are in the canonical attempt output's `library/`. Harness passes for guarded socket scenarios do not prove their exchanges executed. Initial focused runs: local API lock test 1/1, in-process NoSpace test 1/1, w6b 13/13; the focused CLI fake-daemon test returned early for EPERM. No clean-base full workspace runtime or independent acceptance result is claimed.

| Required command (offline; each under timeout 1500) | Exit | Evidence |
| --- | ---: | --- |
| `cargo fmt --check` | 0 | `library/fmt.log`; final fixture also checked by CI |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `library/clippy.log`; CI reran on the final service fixture |
| `cargo test --offline --workspace --no-fail-fast` | 0 | `library/workspace-tests.log`: 699 passed, 0 failed, 14 ignored across 66 harness summaries |
| `cargo build --offline --workspace --features privileged --tests` | 0 | `library/privileged-build.log`; compilation only |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | 0 | `library/ci.log`: source policy (146 owner tags), fmt, Clippy, same 699/0/14 test totals and privileged compilation passed; cargo-deny explicitly skipped as not installed |

Final focused logs: `focused-api-lock.log` exercises the new file-lock test (1/1). `focused-daemon.log` reports w6a3 13/13 and w6b 13/13; eleven w6a3 socket scenarios print EPERM and return, while the authorization/dispatch and new real-scheduler NoSpace cases execute. The new shared-directory service test is one of those eleven pending exchanges. `focused-cli-refusal.log` reports 1/1 with the explicit EPERM early return; its four fake replies were not exercised here. Source SHA-256 records cover all seven changed Rust files. No source mutation or privileged runtime was performed.

## Definition of Done checkpoint

- [x] Owned paths only; no shared-file/dependency edits.
- [x] Format, workspace Clippy, workspace tests and all available CI stages passed.
- [x] Existing privileged tests compile; execution pending VM.
- [x] Handoff, per-part tests/commands, assumptions and unresolved acceptance evidence recorded.
- [x] Existing owner headers retained; no new public undocumented item, unsafe block or production unwrap/expect.
- [ ] Real API/exporter same-directory service exchange and CLI fake-daemon mapping executed: pending unrestricted CI/guest.
- [ ] Installed Rocky behavior, privileged runtime and cargo-deny audit: pending coordinator/equipped host.

Four focused implementation commits cover the four parts; the final handoff commit includes the strengthened actual-service integration fixture and these validation results. Final cleanup and submission receipt are recorded in the external canonical attempt report. This handoff is evidence for independent review, not a verification or integration decision.
