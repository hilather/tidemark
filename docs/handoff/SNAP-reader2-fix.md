# Handoff — SNAP-reader2-fix

## Needs from coordinator
- Nothing blocking. Re-run the review probes (B1/S1/S2/S3) if you want independent confirmation; the §8 representative latency gate was not run here.
- Note for review: chunk validation now costs two extra indexed lookups per cache miss (previous/next chunk), and one extra `length(data)` lookup.

## What changed (`crates/tidemark-store/src/reader_v2.rs` only)
- **B1**: a chunk is admitted (to the cache or to a result) only after `check_neighbours`: the first chunk starts at 0 and
  `prev.last + 1 == first`, `next.first == last + 1`. Overlap or gap → `StoreError::Invalid`. Used by `chunk()` and
  `projected_chunks()` (first and last visited chunk; in between, contiguity is checked while iterating). The cache holds only validated chunks.
- **S1**: `subtree_dirs` answers from `shallow_dirs` (root also from `shallow_dirs`) when `root.depth + depth <= plain_depth`,
  same `(−key, name, id)` ordering as the chunk path. `metrics_candidates` already used `shallow_dirs` via `eligible`; it now also reads
  its root row from `shallow_dirs`, so it no longer decodes chunk 0.
- **S2**: chunk coordinates and `length(data)` are read first; blobs above 41 MiB (mirror of chunk.rs `MAX_BYTES + 1 MiB`, which is private) are rejected before the BLOB is read, in the single-chunk and projected paths.
- **S3**: negative/out-of-range/wrong-typed `first_id`/`last_id`/`length`, `last < first`, and missing/non-numeric `plain_depth`/`hash_width` meta map to `Invalid` naming the table/column or key. A legitimately missing id still gives the v1 `QueryReturnedNoRows` error.

## Tests (`crates/tidemark-store/tests/reader_v2.rs`)
New: `b1_overlapping_chunks_are_invalid_cold_warm_and_range`, `b1_gaps_between_chunks_are_invalid`,
`s1_shallow_subtree_does_not_decode_deep_chunks`, `s2_oversized_blob_is_rejected_by_length`, `s3_corrupt_coordinates_and_meta_are_invalid`.

Before (original reader, new tests): all 5 FAILED. After: 5 passed. Full `reader_v2`: 14 passed (the 50k-directory test takes ~2.5 min unoptimized).

## Assumptions
- Strict contiguity is enforced for single lookups too (not only range scans); writer output satisfies it (all parity tests pass).
- S2's ceiling is mirrored as a constant because `chunk.rs` keeps it private.
- A `zeroblob` test uses 64 MiB sparse SQLite data; it is never read by the reader.

## Gates
- `cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, `cargo build --workspace --features privileged --tests` (built, not run), `RUSTDOCFLAGS=-D warnings cargo doc`: all clean.
- `cargo test --workspace --no-fail-fast`: all `tidemark-store` tests pass (incl. reader_v2 14/14, cold, storage, w3a-c). 9 targets in other crates fail in this sandbox and are unrelated to this change: tidemarkd/tidemark-cli socket tests (`socket ancestor / has untrusted owner 65534`), `tidemark-fs rules`, and `xtask w6c upgrade_fixture_options_and_drift` (already known, review S6). `tools/ci.sh` fails at the same step for the same reason.
