VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 2 / NOTE 0

Reviewed exact candidate `da5067463a6a34fb7498b9b675ad6b63714a4ff3` against base `239a0724b8df5f636cf8058e1d267c2e9db9ab25`. The supplied checkout `a77a49c65b112edf825118b0512ae64c3137336e` has the identical tree, `01bba75f9b9450e2f506d482c82d58987ac5af10`; repository checks ran there. Inspected the requested `239a072..a77a49c` diff, AGENTS, reviews 8–14, the fix3 handoff and the relevant link/storage/v2 contracts. This report is evidence, not integration or release approval. No production source or repository tests were edited.

**S1 — SHOULD-FIX: filtered empty wide-directory queries still exceed the original fresh-query work bound.**

Reference: `finding:w9-review15-gate8fix2-s1`. Location: `crates/tidemark-store/src/reader_v2.rs:1168` (path construction before filtering at `:1172`).

On the existing balanced 121-directory fixture, with 16 directories per chunk, every row has `entries=1`. A fresh `wide_dirs(40,2,1)` returns an empty list in every revision, but still performs three chunk decodes versus two in the pre-rewrite reader. The root lookup decodes chunk 32–47; eager reconstruction of leaf 40's path adds ancestor chunks 16–31 and 0–15. The old reader decoded the leaf chunk and its entries projection, found no qualifying row, and never reconstructed a path. Leaves 80 and 120 need four decodes in the candidate versus two originally.

| Fresh query, `plain_depth=0` | Original `7cde8e6` | `25eb2f5` | `2b9c6e3` | Candidate `da50674` |
|---|---:|---:|---:|---:|
| `wide_dirs(40,2,1)` | 2 | 4 | 3 | 3 |
| `wide_dirs(120,2,1)` | 2 | 5 | 4 | 4 |
| `children(40,Name,1)` | 1 | 3 | 3 | 1 |
| `subtree_dirs(0,4,Disk,0)` | 1 | 9 | 8 | 1 |
| `subtree_dirs(40,0,Name,0)` | 1 | 4 | 3 | 1 |

The wide-query increase reproduces at every plain depth 0–4 and limits 1, 3 and 1000: 45 cases in the matrix. This is a residual earlier-rewrite regression, not a newly introduced increase over `239a072`. Review14's listed examples are fixed, but its broader fresh-query requirement remains unmet. Defer path work for filtered-out results while retaining the required validation. These are fresh readers; the accepted repeated-query/LRU exception does not apply. No elapsed-latency regression is claimed.

**S2 — SHOULD-FIX: zero-limit subtree/wide returns bypass previously required corruption checks.**

Reference: `finding:w9-review15-gate8fix2-s2`. Locations: `crates/tidemark-store/src/reader_v2.rs:1105` and `:1165`.

Using separate copies of the same fixture, retaining intact chunks for shallow-table mutations, each scenario below returns `StoreError::Invalid` in all three comparison revisions and `Ok([])` in the candidate:

| Mutation | Query |
|---|---|
| `plain_depth=2`; `DELETE FROM shallow_dirs WHERE id=41` | `subtree_dirs(0,1,Name,0)` |
| `plain_depth=2`; `UPDATE shallow_dirs SET own_charged='x' WHERE id=41` | `subtree_dirs(0,1,Name,0)` |
| `plain_depth=2`; `UPDATE shallow_dirs SET depth=2 WHERE id=41` | `subtree_dirs(0,1,Name,0)` |
| `plain_depth=0`; `UPDATE dir_chunks SET data=x'00' WHERE first_id=32` | `wide_dirs(0,0,0)` and `subtree_dirs(0,4,Name,0)` |

