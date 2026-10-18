# SNAP-gate8-fix4 — v2 reader: review15 S1/S2

## Needs from coordinator
- Run the 2M-directory snapbench gate on the host (not run here).
- Tests were run in the dev profile (release store tests fail in this sandbox on strict ancestry, as in fix3). Commands needed `CARGO_HOME=/home/brewerm/.cargo`.

## Changes (`crates/tidemark-store/src/reader_v2.rs`)
- S2: removed the `limit == 0` early returns in `subtree_dirs` and `wide_dirs`. A zero limit now runs the same enumeration and checks as limit 1 (shallow walk, `Walk::finish`, malformed-row rejection, chunk scan with envelope checks) and admits nothing.
- S1: `wide_dirs` walks with paths relative to `/` and builds the root's real path (`self.path`) once, on the first admitted hit; hit paths are base + relative suffix. Filtered-out queries decode no ancestor chunks. `children` lazy path from fix3 is unchanged.

## Decision (coordinator)
A zero-limit query may decode as much as the same query at limit 1; rejection correctness wins over the work bound for this degenerate case. Side effect: in `wide_dirs` the `MAX_PATH_BYTES` check on the full path now applies to admitted hits only (relative length is still checked for every row); an over-long path of a non-admitted row is no longer rejected (v1 never built such paths either).

## Tests
- `empty_results_do_no_extra_chunk_work` updated: `subtree_dirs(0,4,Disk,0)` and `(40,0,Name,0)` ≤ the limit-1 count; empty leaf `children` ≤ 1; `wide_dirs(40,2,1)` and `wide_dirs(120,2,1)` ≤ 2 (plain depth 0 and 3).
- New `zero_limit_keeps_corruption_checks`: the review15 S2 table (3 shallow mutations at plain_depth 2; chunk 32 → x'00' at plain_depth 0 for `wide_dirs(0,0,0)` and `subtree_dirs(0,4,Name,0)`) is `Invalid` at limit 0 (and 1).
- With the early returns re-added, the new test fails (verified).
- Results (dev profile): lib 56 pass/1 ignored; reader_v2 42/1; diff_walk 10/2; snapbench example 2; strict-ancestry 2; fmt, clippy -D warnings, rustdoc clean.

## Counts (fresh Snapshot, 121 dirs, 16/chunk)
| Query | review15 | now |
|---|---|---|
| wide_dirs(40,2,1) | 3 | ≤2 |
| wide_dirs(120,2,1) | 4 | ≤2 |
| subtree_dirs(0,4,Disk,0) | 1 (skipped checks) | = limit 1 |
| children(40,Name,*) | 1 | 1 |

## Assumptions
- `most_shared` keeps its own limit-0 early return (not in scope).
