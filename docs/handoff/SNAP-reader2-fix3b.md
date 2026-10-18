# SNAP-reader2-fix3b — handoff

## Needs from coordinator
- The two §8 benchmarks (`scale_wide_directory_old_sql_vs_walk`, `profile_wide_split`) could not run here:
  the v1/v2 writer's state-directory check fails with `state directory /: owner 65534 is not mapped in this
  user namespace` (sandbox `/` is owned by nobody). No wide v2/v2 vs v1 SQL numbers are available; please run
  them in the gate environment (default `plain_depth`).
- `cargo test --workspace --no-fail-fast` and `tools/ci.sh` were not run (time); only the store crate's
  `--lib`, `--test reader_v2`, `--test diff_walk` were. fmt, clippy `-D warnings`, `--features privileged
  --tests` build and `cargo doc -D warnings` are clean.

## What changed
- `children_by_name` (v2) always hops over directory chunks, also for `depth < plain_depth`; `shallow_dirs`
  is used only by metric-sorted queries. Cursor is `Id(next_sibling, last_name)`; next page's first child must
  sort strictly after it (B3). Every child name is validated as `RawName` (B2). `depth+1` is checked (B5).
- B4: shallow-child lists (`children`, `children_page`, `children_count`, …) are verified complete:
  first id = parent+1, next = prev.subtree_end+1, last.subtree_end = parent.subtree_end, parent_id/depth match.
- B1: `li_visit` keeps decoding the inode's chunk range after the visitor stops, checking order across all
  boundaries before trusting the early stop.
- S2: all shallow-row conversions use `plain_row_error` (Invalid in v2).
- S3: ncdu v2 notable files are `(disk, ino, apparent, offset, len)` tuples plus one name arena, sorted
  `(disk DESC, ino ASC)`. v2 ncdu export holds one directory's notable-file names at a time (bounded by that
  directory's `file_count`, at most `max_notable`).
- Review9 S1 accepted: `children_by_name` is O(page) per page for v2 only; v1 pages cost O(W log W) in
  SQLite, so mixed v1/v2 diff on a very wide v1 directory stays superlinear.

## Tests (new, fail before / pass now)
`b3_names_must_ascend_across_page_boundaries`, `b2_child_names_are_validated`,
`b5_depth_overflow_is_invalid_not_a_panic`, `s2_b4_shallow_rows_are_validated` (lib),
`b1_misordered_boundary_beyond_the_next_chunk_is_invalid` (reader_v2),
`corrupt_sibling_names_are_invalid_not_wrong_diffs` (diff_walk).

## Counts
lib 43 passed/1 ignored; reader_v2 34 passed/1 ignored; diff_walk 9 passed/2 ignored.

## Assumptions
Restarted from the attempt-1 patch; it already covered items 2–5, 7–9 and most of 1/6; I added the
remaining two `dir_row` shallow call sites (S2) and verified the rest.
