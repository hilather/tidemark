# SNAP-gate8-fix5 — v2 reader: always validate the requested wide_dirs root (review16 S1)

## Needs from coordinator
- Run the 2M-directory snapbench gate on the host (not run here).
- Tests ran in the dev profile with `CARGO_HOME=/home/brewerm/.cargo`: release store lib tests fail in this sandbox (strict ancestry), as in fix3/fix4 (33 lib failures in release, 0 in dev).
- `children` still does not validate a parent's own name/ancestry when it returns no rows (see Check 2); decide whether that matters.

## Change (`crates/tidemark-store/src/reader_v2.rs`)
`wide_dirs` computes `self.path(snap, &root)` (validating every name and the ancestry) eagerly, before the walk and independent of `top.admits`, and uses it as the base for hit paths. Hits below the root keep fix4's lazy relative-path construction.

## Decision (coordinator)
Validation of the requested root wins over the empty-result work bound. `wide_dirs` on an empty or zero-limit result may decode the root's ancestor chunks, as `5feba30` did. Recorded in a code comment.

## Check 2: other queries
- `subtree_dirs`: already builds `self.path(snap,&root)` eagerly (after `authoritative_root`); rejects at limit 0/1 — covered by the new test.
- `metrics_candidates`: root is id 0, path built eagerly; rejects root-name corruption at limit 0/1 — covered by the new test.
- `children`: **has the gap** for a parent that yields no rows (leaf, or limit 0): `children(40)` with name `a/b` succeeds empty. Not fixed: it is a hot path the task says not to touch, an eager path would raise the empty-leaf `children` work from 1 to ~3 decodes (fix3/4 bound), and `5feba30` and `7cde8e6` behave the same (review16 flags no children regression). Left as is.

## Tests (lib, `reader_v2.rs` tests)
- New `wide_dirs_always_validates_the_requested_root`: ID 40 name `a/b`, ID 0 name `/r\0bad`, ID 40 parent 0 (rows mutated before writing, so chunks are encoded correctly), plain depth 0 and 3: `wide_dirs(id,0,0)`, `(id,0,1)`, `(id,2,1)` → `Invalid`; also `subtree_dirs(id,0,_,0|1)` and, for id 0, `metrics_candidates(0,0|1)`. Fails with the eager path replaced by a plain "/" base (verified).
- `empty_results_do_no_extra_chunk_work`: wide bounds updated to `wide_dirs(40,2,1)` ≤ 3, `wide_dirs(120,2,1)` ≤ 4.
- Results (dev): lib 57 pass/1 ignored; reader_v2 42/1; diff_walk 10/2; snapbench example 2; strict-ancestry 2; fmt, clippy -D warnings, rustdoc clean.

## Counts
Bounds only (exact counts not measured): wide_dirs(40,2,1) ≤ 3, wide_dirs(120,2,1) ≤ 4 (was ≤ 2 in fix4; review16 candidate 1).

## Assumptions
- `readable_storage_warns_once` ignored as instructed.
