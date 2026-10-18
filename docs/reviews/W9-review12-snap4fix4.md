VERDICT: READY
BLOCKER 0 / SHOULD-FIX 0 / NOTE 0

Reviewed exact candidate `57016584c9ae9a1486468640121037a13410d8dc` against base `81a6502f5df088fefe7e7d6dba5d66c38e718420`. The supplied integration checkout `30e43437e83add0fe6a459d060203e8db6524aed` has the identical tree, `ff31c5cb545a0a8b34d921af393375187c61c864`; candidate checks ran there. Read the requested `81a6502..30e4343` diff/log, review11, the `SNAP-reader2-fix5b` handoff, `AGENTS.md`, normative link semantics and the v1/v2 storage contracts. This report is review evidence, not integration or release approval. No production source or repository tests were edited.

**Review11 B1 is fixed in the exercised cases, with fail-before/pass-after evidence.**

`Backend::authoritative_root` (`crates/tidemark-store/src/reader_v2.rs:819`) reads the requested row from its directory chunk and compares any shallow copy's ID, parent, depth and subtree end. `subtree_dirs` calls it before computing the absolute depth or selecting either query branch (`:839`); `metrics_candidates` calls it before supplying the boundary to `eligible` (`:1012`). Both review11 reproductions now return `StoreError::Invalid`:

| Reproduction, with authoritative chunks unchanged | Base production code | Candidate |
|---|---|---|
| Root plus one child, `plain_depth=2`; set shallow root `subtree_end=0`; query `subtree_dirs(0,1,Name,100)` | Successfully returns only `(id=0, subtree_end=0)` | `Invalid` |
| Balanced 121-directory tree through depth 4, `plain_depth=3`; set shallow root `depth=1`; query `subtree_dirs(0,3,Name,1000)` | Successfully returns 121 rows instead of 40 | `Invalid` |

The candidate also rejects the corresponding metrics queries. The committed `fix5_subtree_root_bounds_come_from_the_chunk` test passes on the candidate and fails at its first subtree assertion on the base. `fix5_valid_subtree_results_match_v1` passes on both. Two separate scratch regressions reproduce each review11 failure independently and both fail against the base, so the first failing assertion does not hide the second case.

The old-code comparison compiles the base's unchanged production `reader_v2.rs` together with the candidate's test module and added scratch tests. Other modules are referenced directly from the unchanged checkout; the diff confirms they are identical between base and candidate. It does not substitute mocked query implementations. Logs retain the expected old-code test failures (exit 101).

**Corrupt-input variants and the handoff's other query claims.**

All mutations below affect private temporary snapshot copies and leave chunks intact. Tests compare complete returned rows, not just IDs or counts. Metrics comparisons also check both candidate lists and eligible counts, while verifying each result's source filename separately.

- Requested roots at each shallow depth 0–3: four internal roots in the balanced tree plus four chain leaves, including a single-directory snapshot. Six single-field mutations per root exercise truncated/extended spans, `u32::MAX` spans, increased/extreme depths, self-parent links and NULL/extreme parents. All 2,304 subtree queries (48 mutations × six relative depths × eight sorts) return `Invalid`; all 72 root-zero metrics queries return `Invalid`. Relative depths are 0/1/2/3/4/`u32::MAX`, covering shallow, projected-chunk and full-chunk branches. `dir(id)` continues to return the unchanged authoritative row.
- On those same corrupted requested roots, `wide_dirs`, `children`, `children_count`, `children_page` and the internal `children_by_name` cursor produce 3,246 exact intact-snapshot matches and 402 `Invalid` results, with no successful wrong result. Coverage includes entry thresholds, substring filters, all sorts, both traversal directions, resumed pages and three name-page sizes. Errors arise where a corrupt self-parent row enters the SQL child set; otherwise these queries can safely ignore the inconsistent parent copy.
- Metrics at nonroot shallow depths 1–3: mutate the first and last row at each depth, with the same six mutation forms, six query depths and limits 1/5/1000. Of 648 checks, 216 reject corruption and 432 exactly match the intact snapshot. This includes excluded rows and requests extending beyond the shallow table, where intact chunks remain authoritative.

The code supports these observations: `visit_children` obtains the parent through `raw` (`reader_v2.rs:483`) and validates the shallow child set against that parent; `children_by_name` uses chunks (`:541`); `wide_dirs` gets both its root and range candidates from chunks (`:891`, `:896`). No additional fix is needed for the tested requested-root mutations.

**Valid results and query work.**

No valid-input result change was established. Additional v1/v2 comparisons cover chunk sizes 1 and 16, eight roots across shallow/deep and internal/leaf positions, all eight sorts, six relative depths and limits 1/5/1000: 2,304 subtree, 36 metrics, 144 wide-directory, 384 children, 48 child-count, 832 child-page and 48 complete name-page comparisons all pass. The focused repository suite also retains its raw-byte ordering, deep/wide tree and other directory/file/link query coverage.

By inspection, the production delta adds at most one direct `raw` chunk-row lookup per affected query. The shallow lookup already existed; when the shallow row is absent, the old implementation already performed the same `raw` lookup. The comparison is constant-size and adds no per-result query or traversal. A cold lookup may decode its bounded containing chunk and perform the existing chunk metadata checks; this is not a claim of one SQLite statement. On fresh readers, root-only shallow subtree and metrics queries decode zero chunks on the base and exactly one on the candidate. Repeating either query keeps the candidate's cumulative decode count at one. No latency/RSS benchmark or worst-case cache-eviction experiment was performed.

**Validation and limits.**

- `cargo test --offline -p tidemark-store --lib --test reader_v2`: exit 0; library 46 passed/1 ignored (37.10 s), integration reader tests 35 passed/1 ignored (197.62 s). Total 81 passed, zero failed, two ignored scale tests.
- Six added scratch tests pass against unchanged candidate production code (3.69 s). The old committed regression and two independent exact reproductions fail as expected; the old valid-input regression and decode-work comparison pass.
- Used pinned Rust 1.98.0 with `CARGO_HOME=/home/brewerm/.cargo`. Scratch `rustc` builds reuse the offline Cargo dependencies. Sources, runner and logs are retained in this attempt's `library/`: `review12_exact_additions.rs`, `review12_variants_additions.rs`, `run_review.py`, `focused-tests.log`, `candidate-variants.log`, `old-exact.log`, `old-regressions.log` and `old-work.log`.
- `cargo clean --offline`: exit 0, removed 1,837 files/925.6 MiB. Scratch executables were removed as well. `git diff --check` is clean; the only repository change is this report.
- Full workspace CI, fmt/clippy, privileged/VM tests, ignored scale benchmarks and the full storage §8 performance matrix were not run. This is not an exhaustive malformed-input or panic proof. No privileged commands, branch creation, merges or pushes were performed. Accepted review9 decisions were not reopened.
