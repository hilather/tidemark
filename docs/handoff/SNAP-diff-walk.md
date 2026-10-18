# SNAP-diff-walk — snapshot diff as a tree walk

## Needs from coordinator

1. **Latency gate (§8) for diff is NOT met on v2/v2.** 2,000,101 dirs vs 1%-changed copy, depth `u32::MAX`, limit 50, `-O3` + debug-assertions test build, median of 5 (machine was noisy, other jobs running):

   | pair | implementation | median | min–max |
   |------|---------------|--------|---------|
   | v1/v1 | old ATTACH join (reference) | 8.1 s | 6.9–13.1 s |
   | v1/v1 | walk | 48.6 s | 36.6–56.6 s |
   | v2/v2 | walk, warm | 14.0 s | 9.7–22.1 s |
   | v2/v2 | walk, fresh `Snapshot::open` | 25.4 s | 23.0–27.3 s |

   Gate = max(1.25 × 8.1 s, 100 ms) ≈ 10 s, so v2/v2 is ~1.4–2.5x over. Cost is per visited directory: each `DirRow` the readers return carries a materialised `RawPath` (v2 rebuilds it by walking parents through chunks) and each side is visited once (4M rows). Leaf dirs (`subtree_end == id`) already skip the child query. A cheap fix needs the reader (not mine): a crate-internal `children_names_page(id, cursor, limit) -> (id, name, subtree_end, metric values)` that skips path materialisation, and/or v2 `children_page(SortKey::Name)` reading chunk rows without `path()`. With `path` built by the diff from parent path + name this should be several times faster. Tell me if you want me to use such a method once SNAP-reader2-queries lands one.
2. `Snapshot::backing_file` / `reader.rs:48` would be dead code if the v1 fast path were removed; it is still used (v1/v1 path), so no warnings today. If you remove the v1 path later, delete `backing_file`.
3. `tests/reader_v2.rs` still asserts diff → `NotYetImplemented` on v2; untouched as instructed. Those assertions now fail until the parallel task removes them (check: `cargo test -p tidemark-store --test reader_v2`).

## What changed (`crates/tidemark-store/src/diff.rs`)

- `diff`/`explain` signatures, types, errors, caps unchanged.
- **Walk** (all pairs except pure v1/v1): iterative DFS over matched pairs; each frame holds two `Kids` cursors that page `children_page(SortKey::Name)` (page 256, doubling to 16,384; leaf dirs skipped) and merge by name bytes; added/removed subtrees are the same loop with one side empty. Recurses only to `max_depth_rel`. State = O(depth × page), no recursion. I chose paged `children_page` over merging two `subtree_dirs` streams: `subtree_dirs` returns a Vec (bounded only by the 1M cap, O(subtree) memory).
- **Top-N**: `TopN` bounded max-heap keyed exactly like the old ORDER BY (`|delta|` desc / `delta*sign` desc, then raw path bytes asc — byte order, not preorder). Memory O(limit). `explain` count of children moving in the target direction is computed in the same pass.
- **v1/v1 fast path kept** (`ranked()` dispatch by `v1_conn().is_ok()` on both): the old ATTACH join code is retained unchanged (`with_old`, `joined`, `joined_sql`, `bind_joined`). The walk is ~6x slower than the SQL join on v1 (per-row `children_page` SQL round trips), so per the task I kept the SQL path for v1/v1 only. Output is byte-identical either way (tests below). This is the only remaining use of `v1_conn()`/`conn()` in diff.rs.
- The old `#[ignore = "scale"]` EXPLAIN-based unit test and the `with_old` restore/unwind test were removed from `diff.rs` (the first needed `new.conn()` and the `w3a/row.rs` include; the scale test is superseded by `scale_diff_old_sql_vs_walk`). Replaced by 3 `TopN` unit tests.

## Tests (`tests/diff_walk.rs`, new)

- Reference: `sql_reference` is a verbatim copy of the old SQL run through rusqlite directly (v1/v1), plus a `model` oracle built purely from `dir_by_path` + `subtree_dirs` (all pair kinds).
- `diff_matches_model_and_old_sql_for_every_pair_kind`: pairs v1/v1, v2/v1, v1/v2, v2/v2, cold-v1/v2, v2/cold-v2; generated trees with grow/shrink/same/new/removed/rename/move, non-UTF-8 names, `a/x` vs `a-c`, tied deltas; targets root/random/one-side-only; 3 metrics × depth {0,1,2,100,MAX} × limit {0,1,3,1000,1M}; asserts every `DiffClass` occurred. Writer chunk sizes 1–4 dirs and `plain_depth` 0–2, hash width 32/64 for v2.
- `explain_matches_reference_loop_for_every_pair_kind`: steps, target/own delta, spread rows, `spread_children`, and SQL equality of each step's children query on v1/v1.
- `errors_and_missing_targets` (absent both, limit > 1M, bad ratio, different roots v1 & v2), `ties_order_by_raw_path_bytes_not_preorder`, `wide_directory_crosses_page_boundaries` (1,500 children, v1/v1, v2/v2, v2/v1).
- `scale_diff_old_sql_vs_walk` (`#[ignore = "scale"]`): produced the table above. Run: `CARGO_PROFILE_TEST_OPT_LEVEL=3 cargo test -p tidemark-store --test diff_walk -- --ignored --nocapture scale` (plain `--release` fails here because release builds reject this sandbox's `/` owner in the state-dir check; `snapbench` fails the same way, and only supports `--layout v1`, so I did not use it).
- Caveat: for pure v1/v1 the walk is not what runs, so v1/v1 coverage of the walk code comes from the mixed pairs (the v1 reader is used on one side there). Cold tests (`tests/cold.rs`, `tests/w3a`) untouched.

Failing before / passing now: all `diff_walk` diff/explain tests on any pair with a v2 side returned `NotYetImplemented` before; they pass now.

## Assumptions

- "v1/v1" is detected by `v1_conn()` succeeding on both (v2 returns `NotYetImplemented`).
- i128 arithmetic for ordering keys; row deltas still go through the old checked `i64` conversion (same error on overflow, only for returned rows).

## Test counts / gates

- `tidemark-store`: lib 34 pass (3 new TopN tests); `diff_walk` 5 pass + 1 ignored(scale); `cold` 25, `w3a` 7 (1 ignored), `w3b`/`w3c`/others pass.
- Expected failure: `tests/reader_v2.rs::legacy_queries_return_not_yet_implemented` (asserts diff is NotYetImplemented on v2; owned by the parallel task).
- `tidemarkd` tests `w6a2`/`w6a3`/`w6a5` (and the operator socket tests) fail in this sandbox because sockets are unavailable; unrelated to this change.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --features privileged --tests`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`: clean. `tools/ci.sh` not run (it repeats the workspace tests, which need sockets).
- Before adding the v1/v1 SQL fast path, the same `diff_walk` suite ran with the walk handling v1/v1 too and matched the old SQL on every case, so the walk is verified against the old SQL for v1 data; the fast path is a pure speed choice.
