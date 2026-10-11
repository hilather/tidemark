# Handoff — W1-E-fix: Address all five store review findings

- Branch: `hp-eb657f11f6454d5d4ff7aead015eeafdef660a130b8c3f14c078c19a03a48be5`
- Attempt: `attempt-eb657f11f6454d5d4ff7aead015eeafdef660a130b8c3f14c078c19a03a48be5`
- Date: 2026-10-11
- Status: implemented; submitted as evidence for independent verification.

## What changed and evidence

| Finding | Implementation | Regression evidence |
|---|---|---|
| 1 — newest complete snapshot | Protect index zero in each root's newest-first list regardless of policy; calendar representatives still include it. | `retention_utc_days_dst_and_iso_weeks_per_root` now expects five deletions for all-zero policy; `retention_preserves_newest_with_expired_windows_for_each_root` exercises zero recent retention, expired windows, multiple roots and all zeros. |
| 2 — boundary globs | Compile globset with literal path separators, match `Candidate::from_bytes`, assign every subtree directory its deepest matching ancestor region using an iterative stack, and select sets spanning distinct regions, including the unmatched outside region. Preserve ordering, stored-instance counts, incomplete filtering, mutual exclusion with depth, and apply limit after filtering. Region storage and pattern count use the existing 1,000,000-row cap; candidates and instances stream. | `glob_boundaries_match_raw_bytes_and_nested_regions` covers matching/nonmatching regions, shared parent region, nested/overlapping globs, matched versus outside, invalid UTF-8, replacement-character nonmatching, invalid globs, mutually exclusive depth, and zero limit. Existing depth/no-instance regressions remain passing. |
| 3 — sample query work | Fixed resolution equality in all correlated checks; coarser checks seek exact bucket timestamps, finer fallback checks seek bounded timestamp ranges. SQL constants allow testing the actual production statements. | `tier_checks_seek_timestamp_primary_keys` checks EXPLAIN QUERY PLAN for all five tier checks in each of fs/LVM/PG statements. `retained_400_day_history_returns_all_tiers` inserts 24,024 rows per query family (8,760 hourly + 9,504 five-minute + 5,760 raw) and compares all returned timestamps. `overlapping_sample_tiers_preserve_selection_for_all_query_families` checks all three requested resolutions, finer fallback, coarse precedence, and optional PG name. Existing mixed-history and compaction tests pass. No time assertion. |
| 4 — corrupt recovery | Add `catalog::recover_catalog`, preserving the DB/WAL/SHM by rename into an exclusively created private `timeseries-recovery-N` directory inside state_dir (bounded 10,000 namespace attempts), syncing directories, creating a fresh database and rebuilding published snapshot rows. Ordinary opens retain their previous behavior. | `recover_corrupt_catalog_preserves_database_and_sidecars` checks ordinary open refusal and unchanged corrupt DB bytes, rebuilt scan rows, exact preserved bytes for all three artifacts, and unique names on repeated recovery. |
| 5 — floor rounding | Decode the finite nonnegative percentage f64 as an exact binary rational; use checked u128 multiplication and integer ceiling division, then enforce the 2 GiB floor. Very small positive percentages conservatively round to one byte before the fixed floor. | `space_floor_does_not_round_down_large_sizes` tests size 2^62 + 512, avail 2^62, 100%; also fractional 12.5% requiring one extra byte. Existing decision-table tests pass. |

## Shared-file edits

- `crates/tidemark-store/Cargo.toml`: attach `globset.workspace = true`, required to implement the contracted byte-path boundary matcher.
- `Cargo.lock`: add globset to tidemark-store's dependency list; existing cached version and dependency graph unchanged.
- No workspace manifest, core types, schema, plan, or module declaration changed.

## Recovery API notes

The task explicitly accepts SCR-1 as proposed; the older SCR document still says pending and was left untouched as required. Callers must stop writers and close all catalog/sample connections before recovery. `Catalog::recovery_files() -> &[PathBuf]` reports the preserved paths; historical samples are unavailable in the fresh database but remain in those artifacts. Each successful preservation also emits a tracing warning with its path, useful if later rebuilding fails. Recovery performs no deletions. `tidemark_store::catalog::recover_catalog` is callable now; any crate-root re-export is left to the coordinator because `lib.rs` is outside scope.

Glob region interpretation is the same as existing depth-region ownership: a matched directory defines a subtree; a deeper matching directory takes precedence, and all unmatched instances share an outside region. Globs match complete raw directory paths, with `*` confined to a component. All links in one region are excluded. No paths are converted lossily for matching.

## Validation

Rust 1.98.0 on PATH, `CARGO_HOME=/tmp/tidemark-w1efix-cargo` copied from the host registry, `CARGO_NET_OFFLINE=true`. All final checks exited zero:

- `cargo fmt --check`
- `cargo clippy --offline --workspace --all-targets -- -D warnings`
- `cargo test --offline --workspace`
- `cargo build --offline --workspace --features privileged --tests`
- `tools/ci.sh` (source policy, format, clippy and workspace tests)
- `git diff --check`

Logs and exit files are under this attempt's worker-output `library/`. CI includes 3 store unit tests, 23 passing store integration tests, one ignored store scale test, and 2 store doctests. Cargo-deny is not installed; CI explicitly skipped that audit.

## Remaining limits

All five requested implementation changes are complete locally. Independent verification and integration remain coordinator responsibilities. Existing privileged publication/ownership and bind-mount tests compile; runtime execution is pending VM. No privileged, destructive, mount, LVM or service operation was run. Crash/power-cut recovery behavior was not exercised. The previously established 1,000,000-row reader cap also bounds glob-region materialization; queries above that cap return an error rather than truncate.
