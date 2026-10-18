# Handoff: SNAP-gate8-fix — v2 reader meets the §8 latency gate

## Needs from coordinator

1. **Run the 2M-directory gate on the host**: `snapbench --layout v2` (command in `docs/perf/snapbench.md`, "v2"),
   compare with the v1 log per §8 (v2 ≤ max(1.25 × v1, 100 ms)), and fill the `TBD (coordinator host run)` cells.
   Not run here (sandbox RAM/time). Prototype numbers in the plan: children_wide ~48 ms, top2 ~490 ms, top4 ~550 ms.
2. Run `cargo test --workspace` / `tools/ci.sh` on the host (not run here; they hang in this sandbox).
3. **Release-profile store tests cannot run in this sandbox**: release builds enforce strict ancestry and reject `/`
   (owner uid 65534, unmapped). I ran every store test target in the **debug** profile instead (relaxed ancestry).
   Please run `cargo test --release -p tidemark-store` on the host.
4. Privileged tests: only built (`--features privileged --tests`), never run.

## What changed vs the prototype

Nothing in behaviour. The only non-test edit to `src/reader_v2.rs` is a `#[cfg(test)]` counter `Backend::scanned`
(chunks streamed by `scan`, incremented once per chunk) used by the work-bound tests. No new bug was found by the new
tests; no prototype fix was needed. Commit 1 is the prototype verbatim.

## Validation narrowing (for the reviewer)

Shallow `children_page` (parent depth < plain depth) now decodes only the rows it **returns** (v1 semantics): an
undecodable metric in a sibling *outside* the page no longer raises `Invalid` from `children_page`. Still enforced:
the structural child-set walk over every sibling (`check_shallow_children`: first child at id+1, contiguous spans, last
span ends at the parent's, depth = parent+1), per-row `child_path` checks (parent, depth, span, `valid_name`) on returned
rows, and `children_count` / `children` still decode every sibling. `s2_b4_shallow_rows_are_validated` and
`s1_shallow_subtree_does_not_decode_deep_chunks` are unchanged and pass.

## Tests added (plan §4)

- 1 `tests/reader_v2.rs::ties_and_limits_equal_v1_for_every_sort` — equal and varying metrics, names differing by
  prefix/length/high bytes, limits 0,1,2,n−1,n,n+1,1_000_000,1_000_001, all 8 sort keys, plain depth 0/1/3, subtree_dirs /
  children / wide_dirs / metrics_candidates vs v1 (results or both-error).
- 2 `shallow_children_pages_cover_every_edge` — 41-child shallow parent, 6 pages of 8, forward/reverse/cursor, name asc/desc,
  4 filters, every sort; vs v1; concatenation equals the one-shot page; `children_count`.
- 3 `shallow_child_set_and_row_tampering_is_invalid` (deleted first/middle/last child → `Invalid` from `children`,
  `children_page` every sort ± cursor, `children_count`; wrong depth/parent_id/subtree_end → `Invalid`; TEXT metric in a
  returned row → `Invalid`, not `Sql`) and `bad_names_are_invalid_for_shallow_and_scanned_rows` (`""`, `/`, `a/b`, `.`, `..`, NUL;
  plain depth 3 = shallow, 0 = scanned).
- 4 `src/reader_v2.rs`: `walk_builds_paths_and_skips_below_the_limit`, `walk_rejects_every_structural_violation`.
- 5 `scan_rejects_tampered_chunks` (bad data, size mismatch, gap, overlap, oversized; for deep `subtree_dirs`, `wide_dirs`,
  `metrics_candidates`), `scan_from_a_subtree_root_touches_only_its_chunks`.
- 6 `shallow_work_is_bounded_by_the_root_chunk` (≤ 1 decode, 0 scans; 5,000-child `children_page`), `deep_subtree_scan_streams_each_chunk_once`.
- 7 `src/chunk.rs::projection_depth_and_streaming_view_match_the_decoder` (4,096-row chunk).
- 8 `examples/snapbench.rs::writer_roundtrip_and_every_query` loops `"v1"`, `"v2"` (helper `roundtrip_and_every_query`).

Docs: `docs/perf/snapbench.md` v2 section (method unchanged, `--layout v2`, TBD table) and name length list 40 → 41
(four digits + the 37-byte suffix).

## Counts and checks (debug profile, offline)

- `cargo fmt --check` OK; `cargo clippy --offline --workspace --all-targets -- -D warnings` clean.
- store `--lib` 53 passed, 1 ignored (was 46; `readable_storage_warns_once` passes in debug); `--test reader_v2` 39 passed, 1 ignored (was 35);
  other targets all pass (cold 25, compaction_crash 2, crash_recovery 12, diff_walk 10, fuzz_smoke 4, locks 3, state_trust 4,
  storage 33, w3a 7, w3b 7, w3c 6, writer_v2 14); snapbench example 2 passed.
- `strict_feature_tests` 2 passed; `cargo build --workspace --features privileged --tests` OK; `RUSTDOCFLAGS='-D warnings' cargo doc` OK.
- `--dirs 20000` smoke of both layouts ran to completion (debug build; timings not meaningful).

## Assumptions

- Ran tests in the debug profile (see Needs 3) rather than `--release` as the brief says.
- New integration helpers carry a `g8_` prefix to avoid clashing with the existing `same`/`invalid`/`tamper`-style helpers; `Pair` is unchanged.
