# Handoff — W2-C: Orchestrator, mode selection, progress and scan/query CLI

- **Branch:** hp-da73b87c390b218db7c6c7dee643212999c62ce3c40d9c92ed15a7d6eca69d96
- **Agent/session:** Codex / attempt-da73b87c390b218db7c6c7dee643212999c62ce3c40d9c92ed15a7d6eca69d96
- **Date:** 2026-10-11
- **Status:** partial implementation; end-to-end acceptance pending dependencies, contract decisions and VM evidence. This is an evidence candidate, not verified task success.

## Needs from coordinator

1. Merge operational W2-A and W2-B implementations, then enable and run the three tests marked `needs W2-A` below. The baseline aggregator and bulk pipeline still return NotYetImplemented.
2. Resolve [SCR-001](scr-W2-C/SCR-001.md): bulk failure phase, live progress, partial cancellation ownership and namespace noatime counts are absent from the frozen bulk interface.
3. Resolve [SCR-002](scr-W2-C/SCR-002.md): whole-scan memory accounting and retry ownership for a collision during final publication. The candidate accounting is implemented, but total memory is not a hard budget guarantee.
4. Run the VM-only auto-selection test on Rocky 8.8 and 8.10 after W2-A/B land. No VM was available here.
5. Decide whether pruning should delete catalog rows or keep historical scan records. The store exposes no catalog deletion method; this implementation removes snapshot files and leaves catalog rows. `rebuild-catalog` removes rows for absent files. The current store rebuild API also rebuilds globally: `rebuild-catalog --root` validates the path but does not narrow the rebuild.
6. Daemon startup must stop its sample writer and close connections before catalog recovery. `run_scan` uses ordinary, non-destructive catalog opens; direct CLI preflight and rebuild recover only with the daemon absent, the host lock held, and no retained connections.

## What was built

Implemented pure mode selection and deterministic progress cadence/ETA. The orchestrator holds an exclusive nonblocking flock, discovers mounts, constructs and stops ScanGate, refuses resource/space failures, prunes eligible old complete scans while protecting recent/latest scans, collects walk records on a bounded collector thread, invokes the frozen bulk interface, annotates and streams snapshots, records lifecycle failures and directory series, and applies retention. Final walk metadata uses the uncapped WalkSummary error/race totals rather than truncated detail vectors. A bounded heap retains at most 20,000 series rows, ordered by disk size and raw path ties.

Implemented direct CLI scan with canonicalization before RawPath construction, configured mode defaults, SIGINT/SIGTERM cancellation and typed exit codes. Added catalog list/show/prune/rebuild, top with filters/tree layout/wide columns/older-snapshot signed growth and live walk comparison, and notable-file queries. Renderers preserve arbitrary path bytes, handle unresolved names honestly, support ASCII/Unicode branches, and emit versioned JSON. Seven external insta snapshots and an inline progress snapshot are committed.

## Shared-file edits

None. No manifest, dependency, lockfile, core contract, module declaration, plan, shared test or other task source was changed. Test entry points use Cargo's existing directory/main.rs discovery. No new dependencies were needed.

## Deviations from the spec

