VERDICT: BLOCKED
BLOCKER 1 / SHOULD-FIX 0 / NOTE 0

Reviewed exact candidate `79250d3875d5527364fcd7f6a9a1fbd699d65bc6` against base `507cb744687da1bc7203c6fa96763c3bfbf996c6`, including the candidate tree. The supplied integration checkout `163f2fe523677e9b6d02b92904be93d8d81c50fa` has the identical tree, `b2c1d243a52cb5752c4c96660e90358087438e4b`; candidate checks ran there. Reviewed the requested `507cb74..163f2fe` diff/log, `AGENTS.md`, review10, `SNAP-reader2-fix4b.md`, normative link semantics, and the v1/v2 storage contracts. This report is review evidence, not integration or release approval. Production source and repository tests were not edited.

**B1 — BLOCKER: unchecked shallow-root bounds still permit successful wrong subtree results.**

Reference: `finding:w9-review11-snap4fix3-b1`. Locations: `crates/tidemark-store/src/reader_v2.rs:825` (prefer the shallow root), `:830` (derive absolute depth), `:840` and `:841` (use its span for both SQL and validation), `:856` and `:862` (reuse its bounds in chunk queries).

Two single-field corruptions reproduce the problem while leaving all authoritative directory chunks intact:

- A root-plus-child snapshot with `plain_depth=2`: run `UPDATE shallow_dirs SET subtree_end=0 WHERE id=0`. `subtree_dirs(0, 1, Name, 100)` succeeds with only root ID 0, whose returned `subtree_end` is 0. `dir(0)` still returns the authoritative span ending at 1. The new walk accepts this truncated result because the same corrupt span defines its expected end and excludes the child before validation.
- A balanced three-way tree through depth 4 (121 directories), with `plain_depth=3`: run `UPDATE shallow_dirs SET depth=1 WHERE id=0`. `subtree_dirs(0, 3, Name, 1000)` succeeds with 121 rows instead of the sound result's 40, including actual depth-4 descendants. `dir(0).depth` remains 0. The corrupt shallow depth makes `max=4`, selects the chunk branch, and bypasses `ShallowWalk`; intact chunk rows are then filtered using the wrong absolute depth.

These are inherited query-bound trust defects, not valid-input regressions introduced by this patch. Both wrong subtree outcomes were also executed against base production code. The candidate's `metrics_candidates` rejects both mutations, so this finding concerns `subtree_dirs`. Establish the requested root's depth/span from validated authoritative data before choosing the query branch and range; the validator cannot establish completeness when its own boundary comes from the inconsistent copy. Add regressions for both cases. The classification follows the requested wrong-data criterion and whole-candidate-tree scope.

The exact review10 B1, B2 and S1 reproductions are fixed, with measured fail-before/pass-after regressions:

| Review10 item | Candidate result and regression evidence |
|---|---|
| B1, omitted/misplaced shallow rows | `review10_b1_shallow_subtree_and_metrics_rows_are_validated` passes. Deletion of the first child fails immediately against base code; the candidate also rejects the supplied last-row deletion and depth-2 row moved under root. Additional first/middle/last and nonroot variants below pass. The unchecked root bounds above remain a separate gap. |
| B2, malformed v1 component in mixed diff | `review10_b2_mixed_diff_rejects_invalid_v1_child_name` passes and fails against base code. The candidate returns `Invalid` for `../escaped` before diff can construct an invented path. Additional invalid components, both format orientations and a later-page name are rejected. |
| S1, malformed ncdu notable-file component | `review10_s1_ncdu_export_rejects_invalid_notable_file_name` passes and fails against base code. Ncdu and `subtree_files` now agree on all seven invalid-name variants tested; six valid/NULL variants remain accepted. |

For the old-code comparison, scratch module entry points reference unchanged repository modules and the three changed production files extracted at `507cb74`. The new B1 unit test is appended to the old reader module; the current B2/S1 integration tests link against that old library. Each of the three tests exits 101 at its expected `Invalid` assertion. This comparison does not substitute mocked implementations. Scratch sources and build/run logs are retained under this attempt's `library/` directory.

The additional public-API checks use private temporary snapshots and preserve the original chunks for shallow-table mutations:

- A 121-directory balanced fixture provides 288 valid v1/v2 subtree comparisons over six roots, all eight sorts, and relative depths 0/1/2/3/4/`u32::MAX`; metrics parity also covers 0/1/3/4/`u32::MAX`.
- Ten first/middle/last deletions across shallow depths 0 through 3, queried at depth limits 0/1/`plain_depth=3`: 45 `Invalid` assertions and 45 parity checks where the deleted row is below the requested depth. Root deletion is included.
- Seven deletions within a nonroot subtree: 24 `Invalid` assertions and 46 parity checks for excluded rows or queries beyond `plain_depth` that correctly use intact chunks.
- Sixteen misplaced-row mutations across shallow depths 0 through 3: 48 `Invalid` assertions, plus parity for excluded rows. Cases include displaced root ID, invalid parents at depths 1–3, and depth-2/3 rows moved one level up.
- Mixed v1/v2 diff: 84 `Invalid` assertions across first/middle/last child IDs, seven invalid component forms, both format orientations and depth limits 1/3. `z/escaped` and a z-prefixed NUL name sort beyond the 256-child page boundary in a 300-child directory.
- Ncdu: empty, `.`, `..`, `../escaped`, `a/b`, `/abs`, and embedded-NUL names are rejected. Ordinary, non-UTF-8, backslash, dot-prefix and newline names, plus NULL names, remain accepted.

The two B1 observation tests deliberately assert the observed wrong results; their passing status is evidence of the defect, not acceptance coverage. The first old-code observation run also checked the candidate's improved metrics rejection and failed that auxiliary assertion; a separate old-code version confirms that old metrics accepts the truncated-span case while both old subtree defects remain. These logs are retained rather than counted as candidate failures.

No valid-input result change or new panic/overflow was established. Existing directory/file/link/ncdu parity, all-format diff/explain oracles, raw-path tie ordering, cold-format cases, the 120,000-child oracle and the 4,096-deep bounded-memory test are included in the focused suite. The new walk adds one push and at most one eventual pop per selected row: O(rows read) validation time and O(selected depth) stack space, with no additional SQL query per row. ID successors widen to `u64`; depth increments use `checked_add`; the shallow-branch condition guarantees `max_u32(max)` fits. Name validation adds linear work in consumed name bytes and a temporary clone, preserving valid raw bytes and ncdu ordering. Existing heap/path reconstruction costs and the accepted superlinear v1 side of very wide mixed diffs are unchanged. V1/v1 SQL diff is unchanged.

Coordinator-supplied timings, reported as given and not rerun here: host commit `36560dc`, release build, five runs, load approximately 2.5–3.4; command `cargo test --release -p tidemark-store --test diff_walk -- --ignored --nocapture scale`.

| Shape, 1% changed | v1 SQL | v2 warm | v2 fresh | Threshold |
|---|---:|---:|---:|---:|
| One 150,000-child directory | 225 ms | 150 ms | 145 ms | 281 ms |
| 2,000,101 directories | 4,751 ms | 1,914 ms | 1,883 ms | 5,939 ms |

Both supplied measurements meet their thresholds. They are historical evidence at the stated commit, not a measurement of this candidate. The full §8 query/file-size/RSS matrix remains a later task.

Validation and limits:

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --test w3c`: exit 0; lib 44 passed/1 ignored (28.73 s), diff_walk 10/2 (130.12 s), reader_v2 35/1 (154.78 s), w3c 6/1 (0.09 s). Total 95 passed, zero failed, five ignored scale tests.
- Seven additional scratch tests pass against the unchanged candidate libraries (six reader/ncdu tests and one mixed-diff matrix). Sources: `library/review_variants_additions.rs`, `library/review_diff_additions.rs`; runner: `library/run_review.py`. Logs include `focused-tests.log`, `review_variants_tests.log`, `review_diff_tests.log`, `old-comparison.log`, and the individual old-test logs.
- Used pinned Rust 1.98.0 on PATH and the existing offline cache at `CARGO_HOME=/home/brewerm/.cargo`. Direct scratch `rustc` invocations use the same compiled dependencies and perform no network access.
- `cargo clean --offline`: exit 0, removed 2,046 files/1.0 GiB. Scratch executables and the scratch old-library archive were also removed; sources and logs remain. `git diff --check` is clean; the only repository change is this report.
- Full workspace CI, fmt/clippy, privileged/VM tests, ignored scale benchmarks and the full §8 matrix were not run. This is not an exhaustive malformed-input or panic proof. No privileged commands, branch creation, merges or pushes were performed.
