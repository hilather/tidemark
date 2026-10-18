VERDICT: BLOCKED
BLOCKER 1 / SHOULD-FIX 2 / NOTE 1

Reviewed exact candidate `d6369a7f4ff61940ef9ce6ffde9b84583d31cf25` against base `7cde8e61371ca46677fa685980dcedf4685cdfbc`. The supplied integration checkout `9d5f0af9a7c2d6c0e4f11e25a6b2020cb9926efb` has the identical tree, `2c31860608e62f6b3592123e49d5a5699c7896b6`; checks ran there. Also inspected the requested `cbd9422..9d5f0af` diff/log, distinguishing its unrelated historical TUI changes from the six-file reader rewrite. All findings below apply to the exact candidate. This report is review evidence, not integration or release approval.

Read `AGENTS.md`, reviews 8–12, `docs/handoff/SNAP-gate8-fix.md`, the supplied `scratch/wip/SNAP-gate8-fix.FIX-PLAN.md` including §§2 and 4, normative link semantics, and the v1/v2 storage contracts. No production source or repository tests were edited.

**B1 — BLOCKER: a requested shallow subtree root can return an invalid name.**

Reference: `finding:w9-review13-gate8-b1`. Locations: `crates/tidemark-store/src/reader_v2.rs:328`, `:1104`, `:1235`.

`Walk::step` validates descendant names, but its first-row arm does nothing. The path supplied to that first row comes from the authoritative chunk, while the returned row comes from `shallow_dirs`. `authoritative_root` checks agreement only on ID, parent, depth and span; it does not validate or compare the shallow name.

Reproduced on a valid tree with `plain_depth=3`, leaving every chunk intact: change only shallow ID 1's name to `x'612f62'` (`a/b`), then call `subtree_dirs(1,0,Name,100)`. The candidate succeeds with `name=a/b` and the original chunk-derived path. Empty, `/`, `.`, `..` and embedded-NUL variants also succeed. Changing only shallow ID 0's name to `x'780079'` makes both `subtree_dirs(0,0,Name,100)` and both lists from `metrics_candidates(0,100)` return the invalid NUL-containing root name successfully. These are returned malformed rows, outside the declared page-validation narrowing.

The same nonroot `a/b` reproduction against base production code returns `StoreError::Invalid`. Its materialization started from the shallow row and applied `RawName::new`. Validate the first returned row's name with the appropriate root-path/component rule, and reconcile it with the path used to construct the result. The new invalid-name tests mutate descendants and do not cover a requested root whose shallow name alone is corrupt.

**S1 — SHOULD-FIX: `children()` also loses validation of unreturned siblings.**

Reference: `finding:w9-review13-gate8-s1`. Locations: `crates/tidemark-store/src/reader_v2.rs:791`, `:800`; `docs/handoff/SNAP-gate8-fix.md:26`.

For a shallow parent with two children, set only the later child's `own_charged='x'`. `children(0,Name,1)` now succeeds with the first child. `children(0,Name,2)` and `children_count(0,b"")` return `Invalid`. The same limit-1 query against base production code returns `Invalid`, because the old visitor decodes every sibling before selection.

The structural pass reads only ID, span and depth; SQL `LIMIT` now excludes the malformed row before full decoding. This is a second validation narrowing, contrary to the handoff's explicit statement that `children` still decodes every sibling and only `children_page` narrows validation. Restore the promised `children` behavior, or explicitly reconcile this additional behavior change with the stated review requirement. No wrong returned metric was observed in this reproduction; the defect is the lost rejection guarantee and inaccurate handoff.

**S2 — SHOULD-FIX: cached queries decode more chunks than before.**

Reference: `finding:w9-review13-gate8-s2`. Locations: `crates/tidemark-store/src/reader_v2.rs:1054`, `:1058`, `:1249`; work-count test at `:3063`.

`scan` always reads and decompresses each chunk, bypassing the existing LRU. The root has already been decoded by `raw`; deep metrics runs the scan twice. Measured with the existing balanced 121-directory fixture, `plain_depth=0`, 16 directories per chunk, and a limit of 1000. All eight chunks fit in the existing cache. Counts include both ordinary decodes and streamed decodes, and repeat counts are incremental on the same `Snapshot`:

| Query | Base fresh | Candidate fresh | Base repeat | Candidate repeat |
|---|---:|---:|---:|---:|
| `subtree_dirs(0,4,Name,1000)` | 8 | 9 | 0 | 8 |
| `metrics_candidates(4,1000)` | 8 | 17 | 0 | 16 |

