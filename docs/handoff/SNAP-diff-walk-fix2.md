# SNAP-diff-walk-fix2 — handoff

## Needs from coordinator
- **`children_by_name` is ~6x slower on plain-row directories (plain_depth ≤ 2, the production default) than on chunked ones.**
  One 150,000-child directory, pages of 16,384, one side: 312 ms with `plain_depth=2` vs 52 ms with `plain_depth=0`
  (`profile_wide_split`, `TM_PLAIN=0|2`, ~2 µs/row). Whole v2/v2 diff of the wide-directory shape: 678 ms vs 116 ms.
  This is why the wide shape (below) misses §8 (max(1.25 × 184 ms, 100 ms) = 230 ms): the walk itself is not the cost,
  paging the plain table is. Fix belongs in `reader.rs` (plain-row child query: likely per-row decode of unneeded columns or
  a sort/seek on a non-covering index; check `EXPLAIN QUERY PLAN`). Not changed here (out of scope). §8 thresholds untouched.
- No `#[ignore]`d failing test was added: `children_by_name` is correct, only slow on that path.

## What changed (`diff.rs`, `tests/diff_walk.rs`)
- B3: `Kids` holds a page of compact `ChildEntry` plus a `ChildCursor` (no `DirRow`, no paths). One shared `path` buffer:
  name pushed on descent, truncated per frame (`Frame::path_len`). A `RawPath` is built only for rows entering the top-N heap
  (`TopN::offer` takes `&[u8]`). Walk state is O(depth × page + limit + longest path).
- S3: paging uses `children_by_name`'s cursor, so a W-child directory costs O(W) visits per side. A `#[cfg(test)]` thread-local
  `FETCHED` counts fetched entries.
- Output unchanged; v1/v1 SQL fallback untouched.

## Tests that fail on the old code, pass now
- `diff::tests::wide_directory_costs_one_visit_per_child_per_side` (120,000 children, v2/v2 and v2/v1; asserts fetched == sum of child counts exactly; the old
  pagination restarted per page and had no cursor-based count).
- `deep_single_child_chain_diffs_in_bounded_memory` (4,096-deep chain, 255-byte names, `limit=1`, child process, VmHWM < 192 MiB). The old walk cloned each
  frame's rows/paths (~2·256·D²/2 ≈ 4 GiB of path bytes). Not run against the old code in this attempt (it would need >4 GiB); the bound is by analysis.
- `directory_with_120k_children_matches_oracle_for_v2_pairs` (equality with an independent oracle, v2/v2 and v2/v1).
- Existing equality tests (`diff_matches_model_and_old_sql_for_every_pair_kind`, explain, ties, errors, page-boundary) pass unchanged.

## Scale (S4), release build with `debug_assertions` on (release refuses the sandbox's unmapped-uid `/` in state_dir), 12 cores, load avg ≈ 2–3 before/after; 5 runs, median
2,000,101 dirs, 1% changed, depth MAX, limit 50:
| case | median |
|---|---|
| v1/v1 old SQL (ATTACH join) | 3889 ms |
| v1/v1 walk (warm) | 3960 ms |
| v2/v2 walk (warm) | 1539 ms |
| v2/v2 walk (fresh open) | 1732 ms |

v2/v2 is well under 1.25 × v1 SQL (4861 ms). One 150,000-child directory, 1% changed (`scale_wide_directory_old_sql_vs_walk`):
| case | median |
|---|---|
| v1/v1 old SQL | 184.5 ms |
| v2/v2 walk (warm) | 725.8 ms |
| v2/v2 walk (fresh open) | 719.1 ms |

Above the threshold; cause in "Needs from coordinator". No `perf` available; split measured with timers (`profile_wide_split`).

## Assumptions
- The attempt-1 patch was reviewed and kept (it was sound); the 4,096-deep test uses a child process like `tests/reader_v2.rs`.
- Only 5-run medians from the existing test harness (it hard-codes 5 runs).

## Test counts
`cargo test -p tidemark-store --lib --test diff_walk`: 39 lib passed (1 ignored), 8 diff_walk passed (2 ignored scale). Workspace-wide counts: see report.
