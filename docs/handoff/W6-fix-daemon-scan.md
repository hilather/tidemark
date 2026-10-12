# Handoff — W6-fix-daemon-scan

Date: 2026-10-12. Branch: `hp-47d4409e5e91620bb2fcedc04d9d86a64e17e7f7277b8df85ca68b473b1e3c63`. Status: owned changes implemented; coordinator follow-up required.

## Needs from coordinator

The store persists process-local snapshot filenames. This is a further defect behind the original scan preflight failure, outside this task's owned paths. `SnapshotWriter::finish_with` returns its descriptor-anchored `final_path` (`tidemark-store/src/writer.rs:358`); `run_scan` copies it into `ScanEntry.file` (`tidemark-scan/src/orchestrator.rs:583`). `Catalog::record` persists those bytes unchanged (`tidemark-store/src/catalog.rs:175`). Retention's `Catalog::rebuild_from_files` also records `file.path()` under the anchored state directory (`catalog.rs:226,249`; called by `orchestrator.rs:606`). Consequently a separate CLI process interprets `/proc/self/fd/<daemon-fd>/...` against its own descriptors. A restarted daemon can also have a different descriptor number.

A standalone diagnostic using the real bound-storage scheduler completed a scan and joined both services. It printed `real directory snapshot exists: true`, `producer can resolve stored file: true`, and `separate process test -f stored file: exit status: 1`. Source and output are in the attempt report's `library/catalog-process-path.rs` and `.log`. No scan/store files were changed. Please arrange a store/scan follow-up that makes catalog filenames usable by readers in another process and after restart, while retaining descriptor-anchored writes and the frozen schema. Simply canonicalizing in the daemon recorder is insufficient: the orchestrator's catalog rebuild writes the anchored names again. The existing real operator test's separate CLI `top` is expected to expose this follow-up on an unrestricted root host; that prediction was not run here because sockets are denied.

Real Unix-socket and Rocky guest checks remain pending outside this sandbox. The existing five `w6a1` lifecycle/readiness failures prevent a green aggregate workspace/CI exit here. Details and commands follow.

## Cause and changes

The daemon retains its verified state-directory descriptor and passed `/proc/self/fd/<fd>` to scan orchestration. That final component is a symlink: opening it with `O_DIRECTORY | O_NOFOLLOW` fails with `ENOTDIR`. `LiveState::storage_path()` now returns `/proc/self/fd/<fd>/.`, whose final component is the directory itself. Startup's extra `.join(".")` was removed. The descriptor remains retained; the configured pathname is not substituted, and store no-follow opens remain intact.

The shared scheduler runtime harness now binds an opened temporary state directory before starting either the writer or scheduler. New tests exercise publication, status, actual snapshot location, temporary-file cleanup, state-directory exclusion, growth, series and predecessor removal under bound storage. They exercise an initial `scan.start` dispatch and a scheduled scan followed by another API request. The latter uses `api::start_with_peer` with an injected root credential on hosts permitting Unix sockets, and directly dispatches with injected uid 0 when the sandbox refuses sockets. A new unprivileged real-binary test waits at most 30 seconds for an actual `.db` snapshot, sends SIGTERM and checks exit code 0; it prints the socket-probe skip line and returns on EPERM. Its daemon lifecycle assertion messages include captured stderr.

Scheduler retry inventory is bounded by `MAX_SUBJECTS`. Failures retain their root, error, attempt count and next retry time; inventory admission also counts preflight-only roots. Failed scans emit one WARN with root, mode, scheduled/requested flag, error, attempt and retry time. Admitted scans emit INFO with root and scan id at start and successful completion. Component publications consult all retained failures, so running/waiting/success of another root cannot erase degraded health. A root's successful scan or its removal from configuration clears its failure. Reload resets attempts/delays while retaining unresolved error health. Manually requested roots absent from configuration retain health until successful recovery.

Retry delays are 30, 60, 120, 240, 480, 960, 1920 and then 3600 seconds, remaining at 3600. Busy and cancellation retain the existing fixed 30-second scheduled deferral without incrementing failures or creating degraded health. Operator requests bypass scheduled delay checks. `start` remains signature-compatible and delegates to a new documented `start_with_clock` seam using the existing `crate::Clock` type; retry/due computation and recorder schedule computation use that clock. Orchestrator-owned snapshot timestamps are unchanged. The complete retry sequence and reload reset are tested with an atomic injected clock and no sleeps.

## Storage-path consumers audited

