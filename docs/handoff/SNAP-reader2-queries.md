# Handoff — SNAP-reader2-queries

## Needs from coordinator

- **Remaining v1-only path: `diff.rs` only** (`v1_conn()` guard kept; SNAP-diff-walk ports it). No other
  `NotYetImplemented` path is left in `reader.rs`, `links.rs` or `export_ncdu.rs`. The `v1_conn()` calls that
  remain in `links.rs`/`export_ncdu.rs` sit only in the v1 branches.
- Review point (no contract change): v2 `FileRow.id` / link-instance `rowid`. v2 `files.id` is the zero-based
  clustered row number (as `15` §9). `link_instances_page` returns `row number + 1` as the `i64` rowid so a
  v2 file written from rows inserted in `(ino, dir_id, name)` order equals v1's one-based `rowid` exactly.
  v1 files written from unsorted input have insertion-order rowids and file ids; v2 cannot reproduce those
  (it re-sorts). Tie-breaks in v2 therefore follow the stored (sorted) order. Parity tests insert sorted input.
- ncdu notable files: v1 emits per-directory in stored-`id` order; v2 emits in stored row (= name) order. The
  emitted set is identical, and the order is identical whenever v1 ids ascend in `(dir_id, name)` order (as the
  v2 writer assumes). Not a codec/writer bug.
- Environment: eight test targets fail in this sandbox for reasons unrelated to this task (details under
  "Tests"). Please re-run them under the coordinator gate.

## What changed

- `reader_v2.rs` (`Backend`)
  - Notable files: `subtree_files` selects `file_chunks` with `last_dir >= lo AND first_dir <= hi`
    (ordered by `first_row`), decodes only those chunks, filters by directory range and size/mtime bounds,
    and keeps a bounded top-N heap ranked `(disk desc, containing-directory path bytes, name (NULL first),
    row id)` — exactly v1's `ORDER BY f.disk DESC,d.path,f.name,f.id`. Paths are reconstructed only for rows
    that can enter the heap (disk compared first); the directory path is cached. Preorder is not path-byte
    order (`a/b` < `a-c` in preorder, > in bytes), so ranking uses reconstructed path bytes.
  - `dir_files`: ncdu paging via the directory's `file_lo`/`file_count` (contiguous row range, chunk by chunk).
  - Link instances: `li_lower_bound` binary-searches `li_chunks` by row number using only metadata (no payload
    reads, ≈log₂(chunks) one-row queries) for the first chunk with `last_ino >= ino`; `li_visit` then walks
    forward while `first_ino <= ino`. `link_instances` (v1 `ORDER BY dir_id,name` = stored order),
    `link_instances_page` (tuple cursor `(dir_id, name IS NOT NULL, name, rowid)`, forward or reverse, streamed
    with a `limit`-sized deque for reverse).
  - `link_sets_under`: `link_sets` via the `ls_lca` index (plain SQL), top-N heap ranked `(disk desc, lca path
    bytes, ino)`; cheap disk comparison first, path only when it can matter; boundary check (glob regions or
    ancestor-at-depth walk) and `instance_count` only for sets that would enter the heap.
  - `stream_sets` (for `crossing_sets`/`link_report`): inode-ordered `link_sets` plus a forward-only instance
    cursor that skips whole chunks below the wanted inode without decoding them. `most_shared` (the
    `dirs.path` helper in `link_report`): projected `t_disk`/`t_reclaimable` columns + heap, path only for
    admitted rows. `visit_paths`: preorder paths with one reused buffer (no per-row ancestor walk).
  - Plain tables (`skipped`, `errors`, `annotations`, `link_sets`) use v1's SQL unchanged.
  - New chunk validation shared by file and link chunks (messages name `file_chunks`/`li_chunks`): `typeof`
    and `length` before the payload is read, ceiling `MAX_CHUNK_BLOB`, coordinates checked as non-negative
    integers (file rows/dirs fit `u32`), `last >= first`, `hi >= lo`, first chunk starts at row 0, neighbours
    contiguous and ordered by their key, decoded row count/first/last key must equal the metadata.
    Every failure is `StoreError::Invalid`.
  - Cache bound: the directory cache is unchanged (≤ 32 chunks / 8 MiB). New: decoded file chunks and
    decoded link-instance chunks each keep ≤ 4 chunks / 8 MiB; reconstructed directory paths ≤ 4 MiB /
    65,536 entries (cleared when full). Each decoded chunk is ≤ 8192 rows (codec limit).
