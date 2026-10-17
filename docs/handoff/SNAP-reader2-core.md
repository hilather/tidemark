# Handoff — SNAP-reader2-core

## Needs from coordinator

- Review the existing `tidemark-fs::lvm::tests::timeout_kills_and_reaps_child` intermittent failure noted below if it repeats with default parallel tests. It passed the standalone workspace run and an isolated retry. CI retry outcomes and harness settings are listed below. No out-of-scope LVM code was changed, and the underlying cause is unestablished.

- Independent review, uid 1000 / umask 022 / socket-enabled execution, and GitHub ubuntu-24.04 validation remain coordinator gates. This handoff is worker evidence, not independent acceptance.
- Run the §8 representative 2M-directory latency gate before enabling production v2 writing. Optional snapbench timings were not collected in this attempt.
- Privileged suites are compiled only; execution remains **pending VM**. Run an equipped cargo-deny advisory/license gate if the local CI wrapper reports that tool unavailable.

## What changed

`Snapshot` dispatches hot and inflated cold SQLite files by user_version (1 or 2), checks matching metadata schema_version, and rejects other versions. Its public methods and v1 SQL remain intact. A private optional backend in `reader.rs` delegates directory methods to `reader_v2.rs`; metadata remains the existing validated cached metadata.

The new backend locates chunks by indexed predecessor search and keeps an LRU of at most 32 decoded chunks with at most 8 MiB of retained row/name allocations. One active range chunk can outlive cache eviction; codec scratch remains codec-bounded (40 MiB decoded columns, 41 MiB compressed input, at most 8192 full rows). Top-N heaps hold at most min(requested limit, 1,000,001) candidates and reject results beyond the existing million-row ceiling. Returned row paths and full 64-bit hashes are reconstructed iteratively through validated parent chains. Reconstruction supports paths longer than PATH_MAX; explicit defensive bounds are 1,000,000 ancestors and 256 MiB of path bytes.

Shallow children and shallow metrics candidates stream plain SQL rows. Deep children hop sibling subtree ends and never decode intervening descendant chunks. Path lookup probes the configured 32/64-bit hash index and verifies candidate paths. Subtree sorts use bounded heaps, with projected metric columns rejecting inferior candidates before full-row/name decoding. Wide-directory thresholds use projected entries. Ordering retains v1's actual raw-name/path tie breaks, including prefix paths where preorder differs from bytewise full-path order.

The checked `v1_conn()` accessor rejects v2 before legacy queries. All non-directory public Snapshot queries call it, and the only changes in diff/export_ncdu/links replace connection access with that check. Diff's first accessor also checks the attached old snapshot, covering both mixed-version directions.

## Scope and dependencies

Only the seven assigned paths changed. No dependency, manifest, core type, schema, codec, writer or lib.rs changes. The private module declaration is exactly `#[path = "reader_v2.rs"] mod v2;` in reader.rs. No production v2 writer was enabled.

## Assumptions

- The task's explicit scope replaces a wave subsection; the full v2 contract, named handoffs, repository conventions, normative link semantics and playbook/handoff requirements were read.
- Directory identifiers and shallow duplicate rows follow the published writer's validated preorder contract. Chunks remain the source for parent-chain reconstruction and range scans.
- SQL path tie breaks are preserved literally rather than assuming preorder is equivalent for all byte names (for example, `/r/a-` sorts before `/r/a/x`).
- Missing parents produce empty child listings, matching v1. File, link, skip, error, annotation, diff and ncdu work stays with the named follow-up tasks.
- Fixed metadata timestamps supply all fixtures; no real-time assertions, sockets or privileged host operations were added. Performance clocks, if used separately, do not affect correctness assertions.

## Regression and parity evidence

Nine new integration tests compare serialized complete DirRows from both writers/readers, including reconstructed path and path_hash:

- `golden_and_deep_tree_all_query_variants`: test-support's normative golden topology and counters plus a depth-12 generated tree; plain_depth 0/2/4, hash_width 32/64, chunks 2/7; all SortKeys, both directions, reverse/cursor pages, raw-byte filters, limits 0/1/7/large, relative depths 0..14, metrics counts and both candidate families, wide thresholds, missing paths and missing-parent listings.
- `fifty_thousand_directories_small_chunks_and_wide_children`: 50,013 directories, chunks 2/5/7, all three plain-depth choices, both hash widths, 50,001 root children, cross-chunk reads, top-N sorts, pages, wide queries and metrics depths 0/2/4/12.
- `thousands_of_deep_children_and_paging_edges`: 3,000 children at depth 13, below every shallow cutoff; all sorts, forward/reverse cursors at both page edges and query-cap rejection.
- `cold_v2_and_hash_collision_verify_full_path`: both hash widths, deliberately colliding earlier candidate, valid cold compression/open with injected guard, directory/path/page parity.
- `corrupt_chunks_and_unsupported_versions_are_invalid`: malformed chunk and user_version 0/3 rejection.
- `legacy_queries_return_not_yet_implemented`: every deferred Snapshot query, diff in both mixed-version directions, ncdu and link-region construction.
- `sibling_hopping_and_path_lookup_skip_unvisited_chunks`: unrelated corrupt chunks prove the two queries do not scan unvisited chunks.
- `deep_paths_exceed_path_max_and_snapshot_remains_send`: 2,200 ancestors, path longer than 4096 bytes, exact reconstruction and compile-time Send preservation.
- `missing_directory_and_limit_errors_match_v1`: unknown IDs, missing subtree/wide roots and invalid limits retain v1 error text; missing internal ancestors remain Invalid.