The new return precedes shallow enumeration and `Walk::finish`, dropping the completeness/hierarchy checks established after review10 B1, as well as malformed-row rejection. It also precedes scanning later chunks. At limit 1 the candidate still rejects these scenarios. The handoff explicitly assumes validation may be skipped because nothing is returned, but the task asks whether earlier validation was dropped and explicitly accepts only the specified `children_page` validation narrowing. This is an additional narrowing. In particular, preserving shallow-row validation requires no extra chunk decompression, so the shallow cases do not conflict with the fresh-query work bound. Restore the rejection behavior without rebuilding unnecessary result paths.

**Valid-input and retained-validation checks.**

The scratch matrix ran 9,280 fresh queries per revision: `7cde8e61371ca46677fa685980dcedf4685cdfbc`, `25eb2f515732558b0d07fe7c38a9e9a6a785ad6e`, `2b9c6e33758c97a0d90becbe5474651440032b5b`, and the exact candidate. Coverage is plain depths 0–4; nine root/internal/leaf IDs; all eight sorts; relative depths 0/1/2/4/`u32::MAX`; limits 0/1/3/1000; and wide thresholds 0/1/2. Every complete `DirRow` vector is asserted equal to v1. Metrics compares both rankings and eligible count, checking each source filename separately. All 37,120 comparisons passed; result digests also agree across revisions. The candidate has no work increase in this matrix outside S1's 45 wide queries.

`metrics_candidates(4,0)` still returns empty rankings and count 121, with eight decodes at plain depth 0, matching the original reader. Full root subtree and metrics queries also use eight decodes, matching the original. Empty leaf children at limits 0/1/3/1000 and zero-limit subtrees meet the work bound throughout the matrix. Additional missing-parent checks preserve the normal empty listing and the oversized-limit `Integer` error.

The corruption matrix records 300 calls per revision across six mutations, plain depths 0–4, limits 0/1 and children/subtree/wide/metrics APIs. Apart from S2, the inspected changed paths retain review13/14's requested-root name checks, authoritative root agreement, complete shallow-sibling decoding and limit validation. For example, candidate `children(0,Name,0)` still rejects missing shallow siblings and malformed `own_charged`, and a corrupted shallow root name is still rejected by subtree/metrics queries. Lazy child-path construction invokes cache access after the sibling lookup has released its borrow; no reentrant `RefCell` borrow defect was found. The accepted page-validation and repeated-query cache behavior are not findings.

**Validation and limitations.**

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --example snapbench --message-format=json`: exit 0. Library 55 passed/1 ignored; reader integration 42/1; diff walk 10/2; example 2/0. Total 109 passed, zero failed, four ignored scale tests. The example tests use their small fixture, not the 2M benchmark.
- Two scratch tests per revision passed: valid-result/work assertions and corruption observations. Successful observation tests document the bad outcomes above; they are not acceptance tests for those behaviors. The scratch harness extracts each revision's unchanged `reader_v2.rs` and `chunk.rs`, references the other unchanged checkout modules, and adds tests plus counters outside the repository. Counts include ordinary decodes, streaming decodes and original projection decodes. Production behavior was not replaced.
- An initial scratch assertion incorrectly compared the two fixtures' source filenames; it was corrected to validate each filename against its own snapshot before the successful four-revision run. The initial failure log is retained. Cargo initially needed explicit toolchain/cache locations; successful builds use pinned Rust 1.98.0 and offline dependencies.
- Evidence is in this attempt's output `library/`: `focused-tests.log`, `{original,review13,review14,candidate}-matrix.log`, `matrix-summary.log`, `remaining-work-regressions.json`, `assert-findings.log`, `assert_findings.py`, `additions.rs`, `run_review15.py`, `compare_matrix.py`, extracted source/entry points and compile logs.
- No 2M benchmark, privileged commands, VM tests, ignored scale tests, full workspace CI, fmt/clippy or release-profile tests were run. Decode work does not establish elapsed latency, RSS, file-size gates or release readiness. No branch creation, merges or pushes were performed.

`cargo clean --offline` completed (2,036 files / 1.0 GiB removed); scratch executables were removed too. `git diff --check` is clean. The only repository change is this report.
