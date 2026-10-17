# Handoff — SNAP-codec: snapshot v2 chunk codec

## Needs from coordinator

- Independent review and uid 1000 / umask 022 / socket-enabled and GitHub runner gates remain with the coordinator.
- `cargo-deny` is unavailable here; run the equipped license/advisory gate. New-crate license inventory is below.
- Review the documented API assumption that encoders return `Result<Vec<u8>, StoreError>` so invalid input and compression failures are reportable.
- Later SNAP tasks must integrate these pure codecs with writers/readers and run the v2 latency gate. This task changes no snapshot file handling.

## What changed

Added the owner-supplied `15-snapshot-v2.md` verbatim and its README contents entry. Added zstd with default features disabled and locked the task's cached versions. Added a pure, safe `chunk` module with documented `ChunkDir`, `ChunkFile`, `ChunkLinkInstance`, `HashWidth`, default/custom-level encoders, full decoders, and the five-column `DirProjection` decoder.

The codec writes the version/column-length header and exactly one zstd frame. It validates canonical LEB128, integer widths and SQLite signed limits, parent/subtree arithmetic, consecutive directory IDs, file/link ordering, all column boundaries and end-of-frame/end-of-column boundaries. Signed arithmetic uses i128 intermediates, allowing zig-zag deltas between opposite i64 timestamp extremes without overflow. Projection validates all columns while constructing only the five scalar sort values, without allocating names or full directory rows.

## Shared-file edits

| File | Edit | Reason |
|---|---|---|
| `Cargo.toml` | One workspace zstd dependency | Owner-approved in-memory frame compression |
| `crates/tidemark-store/Cargo.toml` | One `zstd.workspace = true` line | Use the workspace dependency |
| `Cargo.lock` | zstd, zstd-safe, zstd-sys and jobserver; cc dependency edges | Offline dependency resolution |
| `crates/tidemark-store/src/lib.rs` | Only `pub mod chunk;` | Export the codec module |
| `docs/plan/README.md` | One contents row | Link the approved v2 contract |

## Assumptions and bounds

- Encoders return `Result<Vec<u8>, StoreError>` so invalid input, compression-level errors and zstd failures are reportable without panic; the spec's signatures are treated as illustrative pure API shapes. All codec failures use `StoreError::Invalid`.
- `file_lo` and `file_count` are u32, matching the frozen notable-file row-id contract. File row IDs are supplied externally by the chunk-table row range; they are not encoded.
- NULL names sort before present names. Arbitrary bytes, including empty names, are preserved without filesystem name validation.
- Constants cap every chunk at 8192 rows, every name/root path at 4096 bytes, raw columns at 40 MiB and compressed input at 41 MiB. The writer's normal directory target remains 4096. Zstd decoder window limit is 64 MiB. No recursion, I/O, SQLite, clock, async runtime or unsafe code was added.
- Signed LEB128 zig-zag deltas may need 65 payload bits to represent the difference between i64::MIN and i64::MAX; unsigned stored scalars remain checked against their v1 widths and SQLite limits.

## Dependency licenses

`cargo-deny` is not installed. The new packages' cached Cargo manifests declare:

| New package | Version | License |
|---|---|---|
| zstd | 0.13.3 | MIT |
| zstd-safe | 7.2.4 | MIT OR Apache-2.0 |
| zstd-sys | 2.0.16+zstd.1.5.7 | MIT/Apache-2.0 |
| jobserver | 0.1.35 | MIT OR Apache-2.0 |

The bundled upstream Zstandard license is BSD-3-Clause (also dual-licensed with GPL-2.0; the BSD option applies). These permissive license options are allowed by `deny.toml`. No OpenSSL dependency was added.

## Validation evidence

All long commands used `timeout 1500` and offline Cargo configuration, as uid 0 in the socket-restricted worker.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0; also rerun by CI on the final tree |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0; final-tree rerun included in CI |
| `cargo test --workspace --no-fail-fast` | Exit 0; summed harness results: 1044 passed, 0 failed, 14 ignored |
| `cargo build --workspace --features privileged --tests` | Exit 0; built only, never executed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Exit 0 |
| `cargo test --locked --offline -p tidemark-store --lib chunk:: -- --nocapture` | Final 12 codec tests passed, 0 failed, 0 ignored |
| `tools/ci.sh` | Exit 0; source checks: 151 owner tags; final printed harness totals: 1048 passed, 0 failed, 14 ignored; privileged build passed; cargo-deny unavailable (documented skip) |
| Exact specification comparison and `git diff --check` | Passed |

Harness totals include subprocess test harnesses. The standalone workspace run compiled the codec before the last two additional wire-layout/level regression tests were added; the final focused run includes both, and CI runs the final tree. Property configurations execute 64 cases for each of three row kinds, 8 full-chunk cases, and 3072 mutation cases (3272 generated cases total). No real-time assertions or clock calls were added.

## Before/after and acceptance coverage

Before this change the v2 specification and chunk module did not exist and store had no zstd dependency. Afterward all three column layouts have round-trip properties, independent wire-column fixtures, and full-chunk coverage. No existing product output is changed. No baseline failing regression was claimed: the new tests establish new functionality.

Initial command setup failed because `cargo` was absent from PATH and the default rustup home was unwritable. Subsequent commands explicitly use `/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin`, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`.

Property tests cover all fields, SQLite maximum counters, negative/extreme mtimes, arbitrary empty/4096-byte names, NULL names/project IDs, single rows and full chunks. Mutation testing performs 3072 generated cases, applying 1–15 edits to valid blobs and also decoding arbitrary random byte strings through all four decoder APIs. Deterministic corruption tests reject every truncated prefix, wrong versions, extra frames/trailing bytes, overlong varints, column under/overruns, excessive row counts, length bombs and integer overflows. Invalid results must be `StoreError::Invalid`; panics fail tests.

## Measurements

`cargo test --locked --offline -p tidemark-store --lib chunk:: -- --nocapture` printed:

```text
4096 directory sample: 25819 bytes; 6.3035 bytes/row
```

This includes the column header and frame at level 3. `deterministic_size_sample` specifies the complete fixture: a synthetic preorder chain, fixed-pattern names, increasing inode IDs, daily timestamp steps, modular counters and many zero/default fields. It is highly compressible synthetic data, not a production footprint estimate or the later SNAP-bench workload. The sample round-trip is asserted. The verbatim spec SHA-256 is `7ee3d702556b19f0b8d4d24c0823616aeedebbe5e9fe575d370399bd4a5fcc5c`.

## Remaining uncertainty

No privileged test execution was performed. The codec itself has no privileged behavior; existing privileged tests are build-only here and pending VM execution. Full snapshot integration, cross-version reading, representative snapshot-size measurements and the latency benchmark belong to later SNAP tasks. This handoff is local evidence for coordinator review, not independent verification or release approval.

## Completion checkpoint

- Only the eight assigned paths changed; shared edits are listed above.
- Source checks, formatting, Clippy, workspace tests, privileged compilation and rustdoc checks completed locally.
- Final CI exercised all 12 codec tests. It includes two strict-ancestry tests and subprocess harnesses in its aggregate totals.
- License inventory is supplied; equipped cargo-deny, uid 1000/GitHub checks and VM execution remain external gates.
- No unexplained TODOs, unsafe blocks, or production unwrap/expect calls were introduced.
