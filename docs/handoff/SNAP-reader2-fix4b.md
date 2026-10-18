# SNAP-reader2-fix4 — review10 validation gaps

## Needs from coordinator
- This is the committed restart of SNAP-reader2-fix4 (attempt 1 hung in a workspace-wide test run and never committed). `cargo test --workspace` and `tools/ci.sh` were NOT run here (they hang in this sandbox); the coordinator gate must run them on the host.
- Nothing blocking. No benchmarks were run (per task); the coordinator should measure on the host.
- Shared files: none touched. No new dependencies.

## What changed
- **B1** (`reader_v2.rs`): new `ShallowWalk` validator, fed by the shallow subtree enumeration in `subtree_dirs` and the shallow branch of `eligible` (used by `metrics_candidates`). Single pass, O(rows), ancestor stack: each row's id must equal the expected next id; `parent_id` must be the innermost open ancestor (finished ancestors popped by `subtree_end < id`); `depth == parent.depth + 1` (checked); id and `subtree_end` inside the parent's span. Expected next id is `id+1` if `depth < limit` and the row has descendants, else `subtree_end+1`. At the end the walk must have reached exactly `range_end+1`. Anything else → `StoreError::Invalid`.
- **B2** (`reader.rs`): v1 branch of `children_by_name` validates every child name with `RawName::new`; invalid → `Invalid`, never normalised.
- **S1** (`export_ncdu.rs`): the v2 export validates each notable file name as a `RawName` before it enters the arena (NULL names still allowed).
- **Handoff correction** (fix3b wording): `shallow_dirs` is still used by `children`, `children_page` and `children_count` (all sorts). Only `children_by_name` always uses chunks.

## Tests that failed before and pass now
(confirmed by reverting the source change only)
- `reader::v2::child_cursor_tests::review10_b1_shallow_subtree_and_metrics_rows_are_validated` (lib): deleted `shallow_dirs.id=1` (root+1 child, and a wider tree) for `subtree_dirs(0,1,…)` and `metrics_candidates(1,…)`; depth-2 row moved to `parent_id=0, depth=1`; deleted last row.
- `review10_b2_mixed_diff_rejects_invalid_v1_child_name` (`tests/diff_walk.rs`): v2 new / v1 old with `dirs.name='../escaped'` → `Invalid`; sound control passes first.
- `review10_s1_ncdu_export_rejects_invalid_notable_file_name` (`tests/reader_v2.rs`): re-encoded file chunk with `../escaped` → `Invalid`; control export passes first.

Before: B1 cases returned wrong Ok data; B2 returned an Ok diff with an invented path; S1 exported `"name":"../escaped"`. After: all three return `Invalid`.

## Assumptions
- Rows deeper than the depth limit are legitimately absent; the root's own parent is not checked (the enumeration starts at the requested root).
- Error text is generic (`invalid v2 directory data` etc.), matching neighbouring checks.

## Test counts
Re-run in fix4b: `cargo test --offline -p tidemark-store --no-fail-fast`: lib 44 passed/1 ignored, diff_walk 10/2 ignored, reader_v2 35/1 ignored, cold 25, compaction_crash 2, crash_recovery 12, fuzz_smoke 4, locks 3, state_trust 4, storage 33/1 ignored, w3a 7/1, w3b 7/1, w3c 6/1, writer_v2 1 (+ doctests 14, 4); all green. `cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, `build --workspace --features privileged --tests`, and `cargo doc` with `-D warnings` all clean. `cargo test --workspace` and `tools/ci.sh` not run (hang in sandbox); coordinator gate runs them on host.