- `reader.rs`: the seven methods dispatch to `Backend` (v1 code paths unchanged apart from taking the subtree
  end from a shared helper). New `pub(crate)` helpers used by `links.rs`/`export_ncdu.rs`: `is_v2`,
  `dir_lite` (directory row without path), `parent_of`, `own_apparent_sum`, `visit_dir_paths`, `v2_dir_files`,
  `v2_stream_sets`, `v2_most_shared`; `LITE_COLUMNS` (shared with `reader_v2.rs`' `SHALLOW_COLUMNS`).
- `links.rs`: `Regions::build` reads paths via `visit_dir_paths` (v1 and v2); `stream_sets` and the
  most-shared query branch on `is_v2()`; the ancestor walk uses `parent_of`.
- `export_ncdu.rs`: directories come from `dir_lite(id)` (primary-key lookup instead of one `SELECT *` cursor,
  so v2 never builds paths and a depth cut can skip the cut subtree); notable files come from the v1 paged
  query (query-plan marker kept) or `v2_dir_files`; the depth-cut aggregate uses `own_apparent_sum`.
- `tests/reader_v2.rs`: `legacy_queries_return_not_yet_implemented` replaced (including the two `diff::diff`
  assertions, removed) by `nothing_but_diff_is_left_unported`; new parity and corruption tests (below).

## Tests that failed before and pass now

Before this change every v2 call below returned `StoreError::NotYetImplemented` (that is what the replaced
test asserted). New tests, all pass:

| Test | Covers |
|---|---|
| `files_links_errors_and_ncdu_match_v1_across_shapes` | v1 vs v2 equality for `subtree_files`, `link_sets_under`, `link_instances`, `link_instances_page`, `skipped_under`, `errors_under`, `annotations`, `export_ncdu`, `Regions::build`, `crossing_sets`, `link_report` over two trees (preorder/byte-order traps, non-UTF-8 names, NULL file/instance names), `plain_depth` 0/2/4, hash 32/64, chunk sizes 1–7 and default, limits 0/1/2/7/`i64::MAX`/`usize::MAX`, filters, paging cursors both directions, missing ids |
| `golden_fixture_files_and_links_match_v1` | same battery on the golden link-semantics tree |
| `files_links_and_ncdu_match_v1_through_a_cold_file` | same, v2 opened from a `.db.zst` |
| `many_files_in_one_directory_span_chunks_and_break_ties_like_v1` | equal-size tie-breaking across chunks |
| `preorder_is_not_path_byte_order_for_ranked_results` | ranking by path bytes, not preorder; asserts the two differ |
| `corrupt_file_chunks_are_invalid_not_panics`, `corrupt_link_instance_chunks_are_invalid_not_panics` | 16 + 16 tamperings (bad/oversized/TEXT/INTEGER payload, wrong-typed or negative coordinates, mismatching keys, gaps, overlaps, deleted table) → `Invalid`, cold and warm |
| `text_and_oversized_file_and_link_chunks_are_not_read_into_memory` | rejected by `typeof`/`length`, message names the table |
| `corrupt_file_chunk_only_affects_queries_that_need_it`, `link_instance_lookup_touches_only_its_chunks` | bounded access: a corrupt unrelated chunk does not affect a narrow query |
| `link_set_pointing_at_a_missing_directory_is_invalid` | out-of-range LCA ids behave like v1 |
| `more_than_a_million_instances_of_one_inode_exceed_the_result_cap_like_v1` (`#[ignore = "scale"]`) | 1,000,001 instances: `MAX_QUERY_ROWS` error identical to v1; run once, passes (44 s) |

Existing v1 tests are untouched and pass (`w3c`: 3 passed incl. the ncdu query-plan test).

## Before / after

- Before: `s.subtree_files(0, ..)`, `s.link_instances(1)`, `export_ncdu(s, ..)` … on a v2 snapshot →
  `Err(NotYetImplemented)`.
- After: identical results to the v1 file of the same input, e.g. `export_ncdu` output is byte-identical
  (asserted for every sampled root × lossy/non-lossy × `max_depth` None/0/1/3) and `ExportStats` equal.

## Assumptions

- No user questions were possible. v2 link-instance `rowid` = row number + 1 (see above).
- A v1 snapshot whose `files.id`/`link_instances` insertion order is not sorted cannot be compared row-for-row;
  the entry sets are equal, ranking ties follow stored order.
- v1 ncdu used one `SELECT *` cursor; it now does a primary-key lookup per directory (`dir_lite`). Output is
  unchanged; v1 cost per directory is one B-tree seek.
- `link_sets_under` on v2 reads `link_sets` in `(lca_dir_id, ino)` order and may revisit an instance chunk
  per admitted set; the 4-chunk cache bounds the cost. No query decodes more than the chunks it needs, except
  boundary-glob regions, which (like v1) visit every directory path of the subtree once.
- `MAX_QUERY_ROWS` bounds mirror v1: results of more than 1,000,000 rows → `Invalid`.

## Tests (counts)

- `cargo fmt --check`: clean. `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`: clean.
  `cargo build --workspace --features privileged --tests`: builds (privileged tests not run).
- `cargo test -p tidemark-store`: lib 32 passed / 1 ignored; `reader_v2` 29 passed / 1 ignored (scale);
  `w3c` 3 passed; every other store test binary passes.
- `cargo test --workspace --no-fail-fast` and `tools/ci.sh`: fail only in eight targets that do not touch this
  code and fail here for sandbox reasons: `tidemark-cli --test e2e` / `--test rules` (test identity helper:
  `Invalid argument`), `tidemark-fs --test rules` (`foreign_directory_fallback_count`: sandbox root-owner
  assumption), `tidemarkd --lib`, `w6a1`, `w6a2`, `w6a3`, `w6a5` (API socket: `socket ancestor / has untrusted
  owner 65534`). Logs: the attempt's `library/workspace-tests.log` and `library/ci.log`.
