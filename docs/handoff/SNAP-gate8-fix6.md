# SNAP-gate8-fix6 — handoff

## Needs from coordinator
- Nothing blocking. Please re-run the store tests in release mode as uid 1000: in this sandbox `/` is owned by an unmapped uid (65534), so release builds (strict ancestry) cannot create temp state dirs. I ran the same tests in the debug profile instead.
- The new test lives in the `reader_v2.rs` unit-test module (it needs the private `decodes`/`scanned` counters), not in `tests/reader_v2.rs`.

## Change (`crates/tidemark-store/src/reader_v2.rs`)
- New `raw_held` returns the requested row plus its decoded chunk `Arc` (`type Held`); `raw` wraps it.
- `authoritative_root` now returns `(ChunkDir, Held)`; `wide_dirs` uses `raw_held` before the eager `self.path(...)`.
- `scan` takes `held: &Held` and uses it for the chunk with the same `first..=last` before looking in the LRU, so the ancestor climb can no longer force a second decode of the requested chunk.
- Applied the same to `subtree_dirs` and `metrics_candidates` (same raw → path → scan order).

## Tests
`wide_dirs_keeps_the_requested_chunk_across_root_validation` (review17 fixtures, results compared to v1 by `Debug`):
- chain 0–32, 1 dir/chunk, `wide_dirs(32,0,1)`: ≤ 33 decodes
- chain 0–33, `wide_dirs(33,0,1)`: ≤ 34
- 4,096-dir chunks, 32,769 dirs, `wide_dirs(32768,0,1)`: ≤ 9
Reverting the held-chunk use makes it fail (`id 32: 34 > 33`).

## Counts (debug profile)
lib 58 passed/1 ignored; reader_v2 42/1; diff_walk 10/2; writer_v2, cold, storage, w3a/b/c, snapbench example (2) pass; strict_feature_tests 2 pass; fmt, clippy `-D warnings`, doc `-D warnings` clean. 2M snapbench not run. `readable_storage_warns_once` fails in the release/sandbox environment only.

## Assumptions
Held chunk is used only when its `first`/`last` match the scanned chunk row.