Both versions return the expected 121 rows/count. This follows the plan's no-cache scan design, but disproves the requested “no query decodes more than before” bound. The committed test checks `scanned` and `decodes` separately, allowing the duplicated root decode and never comparing repeated queries. Reuse already decoded chunks where appropriate, or reconcile this explicit work-bound exception. This is a measured decode-work regression, not a claim that elapsed latency or the §8 gate regressed; neither was benchmarked here.

**N1 — NOTE: a missing parent now bypasses oversized-limit validation.**

Reference: `finding:w9-review13-gate8-n1`. Location: `crates/tidemark-store/src/reader_v2.rs:784`.

On this 64-bit build and a valid small snapshot, `children(100,Name,usize::MAX)` returns `Ok([])` in v2 for nonexistent parent 100. V1 returns `StoreError::Integer("18446744073709551615")`. The base v2 implementation also calls `sql_limit` before visiting children, whereas the candidate returns before either limit-validation branch. Validate the limit before the missing-parent early return to preserve this API behavior. This is an invalid-argument edge, not a failure for normal supported limits.

**Assessment of the declared narrowing and other review questions.**

The declared `children_page` narrowing is acceptable as a bounded page-query policy: every sibling still participates in the structural child-set check, returned rows are fully decoded and checked by `child_path`, and `children_count` still decodes every sibling. It aligns this page's payload validation with v1, without making the page a whole-snapshot integrity check. The scratch two-child reproduction confirms the declared page behavior and the retained count rejection. This judgment does not extend to B1 or conceal the additional `children` change in S1.

For valid inputs, inspection and the differential tests support the existing row sets, metric/name/ID ordering, raw-path metrics ties, reverse paging, cursors and substring filters. `TopN` transitions at its cap, compares borrowed keys consistently with final sorting, and retains the `MAX_QUERY_ROWS+1` sentinel before checking the result bound. Limits 0, 1, intermediate values, 1,000,000 and 1,000,001 are covered on small fixtures. A query returning more than one million directory rows was not executed; those result-bound branches were inspected. No valid-input result regression within the requested limit matrix was established.

Apart from B1 and S1, no further loss of the examined corruption checks was established. Streaming preserves chunk-envelope validation, count/span agreement, first/last-neighbour checks and gap/overlap rejection. The codec retains checked varints, all-column validation and trailing-byte checks. `Walk` retains preorder completeness, parent/depth/span checks and adds descendant component checks; base paths and descendant growth retain `MAX_PATH_BYTES` checks. The existing root-bound, shallow-set, malformed-column, chunk-tampering and walk regressions are included in the focused run. This is not an exhaustive malformed-input or panic proof, and the maximum path-byte allocation was not stress-tested.

The shallow work tests retain at most one root/ancestor chunk decode and zero streamed chunks for the exercised subtree, metrics and 5,000-child page queries. Narrow subtree scans exclude unrelated deep payloads. The broader no-extra-decoding claim is limited by S2. Accepted review9 mixed-format wide-directory cost and ncdu buffering decisions were not reopened.

**Validation and limits.**

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --example snapbench`: exit 0. Library: 53 passed/1 ignored; reader integration: 39/1; diff walk: 10/2; snapbench example: 2/0. Total 104 passed, zero failed, four ignored scale tests. The example tests exercise both layouts on their small fixture; they do not run the 2M benchmark.
- Three scratch public-API observation tests against the unmodified candidate library passed. Two scratch unit tests passed on each of candidate and base, confirming B1/S1's changed behavior and S2's decode counts. These tests assert the observed defects; their passing status is not acceptance evidence for those behaviors. Scratch entry points reference unchanged repository modules, extracting only `reader_v2.rs` and `chunk.rs` at the selected revision and adding test-only code. No production logic was substituted. An initial base harness compile lacked its local mutation helper; that harness error was corrected before the successful runs.
- Used pinned Rust 1.98.0, the existing offline dependency cache at `CARGO_HOME=/home/brewerm/.cargo`, and a target directory confined to this attempt's output. `cargo clean --offline`: exit 0, removed 2,036 files/1.0 GiB. Scratch executables were also removed. `git diff --check` is clean; the only repository change is this report.
- Evidence is retained under this attempt's output `library/`: `focused-tests.log`, `review13_additions.rs`, `review13-repro.log`, `review13_work_additions.rs`, `candidate-work.log`, `base-work.log`, `base-work-initial-compile.log`, the extracted source/entry points, `run_review.py`, `run_work_comparison.py`, and `clean.log`.
- Full workspace CI, clippy/fmt, release-profile tests, privileged/VM tests, ignored scale tests and the full §8 latency/file-size/RSS matrix were not run. The candidate's performance table remains TBD; no latency-gate pass or miss is claimed. No privileged commands, branch creation, merges or pushes were performed.
