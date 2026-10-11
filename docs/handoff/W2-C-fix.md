# Handoff — W2-C-fix: review findings

- Attempt: `attempt-0db272d9971905c9d902fad027892b2f99c7af466f9b781475bbec7c5ba8a8a3`
- Base: `74e16714453fd3992269de91d92fae70274c84af`
- Date: 2026-10-11
- Status: evidence candidate; coordination and one storage-interface gap remain. This is not verified task success.

## Findings

“Fixed now” describes the candidate implementation, not independent verification. Except for the walker library test, new regression execution is blocked by the out-of-scope constructors listed below.

| Finding | Status | File and test evidence |
|---|---|---|
| 1: record memory bound | not done | Owned implementation in `tidemark-fs/src/walk/{mod,worker}.rs` and `tidemark-scan/src/orchestrator.rs`; `record_byte_bound_names_directory_and_preserves_smaller_records` passes. `record_limit_derivation_table` is written. Workspace callers still need the new field; see coordinator needs. |
| 2: cancellation while writing | not done | `orchestrator.rs` checks every RowSink batch and immediately before calling finish; `cancellation_after_last_batch_changes_publication_policy` and the row-batch checks in `finish_collision_preserves_existing_snapshot_and_removes_temporary_file` are written. Cancellation inside the store's finish remains unsupported. |
| 3: failed completion recorder | fixed now | `orchestrator.rs`: `CompletionRecorder` tracks acknowledged completion; `rejected_completion_is_finalized_as_failed_preserving_original_error` covers rejected completion, published-file preservation and cleanup failure. |
| 4: prune/query consistency | fixed now | Explicit CLI prune was already fixed in `cmd/snapshots.rs`, shown by existing `prune_rebuilds_catalog_for_remaining_files`. Extended that test to query top/files/diff on two retained snapshots. Automatic retention now rebuilds the catalog; `automatic_retention_rebuilds_catalog_after_deleting_old_files` is written. `top.rs` excludes missing optional comparison files. |
| 5: scoped rebuild | fixed now | `cmd/snapshots.rs` rejects `rebuild-catalog --root` before recovery or mutation with a global-rebuild message. `scoped_rebuild_refuses_without_changing_other_root_rows` covers complete, failed and running entries of another root. |
| 6: growth reader cap | fixed now | `cmd/top.rs` requests 1,000,001 rows so the reader's existing one-million cap reports overflow. `growth_reader_cap_refuses_instead_of_ranking_disk_prefix` writes rows in bounded batches and checks the cap error; ignored as `scale`, not executed. |
| 7: emergency partial/failed pruning | fixed now | `orchestrator.rs`: expired partial/failed artifacts are considered before protected complete groups; deletion rebuilds the catalog and rechecks space. `emergency_prune_includes_expired_partial_and_failed_artifacts` covers both statuses. Existing `emergency_prune_protects_recent_and_latest` remains. |
| 8: failed walk keep-partial | fixed now | `orchestrator.rs` retains available records after I/O and resource-limit failures; disconnection and worker panic fail closed. `partial_walk_failure_classification` and `failed_walk_keeps_valid_collected_records_only_when_requested` are written; invalid partial aggregation still propagates its typed error. |
| 9: bulk lifecycle | already fixed | Baseline `orchestrator.rs` already implements D2 progress forwarding, fresh-aggregator auto fallback only on Unsupported/BeforePhase2, cancellation precedence, namespace noatime counts and walk-only partials. Existing `tests/w2b/main.rs`: `namespace_failure_is_redoable_and_cancellation_is_not`, `bulk_namespace_progress_reports_while_setup_is_blocked`; `mode::tests::truth_table` and feature-gated auto-mode integration coverage. No bulk-lifecycle rewrite. |
| 10: invalid scan options | fixed now | `cmd/scan.rs` validates CLI threads/globs before scan work; `run_scan` also validates before lifecycle recording and returns typed Configuration errors, mapped to exit 2. `invalid_scan_overrides_exit_two_without_catalog_mutation` covers 0, 65 and invalid glob syntax. |

## Decision implementations

**Record budgeting:** the walker charges before allocating retained link references, skips, error samples, namespace children and notable candidates. Charges include twice element layout for vector growth, name/path bytes, padding and top-candidate heap storage. Initial record overhead covers small-vector growth. Replacing a top candidate refunds its old name length. A breached bound fails without emitting a truncated record, with the byte-preserving directory display, entry count, bound and instruction to raise `scan.memory_budget`.

