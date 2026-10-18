# SNAP-reader2-fix5b handoff

## Needs from coordinator
- Run `cargo test --workspace` / `tools/ci.sh` on the host (hang in this sandbox). Cargo needed `CARGO_HOME=/home/brewerm/.cargo` here.

## What changed (`crates/tidemark-store/src/reader_v2.rs` only)
- New `Backend::authoritative_root`: reads the root from its directory chunk (`raw`, same source as `dir(id)`); if a shallow copy exists it must match in id, parent_id, depth and subtree_end, else `StoreError::Invalid`.
- `subtree_dirs` and `metrics_candidates` (root 0, which feeds `eligible`) use it before choosing branch, ranges and depth limits.
- Audited other shallow-row consumers: `visit_children` (children*) already uses `raw` for the parent; `wide_dirs` uses `raw`; `eligible` takes `hi` from the now-authoritative root, and its shallow walk is validated by `ShallowWalk`. No further changes needed.

## Tests (in `reader::v2::child_cursor_tests`)
- `fix5_subtree_root_bounds_come_from_the_chunk`: (a) shallow subtree_end=0 → `subtree_dirs(0,1,..)` and `metrics_candidates` Invalid; (b) 121-dir tree, plain_depth=3, shallow depth=1 → Invalid. Fails with the source change reverted, passes now.
- `fix5_valid_subtree_results_match_v1`: v1/v2 parity on valid input (passes before and after).

## Counts
store lib 47 pass (1 ignored); `--test reader_v2` 35 pass; other store test targets all pass. fmt, clippy (workspace, -D warnings), privileged tests build, rustdoc -D warnings clean.
