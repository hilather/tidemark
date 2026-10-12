# Handoff — W23-fix-queries: aggregator, ncdu export and diff gate fixes

## Needs from coordinator

None for the five implementations. Independent verification and the existing VM gate remain pending.

- Attempt: `attempt-ff0a7ec24c45770a284a3de47b21fe6b49e7aa3438e7a46ffab524ee58ba2d15`
- Branch: `hp-ff0a7ec24c45770a284a3de47b21fe6b49e7aa3438e7a46ffab524ee58ba2d15`
- Base: `88ef17a4ecc7a78e44abb0803b610af9a737c393`
- Date: 2026-10-12
- Status: implementation and local evidence candidate; not independent verification.

## Findings

| Finding | Status | File and regression evidence |
|---|---|---|
| S2: ingestion rescans retained buckets | fixed now | `crates/tidemark-scan/src/agg/notable.rs`: incremental total and minimum-size cutoff; remove only losing buckets. Unit `distinct_sizes_have_linear_bucket_work_at_default_top_files` counts bucket inspections for 30,000 distinct sizes, both ascending and descending, at top_files=10,000/max_notable=200,000. Existing `bounded_matches_unbounded` and `cutoff_ties_keep_each_directorys_smallest_inodes` retain threshold, tie, final-ID and arrival-order coverage. |
| S5: symlinks counted twice | fixed now | `crates/tidemark-store/src/export_ncdu.rs`: remainder starts from own_files, then subtracts emitted notable entries. Store `tests/w3c.rs::symlinks_are_a_subset_of_other_files`; CLI `tests/w3c.rs::scanned_symlink_other_count_through_cli` checks the executable export of an actual scanned file/symlink tree. |
| S6: truncated apparent union differs from additive export | fixed now | `export_ncdu.rs::descendant_apparent` streams checked sums of descendant own_apparent. Store `outside_subroot_link_keeps_apparent_charge_at_every_depth` covers full export and depths 0–3; CLI `scanned_outside_subroot_hardlink_apparent_matches_at_all_depths` scans a real cross-subroot hard link and compares all those depths. Store `truncated_apparent_sum_reports_overflow` checks overflow refusal. |
| S7: notable paging repeatedly sorts/scans directory | fixed now | `export_ncdu.rs`: bound `(dir_id, id > last)` pages ordered by id; existing files_dir index has implicit integer-primary-key/rowid suffix. `notable_paging_uses_index_seek_without_sort` checks the actual production query at first/subsequent-page keys: SEARCH files USING INDEX files_dir with rowid>, no TEMP B-TREE. `notable_files_above_old_cap_stream_and_io_errors_propagate` checks all 4,097 names in ID order and identical bytes across two exports. |
| S8: diff hides partial/degraded coverage | fixed now | `crates/tidemark-cli/src/{cmd,render}/diff.rs`: labeled From/To catalog snapshot headers with ID, mode, injected age, status and degraded marker; WARNING says deltas may reflect incomplete coverage. Existing JSON notice carries the same warning, combined with the existing age notice. `tests/w3a/catalog.rs::explicit_partial_or_degraded_diff_warns_and_shows_both_inputs` tests partial-only, degraded-only and both, on either side, text/JSON, allowed comparison and unchanged three-field payload. `reports::text_and_json`, `reports::coverage_warning_and_age_notice_goldens` and updated/new snapshots cover headers and combined warning/age output. |

## Reproduction before fixing

Logs are in the attempt output's `library/`:

- `s2-before.log`: assertion failed with **250,025,000 bucket visits** for 30,000 increasing sizes. The fixed regression requires at most 60,000 inspections per direction. Tree lookups/removals are logarithmic; each inserted distinct bucket can be removed only once. Final selection and per-directory cutoff ties retain R2-3 ordering/bounds.
- `store-before.log`: S5 emitted `[other files: 4]` for own_files=3/own_symlinks=1; S6 emitted apparent 532 instead of 32 at depth 0; S7's exact query plan included `USE TEMP B-TREE FOR ORDER BY` and lacked a rowid seek.
- `cli-s5-before-extra.log`: scanned symlink executable regression emitted `[other files: 1]` instead of `[other files: 0]` after both notable rows were emitted individually.
- `s8-before.log`: explicit partial comparison succeeded but failed the assertion requiring both input IDs/mode/age in text.

An initial store regression compile failed because Snapshot::conn is crate-private; the test was corrected to open its temporary snapshot through a separate read-only rusqlite connection before recording the reproductions above. An initial fixed scan compile needed an explicit `sum::<usize>()` in a test-only accounting assertion; corrected before final validation.

## Design and contracts

Only owned paths changed. No dependencies, manifests, lockfile, schema/migrations, reader/writer/store diff, core types or crate roots changed. No existing public function signature or public struct fields changed. The existing renderer report remains callable; new input-header/coverage-notice helpers let its command and direct test callers preserve that signature. Headers and coverage flags use the selected catalog entries, consistent with snapshot selection. Existing age notices and explicit partial selection remain supported.

Notable export order is now ascending stored file ID within each directory, as explicitly authorized; directory order remains preorder. ID -1 is the initial bound, so ID zero is included. SQLite seeks files_dir's existing rowid suffix; there is no added index or disk sort. Descendant apparent summation holds one borrowed row and checked u64 accumulator; disjoint truncated intervals avoid repeated subtree summation. Export disk accounting and raw-byte name handling remain unchanged.

No privileged/destructive host commands, service operations, scanned-file content reads, uid/group gates or live memory edits were performed. No branch creation, merge or push. Cargo uses pinned Rust 1.98.0 through its toolchain binary directory, a private `/tmp/w23-cargo` copy of the host registry, and offline mode.

## Validation

All five required commands returned exit 0, using the offline environment described above.

| Command | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/fmt.log`, `fmt.exit` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log`, `clippy.exit` |
| `cargo test --offline --workspace` | exit 0; 491 passed, 0 failed, 13 intentional scale ignores, including doctests | `library/test.log`, `test.exit` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only | `library/privileged.log`, `privileged.exit` |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | exit 0; all available checks passed; 139 owner tags and forbidden-symbol scan passed | `library/ci.log`, `ci.exit` |

CI explicitly skips cargo-deny because it is unavailable. Dependency advisory/license auditing remains pending on an equipped host. No out-of-scope compilation or test failure blocked this attempt. Focused post-fix logs are `store-after.log`, `scan-after.log` and `cli-after.log`; final workspace tests include the added checked-sum overflow regression. Updated/new goldens were reviewed; final workspace/CI runs pass without snapshot auto-update. Owned-path and whitespace checks passed.

## VM and remaining uncertainty

Existing feature-gated VM regressions are retained, including ncdu import/reexport, XFS aggregation and scan/mutate/diff. Runtime is **pending VM**; this attempt runs only unprivileged temporary fixtures and compiles the privileged suite. Target Rocky Linux/XFS performance and ncdu import are not asserted. The operation-count regression bounds bucket work rather than wall-clock time or whole-process RSS. Submission is an evidence candidate, not gate clearance or integration authority.

## Definition of Done evidence

- [x] Owned paths only; no shared-file or dependency edits.
- [x] Format, workspace Clippy, workspace tests and available CI checks pass locally.
- [x] Privileged suite compiles; retained VM regressions listed above, runtime pending VM.
- [x] All five findings reproduced and covered by passing regressions.
- [x] Public additions documented; owner headers retained; no production unwrap/expect, unsafe code or TODO introduced.
- [x] Handoff and canonical attempt evidence report written.
- [ ] Independent verification, VM gate and dependency advisory/license audit.