Before: the baseline explicitly returned `Invalid("unsupported snapshot version 2")` at open, so none of these v2 directory queries was available. After: new tests exercise successful v2 opens and compare complete serialized rows with v1. No baseline red-test run is claimed; these tests were introduced with the implementation. Initial local Clippy failures were confined to two test expressions (a redundant min and a cloned reference slice) and were corrected.

## Validation

All Cargo commands used Rust 1.98.0 and the host cache (`RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, offline networking). Long commands were bounded by timeout 1500; the last CI retry used a stricter 600-second bound to preserve the submission checkpoint. Initial setup found cargo absent from PATH and the default rustup home unwritable; explicitly selecting the installed host toolchain resolved that setup issue.

| Check | Observed result | Evidence in output library |
|---|---|---|
| `cargo fmt --check` | Exit 0; final CI also checks formatting | final-format.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 on final nine-test source | post-parity-clippy.log |
| `cargo test --offline --workspace --no-fail-fast` | Exit 0; 1,095 passed / 0 failed / 14 ignored | final-workspace-tests.log |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; build only | final-privileged-build.log |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 | final-documentation.log |
| Default parallel `tools/ci.sh`, final source | Exit 1; 1,097 passed / 1 failed / 14 ignored; only fake-LVM timeout test failed | final-tools-ci.log |
| `RUST_TEST_THREADS=1 tools/ci.sh` | Exit 1; 1,097 passed / 1 failed / 14 ignored; only exclusive-lock child READY protocol failed | serial-ci.log |
| `RUST_TEST_THREADS=2 tools/ci.sh` | Exit 0; 1,098 passed / 0 failed / 14 ignored | two-thread-ci.log |
| Focused ncdu marker regression | 1 passed / 0 failed | ncdu-marker.log |
| Focused missing-ID/limit parity | 1 passed / 0 failed | error-parity.log |
| Isolated fake-LVM timeout regression | 1 passed / 0 failed | lvm-timeout-rerun.log |

Totals sum completed harness summaries, including nested subprocess harnesses; they are not counts of distinct test functions. The standalone workspace run compiled eight reader tests before the ninth error-parity regression was added. The focused ninth test passed, and the later complete CI invocations compiled and exercised all nine tests on the final source. All nine passed in both the default parallel and one-thread runs. The default run's reader harness took 123.27 seconds (debug build, overlapping validation); this is not a query-latency benchmark.

The source-policy scan reports 154 owner tags and no forbidden symbols. The CI wrapper's advisory/license check is subject to cargo-deny's availability; no new dependency was added. Existing privileged suites, including cold-file mount protection, were compiled only and remain pending VM. This pure SQLite/codec directory backend introduces no privileged behavior, so no new VM-only test was added.

## Failures investigated and before/after output

The required ncdu accessor switch exposed an existing source-extraction test: `w3c::notable_paging_uses_index_seek_without_sort` splits export_ncdu.rs at the literal old call `snap.conn().prepare(`. It initially panicked at w3c.rs:314 (`Option::unwrap()` on None). An adjacent compatibility comment retains that marker immediately before the unchanged SQL literal; the executable call uses `v1_conn()?`. The existing test remains untouched and now passes, including its index-seek/no-sort checks. This comment is part of the accessor-only switch; no SQL changed.

The default parallel CI run had one unrelated failure at the fake-LVM test's 100 ms timeout expectation. That test passed the standalone workspace run, an isolated retry, and the one-thread retry; its underlying cause is unestablished, and no LVM code was changed. The one-thread retry instead failed storage's child-process READY protocol. Its source expects a standalone `READY` line, while libtest's single-thread nocapture mode prefixes the current test name; the two-thread retry preserves normal parallel output formatting while reducing contention. Both failed transcripts are retained rather than suppressed. No existing test was modified, filtered out, or newly ignored.

## Scope and completion checkpoint

Only the seven assigned paths changed; no shared manifest/lib.rs edit, new dependency, branch creation, merge or push. Source hashes are recorded in library/source-sha256.txt. No production unwrap/expect, unsafe code, async runtime, real-time assertion or host configuration operation was added. Test timestamps are fixed metadata inputs.

The implementation and these local observations are an evidence candidate. Independent review, default-harness uid 1000/GitHub validation, equipped cargo-deny, privileged VM execution and the representative 2M-directory latency gate remain with the coordinator. Optional snapbench query timings were not collected. Commit, cleanup, final scope/status and submission receipt are recorded in the attempt output report/library.
