# Handoff — CRASH-recovery: abandoned scan cleanup and compaction crashes

## Needs from coordinator

Independently review this evidence candidate, run the snapshot fault script as root in disposable Rocky guests, and run cargo-deny on an equipped host. Socket readiness and privileged runtime acceptance cannot be established in this worker sandbox. The requested `docs/handoff/W7-A2.md` is absent from this baseline; current reader interfaces and the available STORE-seams/catalog-paths notes were preserved.

## What changed

`catalog::cleanup_abandoned_scans(state_dir, now)` removes only regular `*.db.tmp` entries one level below `snapshots`, then repairs every `running` row to `failed`, with the supplied finish timestamp and NULL file. It streams directory entries and repairs one row per committed transaction, advancing through scan IDs with bounded memory. Successful removals and committed repairs each emit WARN with root/scan identity; individual errors are logged and recovery continues where possible. Nothing is logged when no work or errors exist. File and catalog failures are independent.

Cleanup opens directories with NOFOLLOW/CLOEXEC/NOATIME (EPERM fallback), binds entries with O_PATH/NOFOLLOW, and uses descriptor-relative fstat/unlinkat. It skips symlinks, directories with temporary-looking names, nested directories, other filenames, and different mount IDs, including bind mounts. The existing bounded fdinfo mount-ID reader is reused. File logs identify the encoded root directory; row logs use the raw root's lossy display. No scanned regular-file content is read.

`run_scan` cleans immediately after acquiring its existing exclusive lock, before recording the new scan, using its injected clock. Daemon startup tries the same nonblocking lock before services/scheduler start. Busy logs one INFO and skips cleanup; other lock/cleanup failures are warnings. Its retained `/proc/self/fd/<n>/.` storage path works as does an ordinary path.

A necessary retention seam was found: the existing automatic rebuild deleted the freshly repaired history. New `Catalog::rebuild_after_scan` retains failed/running lifecycle rows without files while refreshing published snapshots after automatic retention. The explicit `rebuild_from_files` contract and all existing signatures remain unchanged. The scan regression asserts that the repaired row survives the complete following scan.

`TimeSeries::compact` was already public; no compaction production change or new exposure was needed. A Rust subprocess test seeds 500,000 raw samples over 5,000 independent filesystem streams, kills the actual store compaction at ten fixed delays after a ready handshake (0, 1, 2, 5, 10, 20, 40, 80, 120, 200 ms), verifies SIGKILL, reopens through the store, checks SQLite integrity, completes compaction, compares all table digests with an uninterrupted oracle, and verifies a second compaction reports zero changes and identical digests. Baseline compaction must exceed the largest delay. The focused run measured 5.30 seconds and completed all ten cases in 128.32 seconds.

## Scope and shared-file edits

Only assigned sources, tests and this note changed. `space_guard::mount_id` became crate-visible solely for reuse; no new dependency, manifest, module declaration, schema, CLI, scheduler, core contract or plan edit. The daemon lib edit is only its startup call. Branch is the supplied attempt branch; no branch creation, merge or push.

## Assumptions

- The exclusive scan lock and root-owned private state directory exclude concurrent legitimate snapshot publishers during cleanup. Descriptor-relative unlink never follows a target symlink. As with POSIX unlink generally, a privileged actor replacing an entry between inspection and unlink is outside this serialization guarantee.
- Automatic retention must preserve repaired no-file history to satisfy the explicit “do not delete rows” decision; explicit operator catalog rebuild keeps its existing reconstruct-from-files behavior.
- The scan subprocess uses the actual configured gate at 10,000 ops/s (slower than `max`) over 15,000 directories. The parent observes writer creation before SIGKILL; no production fault hook was added.
- Daemon startup tests deliberately continue far enough to run cleanup before API binding; denied sockets are permitted after that checkpoint, and a pre-set shutdown bounds unrestricted execution. The restart test asserts actual catalog/temp results, so an earlier startup failure cannot produce a passing repair assertion.
- Test child entry points return immediately without their opt-in environment variables. Their ordinary harness passes are counted separately from actual subprocess crash scenarios.
- Offline host Cargo/Rust caches and pinned 1.98.0 were used. No privileged test executed, no host system configuration changed, and no memory projection was edited. Canonical attempt receipts were checked; no delivered update was pending.

## Regression tests and counterfactual behavior

- Store `cleanup_is_selective_repairs_rows_and_is_idempotent`: exercises both path forms, selective removal, symlink/directory preservation, completed-row preservation, cleared file and stable finish timestamp on repetition. Without cleanup its temporary and running row remain.
- Store `empty_and_missing_snapshot_directories_are_supported`: proves missing state/snapshot handling. This boundary case can also pass before cleanup exists if no-op behavior is substituted; it is compatibility coverage.
- Store `file_cleanup_failure_does_not_prevent_catalog_repair`: a symlinked snapshots directory causes a no-follow open failure; the outside target survives and the running row still repairs. Without row repair the assertion fails.
- Store `catalog_failure_does_not_prevent_file_cleanup`: corrupt catalog bytes remain intact while the temporary disappears. Without temporary cleanup the assertion fails.
- Scan `killed_scan_is_cleaned_and_following_scan_completes`: re-executes its test binary, observes `.db.tmp`, kills/waits, verifies an abandoned running row, then calls the public `run_scan`; temporary removal, retained failed history and successful next publication are asserted. Without the call the temporary remains; without the retention seam the repaired record disappears.
- Daemon `shutdown_interrupted_outcome_is_repaired_at_startup`: replaces the old open-point assertion with actual lifecycle startup, catalog finish timestamp 123 and temporary removal. Without the startup call it stays running.
- Daemon `startup_skips_cleanup_while_another_process_holds_lock`: another re-executed test binary holds ScanLock; startup preserves row/temp, then a later startup after SIGKILL repairs them. Removing the lock guard makes the first preservation assertion fail; omitting cleanup makes the later repair fail.
- Store `ten_compaction_crashes_recover_and_are_idempotent`: new coverage of an already correct transaction-based compactor; no claimed compaction production regression/fix. All table contents are hashed in deterministic column order with type-preserving value representations, including empty non-filesystem tables.

