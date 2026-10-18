VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 1 / NOTE 0

Reviewed exact candidate `c92413c37156160d1be6f6c12c8f6421eea2239e` against base `5feba30ba600bd9f5b4da42b565b815d9e6bf9df`. The supplied checkout `402a91303db11569f0cf2e63c26d097ff4a20c14` has the identical tree, `ec09067389de73fa46e10a067d6125ad7b75e267`; repository tests ran there. Inspected the requested `5feba30..402a913` diff, review15, the fix4 handoff, reviews 8–14, AGENTS and the relevant link/storage/v2 contracts. This report is evidence, not integration or release approval. No production source or repository tests were edited.

**S1 — SHOULD-FIX: zero-limit wide queries still skip requested-root validation.**

Reference: `finding:w9-review16-gate8fix3-s1`. Location: `crates/tidemark-store/src/reader_v2.rs:1173`–`:1175`; the first-row exemption in `Walk::step` is at `:328`.

`wide_dirs` now calls `self.path(snap, &root)` only when `top.admits` succeeds. Admission is always false at limit 0. The relative walk validates descendants, but its first row does not validate the requested root's name or ancestry. Consequently the requirement that S2 reject at limit 0 exactly as at limit 1 remains unmet.

Reproduced on separate copies of the balanced 121-directory fixture, 16 directories per chunk, with fresh readers and correctly re-encoded chunks. Only the indicated field changes; chunk envelopes and other rows remain intact. Each case reproduces at every plain depth 0–4:

| Mutation | Query at limit 0 | Same query at limit 1 |
|---|---|---|
| ID 40 name becomes `a/b` | `wide_dirs(40,0,0)` succeeds with an empty list | `Invalid` |
| ID 0 name becomes bytes `/r\0bad` | `wide_dirs(0,0,0)` succeeds with an empty list | `Invalid` |
| ID 40 parent becomes 0, retaining depth 4 | `wide_dirs(40,0,0)` succeeds with an empty list | `Invalid` |

Empty/dot/dot-dot/slash/NUL component variants and corrupt ancestor name, depth and payload cases also reproduce the mismatch. These use short paths; the handoff's exception for the full length of non-admitted paths does not cover these malformed names or relationships.

There is also a newly weakened nonzero-limit case: all fixture rows have `entries=1`, so `wide_dirs(40,2,1)` filters everything out. With ID 40's chunk name changed to `a/b`, immediate base `5feba30` returns `Invalid`; this candidate returns an empty success. The original pre-rewrite reader `7cde8e6` also accepted this filtered case and the limit-0 cases, so this is specifically a regression against the immediate base and incomplete compliance with the explicit new zero-limit requirement, not a claim that the pre-rewrite reader checked every excluded path. Revisions `25eb2f5` and `2b9c6e3` reject these examples at both limits.

Keep required root validation independent of output admission while preserving the empty-query work bound. Validating the requested row's name needs no ancestor decode. No wrong nonempty result was observed; this is a rejection-contract defect, matching review15 S2's severity.

**Review15 recheck and valid-input results.**

Review15 S1's decode-work regression is fixed throughout the exercised matrix. Fresh-reader counts include ordinary, projection and streaming decodes:

| Query, plain depth 0 | `7cde8e6` | `25eb2f5` | `2b9c6e3` | `5feba30` | Candidate |
|---|---:|---:|---:|---:|---:|
| `wide_dirs(40,2,1)` | 2 | 4 | 3 | 3 | 1 |
| `wide_dirs(120,2,1)` | 2 | 5 | 4 | 4 | 1 |
| `children(40,Name,1)` | 1 | 3 | 3 | 1 | 1 |
| `subtree_dirs(0,4,Disk,0)` | 1 | 9 | 8 | 1 | 8 |
| `subtree_dirs(40,0,Name,0)` | 1 | 4 | 3 | 1 | 3 |
| `metrics_candidates(4,0)` | 8 | 17 | 8 | 8 | 8 |

The candidate has zero work regressions after applying the authorized zero-limit allowance. Its 1,112 increases over the original zero-limit counts are no greater than the same candidate query at limit 1 and the original query at limit 1. The two zero-limit subtree examples above equal their candidate limit-1 counts. No repeated-query/LRU or `children_page` finding is raised.

Review15 S2's exact table is fixed: at plain depth 2, deleting shallow ID 41 or changing its `own_charged` to text or depth to 2 makes `subtree_dirs(0,1,Name,limit)` return `Invalid` at both limits 0 and 1. At plain depth 0, replacing chunk 32's payload with `x'00'` makes both `wide_dirs(0,0,limit)` and `subtree_dirs(0,4,Name,limit)` return `Invalid` at both limits. The finding above is an additional case that these tests miss.

The work/result matrix ran 9,280 fresh queries per revision, using the three earlier comparison revisions, the immediate base and this candidate. Coverage: plain depths 0–4; nine root/internal/leaf IDs; all eight sorts; relative depths 0/1/2/4/`u32::MAX`; limits 0/1/3/1000; wide thresholds 0/1/2. All 46,400 full-result comparisons with v1 passed; cross-revision result digests agree. Metrics checks both rankings, eligible count and each snapshot's own filename. In particular, `metrics_candidates(4,0)` returns empty rankings and count 121 in every revision.

The corruption matrix recorded 2,670 calls per revision across shallow omissions, malformed rows, misplaced hierarchy, root-copy disagreement, chunk envelopes, requested names and ancestry. The 65 `Invalid`-to-success changes against the immediate base are the filtered-wide variants of S1 above. No other dropped rejection was found in that matrix. Focused repository tests retain prior child-order/name/completeness, file/link chunk, mixed-format diff and ncdu checks. The production diff does not alter those implementations; this is not an exhaustive malformed-input proof.

**Validation and limits.**

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --example snapbench --message-format=json`: exit 0; library 56 passed/1 ignored, reader integration 42/1, diff walk 10/2, example 2/0. Total 110 passed, zero failed, four ignored scale tests. Example tests use a small fixture; the 2M benchmark was not run.
- Four scratch tests per revision passed. Observation tests record corruption outcomes; their passing status is not acceptance of the defect. `compare_matrix.py` independently asserts the exact review15 S2 rechecks and 75 selected finding outcomes.
- Scratch harnesses extract each revision's `reader_v2.rs` and `chunk.rs`, reference the other unchanged checkout modules, and add tests plus a counter at the directory codec's common decode entry. No production behavior is substituted. Sources, compile logs, five matrix logs, summary, regression lists and `focused-tests.log` are retained in this attempt's output `library/`. The initial scratch compile lacked `CARGO_PKG_VERSION`; its log is retained, and all reported runs used the corrected environment.
- Used pinned Rust 1.98.0 and the existing offline Cargo cache. No privileged commands, VM/release tests, full workspace CI, fmt/clippy, ignored scale runs or latency/RSS/file-size benchmark were run. Decode counts do not establish elapsed latency or release readiness.

`cargo clean --offline` completed (2,036 files / 1.0 GiB removed); all five scratch executables were removed. `git diff --check` is clean. The only repository change is this report. No branches, merges or pushes were created.
