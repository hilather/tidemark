# Handoff — SNAP-queries-fix

## Needs from coordinator

- **v1 `children_by_name` is O(W) per page** (`dirs` has only `dirs_parent(parent_id)`, no `(parent_id, name)`
  index, and `crates/tidemark-store/migrations/` is outside this task). SQLite top-N sorts the parent's rows
  for every page, so paging a W-child v1 directory in pages of P costs O(W²/P) *rows examined inside SQLite*
  (no path building, no allocation per skipped row). v2 is O(W) in total, as required. If v1/v1 diff on wide
  directories matters, add `CREATE INDEX dirs_parent_name ON dirs(parent_id, name)` to the v1 schema; the
  query already matches it.
- Diff must hold the `ChildCursor` and a reused `Vec<ChildEntry>`, never a `DirRow`/path (B3). Contract
  (pinned, `reader.rs`): `Snapshot::children_by_name(parent, cursor, limit, &mut out) -> Result<Option<ChildCursor>>`;
  `limit == 0` is treated as 1; `limit > MAX_QUERY_ROWS` → `Invalid`; a missing `parent` yields no children and
  `None`; a cursor from the other format or outside the parent's subtree → `Invalid`. The methods carry
  `#[allow(dead_code)]` until `diff.rs` uses them; remove the attribute then.
- Order contract assumed from `15` §3: sibling ids ascend in name-byte order. v2 verifies it while hopping
  (names strictly increasing within a page, parent/depth/subtree bounds per child) and returns `Invalid`
  otherwise. It is not checked *across* pages (the cursor is just the next sibling id, as pinned).
- The `data()` fixtures of the older parity tests still assign file ids in `(dir_id, name)` order, so their
  ncdu comparison is now order-insensitive within a directory (`compare_exports`); the scanner-style test
  compares bytes exactly. No writer/codec bug found.

## What changed

- **B1** (`reader_v2.rs`, `li_visit`): the full `(ino, dir_id, name IS NOT NULL, name)` order is checked across
  every link-chunk boundary the walk crosses (`check_link_order`). When the visitor stops early (reverse
  paging, `limit`), the boundary to the next chunk is still checked if the inode's run continues into it.
  The binary search in `li_lower_bound` trusted metadata only, so `li_visit` now also decodes (and thereby
  validates) the chunk *before* the one found and requires it to end below `ino`; a row no chunk covers is a
  gap (`Invalid`) rather than "no instances".
  Limitation (documented, not fixed): an early stop does not walk the rest of a run that spans three or more
  chunks, so a misordered boundary *beyond* the next one is only found by a query that reads that far.
- **B2** (`reader_v2.rs`, `subtree_files`): the directories' claimed span is
  `file_lo(id) .. file_lo(end)+file_count(end)` (the writer sets an empty directory's `file_lo` to the running
  offset, so the span is contiguous and non-decreasing). The chunks read must start at or before it, be
  contiguous, and reach its end — a missing first, last or middle chunk is `Invalid`; a row of a directory in
  range outside the span is `Invalid`. Cost: two directory lookups. `dir_files` (ncdu) already failed for a
  missing chunk (`side_chunk` demands coverage); a test now pins it.
- **S1** (`export_ncdu.rs`): v2 collects one directory's single-link notable files (bounded by its
  `file_count`) and emits them in `(disk DESC, ino ASC)` order, which is v1's `files.id` order for
  scanner-assigned ids (`notable.rs`, `05` §4 Step G).
- **S2** (`reader.rs`, `reader_v2.rs`): `plain_row_error` maps `FromSqlConversionFailure`,
  `InvalidColumnType` and `IntegralValueOutOfRange` to `StoreError::Invalid`; applied for v2 in
  `skipped_under`, `errors_under`, `annotations` (via `Snapshot::plain_error`, which keeps `Sql` for v1) and
  in the v2 `link_sets` reads (`link_sets_under`, `stream_sets`).
- **Child cursor** (`reader.rs`, `reader_v2.rs`): `ChildEntry`, `ChildCursor`, `Snapshot::children_by_name`.
  v2: parents with `depth < plain_depth` read `shallow_dirs WHERE parent_id=? AND id>=? ORDER BY id LIMIT n+1`
  (index `shallow_parent`, so O(page)); deeper parents hop `child.subtree_end + 1` from the cursor's next-sibling
  id through the existing directory chunk cache. No path is built. v1: `parent_id=? AND (name>? OR NULL) ORDER BY
  name LIMIT n+1`, cursor = last name. `#[cfg(test)]` counter `Backend::decodes` counts directory-chunk decodes.