## Proposed plan text (not applied)

“While holding the host scan lock, before a new running record, clean regular `snapshots/*/*.db.tmp` files without following symlinks or crossing mounts and repair abandoned running catalog records to failed with finish time and no file. Daemon startup attempts this lock nonblockingly before starting the scheduler, logging INFO on Busy; errors are logged without preventing startup or scans. Automatic snapshot retention preserves failed no-file lifecycle records. Exercise compaction crashes directly through its public store entry point with fixed-delay SIGKILL, integrity checks, uninterrupted-result comparison and repeated-compaction table digests.”

## Guest command — pending VM

As root in a disposable Rocky guest, from the candidate checkout, with the existing VM marker:

```sh
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 bash tests/fault/02-snapshot-crash.sh
```

Also run the existing privileged gates via the coordinator's VM workflow. No privileged runtime was run here.

## Validation

Final command exits, counts and remaining limitations are appended after completion. Evidence artifacts live in this attempt's worker-output `library/`. Every long command uses `timeout 1500` and offline Cargo.

| Final-source check | Outcome | Artifact |
| --- | --- | --- |
| `cargo fmt --check` | 0; also repeated after exact restoration | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | Initial 101: 689 passed / 1 failed / 14 ignored; recheck with `RUST_TEST_THREADS=2` exit 0: 690 / 0 / 14 | `workspace-tests.log`, `workspace-recheck.log` |
| `cargo build --offline --workspace --features privileged --tests` | 0; compilation only | `privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 | `rustdoc.log` |
| `tools/ci.sh` with offline environment | 0: all available checks passed, 690 / 0 / 14; cargo-deny absent and explicitly skipped | `ci.log` |

The first final-source workspace failure was the unchanged `w6b::atomic_publication_and_modes`, at w6b.rs:280, returning OS error 11 (`WouldBlock`) while repeatedly writing metrics. Its isolated exact recheck passed, as did CI and the full two-thread workspace recheck. The related earlier W6-watch-windows handoff records an intermittent metrics-lock failure in another w6b test; the underlying cause of this occurrence is not established. No metrics source/test changed. Keep the initial failure as evidence rather than claiming it never happened. Some socket/root operator tests return early in this sandbox; harness passes do not prove their transport scenarios ran. No socket acceptance assertion was weakened by this task.

Test counts: store cleanup 4/4; compaction 2/2 harness tests with ten actual SIGKILL trials; scan recovery 2/2 harness tests with one actual killed scan; daemon w6a4 5/5. These changes add ten ordinary test definitions (three are opt-in subprocess helpers), and replace one existing daemon open-point test. Final workspace summaries aggregate 690 passes and 14 existing ignores. No clean-base workspace runtime was measured; no baseline pass count is claimed.

Seven real counterfactual runs were performed: disabled the cleanup body, then ran the selective store, independent file/catalog error, killed scan, daemon startup repair and daemon lock/retry cases; each exited 101 with one test assertion failure. Separately restored `rebuild_from_files` in the scan retention paths: the killed-scan test failed because the repaired catalog row disappeared. The script restored exact saved source bytes in `finally`; SHA-256 comparisons against the required-check source hashes passed. No counterfactual source is committed. `counterfactual-summary.log`, seven `without-*.log` files and `checked-source-hashes.json` retain the evidence. The empty/missing-boundary test and helper entry points are compatibility/harness coverage, not claimed negative regression checks. Compaction is coverage of existing behavior rather than a production fix.

Initial development checks caught test-helper unwrap lint allowances and an extra injected-clock read. Both were corrected before the final-source checks above; cleanup and preflight retention now share the first clock read, preserving the existing provenance test unchanged. An initial single-stream compaction fixture was interrupted because it took too long; the committed fixture uses independent bounded streams and completed all trials quickly. No result from the interrupted fixture is claimed.

## Remaining checks

- Independent review and integration decision: pending coordinator.
- Rocky snapshot fault script and existing privileged runtime gates: pending VM; privileged code compiled only.
- Real socket readiness: pending unrestricted CI/guest where guarded operator scenarios execute.
- cargo-deny dependency/advisory audit: pending an equipped host.
- Intermittent pre-existing metrics publication lock failure: coordinator follow-up; exact recheck and subsequent aggregate runs passed.

The implementation, local validation, and failure evidence are submitted as a candidate, not a verified release/task-success declaration. Final cleanup and submission receipt are recorded in the external attempt report.

After counterfactual restoration, `cargo fmt --check` passed and the focused store/scan/daemon recovery targets passed 11/11 (4 store + 2 scan + 5 daemon). `restored-positive-tests.log` records this final positive recheck; source hash comparison confirmed byte-for-byte restoration to the required-check tree.
