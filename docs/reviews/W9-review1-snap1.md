VERDICT: READY
BLOCKER 0 / SHOULD-FIX 0 / NOTE 1

Reviewed exact candidate `82d7e1717a238b23d66d3c567bcc7dc8c2f23075` against base `2a5cdea611ea36245688778f82a39fea4c6669b9`. Also reviewed the requested benchmark supplement at supplied checkout `6e3e4974eae7bead8acde056efbfe750e50c38c4`, including `git diff 2a5cdea..6e3e497` and the corresponding commit log. The candidate contains the codec, dependency and v2 specification; the checkout adds only `snapbench.rs`, its handoff and performance note. All candidate files are unchanged in that checkout. The single NOTE below belongs to the benchmark supplement, not the exact codec candidate. READY is a review recommendation, not release approval or independent verification of every handoff claim.

Read both SNAP handoffs, `AGENTS.md`, `15-snapshot-v2.md`, the checked-conversion and row contracts in `05-storage.md`, normative `03-link-semantics.md`, and the relevant reader, writer, CLI/TUI and dependency sources. No SNAP task section exists in the supplied `docs/plan/waves/` files; the assigned review instructions and handoffs supplied the scope.

## Finding

**NOTE N1 — Correct the documented longest generated name.** `docs/perf/snapbench.md:11` says the four name lengths are 4, 8, 17 and 40 bytes. In `crates/tidemark-store/examples/snapbench.rs:117–130`, the four-byte base-36 prefix followed by `-results-long-repeated-component-data` produces **41** bytes. For example, `name(3)` is 41 bytes. Someone reconstructing the workload from the prose would therefore use a different path-length distribution. Change 40 to 41 in the performance note; the supplied executable command still reproduces the intended fixture and this does not invalidate its measured queries.

## Codec and format assessment

No supported format mismatch was found against `15` §4. The version byte, canonical unsigned LEB128 column count and lengths, concatenated column payload, and single zstd frame agree with the contract (`chunk.rs:153–310`). The default compression level is 3.

| Layout | Ordered fields checked |
|---|---|
| Directories, 33 columns (`chunk.rs:324–411`) | Parent distance, subtree distance, depth, raw length/name, signed inode and mtime deltas, signed newest-minus-mtime, nullable project id, the 23 counters from `dir_disk` through `max_entries`, file-start delta, file count |
| Files, 7 columns (`chunk.rs:580–606`) | Directory delta, nullable name, signed inode delta, disk, apparent, signed mtime delta, link count |
| Link instances, 3 columns (`chunk.rs:651–672`) | Unsigned inode delta, signed directory delta, nullable name |

The first delta uses zero as its predecessor. Root parent zero is interpreted only at id zero. Project id zero means NULL and present values are shifted by one; nullable names distinguish NULL from a present empty name. File and link ordering is checked on encoding and decoding. Scalar widths match `05` §1: u32 fields use checked narrowing, unsigned u64 values must fit SQLite i64, and reconstructed timestamps must fit i64. i128 arithmetic accommodates the 65-bit zig-zag representation of differences between opposite i64 timestamp extremes.

The parser checks the fixed expected column count before allocating its length vector. It caps compressed input at 41 MiB and the checked sum of raw columns at 40 MiB before decompression (`chunk.rs:260–295`). Cached zstd source confirms the bulk output allocation cannot exceed that supplied raw bound. Names are limited to 4096 bytes and rows to 8192; loops consume input and do not recurse. Exact compressed-frame extent, decompressed size and final exhaustion of every column reject truncation, extra frames and trailing bytes. No input-driven unbounded allocation, arithmetic panic or nonterminating path was established.

Full directory decoding and projection share `visit_dirs` (`chunk.rs:428–572`), including validation of columns omitted from the projection. Round-trip tests cover all fields, SQLite-limit values, u32 boundaries, negative/extreme timestamps, arbitrary byte names, NULL names/project ids, full chunks and projection agreement. The wire fixture independently checks representative column bytes. These checks do not prove correctness for every possible input; cross-chunk tree relationships and file-range membership still require validation in the later snapshot integration.

## Dependency assessment

