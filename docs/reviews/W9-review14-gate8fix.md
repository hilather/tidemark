VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 1 / NOTE 0

Reviewed exact candidate `2b9c6e33758c97a0d90becbe5474651440032b5b` against its immediate base `25eb2f515732558b0d07fe7c38a9e9a6a785ad6e`. The supplied checkout `4fa9f0d32525dbfc3738633b405a409d95830d74` has the identical tree (`a6fba29f83e517067c05c6c5e2b50e2adcb022e1`); repository checks ran there. Inspected the requested `9d5f0af..4fa9f0d` diff/log, review13, the fix2 handoff, AGENTS, and the normative link/storage/v2 contracts. Work comparisons also use review13's original pre-rewrite baseline `7cde8e61371ca46677fa685980dcedf4685cdfbc`. This report is evidence, not integration or release approval.

**S1 — SHOULD-FIX: the fresh-query work bound remains false for empty results (residual review13 S2).**

Reference: `finding:w9-review14-gate8fix-s1`. Locations: `crates/tidemark-store/src/reader_v2.rs:794`, `:1099`, `:1127`; regression coverage at `:3094`.

On the existing balanced 121-directory fixture, `plain_depth=0`, 16 directories per chunk, with a fresh `Snapshot` for each query:

| Query | Original baseline `7cde8e6` | Immediate base `25eb2f5` | Candidate `2b9c6e3` | Result in all three |
|---|---:|---:|---:|---|
| `children(40,Name,1)` | 1 decode | 3 | 3 | Empty list |
| `subtree_dirs(0,4,Disk,0)` | 1 decode | 9 | 8 | Empty list |
| `subtree_dirs(40,0,Name,0)` | 1 decode | 3 | 3 | Empty list |

ID 40 is a depth-four leaf in chunk 32–47. `children` reconstructs its path before discovering that there are no children, decoding the ancestor chunks 16–31 and 0–15. The old visitor needed only the leaf's chunk. Limits 0 and 1000 reproduce the same child-listing increase; `plain_depth=3` also reproduces it. The nonzero-limit empty listing is an ordinary supported query.

For the root's zero-limit Disk query, the old projected implementation returned after the root lookup; the candidate still scans the whole range. For the zero-limit leaf subtree, eager path construction adds the same unnecessary ancestor decodes. Two scratch regression assertions (empty children and zero-limit root subtree) pass against `7cde8e6` and fail against both the immediate base and candidate with the counts above. This is a remaining earlier-rewrite regression, not a newly introduced increase relative to `25eb2f5`.

The requested exception covers repeated queries missing chunks from the LRU. These are fresh-query measurements, so that exception does not cover them. The new work regression only checks full, nonempty root queries and does not establish the broader fresh-query bound. Avoid unnecessary path/chunk work for empty results and cover these cases while preserving required sibling validation. No wrong returned data or elapsed-latency gate failure is claimed. The verdict is blocked on this explicit remaining review requirement.

**Recheck of the four original findings.**

- **B1 fixed:** the requested shallow root's name must match the chunk byte-for-byte and satisfy the root-path/component rule. The committed regression passes on the candidate and fails on the immediate base. Scratch checks cover 105 mutations across configured plain depths 0–4 and every shallow depth: empty, slash, slash-containing, dot, dot-dot, NUL, and valid-but-different names. Requested-root subtree queries reject each mutation, including queries extending beyond the shallow range. Metrics rejects the corrupt root and malformed visited shallow descendants. `wide_dirs` and children queries retain their chunk-derived parent paths; tampering only the parent's shallow name does not contaminate their results. A separate 420-case check confirms the internal `children_by_name` results/cursors are unchanged.
- **S1 fixed:** `children` now fully decodes every shallow sibling before selection. The committed regression fails on the immediate base and passes on the candidate. Scratch checks reject text in each of 27 numeric/payload columns of the last sibling for all eight sorts, plain depths 1–4, and limits 0 through 8, 100, 1,000,000 and 1,000,001: 10,368 calls. `children_count` also rejects these mutations. `usize::MAX` retains its argument error. The accepted `children_page` narrowing is unchanged and is not raised again.
- **S2's original reproductions fixed, broader guarantee incomplete:** fresh `subtree_dirs(0,4,Name,1000)` decodes 8 chunks versus 9 immediately before this fix; fresh `metrics_candidates(4,1000)` decodes 8 versus 17. The original baseline needs 8 for both. Both queries return all 121 rows, and metrics counts 121. The committed new work test fails on immediate-base production code (9 decodes), and separate scratch assertions demonstrate the metrics difference too. Repeated candidate queries decode 7 chunks each because streaming does not populate the cache; this is the accepted behavior, not a finding. The remaining fresh-query cases are S1 above.
- **N1 fixed:** `children` validates the limit before a missing-parent return. The committed test confirms the same `Integer` error/text as v1 for `usize::MAX`, and an empty list for a normal limit, at plain depths 0 and 3. It fails on the immediate base.

**Valid-input behavior and architecture.**

No changed valid-input result was established. In addition to committed differential coverage, scratch tests compare every returned `DirRow` field with v1 across 19,600 children/subtree query pairs (all sorts, plain depths 0–4, root/nonroot/leaf IDs, depth and limit boundaries), plus wide and metrics queries. Both metrics rankings and the count agree, including limit 0. Existing tests cover raw-byte/prefix ties, cursor directions, filters, malformed chunks, structural shallow checks, and diff behavior.

`ChunkDir::view` preserves all fields. Cached chunks have already passed the full decoder; scan still checks chunk coordinates, neighbours, coverage and decoded row counts. The simultaneous Disk/Reclaimable heaps preserve their independent keys and path ties. No new defect was found in these changed paths under the immutable-snapshot contract.

**Validation and limits.**

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --example snapbench`: exit 0. Library 54 passed/1 ignored; reader integration 42/1; diff walk 10/2; example 2/0. Total 108 passed, zero failed, four ignored scale tests. The example tests use their small fixture, not the 2M benchmark.
- Three scratch public-API matrix tests and the internal child-name test passed. The work matrix covers 256 fresh-query configurations across three revisions, with repeated subtree/metrics calls. Original-code projection decompressions are included through test-only counter instrumentation; the finding's three scenarios perform no original projection decode and do not depend on that instrumentation.
- Scratch harnesses extract only the selected revision's `reader_v2.rs` and `chunk.rs`, reference the other unchanged repository modules, and add tests/counters outside the repository. No production behavior was substituted and no repository code or tests were edited. The initial public harness incorrectly called a crate-private method; that check was moved to the unit harness before the successful run.
- Used pinned Rust 1.98.0 and the existing offline dependency cache. No privileged commands or 2M benchmark were run. Full workspace CI, clippy/fmt, release/VM tests, ignored scale tests, and the full latency/file-size/RSS gate were not run. Work counts do not establish elapsed latency or release readiness.
- Evidence is retained in this attempt's output `library/`: `focused-tests.log`, `public-tests.log`, `internal-children.log`, `{original,review13,candidate}-work.log`, `{original,review13,candidate}-unresolved.log`, `old-regressions.log`, `committed-work-old.log`, scratch source/entry points and `run_review14.py`.

`cargo clean --offline` completed (2,036 files / 1.0 GiB removed); scratch binaries/rlibs were removed too. `git diff --check` is clean. The only repository change is this review report; no branches, merges or pushes were created.
