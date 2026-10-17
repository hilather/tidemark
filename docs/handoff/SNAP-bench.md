# Handoff — SNAP-bench: synthetic v1 snapshot baseline

## Needs from coordinator

- Independently verify this candidate, including uid 1000 with sockets and the
  Ubuntu 24.04 runner. This worker is uid 0 in an isolated socket-restricted sandbox.
- Use the same fixture and timing definitions for the subsequent v2/cold-file
  implementation. No contract or library changes are requested by this task.
- Run the dependency audit on an equipped host if needed: local CI skipped
  `cargo deny` because the tool is not installed.

- **Date:** 2026-10-17; **branch:** canonical `hp-847a419892806b07561a1326c55cf6a548d6f1df8aacdbe78918a6bee23ca5f0`.
- **Status:** implementation and local evidence supplied; independent verification pending.

## What changed

Added the standalone `tidemark-store` example `snapbench`, with deterministic
SplitMix64 generation, bounded iterative preorder tree construction, byte-sorted
names, path hashes through `RawPath`, log-normal allocated sizes, fixed-clock
mtime/age buckets, and additive subtree roll-up. It writes both snapshots through
`SnapshotWriter`; the second changes local directory-inode counters in 1% of rows
and renames two leaf subtrees, preserving consistent aggregates and producing
path-based additions/removals.

The nine public-API query families report five fresh-handle and five warm samples,
median/p95, snapshot sizes, fixture shape and process peak RSS. Warm queries have
an untimed priming run. JSON is saved in the output directory and optionally
printed to stdout; a human table is emitted to stderr. The layout validation,
writer dispatch and reader `open` dispatch isolate future v2/cold-file additions.
Two example tests cover deterministic rows, preorder/path/name/counter invariants,
incomplete hard links, writer round-trip, all query paths, and diff additions and
removals. No dependencies, manifests, modules or library files changed.

## Before and after

Before: no `snapbench` example or recorded synthetic 2M-directory v1 baseline.
After: the example and reproducible method/results in
[docs/perf/snapbench.md](../perf/snapbench.md).
No pre-existing failing test was identified; this is a new harness, not a
regression fix. The first full release invocation failed before any snapshot
write because `/` is owned by unmapped uid 65534 in this sandbox. The subsequent
invocation uses the library's existing `/proc/self/fd/<n>/.` retained-directory
entry point. The ordinary host invocation and exact sandbox invocation are both
recorded in the performance note. No ancestry policy was weakened.

## Assumptions and deviations

- The task's quoted §8 is the benchmark specification; the parallel task's
  `15-snapshot-v2.md` is absent from this baseline checkout.
- “Cold” means fresh `Snapshot::open`, including open/close cost, with no OS page
  cache eviction. Warm diff still opens the old snapshot internally, as the public
  diff API requires.
- Top uses the same `subtree_dirs(..., 1_000_001)` candidate fetch as CLI top,
  without CLI rendering; notable top-N requests 50 rows. Ncdu serializes the full
  tree to an I/O sink. No private SQL or library changes were needed.
- The 500,000 link instances are 250,000 complete same-directory pairs. This is
  consistent with normative link semantics but does not model cross-branch links.
  The 200,000 notable files are single-link files; link-set inodes are not notable.
  An odd requested link count exercises an incomplete final set.
- Fixed time is 1,780,000,000 Unix seconds. Generated rows are deterministic;
  normal scan IDs include process/sequence identity for publication uniqueness.
- Each count argument has a five-million cap; directory counts start at 32.
  Small smoke runs shrink wide groups and fall back to root for notable-subtree
  queries if no exact 1,000-directory subtree exists.
- RSS is whole-process `VmHWM`, including generation and writes. Measurements ran
  on a shared host while workspace validation was active; they are a local,
  non-isolated baseline. No latency assertions were added to tests.

## Acceptance evidence and measurements

