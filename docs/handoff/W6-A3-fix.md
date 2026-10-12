# Handoff — W6-A3-fix: Enable daemon API and repair Wave 6 tests

## Needs from coordinator

The required workspace tests and CI are blocked here by five out-of-scope W6-A1 readiness tests that do not probe socket bind permission. Please run on an unrestricted host, or authorize a separate task to add sandbox bind probes to `crates/tidemarkd/tests/w6a1.rs`. Production startup deliberately propagates bind failure; it does not falsely advertise API readiness.

Real socket and operator acceptance still needs an unrestricted root host; sandbox Unix bind returns EPERM. Privileged acceptance, Rocky VM gates and RSS/soak measurements remain pending VM. This is an evidence candidate, not independently verified completion.

- Attempt: attempt-373f9be571f528f20a029cb2321e07d735cdefb8566bb3f2d85c08e901baf837
- Date: 2026-10-12

## What changed

W6-A3 coordinator items 1–3 are implemented. The workspace rustix feature list now enables `net` and `termios`, with the existing version and no new dependencies. `api::start` calls the existing transport with kernel SO_PEERCRED; dispatch accepts only uid 0, including when a daemon is started by another uid. The existing daemon lifecycle call already starts this entry point and needed no edit. Human now rendering queries stdout terminal dimensions only for a terminal, uses positive column counts and falls back to 100 for nonterminals, zero width or query failure.

Human `status --show-config` explicitly states that its effective configuration is the CLI's and the running daemon may hold an older configuration until reload. This explanation is printed in the owned status command because the existing status renderer is outside this task's write scope. JSON and frozen API payloads are unchanged.

Replaced the obsolete Wave 5 exit-4 assertion with direct-setup checks. A deliberately missing explicit mount is the existing seam: direct canonicalization fails before `ProcScanner::scan_once`, so content assertions never enumerate this machine's /proc. Binary tests compare fallback and explicit-direct failures and assert exactly one notice only for fallback. A listening-socket test asserts explicit direct makes no connection. Existing binary tests retain exit 4 and the documented service hint for now/hot/alerts/status. No phantom/writers production or direct-mode code changed.

Added real-socket tests for injected nonroot refusal and production credential lookup. Existing authorized transport fixtures inject root independently of the test runner's uid. The operator scenario retains daemon stderr, includes it in startup/deadline failures, and exercises the real lifecycle socket when bind works and the caller is root. Its root requirement follows the coordinator's uid-0-only decision.

## Shared-file edits

| File | Change | Justification |
|---|---|---|
| Cargo.toml | Add only rustix features net and termios | Safe SO_PEERCRED and terminal width, explicitly authorized |

Cargo.lock unchanged; both features resolve offline from the copied host registry cache. No module declarations, other manifests or public signatures changed. No unsafe code added.

## Assumptions

- The task's explicit root-only credential decision supersedes older plan passages accepting the daemon's own uid. No group or additional uid check was added.
- A nonroot unrestricted run can test kernel refusal, but cannot complete the production operator scenario; that scenario reports the root-host prerequisite and returns.
- The existing missing-mount error before scanner construction is sufficient to observe direct mode without changing direct code or introducing a new library seam.
- Existing JSON status output remains the v1 StatusResult; show-config's explanatory text applies to human output.
- Offline validation uses pinned Rust 1.98.0 on PATH and CARGO_HOME=/tmp/tidemark-fix-cargo, a copy of /home/brewerm/.cargo/registry. The initial default-home check could not find base64; the populated host cache resolved the workspace and added features successfully.

## Acceptance evidence

- `render::now::width_tests::terminal_width_selection`: injected terminal answer, query skipped for nonterminal, positive/zero/missing widths.
- `every_method_authorizes_before_dispatch_and_validates_parameters`: root dispatch and nonroot refusal across every frozen method.
- `injected_nonroot_peer_is_forbidden_over_real_socket`: uid 1000 refused through the injected credential provider.
- `lifecycle_start_uses_kernel_credentials_over_real_socket`: production start answers root status or refuses a nonroot caller.
- `operator::real_operator_status_scan_top_now_reload_cancel_and_sigterm`: retained full real-daemon scenario, pending unrestricted root host.
- `missing_daemon_takes_direct_setup_without_scanning_proc`, `phantom_and_writers_fallback_notice_and_direct_setup_seam`, `explicit_direct_never_connects_to_listening_daemon`: revised fallback contract.
- `live_commands_without_server_exit_four`: required commands retain exit code/message.
- `live_tables_and_shared_byte_path_machine_output`: now includes the show-config reload explanation.

## Validation

Before: the inherited W6-A3 handoff records one failing obsolete Wave 5 test; a fresh baseline runtime total was not collected. Source inventory for the changed test files was 14 tests before and is 19 after (Wave 5 command tests 4→4, CLI W6-A3 4→6, daemon W6-A3 including operator 6→8, width selection 0→1). The removed test is replaced, and five tests are added. Final workspace result counts are recorded below. All long commands use timeout 1500 and offline mode. Logs live under this attempt's worker-output/library directory. Privileged tests are compiled only; no privileged workstation commands were run.

## Remaining item 4

Socket bind restrictions are environmental and remain open. Early-return socket tests are not runtime acceptance evidence. The existing privileged daemon_end_to_end scenario is retained and must run on the VM. No claim is made about deployment, VM performance or unrestricted end-to-end operation.

## Final command results

| Check (offline; timeout 1500) | Exit | Evidence |
|---|---:|---|
| cargo fmt --check | 0 | library/fmt.log |
| cargo clippy --workspace --all-targets -- -D warnings | 0 | library/clippy.log |
| cargo test --workspace | 101 | library/test.log: 491 passed, 5 failed, 13 ignored before stop |
| cargo build --workspace --features privileged --tests | 0 | library/privileged.log; compile only |
| RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps | 0 | library/doc.log |
| tools/ci.sh | 1 | library/ci.log: same five readiness failures; source-policy scan passed, 140 owner tags |
| cargo test -p tidemarkd -p tidemark-cli --test w6a3 -- --nocapture | 0 | library/owned-tests.log: 14 harness passes, 9 early returns after EPERM, 5 exercised tests |

The width test and all four Wave 5 command tests also exercised assertions and passed in the workspace run. Workspace/CI stop before daemon W6-A2/W6-A3 tests and do not reach the advisory/license check. Final CI reaches the same 491 passed / 5 failed / 13 ignored result, not a green suite. Baseline's obsolete failure is gone; the five readiness failures appear now because production API bind is actually attempted.

The five failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`, all in unowned `crates/tidemarkd/tests/w6a1.rs`. Their waits time out after API bind fails. A direct temporary-config daemon invocation confirms `API socket: Operation not permitted (os error 1)` and exit 1 (`library/daemon-bind.log`). This is item 4's known environmental restriction, not a reason to disable production credentials or swallow socket startup errors.

## Definition of Done checkpoint

- Owned paths only; rustix features are the sole manifest change.
- Formatting, Clippy, rustdoc and privileged test compilation passed.
- Items 1–3 implemented; production socket acceptance pending unrestricted host.
- Item 4 remains open; workspace tests/CI cannot be green in this sandbox without changing unowned W6-A1 tests or running them on an unrestricted host.
- Existing privileged scenario retained: pending VM. No RSS/soak claims.
- Public signatures unchanged; no unsafe or direct-mode implementation edits.

Supplemental daemon run excluding the five named environment-blocked tests exited 0: 62 passed, 0 failed, 0 ignored, 5 filtered out (`library/daemon-remaining.log`). This substantiates the remaining daemon suites, while bind-dependent tests still return early.