The record pool gets `Q = memory_budget / 4`. Capacity is `clamp(floor(Q / 16 MiB) - threads, 1, 64)`; reject first if `Q < threads * 16 MiB`. The record bound is `Q / (threads + capacity)`. Thus capacity reaches 1 before allowing a bound below 16 MiB. Admission additionally checks the aggregator estimate and worker buffers/heaps against the remainder, using wide arithmetic, including after bulk-to-walk fallback. The bulk namespace queue is currently fixed at 2 by its owner, so bulk admission refuses derived capacities below 2. CLI live walking also supplies the derived record bound. Aggregate storage remains an estimate; no hard whole-process allocation guarantee is claimed.

**Prune:** explicit pruning already deletes files then globally rebuilds. Automatic retention now does the same; emergency deletions reconcile the catalog too. Optional top comparisons ignore unavailable files, and growth retains its documented missing-comparison precondition.

**Scoped rebuild:** the existing store interface is global and removes historical rows without files. Preserving other roots' failed/running rows cannot be achieved by its rebuild call alone. The command therefore refuses `--root`; omitting it explicitly selects the global operation.

**Recorder recovery:** acknowledgement is separate from proposed entry status. Any error after a start without an acknowledged finish attempts a failed update. Cleanup failure is logged and the original error returned. If publication already succeeded, its path remains in the failed entry and its complete immutable file is preserved. An explicit global `snapshots rebuild-catalog` can recover that file's catalog metadata. A recorder that also rejects the failed update cannot be forced to persist it; the cleanup error remains visible in logs.

**Write cancellation:** without keep-partial, a cancellation observed between batches drops the private writer and returns Cancelled. With walk keep-partial, writing finishes the already collected rows and the final pre-finish check sets Partial. Bulk never retains partial output. This narrows the reviewed gap but cannot close the index/sync/rename interval inside the consuming store API.

## Needs from coordinator

1. Add `max_record_bytes` at the out-of-scope constructors:
   - `crates/tidemark-scan/src/bulk/mod.rs:98`: copy `p.walk.max_record_bytes`, preserving the production bound.
   - `crates/tidemark-scan/tests/w2a/main.rs:553` and `tests/w2b/main.rs:34`: explicit test bounds, or 0 for unbounded test fixtures.
   - `crates/tidemark-fs/tests/w1b/main.rs:15`: explicit test bound or 0.
   - `crates/tidemark-fs/benches/walk.rs:15`: an explicit bounded value for the benchmark.
2. The directory-specific Limit diagnostic requires an owned message. `WalkError::Limit` now holds `String` rather than `&'static str`; all owned constructors were adapted. Update the two out-of-scope literal patterns in `crates/tidemark-fs/tests/w1b/main.rs:599` and `:726` to guards comparing the message. The enum variant and displayed prefix remain unchanged. This necessary payload change is explicitly disclosed, not a core/schema change.
3. Coordinate a cancellation-aware storage finish/publication seam. `SnapshotWriter::finish` (`crates/tidemark-store/src/writer.rs:296`) builds indexes, analyzes, writes final metadata, closes, syncs and renames without exposing a callback. The store path and interface are outside this task's write scope. A check before calling it cannot observe cancellation during those steps. Finding 2 remains unresolved for that interval; no workaround that mutates published immutable files was introduced.
4. Rerun scan/CLI regressions and complete workspace checks after the constructors are updated. Run the growth cap scale regression in a suitable memory environment. Real XFS bulk tests and Rocky compatibility remain pending VM; no privileged runtime work was performed here.

## Shared edits and scope

Only declared paths changed. No dependencies, manifests, lockfiles, core types, store files, plan documents, module declarations, AGENTS.md or memory projections changed. `WalkConfig` and its owned worker are changed only for finding 1. No branches, merges or pushes were created.

## Validation

Pinned cached toolchain and Cargo with `--offline` / `CARGO_NET_OFFLINE=true`. Evidence logs and exit files are in this attempt's `worker-output/.../library/`.

| Check | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 101: out-of-scope new-field constructors and old literal Limit patterns |
| `cargo test --offline --workspace` | exit 101: compilation blocked by the same callers |
| `cargo build --offline --workspace --features privileged --tests` | exit 101: compilation blocked; no privileged tests executed |
| `tools/ci.sh`, offline environment | exit 1: compilation blocked |
| `cargo test --offline -p tidemark-fs --lib` | exit 0: 33 passed, including the new byte-bound regression |
| `cargo clippy --offline -p tidemark-fs --lib -- -D warnings` | exit 0 |
| `cargo test --offline -p tidemark-scan --lib --test w2c` | exit 101: bulk constructor lacks the field |
| `cargo test --offline -p tidemark-cli --test w2c` | exit 101: same scan dependency |
| `git diff --check` | exit 0 |

A first workspace Clippy run also found an owned helper after the test module; that was moved before the tests and the checks repeated. New scan/CLI tests are written but not reported as passed. Privileged compilation, scale execution and cancellation inside storage publication remain unverified. The result is submitted for coordinator review with these gaps, not as a completed acceptance claim.
