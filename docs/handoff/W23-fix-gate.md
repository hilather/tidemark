# Handoff — W23-fix-gate: cancellable waits and safe, charged opens

## Needs from coordinator

Workspace validation is blocked by an outside-scope baseline compile error: `crates/tidemarkd/src/lib.rs:757:22` calls missing `state_files::validate_marker_roots` (E0425). The included deployment module instead defines `validate_marker_roots_at` at `crates/tidemark-cli/src/deploy/files.rs:125`. Coordinate the daemon/deployment owners; this attempt does not change their files. Privileged runtime and the daemon's actual shutdown gate remain pending VM.

- Attempt: attempt-de8ded06c89fbe656c7f5b9716e5810b165e38b8df7e71e4a8792999f6c05d05
- Base: eb9524063dd667c34da312f71bb498f9a7014041
- Status: implementation evidence candidate, independent verification pending.
- No branches created, merges, pushes, privileged operations or host configuration changes.

## Findings

| Finding | Implementation status | Files and regression evidence |
|---|---|---|
| B1: blocked acquisition prevents timely cancellation | fixed now | `fs/src/throttle.rs`: permanent, idempotent cancellation wakes the bucket condition variable under its mutex; FixedGate, AdaptiveGate and ScanGate expose cancel. `scan/src/orchestrator.rs::with_cancellation`: scoped monitor polls at 50 ms across preflight and all walk/bulk phases. Walker and checked directory readers check cancellation after every charge; bulk phase 3 uses the additive cancellable reader. Bulk phase 2 already had the required post-acquire check in `bulk/join.rs::stream`, retained unchanged. Tests: `cancellation_releases_reserved_bulk_wait` observes an actual 129-token reservation at 1 op/s under the bucket mutex after a barrier; `adaptive_cancel_wakes_all_waiters_and_preserves_stats`; `cancellation_monitor_releases_bulk_cost_during_preflight_or_any_phase`; `low_rate_walk_cancellation_cleans_up_and_releases_lock`; both CLI SIGINT tests now use 1 op/s and a two-second post-signal deadline. |
| S3: root ancestors follow symlinks | fixed now | `scan/src/orchestrator.rs::open_directory` delegates to `fs/src/walk/getdents.rs::open_directory`, exported and documented in `walk/mod.rs`. Every component opens relative to the preceding no-follow descriptor from /; ancestors use O_PATH, the final directory uses O_RDONLY and O_NOATIME with EPERM fallback. No metadata probe occurs until this succeeds. `root_open_refuses_symlink_ancestor` replaces an ancestor with a symlink after constructing the raw root and requires refusal. |
| S4: uncharged retry calls | fixed now | Root setup shares the per-attempt charged walker helper. `noatime_fallback_charges_each_attempt_and_checks_cancel` injects EPERM into the actual attempt helper and checks two charges, two attempts, fallback count, and cancellation before the retry. `fs/src/symlinks.rs::read_target` uses readlinkat_raw with a reusable 256-byte buffer growing to a 64-KiB cap, charging and checking cancellation before each syscall. `long_target_retries_are_charged` preserves a 300-byte target and requires exactly one more charge than a short target. |
| Completion order: scan_finished, dir_series, retention | already fixed in baseline; documented and tested now | `ScanRecorder` and `run_scan` rustdoc specify the order and predecessor availability. Extended `automatic_retention_rebuilds_catalog_after_deleting_old_files` asserts the previous file exists in scan_finished and dir_series, completion precedes series, and recent=1/daily=0/weekly=0 deletes it afterwards. |

## Reproduction and design

The baseline reservation regression failed after a two-second timeout; it rescued its blocked worker by raising the test bucket rate before joining. The baseline ancestor-symlink regression failed because root opening succeeded. The long-target regression failed with 16 charges instead of the expected 17. These failures were observed before the respective fixes were validated. Logs/summary are in the canonical output library.

