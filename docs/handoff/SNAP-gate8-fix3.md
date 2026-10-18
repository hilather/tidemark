# SNAP-gate8-fix3 — v2 reader: no extra chunk work for empty results

## Needs from coordinator
- Run the 2M-directory snapbench gate on the host (not run here).
- Note: release-profile store tests fail in this sandbox (strict ancestry rejects `/` owned by uid 65534); all store tests were run in the dev profile, where they pass. Commands needed `CARGO_HOME=/home/brewerm/.cargo`.

## Change (`crates/tidemark-store/src/reader_v2.rs`)
- `children`: the parent path (`cached_path`) is built lazily, on the first admitted hit only. An empty result (leaf, or limit 0) no longer decodes ancestor chunks. Sibling validation (`visit_shallow_children` / `hop_children`) is untouched; limit 0 on a non-leaf still validates siblings (review13 S1).
- `subtree_dirs`, `wide_dirs`: after root authoritative check and limit validation (`TopN::new`), `limit == 0` returns an empty list before building the path or scanning.
- `metrics_candidates`: unchanged (it counts with limit 0, as v1; its base is the root path, no ancestors).

## Tests
New unit test `empty_results_do_no_extra_chunk_work` (balanced 121 dirs, 16/chunk, fresh `Snapshot` per query, plain_depth 0 and 3): `children(40,Name,{0,1,1000})`, `subtree_dirs(0,4,Disk,0)`, `subtree_dirs(40,0,Name,0)` each assert decodes+scanned ≤ 1. With the source change reverted it fails (`children 0: 3`). All existing tests pass (lib 56 incl. new, reader_v2 42, diff_walk 10, snapbench example 2, strict-ancestry 2); fmt, clippy -D warnings, rustdoc clean.

## Counts (fresh query, total chunk work)
| Query | before | after |
|---|---|---|
| children(40,Name,0) | 3 | ≤1 |
| subtree_dirs(0,4,Disk,0) | 9/8 | 0–1 |
| subtree_dirs(40,0,Name,0) | 3 | ≤1 |

## Assumptions
- `children` with limit 0 on a non-leaf still scans siblings to keep required validation; only the leaf/empty case is cheap.
- Validations of rows in a zero-limit subtree/wide query are skipped (nothing is returned).
