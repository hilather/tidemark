VERDICT: READY
BLOCKER 0 / SHOULD-FIX 0 / NOTE 0

Reviewed exact candidate `880026378bb0565f549967df1932e7a354047c69` against base `e520e83263ce4c02176923c583fe75f0853b2661`. The supplied checkout `01c8e9a22fcd8ff00fe5e7147ae3a0358e1356bb` has the identical tree, `8ddee44c147bca20879420ad8d95a49b1bb31f05`; repository tests ran there. Inspected the requested `e520e83..01c8e9a` diff, review17, review16, the fix6 handoff and relevant link/storage/v2 contracts. READY is this review's assessment of the scoped change, not verified task success, integration approval or a latency/release gate result. No production source or repository tests were edited.

**Review17 S1 is fixed.**

Re-ran all three fixtures with fresh readers at the original reader `7cde8e6`, the immediate base and the candidate. Full results agree with v1 and across revisions:

| Fixture and query | Original | Immediate base | Candidate |
|---|---:|---:|---:|
| Chain IDs 0–32, one directory per chunk; `wide_dirs(32,0,1)` | 33 | 34 | 33 |
| Chain IDs 0–33, one directory per chunk; `wide_dirs(33,0,1)` | 34 | 35 | 34 |
| Default 4,096-directory chunks, 32,769 directories; `wide_dirs(32768,0,1)` | 9 | 10 | 9 |

The default-size fixture uses plain depth 2 and the same depth-eight chain of final children `z`, each preceded by 4,095 leaf children. Its ancestor chunks exceed the 8 MiB LRU cap. The candidate no longer decodes the requested chunk again after ancestor validation evicts it. The new in-tree regression test also passed.

At `crates/tidemark-store/src/reader_v2.rs:500`, `raw_held` retains the already decoded `Arc`; the three range-query callers keep it across eager `path` validation. At `:1073`, `scan` reuses it only when both endpoint IDs match the scanned chunk. The initial `chunk` call still performs full codec validation; `decode_dirs` delegates to `visit_dirs`, and converting retained rows back to views preserves every field. Shallow-root agreement, root-name/ancestry validation, interval/neighbour checks, decoded row count and final range coverage remain in place. No new decoding call, unchecked arithmetic path or unbounded collection was introduced. The retained chunk remains subject to existing codec limits, though its lifetime now spans the scan.

**Results and decode work.**

The original review17 matrix ran 10,042 fresh queries per revision: 9,280 balanced-tree cases, 756 deep-chain cases and six default-chunk cases. Coverage includes plain depths 0–4, nine root/internal/leaf IDs, all eight sorts, relative depths 0/1/2/4/`u32::MAX`, limits 0/1/3/1000, and wide thresholds 0/1/2. Deep chains cover depths 30/31/32/33/34/64/128 and chunk sizes 1/2/4. All 30,126 full-result comparisons with v1 passed; result digests agree across revisions. Metrics checks both rankings, eligible count and each snapshot's own filename.

There are no candidate work increases over the immediate base and 150 reductions. No nonempty fresh-query regression against `7cde8e6` was observed in these matrices outside the accepted decisions. The balanced matrix retains 962 zero-limit increases over the original, bounded by the corresponding candidate limit-1 query, and 45 filtered-empty wide increases for requested-root validation. Those accepted decisions are not findings. Repeated-query eviction and the accepted `children_page` narrowing are not reopened.

An additional 120 fresh queries per revision cover deep subtree queries after LRU eviction and a 4,096-row chunk whose decoded rows/names exceed 8 MiB, so it cannot enter the LRU. The latter has four eligible rows at depth 1 and long-name grandchildren, allowing bounded original-reader comparisons. All 360 full-result comparisons passed, with no increase over the immediate base and 102 reductions. Its 36 increases over original zero-limit work fit the same candidate query at limit 1. Representative decode counts:

| Supplemental query | Original | Immediate base | Candidate |
|---|---:|---:|---:|
| Chain to ID 33; `subtree_dirs(33,0,Name,1)` | 34 | 35 | 34 |
| Uncacheable root chunk; `wide_dirs(0,2,1)` | 5 | 2 | 1 |
| Uncacheable root chunk; `subtree_dirs(0,1,Name,1)` | 2 | 2 | 1 |
| Uncacheable root chunk; `subtree_dirs(0,1,Disk,1)` | 3 | 2 | 1 |
| Uncacheable root chunk; `metrics_candidates(1,1)` | 10 | 2 | 1 |

**Corruption and retained validation.**

Re-ran review17's 10,130 corruption calls per revision on separate copied snapshots, opening a fresh reader for every call. The candidate and immediate base have identical statuses and result counts throughout: zero lost or gained rejections. The matrix covers requested/ancestor names, parent/depth/span changes, malformed ancestor payloads, shallow omissions/types/hierarchy, root-copy disagreements and chunk envelopes across plain depths 0–4. Chunk mutations use the real codec, with both chunk-only and synchronized shallow-copy variants.

All 230 explicit earlier-regression rejection assertions passed, including malformed requested/root names and ancestry at limits 0/1 and filtered-empty wide queries, shallow ID 41 omission/type/depth cases, and chunk 32 payload corruption. The unchanged empty-children behavior recorded in review17 remains unchanged; it is not a new finding.

A targeted additional corruption fixture uses chain IDs 0–65, two directories per chunk and requested root 64. Climbing the 32 ancestor chunks evicts chunk `[64,65]`; only descendant 65 has its name changed to `a/b` or depth changed to 64. The candidate rejects all 12 wide/subtree calls, including zero limits and filtered-empty wide queries, exactly as the immediate base does. This exercises descendant validation while scanning the retained chunk after actual eviction. No wrong result, dropped rejection or panic was observed in these checks; they are not an exhaustive malformed-input proof.

**Validation and limits.**

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --example snapbench --message-format=json`: exit 0; library 58 passed/1 ignored, reader integration 42/1, diff walk 10/2, example 2/0. Total 112 passed, zero failed, four ignored scale tests. Only the example's small tests ran; the 2M benchmark was not run.
- Five main scratch tests and three supplemental tests passed per revision. The comparison scripts separately assert result equality, work bounds and retained rejections; corruption observation tests alone do not establish correctness.
- Scratch harnesses extract each revision's `reader_v2.rs`, `reader.rs`, `chunk.rs` and crate entry point, reference unchanged remaining checkout modules, and add tests plus a thread-local counter at `Columns::open` for directory chunks. Counts include ordinary, projection and streaming decompression. No production behavior is substituted. Harness sources, compile commands/logs, matrix logs, `matrix-summary.log`, `supplemental-summary.log` and `focused-tests.log` are retained in this attempt's output `library/`.
- Used pinned Rust 1.98.0 and the existing offline Cargo cache. The initial shell could not find Cargo; the retained error log precedes the successful run with the toolchain directory explicitly added to PATH. No privileged commands, VM/release tests, full workspace CI, fmt/clippy, ignored scale runs or elapsed-latency/RSS/file-size benchmark were run. Decode counts do not establish the latency gate or release readiness.

`cargo clean --offline` completed (2,036 files / 1.0 GiB removed); all six scratch executables were removed. `git diff --check` is clean. The only repository change is this report. No branches, merges or pushes were created.