Gate signatures and the existing consuming ScanGate::stop behavior are preserved: stopping the controller leaves ordinary acquisitions at the last rate. Cancel is separate and irreversible. The additive stop_sampling method lets the orchestrator stop sampling at the existing statistics boundary while retaining the handle borrowed by its scoped monitor. Statistics remain readable after cancellation. The monitor owns no unbounded queue, wakes immediately on work completion/channel disconnection, joins on errors and unwinding, and cannot outlive run_scan. The normal cancellation polling interval is 50 ms; condition-variable notification releases blocked reservations immediately subject to thread scheduling.

The frozen read_dir_entries_checked signature remains intact; the additive read_dir_entries_checked_cancellable passes cancellation through every open, retry, identity stat and getdents refill. Existing wrappers use an unset flag. Symlink analysis also checks its flag after every acquisition. Cancellation during root preflight now returns before metadata I/O; like other pre-start failures, it has no started catalog row. Cancellation after scan_started is finalized as failed, or retains a valid partial walk when requested.

## Scope and dependencies

Only assigned paths changed. No Cargo manifests, lockfiles, dependencies, core types, CLI sources, lib.rs files, plans or memory projections changed. walk/mod.rs exports the two additive directory helpers; it is explicitly owned. Most of orchestrator.rs's textual delta is rustfmt indentation around the scoped cancellation closure.

## Validation

All Cargo commands use pinned Rust 1.98.0, --offline and CARGO_NET_OFFLINE=true, with a private copy of the host registry cache at /tmp/tidemark-gate-cargo. Required full-workspace commands were attempted; their daemon compile failure is reported above. Final command results are recorded in the attempt report and library logs.

| Final command | Observed result |
|---|---|
| cargo fmt --check | exit 0 |
| cargo clippy --offline --workspace --all-targets -- -D warnings | exit 101, outside-scope daemon E0425 |
| cargo test --offline --workspace | exit 101, same daemon error |
| cargo build --offline --workspace --features privileged --tests | exit 101, same daemon error |
| tools/ci.sh (offline environment) | exit 1; source checks (139 owner tags) and format pass, Clippy blocked by daemon |
| cargo clippy --offline -p tidemark-fs -p tidemark-scan -p tidemark-cli --all-targets -- -D warnings | exit 0 |
| cargo test --offline -p tidemark-fs -p tidemark-scan | exit 0; 149 passed, 5 existing scale ignores |
| cargo test --offline -p tidemark-cli --test w2c | exit 0; 15 passed, 1 existing scale ignore |
| cargo build --offline -p tidemark-fs -p tidemark-scan -p tidemark-cli --features privileged --tests | exit 0; compilation only |
| git diff --check | exit 0 |

An additional all-tests invocation including tidemark-cli stopped at its outside-scope surface::daemon_check_config_binary test: that test invokes a daemon build and receives the same E0425. The complete FS/scan suites and owned CLI W2-C target were then run separately as above. No tests were removed, skipped or weakened to hide this error.

## Cancellation limits and pending runtime

This removes userspace token-wait delays, not every possible shutdown delay. A syscall already executing in the kernel can remain blocked indefinitely; no kernel deadline is introduced. Aggregator preparation, in-place bulk sorting, collector processing, recorder callbacks and catalog/retention work have no fixed duration bound. Publication checks cancellation between SQLite indexes, metadata/sync and rename; one index build or sync can take an unbounded duration before its next checkpoint. Sampler/controller joining retains the documented sampler completion bound of one configured interval; scheduling itself is not a real-time guarantee. Therefore no universal ten-second daemon shutdown or kernel-I/O deadline is claimed.

Existing privileged filesystem/mount/bulk regressions are compiled only, pending VM; no privileged runtime command was executed. The EPERM regression uses injected attempts, not host ownership assumptions or uid/group checks. Cargo-deny availability is subject to the environment; workspace CI stops before reaching it because of the daemon compile error.
