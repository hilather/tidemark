# Handoff — SNAP-writer2

## Needs from coordinator

- Independent review, uid 1000 / umask 022 / socket-enabled validation, and GitHub ubuntu-24.04 validation remain coordinator gates.
- Run the equipped cargo-deny advisory/license gate if the local CI reports it unavailable.
- Keep production scans on v1 until the separate reader/latency benchmark and orchestrator tasks pass. This writer is exported but has no production callers.
- The assigned base `3efed8f` contains neither `cold.rs` nor a `pub mod cold;` line, despite the task's retained note that SNAP-cold-store landed. No cold files or declarations were changed; reconcile that integration ordering separately.

## What changed

Implemented `SnapshotWriterV2` with the existing writer's trust checks, exclusive 0600 temporary leaf, batched insert APIs, space refusal, poisoned-writer behavior, abort/drop cleanup, final identity checks, fsync, NOREPLACE rename, and directory fsync. `WriterOptions` validates chunk sizes against the codec's 8192-row cap and records all six options in metadata.

Directories, notable files, link instances and plain auxiliary tables stage in a separate private SQLite file beside the unpublished output. Staging-only indexes sort files by `(dir_id,name)` and instances by `(ino,dir_id,name)`. A disk-backed per-directory span table replaces a whole-tree map. Each output codec buffer holds at most one configured chunk. Both SQLite page caches are configured to 8 MiB; staging sort scratch uses file storage. Final output has only the specified v2 tables and indexes, and no staging free pages. ANALYZE is omitted because there is no measured benefit for the two simple final indexes.

`files.id` in v2 is the zero-based clustered row number supplied by `file_chunks.first_row..=last_row`. Incoming v1-style file IDs are discarded, including repeated IDs across batches. This is the explicit task/spec §9 semantic change; public core row types remain untouched.

Cancellation retains all v1 phases and adds checks after staging indexes, at chunk boundaries, and while copying plain rows. Successfully published files are never removed by Drop. Preexisting staging leaves and competing final publishers are preserved.

## Shared-file edits

Only `pub mod writer_v2;` directly after `pub mod writer;` in `crates/tidemark-store/src/lib.rs`. No manifest or dependency changes.

## Assumptions

- `create` uses defaults; `create_with_options` accepts a `WriterOptions` value. Compression levels are 1..=19, matching the landed codec; all chunk sizes are 1..=8192. `plain_depth` is a caller-selected u32, default 2.
- Directory IDs follow the existing increasing-preorder input contract. The codec validates consecutive IDs inside each chunk. Names and paths remain raw bytes; lookup keys are recomputed from `RawPath::hash64`, rather than trusting a caller-supplied hash.
- Empty directories use the current clustered insertion offset as `file_lo` and zero as `file_count`, so offsets remain monotonic across chunk boundaries.
- Tests use fixed injected metadata timestamps. No wall-clock assertions, sockets, privileged filesystem operations, or production scan changes are introduced.
- SQLite staging is temporary writer state, not part of the published schema. Space checks keep v1's per-input-row scheduling and finish's cancellation semantics.

## Acceptance evidence

- Raw chunk decoding checks every stored directory field, reconstructs paths through parents, checks all file fields and clustered order, checks spans (including empty directories), finds every directory by the configured hash, compares shallow SQL columns with v1, and checks exact link sets/instances/skips/errors/annotations and metadata.
- Golden link-semantics directory topology from test-support exercises 32-bit hashes and chunks of 2/3/4 rows; the 50,000-directory generated tree exercises default chunks and 64-bit hashes.
- Cancellation test counts and then cancels at every reached checkpoint; each cancellation leaves no files. Error, space refusal, explicit abort, Drop, invalid final metadata, staging symlink/preexisting leaf, duplicate creation, and competing publisher paths are covered.
- The streaming workload sends 50,000 directories and 100,000 unordered instances in batches of at most 512, then checks every decoded instance and staging removal.
- Unit tests cover default options, codec row bounds, and final metadata failure cleanup.

## Before/after and validation

Baseline has no v2 writer API or writer-v2 tests. New tests were added with the implementation; no baseline red-test execution is claimed. During development, the new integration test initially failed to compile because rusqlite does not decode u64 directly; changed the page-count read to i64. Initial Clippy found test unwrap allowances and an identical conditional branch; both were corrected, with unwrap allowances confined to tests.

All long commands used `timeout 1500`, the pinned Rust 1.98.0 toolchain, the host Cargo cache, and `CARGO_NET_OFFLINE=true`, as uid 0 without sockets.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0, including final recheck |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0, including final recheck |
| `cargo test --workspace --no-fail-fast` | Exit 0; final CI invocation reports 1,056 passed, 0 failed, 14 ignored across harnesses |
| `cargo build --workspace --features privileged --tests` | Exit 0, including CI's final build; VM suites not executed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Exit 0 |
| `tools/ci.sh` | Exit 0; 152 owner tags and forbidden-symbol checks passed; final harness totals 1,058 passed, 0 failed, 14 ignored, including the separate 2-test strict-ancestry run |
| Writer integration binary, final source | 8 passed, 0 failed; raw-table output and measurements captured with `--nocapture` |
| New writer unit tests | 2 passed, 0 failed in the final workspace run |
| Isolated streaming workload | 1 passed; final unprivileged executable RSS measurement below |

The first full workspace run had 1,055 passing harness results before the final incoming-file-ID regression test was added. Final CI reran the complete workspace on the final source with all 10 new tests (8 integration + 2 unit). Cargo-deny was not installed; CI explicitly skipped its advisory/license check. No dependency changes were made.

Transcripts, reproducible measurement helpers, and executable feature fingerprints are in this attempt's `library/` output directory. Independent uid 1000, GitHub, and VM gates remain pending.

## Measurements

Generated 50,000-directory fixture, identical input rows to both writers (37 notable files, 30 link instances, one link set/skip/error/annotation):

| Format | Bytes | Bytes/directory |
|---|---:|---:|
| v1 | 10,354,688 | 207.09 |
| v2 | 1,220,608 | 24.41 |

These are debug-build synthetic persistence measurements on the worker filesystem, not the §8 latency benchmark. Most generated directories have depth 4; only the depth <=2 prefix is duplicated in shallow SQL. A shallow-heavy tree deliberately has a larger plain-table footprint.

## Remaining uncertainty

The 2-million-directory latency gate and 10M+ link-instance scale run are outside this task's measured workload. No VM operations are required for this writer; workspace privileged suites remain pending VM execution (see the measurement-run deviation below). Independent verification and integration are not claimed by this handoff.

## Measurement-run deviation

The first RSS helper selected `writer_v2-3927b702b9de118b`, whose Cargo fingerprint records `["privileged"]`, by executable filename. It ran only `streamed_50k_dirs_and_100k_instances`, an unprivileged writer test with no VM or privileged operations; no privileged fixture or VM suite was run. This nonetheless departed from the instruction to build only feature-enabled tests. The final measurement explicitly used `writer_v2-831428076e0ad21c`, whose fingerprint records `[]`, and the saved helper now filters fingerprints to exclude feature-enabled binaries. Final peak process RSS: **24,408 KiB (23.84 MiB)**, including the harness, 512-row input batches, SQLite caches, codecs, and post-write decoding. This is an observed workload peak, not a universal maximum or the §8 benchmark.
