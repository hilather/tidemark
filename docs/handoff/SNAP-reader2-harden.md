# SNAP-reader2-harden — handoff

## Needs from coordinator
- Run the 2M-directory snapbench gate on the host (not run here, per instructions).
- Run the store suite in **release** on a normal host/runner. In this sandbox `/` is owned by an unmapped uid (65534) and release builds are always strict-ancestry, so release store tests fail here with "owner 65534 is not mapped in this user namespace" (34 lib failures, environmental, unrelated to the patch). I ran everything in **debug** (non-strict policy) instead.

## What changed vs the patch
Nothing. `SNAP-reader2-harden.patch` applied cleanly to `7a56325` (`reader_v2.rs`, `tests/reader_v2.rs`); no source edits by me.

## Decisions (owner-approved, implemented as in the patch)
- D1: requested-root validation beats the empty-result work bound for `subtree_dirs`, `wide_dirs`, `metrics_candidates`, `children_page`.
- D2: `children` climbs the parent's ancestry on the first returned row (lazy); `children_count` never climbs.
- D3: `most_shared`, `visit_paths`, `dir()` unchanged.

## Tests and fails-before (reader_v2.rs restored from `7a56325`, new tests kept)
- `every_query_checks_the_requested_row` — FAILED before, passes after.
- `siblings_are_checked_before_selection` — FAILED before, passes after.
- `children_page_validates_the_parent_on_empty_pages` — pin; passes before and after (as the plan states).
- In-file unit tests (`walk_rejects_every_structural_violation`, `empty_results_do_no_extra_chunk_work`) live in the source file, so they cannot be run against the old source; not re-verified.

## Counts (debug, uid 0, no sockets)
lib 58 (+1 ignored); reader_v2 45 (+1 ignored); cold 25; compaction_crash 2; crash_recovery 12; diff_walk 10 (+2 ignored); fuzz_smoke 4; locks 3; state_trust 4; storage 33; w3a 7; w3b 7; w3c 6; writer_v2 14; snapbench example 2; strict-ancestry strict_feature_tests 2.
`cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, `doc -D warnings` clean.

## Assumptions
Debug builds substitute for the requested `--release` store runs because of the sandbox issue above.