| Spec reference | Implementation / limitation | Reason / request |
|---|---|---|
| W2-C run sequence | Gate construction precedes gated root stat/statvfs and space estimation; mode/resource preflight precedes recording a started scan | Root metadata must pass through Gate; ScanEntry requires the selected mode. Preconditions fail without a running catalog row. |
| 04 §7 | Queue capacity 1–64; quarter of remaining estimated budget assigned to queue; candidate reserve includes the 4096 threshold cap, maximum 255-byte names, overhead, producer heaps/buffers; 1 MiB extra per record and 64 MiB base reserve | Additional record and aggregator storage remain estimates, not hard maxima. SCR-002. |
| W2-C step 7 / 04 §8–9 | Bulk failures fail closed; no automatic fallback, bulk partial publication or live bulk progress | Current API cannot distinguish phase or recover consumed state. SCR-001. |
| 05 §2 | Id selection and writer-creation collisions retry up to 1024 times; a finish-time collision safely fails without replacing the existing file | Consuming writer/prepared APIs prevent republishing completed rows. SCR-002. |
| 05 §8 | Emergency pruning considers old complete scans outside recent/latest protection | Expired partial/failed files are handled by normal retention after successful scans or explicit prune, but not emergency preflight pruning. Remaining gap. |
| 05 §1 / 04 §9 | host/kernel populated via uname; os_release empty; partial walk totals retain available record counters | No OS-release discovery seam used. Cancelled WalkError does not return WalkSummary; namespace noatime counts and accurate capped-error per-directory progress require upstream support. |
| 04 §8 | Walk cancellation with keep-partial proceeds to prepare/write; other fatal walk failures abort | General fatal-failure partial recovery is not implemented. |
| 05 §4/7 | Prune removes files, retains historical catalog records; rebuild is global | No catalog deletion/root-filtered rebuild API. Needs coordinator decision. |
| W6-A future daemon branch | Direct scan refuses a reachable daemon; --cancel reports exit 4 | Daemon forwarding is explicitly assigned to W6-A. |
| Build instructions | All cargo commands run offline using the pinned 1.98.0 toolchain and existing host cache | No network dependency downloads. |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| scan then top works in both modes on VM | Written `auto_xfs_mount_root_and_small_subdir_record_reasons`; walk CLI end-to-end test written | Pending W2-A/B and VM; not claimed |
| Oracles for every mandatory fixture | `mandatory_walk_fixtures_match_du_find_for_every_directory` iterates all eight fixtures, every directory, du disk/apparent and find counts; apparent normalization probes du and measures metadata | Written, ignored `needs W2-A`; not run |
| JSON schemas and committed insta snapshots | `top_json_schema_and_paths`, `file_rendering_shared_and_unresolved`, `outcome_json_schema_and_text`; seven snapshots under CLI tests/w2c/snapshots | Passed locally |
| Mode truth table/reasons | `mode::tests::truth_table` covers every auto condition, explicit walk/bulk, 50% boundary and arithmetic overflow | Passed locally |
| Nonblocking host lock | `host_lock_is_exclusive_and_released`, `lock_rejects_symlink` | Passed locally |
| Space refusal/no partial artifacts | `injected_space_refusal_creates_no_snapshot`, `emergency_prune_protects_recent_and_latest` | Passed locally |
| Cancellation status/cleanup | `cancellation_drains_bounded_walker`, real subprocess `sigint_records_failed_and_leaves_no_snapshot` | Passed locally |
| Progress cadence and recorder calls | `cadence_and_eta`, `progress_reports_while_a_worker_is_blocked`, `log_cadence`, `lifecycle_records_failure_cancellation_and_success` | Passed locally |
| Id retries and corruption preservation | `scan_ids_retry_without_replacing_and_reject_invalid_years`; ordinary-open/recovery tests in scan and CLI | Passed locally; finish-time retry gap remains |

## Tests added

Unprivileged scan unit tests:
- `mode::tests::truth_table`
- `progress::tests::cadence_and_eta`
- `orchestrator::tests::host_lock_is_exclusive_and_released`
- `orchestrator::tests::lock_rejects_symlink`
- `orchestrator::tests::queue_budget_and_zero_top`
- `orchestrator::tests::scan_ids_retry_without_replacing_and_reject_invalid_years`
- `orchestrator::tests::lifecycle_records_failure_cancellation_and_success`
- `orchestrator::tests::injected_space_refusal_creates_no_snapshot`
- `orchestrator::tests::emergency_prune_protects_recent_and_latest`
- `orchestrator::tests::ordinary_catalog_open_preserves_corrupt_bytes_and_explicit_recovery_logs_paths`
- `orchestrator::tests::cancellation_drains_bounded_walker`
- `orchestrator::tests::progress_reports_while_a_worker_is_blocked`
- `orchestrator::tests::deletion_refuses_paths_outside_state`