| Consumer | Evidence / result |
|---|---|
| `lib.rs` startup catalog open/recovery and time-series open | All continue using the retained path; only the redundant startup space-check suffix changed. Workspace catalog-recovery/readiness tests reach socket-denied lifecycle boundaries; real full lifecycle is pending unrestricted CI. |
| `lib.rs` startup space check | Same no-follow directory operation used by successful bound scan guards now receives the directory form directly. |
| `services/tswriter.rs::start_with_clock` | All shared runtime tests open SQLite through the bound path and persist scan start/finish and series acknowledgements. No consumer patch needed. |
| `scheduler.rs::launch` | Scheduled and API-started walks complete with the retained path substituted into the frozen admitted scan config. |
| `scheduler.rs::initialize_schedules` | Bound harness and injected-clock tests initialize catalog history and configured-root status. |
| `scheduler.rs` idle catalog refresh | Tests observe scheduled completion, repeated scheduled scans, retries, reloads and recovery through this open. |
| `run_scan`: scan lock, catalog, previous snapshot metadata, space guard | Bound lock-conflict and full-scan tests pass. Second scan reads a previous snapshot through the retained path. |
| `run_scan`: snapshot directory/writer, rename and parent sync | Published files resolve under the real state directory; no `.db.tmp` survives. |
| `run_scan`: canonicalized state exclusion | Both first API and scheduled/second API snapshots omit a state directory nested inside their root. |
| `run_scan`: post-scan growth and series | Second snapshot exposes growth and persisted root series. |
| `run_scan`: retention deletion and catalog rebuild | With recent=1, daily=weekly=0, predecessor removal is observed after the second scan; current snapshot remains. Cross-process catalog filename limitation is described above. |

## Regression evidence

Before changing behavior, searched workspace tests for `storage_path`, the waiting message and retry/30-second assertions; no exact old path/waiting assertions or scan-retry timing contracts needed edits. Existing schedule/short-cadence/lock/cancellation coverage was retained.

Temporarily restored only the old `/proc/self/fd/<fd>` return value, with a shell EXIT trap restoring the fixed file after each check:

- `runtime::bound_storage_scheduled_then_api_scan_publishes_and_excludes_state`: failed at its 15-second completion deadline; 0 passed / 1 failed. `library/scheduled-without-fix.log`.
- `runtime::bound_storage_api_request_publishes_first_snapshot`: failed immediately with API error `scan: Not a directory (os error 20)`; 0 passed / 1 failed. `library/api-without-fix.log`.

With the directory form restored, the scheduler integration suite passes. The health test covers idle publication, success of another root, reload without premature recovery, removal of all configured roots, and requested recovery during a future retry delay. The injected-clock test drives the real scheduler through every exponential delay and reload reset. The Busy test proves its first subsequent actual failure still uses 30 seconds.

Unprivileged test inventory at the supplied base versus final code: daemon library 21→22, `w6a2` 11→16, `w6a3` 8→9, total 40→47 (+7). Base counts are source-inventory counts, not an independently rerun baseline. Whole-workspace inventory is 562 tests including 13 ignored scale tests; the base inventory is inferred as 555 (seven additions), not rerun separately. `cargo test --workspace -- --list` confirmed the final count. Socket-dependent tests returning early count as passed in Rust's summaries; they are explicitly pending real execution. No privileged tests were executed.

A temporary bound-harness logging diagnostic captured exactly one scheduler WARN for a scheduled missing-root scan (`mode="walk"`, `scheduled=true`, `attempt=1`, retry due 30 seconds later), and exactly one INFO start and completion for a successful requested scan, with matching scan ids. The diagnostic was removed with an EXIT trap and restoration checked byte-for-byte; its source/output remain in `library/logging-probe.rs` and `.log`. It is excluded from the permanent test inventory.

## Validation

All Cargo work uses `--offline` (or `CARGO_NET_OFFLINE=true` for the CI wrapper), pinned toolchain 1.98.0, `RUSTUP_HOME=/home/brewerm/.rustup`, and a temporary copy of the host registry cache at `CARGO_HOME=/tmp/tidemark-fix-cargo`. No dependency was added. Commands run under `timeout 1500`.