`Cargo.toml:23` disables zstd default features. The resolved offline feature tree contains zstd 0.13.3, zstd-safe 7.2.4 and zstd-sys 2.0.16+zstd.1.5.7 with `std`, without bindgen/libclang, legacy support, dictionary-builder or multithread features. The new manifests offer MIT/Apache-2.0 license options; bundled Zstandard offers BSD-3-Clause. Those options are allowed by `deny.toml`. No OpenSSL dependency was introduced.

The cached zstd-sys build script compiles bundled C/assembly and uses local headers; no build-time download operation was found. The existing EL8 environment installs gcc, and the workspace's pinned Rust version exceeds the new crates' declared minimums. The focused offline build succeeded locally. Native EL8 compilation and the equipped `cargo deny`/GitHub gates were **not** run; cargo-deny is absent here. License inventory and source inspection are not substitutes for those gates.

## Benchmark assessment

The iterative generator assigns contiguous name-sorted preorder ids and inclusive subtree ends. Same-directory hard-link pairs make its union and charged totals additive without double-counting each pair; incomplete odd-count sets are excluded from reclaimable totals. Directory inode bytes, file entries, inode counts, age buckets and newest times are rolled up consistently with `03`. The comparison modifies local directory counters every hundredth row, updates aggregates, and renames two leaves while preserving sibling order. Its limited hard-link topology is explicitly disclosed.

An independent Python structural transcription of the default generator confirmed 2,000,000 directories, median depth 7, maximum depth 12, two 100,001-child directories, and 1,799 exact 1,000-directory subtrees. It checked parent precedence, subtree interval cardinalities and sibling-prefix ordering. This supplements the Rust example's 4096-directory tests; it is not a full-size Rust benchmark execution.

The nine query families call the intended public APIs (`snapbench.rs:463–510`). In particular, top uses the CLI's 1,000,001-row candidate request, child paging uses Disk/descending/50, and notable queries request 50 rows. These are storage/query measurements rather than complete CLI rendering or growth-comparison costs.

Fresh-open timing includes open, query and handle destruction; warm timing follows an untimed priming call (`snapbench.rs:559–586`). Five samples yield the documented third-sample median and fifth-sample nearest-rank p95. No OS page-cache eviction is claimed. Diff's internal old-snapshot open, ncdu's sink output, and process-wide peak RSS including generation/writes are accurately disclosed. The normal and sandbox retained-directory commands are documented separately.

The reported 600,735,744-byte snapshots, roughly 1.9 GiB process peak and query timings are plausible for the fixture and SQL operations; no contradiction was found. The full 2M-directory latency/size run was not repeated, and its original raw measurements are not in the reviewed tree. Exact figures, isolated-host performance and physical cold-cache latency therefore remain independently unestablished. Future v2 comparison must use the same fixture and timing definitions; this review does not establish the v2 latency gate.

## Validation performed

Commands used the pinned Rust 1.98.0 toolchain, offline Cargo and a private target directory `/tmp/tidemark-review-9d19ca-target` on the supplied checkout. Its codec, dependency and existing library files match the exact candidate.

| Check | Observed result |
|---|---|
| `cargo test --locked --offline -p tidemark-store --lib chunk:: -- --nocapture` | 12 passed, 0 failed; includes mutation, bounds, wire-layout and round-trip tests |
| `cargo test --locked --offline -p tidemark-store --example snapbench` | 2 passed, 0 failed; deterministic fixture and writer/query round-trip |
| `cargo tree --locked --offline -p zstd -e features` | Passed; feature assessment above |
| `cargo fmt --all --check` | Passed |
| `git diff --check 2a5cdea..6e3e497` | Passed |
| Independent default-shape transcription | Structural checks passed; artifact identifies its method |
| `cargo clean --offline` with the same private target directory | Passed; removed review build outputs |

Evidence is retained in this attempt's worker-output `library/`: `focused-tests.log`, `zstd-features.log`, `format.log`, `benchmark-structure-review.json`, `cleanup.log`, `reviewed.diff` and `reviewed-log.txt`. No code was edited, no privileged commands or network fetches were used, and no branch was created, merged or pushed. Full-workspace tests, Clippy, cargo-deny, EL8/VM execution and the full release benchmark were not rerun; the handoffs' claims for those checks remain their supplied evidence.