Unprivileged CLI tests:
- Unit `render::scan_progress::tests::log_cadence`
- `tree_unicode_ascii_and_raw_paths`
- `file_rendering_shared_and_unresolved`
- `top_json_schema_and_paths`
- `outcome_json_schema_and_text`
- `catalog_queries_growth_limits_filters_and_canonicalization`
- `missing_growth_is_exit_five`
- `rebuild_preserves_corrupt_database_and_dry_prune_is_read_only`
- `sigint_records_failed_and_leaves_no_snapshot`

Written but ignored pending W2-A:
- Scan integration `mandatory_walk_fixtures_match_du_find_for_every_directory`
- CLI integration `scan_then_top_files_and_snapshots_end_to_end` (also tests --live without another publication)
- CLI integration `sigint_keep_partial_publishes_partial_and_is_not_latest`

Privileged, feature gated and not ignored:
- Scan integration `auto_xfs_mount_root_and_small_subdir_record_reasons` — **pending VM**, compilation passed. Uses test-support LoopXfs; never executed on this host.

## Validation

Pinned Rust 1.98.0; PATH=/home/brewerm/.cargo/bin; RUSTUP_HOME=/home/brewerm/.rustup; CARGO_HOME=/home/brewerm/.cargo; CARGO_NET_OFFLINE=true. Logs and exit files are in this attempt's worker-output/library directory.

| Check | Local result | Evidence |
|---|---|---|
| cargo fmt --check | exit 0 | fmt.log / fmt.exit |
| cargo clippy --offline --workspace --all-targets -- -D warnings | exit 0 | clippy.log / clippy.exit |
| cargo test --offline --workspace | exit 0; 195 passed, 0 failed, 6 ignored (3 W2-C, 3 existing scale) | test.log / test.exit |
| cargo build --offline --workspace --features privileged --tests | exit 0, compilation only | privileged.log / privileged.exit |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | exit 0 | doc.log / doc.exit |
| tools/ci.sh, offline environment | exit 0; cargo-deny absent and skipped by existing xtask | ci.log / ci.exit |
| git diff --check | exit 0 | whitespace.log / whitespace.exit |

## Measurements

No RSS, throughput, bulk/walk timings or VM measurements are claimed. Candidate layout is measured by size_of at runtime in the capacity formula. Task-specific passing unprivileged tests: 22; task-specific ignored dependency tests: 3; task-specific privileged tests compiled: 1.

## Known gaps and follow-ups

The task is not complete against all acceptance criteria. Operational end-to-end scan output is unavailable with the baseline W2-A/B stubs, so oracle/live/partial publication assertions have not run. Bulk fallback/progress/partial recovery, hard whole-scan memory bounds, publication-time collision retry, expired-partial emergency pruning, OS-release metadata, general fatal-walk partial recovery, and catalog pruning/rebuild semantics need follow-up as described above. Capped stored details can undercount per-directory progress errors, although completed walk snapshot metadata uses the uncapped summary. No privileged runtime commands, mounts, LVM actions, service changes or writes to production configuration were performed.

## Notes for the reviewer

Recovery is an explicit CLI/startup operation, never an automatic action while a daemon recorder may retain connections. Channels are bounded; cancellation drains output to unblock producers; all child threads are scoped and joined. Directory-series retention uses a bounded heap rather than a per-row linear search. Snapshot writes delegate no-replacement publication, temporary cleanup and 0600 permissions to W1-E. Deletion checks the canonical parent is inside state_dir/snapshots. Review the acknowledged estimates in SCR-002 before using this candidate on large link-heavy trees.

## Definition of Done

- [x] Only owned paths changed; no shared edits
- [x] Available local CI, format, lint, tests and rustdoc passed
- [x] Privileged test written and compiled; status pending VM
- [x] Public items documented; ownership headers present
- [ ] All runtime acceptance criteria satisfied
- [x] No production unwrap/expect or undocumented TODOs introduced
- [x] Handoff and contract requests written