| Command | Result |
|---|---|
| `cargo fmt --check` | Exit 0, final source; `library/fmt-final.log`. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0, final source; `library/clippy-final.log`. |
| `cargo test --offline --workspace` | Exit 101: only five known `w6a1` socket-denied readiness failures before fail-fast stopped later targets; `library/tests.log`. |
| `cargo test --offline --workspace --no-fail-fast` | Exit 101: 544 passed, 5 failed, 13 ignored. Only failed target is `w6a1`; all other workspace targets and doctests ran; `library/tests-final.log`. |
| `cargo test --offline -p tidemarkd --lib --test w6a2 --test w6a3` | Exit 0 after final source adjustments: 22 + 16 + 9 = 47 passed; `library/daemon-final.log`. Socket skips remain as described above. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0, final source, compiled only; `library/privileged-build-final.log`. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0, final source; `library/docs-final.log`. |
| `tools/ci.sh` with offline environment | Exit 1 at the same five `w6a1` readiness failures. Source checks, fmt and clippy passed before tests. cargo-deny is unavailable and CI did not reach its optional deny stage; `library/ci.log`. |

The five unchanged readiness failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. Each fails at its existing readiness deadline when daemon API bind is denied. No test outside this set failed in the full no-fail-fast run. The new real-binary test's focused `--nocapture` run printed `pending unrestricted host: Unix socket bind denied by sandbox (EPERM)` and returned; `library/binary-scheduled.log`.

After the final source adjustments, reran fmt, workspace clippy, all 47 daemon tests, privileged compilation and rustdoc. The logging-only temporary diagnostic was subsequently restored byte-for-byte, and the workspace test-list command recompiled the final restored test module. No privileged suite was run.

## Pending unrestricted CI / guest commands

- Unprivileged scheduled real binary: `cargo test --offline -p tidemarkd --test w6a3 operator::real_binary_scheduled_scan_publishes_then_exits_cleanly -- --exact --nocapture` on a host permitting Unix sockets. It does not require root API access.
- Bound storage API with injected peer credentials over a real socket: `cargo test --offline -p tidemarkd --test w6a2 runtime::bound_storage_scheduled_then_api_scan_publishes_and_excludes_state -- --exact --nocapture` on an unrestricted unprivileged host.
- Existing full real operator scenario: `cargo test --offline -p tidemarkd --test w6a3 operator::real_operator_status_scan_top_now_reload_cancel_and_sigterm -- --exact --nocapture` on an unrestricted root host. Address the catalog-path follow-up before expecting its separate-process snapshot query to work.
- Existing readiness tests: `cargo test --offline -p tidemarkd --test w6a1 -- --nocapture` on an unrestricted host.
- Rocky 8.8 and 8.10 VM gate: `cargo xtask vm-test --host <vm>`; focused guest commands are `cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture` and `cargo test --offline -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture`. Existing privileged tests were compiled only here, pending VM execution. These commands are for disposable guests, not the workstation.

## Proposed plan text (not applied)

Add to Wave 6 scheduler behavior / architecture scheduler documentation: “After a scheduled scan fails, retry that root after 30 seconds, then 1 minute, 2 minutes, doubling to a maximum of 1 hour and retaining that maximum. Successful completion or accepted configuration reload resets the delay and failure attempt count. Busy/cancelled outcomes keep the fixed 30-second deferral and do not count as failures. Operator scan requests are not delayed by scheduled retry backoff. Keep scheduler health degraded with the root and error until that root completes successfully or leaves configuration; neither waiting publication nor another root's successful scan clears it. Reload resets backoff but does not establish recovery. Log each actual failure once at WARN, with root, mode, source, attempt and retry time, and admitted scan start/completion at INFO with root and scan id.”

## Assumptions and scope

- No user questions were issued. Accepted reload resets backoff at the scheduler's next idle boundary, consistent with existing admitted-config behavior.
- Requested failures participate in the same root failure inventory; future scheduled eligibility uses its retry time, while a new operator request bypasses it.
- INFO start is emitted when orchestration assigns/persists the scan id; preflight failures have no invented scan id and are reported at WARN.
- No frozen core types, wire methods, flags, config keys, metric names or snapshot schema changed. Existing public signatures/callers remain unchanged; the runtime harness alone calls the new scheduler clock seam. Existing production callers still use `start`.
- Owned source/test paths only. Shared-file edit: `tidemarkd/src/lib.rs`, startup `space_check(&storage, ...)` only. No manifests/module declarations or new dependencies. No scan/store edits, plan edits, branches, merges, pushes, privileged test execution or host service/storage changes.
- Task status is an evidence candidate: changes within owned paths are implemented; cross-process catalog paths and unrestricted/guest gates need coordinator work.