The full-size run completed successfully under `timeout 1500` (approximately
15.5 minutes including generation and all repetitions). Reported shape: median
depth **7**, maximum depth **12**, wide-directory children **100,001**,
and notable-subtree directories **1,000**.

| Query | Fresh median ms | Fresh p95 ms | Warm median ms | Warm p95 ms |
|---|---:|---:|---:|---:|
| children_root | 24.637 | 27.551 | 24.654 | 25.447 |
| children_wide | 35.494 | 42.761 | 40.757 | 76.427 |
| top2 | 1465.106 | 2266.071 | 967.294 | 1066.926 |
| top4 | 1107.239 | 1120.611 | 1283.269 | 1347.978 |
| files_root | 2740.861 | 3183.925 | 2909.809 | 3456.611 |
| files_subtree | 2.604 | 2.761 | 1.420 | 1.560 |
| path12 | 1.092 | 1.167 | 0.052 | 0.063 |
| diff | 11211.209 | 13006.853 | 9967.726 | 12768.459 |
| ncdu | 69513.649 | 102254.268 | 36602.273 | 51366.590 |

Original snapshot: **600,735,744 bytes** (572.906 MiB).
Comparison snapshot: **600,735,744 bytes** (572.906 MiB).
Process peak RSS: **1,916,076 KiB** (1871.168 MiB), including generation/writes.
Both generated snapshots were deleted after retaining the JSON report and logs.
Measurements overlapped workspace validation on this shared machine; the p95
spread should not be interpreted as isolated-host variance.

| Criterion | Observed evidence |
|---|---|
| Default 2M / 200k / 500k shape | Full report fields and table above; 100,001-child target, exact 1,000-dir subtree, depth-12 lookup |
| Deterministic valid rows and aggregate accounting | `deterministic_tree_and_link_accounting` passes, including incomplete link accounting and byte-name ordering |
| Existing writer and public query APIs | `writer_roundtrip_and_every_query` passes; all nine query families complete at full size |
| Fresh/warm median and p95, >=5 runs | JSON has 18 measurement entries, each with five sorted timed samples |
| Flags and raw output paths | Final-source CLI smoke: dirs=32, files=3, links=5, seed=7, v1, path12, JSON, non-UTF-8 output directory; exit 0 |
| JSON and human output, sizes/RSS | Retained `full.json` / `full.log`, metadata and table above |
| Snapshot cleanup | Generated full-size and smoke database directories removed after report retention |


## Validation

All requested checks exited 0 using the pinned Rust 1.98.0 toolchain and host
Cargo cache in offline mode. Every long command was bounded by `timeout 1500`.

| Command | Observed result |
|---|---|
| `cargo fmt --check` | Passed on final source |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed on final source |
| `cargo test --offline --workspace --no-fail-fast` | Passed; output summaries total 1,034 passed, 0 failed, 14 ignored across 87 summaries (including subprocess helpers) |
| `cargo test --offline -p tidemark-store --example snapbench` | 2 passed, 0 failed |
| `cargo build --offline --workspace --features privileged --tests` | Passed; compiled only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Passed |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | Passed; 150 owner tags, forbidden-symbol scan, format, Clippy, strict-ancestry feature tests, workspace tests, privileged test build; dependency audit skipped because cargo-deny is absent |
| Final CLI smoke with byte-path output | Passed; custom counts/seed, v1 layout, selected query and JSON |

The two new tests had no pre-existing failing counterparts. The new CLI smoke
also confirms raw-byte output paths work on final source. Snapshots from both
full-size and smoke runs were removed. Raw logs and JSON are retained in this
attempt's output `library/` directory.

 No privileged tests were executed; this task adds no
privilege-dependent behavior or tests. The workspace privileged test build is a
compile-only check; VM execution remains pending the normal project gate.

## Shared-file edits

None. Only the three owned paths were changed.

## Remaining uncertainty

This attempt supplies evidence for review, not an independent verification result.
Actual physical cold-cache latency, cross-directory hard-link workload behavior,
v2 format size/latency, and isolated-machine measurements remain unmeasured.