- Cache bounds unchanged: directory cache ≤ 32 chunks / 8 MiB, file and link caches ≤ 4 chunks / 8 MiB each,
  path cache ≤ 4 MiB / 65,536 entries. `li_visit` now decodes at most one extra link chunk per lookup (the one
  before the start), inside the same 4-chunk cache.

## Tests that failed before and pass now

Run against the unmodified sources (same test file) all six failed; they pass now.

| Test | Was |
|---|---|
| `b1_swapped_link_chunks_are_invalid_for_every_query` (reviewer's two one-row chunks, `z` then `a`; `link_instances`, forward/reverse pages, with and without the first-page cursor) | `Ok([z,a])` / `Ok([])` |
| `b2_missing_first_or_last_file_chunk_is_invalid_per_query` (delete last / first chunk; `subtree_files` ×2, `export_ncdu`, subtree of an affected directory) | `Ok(18 rows)` |
| `s1_ncdu_orders_notable_files_like_scanner_assigned_v1_ids` (`m`,`z` 8192, `a` 4096, `zz` 100 → `m,z,a,zz` in both; byte-equal exports, `plain_depth` 0/2/4, chunk sizes 1/3/100) | v2 emitted `a,m,z,zz` |
| `s2_malformed_plain_rows_are_invalid_in_v2` (skipped `reason='bogus'`, errors `op='bogus'`, annotation `value=x'00'`, link set `complete=2`) | `Sql(FromSqlConversionFailure …)` |
| `corrupt_file_chunks_are_invalid_not_panics` (now: *both* queries fail for every tampering) | passed on "some query fails" |
| `corrupt_link_instance_chunks_are_invalid_not_panics` (now: `link_instances` for every inode with a row in the damaged chunk, and both pages for inodes starting in it) | `Ok` with missing rows when metadata hid chunk 9 / a deleted middle chunk |

New unit tests in `reader_v2.rs` (`child_cursor_tests`, they need the `pub(crate)` method and the counter):
`pages_equal_children_by_name_across_shapes` (v1 and v2 equal `children(id, Name)` minus paths for page sizes
1/2/7/1000, chunk sizes 1/3/5/4096, `plain_depth` 0/2/4, every directory with children plus missing ids, leaf,
`limit 0`), `five_thousand_children_below_plain_depth` (shallow table and chunk hopping, pages 7/250/100,000),
`paging_a_wide_directory_decodes_each_chunk_once` (5,000 children, 101 chunks: pages of 1, 7, 64 decode
≤ chunks + 2 times each, where per-page rescans with a 32-chunk cache would decode ~pages × 101),
`corrupt_sibling_structure_is_invalid` (foreign/out-of-range cursor, deleted chunk).

## Before / after

- Before: swapped link chunks → `link_instances(42) = Ok([z, a])`, reverse page `Ok([])`; deleting the last
  `file_chunks` row → `subtree_files(0, default, 100)` `Ok(18 rows)`; v2 ncdu `a,m,z,zz` vs v1 `m,z,a,zz`;
  bogus `skipped.reason` → `Sql(FromSqlConversionFailure)`.
- After: `Invalid("invalid v2 snapshot: li_chunks rows are not ordered across a chunk boundary")`,
  `Invalid("… file_chunks do not cover the directories' file span")`, identical ncdu bytes,
  `Invalid("invalid v2 snapshot: undecodable row (…)")`.

## Assumptions

- No user questions were possible. `limit == 0` for the cursor method behaves as 1 so a `None` result always
  means "exhausted".
- A directory's notable files in one `file_count` span fit in memory for the ncdu re-sort (they are bounded by
  the notable-file caps of the scanner; the sort buffer is dropped per directory).
- Ties in `(disk, ino)` within a directory cannot occur for single-link files; the sort is stable anyway.
- `file_lo` of a directory without files equals the running offset (`writer_v2.rs`), which B2 relies on.

## Tests (counts)

- `cargo test -p tidemark-store`: lib 38 passed (4 new); `reader_v2` 33 passed / 1 ignored (scale; 4 new,
  2 tightened); every other store target passes (`diff_walk` 5/1 ignored, `storage` 33, `writer_v2` 14, …).
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo build --workspace --features privileged --tests`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`:
  clean.
- `cargo test --workspace --no-fail-fast`: 1116 passed, 25 failed, 15 ignored. All failures are the eight
  sandbox targets listed in `SNAP-reader2-queries.md` (`tidemark-cli` e2e/rules, `tidemark-fs` rules,
  `tidemarkd` lib/w6a1/w6a2/w6a3/w6a5), none touches this code. `tools/ci.sh` was not run separately (it repeats
  these steps); please run it under the coordinator gate.
