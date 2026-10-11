**CHANGES REQUESTED** — 1 BLOCKER, 0 SHOULD-FIX, 0 NIT.

Reviewed candidate `75d6cbfaa536f9d1ea7a12056ff5bd8e5db21c46` against `f2e6686eeab5cdce7e7ca962e05137cf5961492e`, limited to the candidate diff. Required builds ran in the assigned checkout at `d50424c096c421505ef1a632b17cc46d8bd12f9b`. The W3-A implementation, reader, renderer, tests and finish handoff are unchanged between candidate and checkout; other tasks have advanced. This report is review evidence, not an integration or release decision.

**BLOCKER — Bound the explain query's SQLite working memory.** `crates/tidemark-store/src/diff.rs:342` (outer limit at line 365). Reference: AGENTS.md rule 10; `05-storage.md` §5.2; the review requirement for bounded memory and no full-table loads. Finding reference: `finding:w3-a-explain-window-memory`.

The new `SUM(...) OVER ()` computes the contributor count over the entire child union before returning its first row. SQLite retains those child records in a temporary table, and `reader.rs:178` sets `temp_store=MEMORY`. Removing the previous child cap therefore makes this query's heap grow with directory width even when the caller requests only five contributors. The 1 GiB mapping settings bound mapped snapshot pages, not this separate heap allocation.

An independent probe linked the workspace's actual bundled SQLite 3.51.1 library and executed the candidate child query with indexed, 240-byte BLOB paths, one unit of growth per child, and `LIMIT 5`. Additional SQLite heap was 2,939,008 bytes for 10,000 children, 27,632,696 for 100,000, and 137,303,384 for 500,000. Replacing only the window expression with a constant kept the limited query at 20,152 bytes for all three sizes. These measurements exclude the preloaded database and mmap/RSS effects. A wide target with evenly distributed growth can thus consume substantial unbounded scratch memory before returning five rows; the existing million-directory test exercises `diff`, not this explain path.

Replace the window aggregate with a separate streaming aggregate count and a limited contributor query, or stream the child join through a bounded top-K heap while counting. Preserve exact counts, raw-byte ties and shrink symmetry. Add a wide-directory regression showing that fixed `top` does not retain all children. Reproduction source, commands and measurements: `library/store-review/` in the attempt output.

Checked and found correct within the changed implementation:

- Public signatures, field lists and dependencies remain unchanged. The finish changes use the accepted R2-4 reader accessors and retain R2-6 missing-path behavior.
- SQL uses primary-key subtree ranges, `dirs_parent` for child queries, and composite `dirs_path(path_hash,path)` lookups, confirmed with `EXPLAIN QUERY PLAN`. Runtime values are bound; interpolated SQL fragments are closed internal choices.
- Existing tests pass for presence classes, target-relative depths, both missing-target directions, metrics, reordered preorder IDs, hash collisions and raw-byte paths. Independent public-API probes also passed signed deltas `±i64::MAX`, absolute-delta ordering and raw-byte ties.
- Dominant descent, ratio/minimum stops, equal children, spread growth, own-file growth, symmetric shrinkage, zero parent delta and zero requested contributors are covered by passing tests/probes. Traversal is iterative.
- Immutable/read-only opens, encoded ATTACH paths, detachment and mmap restoration on success/error/unwind are implemented and tested. No production unsafe code, scanned-file content reads, filesystem mutations, new scan syscalls or unguarded unwrap/expect were introduced.
- CLI goldens and executable tests pass for escaped text, `path_b64`, the `tidemark.diff.v1` envelope, age notices, latest-complete selection, missing IDs and scan/mutate/explain. An additional CLI probe confirms different-root IDs fail clearly with exit 5.

One verified preexisting issue is excluded from the candidate-diff finding count: explicit partial snapshots as either `--from` or `--to` succeed silently (exit 0, empty stderr, JSON notice null) at `crates/tidemark-cli/src/cmd/diff.rs:74`. Both the logic and renderer are unchanged from the review base. The independent probe also confirms that a failed catalog entry without a snapshot file is refused, and default `--to` skips a newer partial snapshot. See `library/cli-selection-probe.py` and `.log`.

All Cargo commands used Rust 1.98.0, a private copy of the host cache at `/tmp/tidemark-review-cargo`, and offline mode.

| Validation | Observed result |
|---|---|
| `CARGO_NET_OFFLINE=true tools/ci.sh` | Exit 1. Source-policy checks, formatting and Clippy passed; workspace tests stopped at two unrelated W2-C cancellation tests. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0. |
| `cargo test --offline -p tidemark-store -p tidemark-cli --lib --bins --test w3a` | Exit 0: 79 passed, 0 failed, 1 ignored scale test. |
| Optimized million-directory depth-three diff | Exit 0: 4.651976131 seconds for the diff, 21.12 seconds for the fixture/test; strict five-second assertion passed. Command: `cargo test --offline --release -p tidemark-store --test w3a million_directory_depth_three -- --ignored --nocapture`. |
| Independent integer, SQL-memory/query-plan and CLI-selection probes | Completed; observations above. |

The unrelated CI failures are `sigint_records_failed_and_leaves_no_snapshot` (`crates/tidemark-cli/tests/w2c/main.rs:572`) and `sigint_keep_partial_publishes_partial_and_is_not_latest` (`:588`): both observed catalog status `running` instead of `failed`/`partial`. They are outside W3-A and are not findings against this candidate. CI did not reach cargo-deny or finish the full workspace test suite.

No privileged tests, VM access, mounts, LVM operations or service changes were executed. The feature-gated XFS end-to-end test compiled and remains pending VM. SQLite heap measurements establish the query behavior, not full CLI RSS or production-disk performance. No earlier handoff timing or validation claim was treated as independent evidence.

Logs, probe sources and scope comparison are under `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-92cb590165098ca809ef8fd6946a31c0cf9d89f3db6a318bfe2c74decafc87f1/library/`.
