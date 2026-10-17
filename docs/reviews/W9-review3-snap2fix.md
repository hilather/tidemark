VERDICT: BLOCKED
BLOCKER 3 / SHOULD-FIX 1 / NOTE 0

Reviewed exact candidate `1415776bc9174bf1978346a65075566e70eee9cb` against base `60c210de18f8fd30e85ff19a0ea4c61e21026f50`, and separately reviewed the requested integration tree `b953fe91e6a7eeb5e82e80b7a3fe16e3ed98e1aa`. The recorded review session names the former; the checkout and report-delivery contract name the latter. Their only tree differences are `writer_v2.rs`, its tests and its handoff. Cold source and tests are identical. The counts above apply to the pinned candidate: two cold findings below affect both trees, while original B4 and S3 remain only in the pinned candidate's unchanged writer. The integration tree has **2 BLOCKER / 0 SHOULD-FIX / 0 NOTE** from this review. No implementation files were edited.

Read `AGENTS.md`, the original review, snapshot-v2 plan, storage/link contracts and both requested handoffs. Inspected both changesets and `git log 60c210d..b953fe9`. Handoff test claims are distinguished from execution performed here.

## Findings

### R1 — BLOCKER: a mismatch can delete the only readable snapshot

Reference: `finding:w9-review3-snap2fix-r1`. Both trees. Locations: `crates/tidemark-store/src/catalog.rs:851`–`:871`; related rebuild selection at `:374`–`:393`.

Cleanup assumes every failed byte comparison proves the cold file is bad. It repoints the catalog to hot and unlinks cold without establishing that hot is readable. A crash can leave both published forms; if the retained hot file is subsequently truncated or corrupted while the compressed original remains intact, cleanup deletes the only readable snapshot. This destructive behavior is new in the fix. Rebuild independently hides the healthy cold file merely because the corrupt hot leaf exists, then skips the unreadable hot file, leaving no catalog entry.

External probes `review_healthy_cold_corrupt_hot_deleted` and `review_rebuild_hides_healthy_cold_behind_corrupt_hot` reproduced both outcomes. Each creates a real snapshot and healthy cold copy, replaces the hot bytes with an invalid SQLite header, and verifies cold can still be read. Cleanup then removes cold and retains the unreadable hot path; a separate rebuild-only fixture returns zero entries while the healthy cold file remains. These probes assert the observed defects, not desired behavior.

The added B2 regression covers only bad-cold/good-hot. Failed equivalence is insufficient evidence to destroy either member. Preserve an inconclusive pair, or establish which member is valid before discarding the other. Rebuild must establish hot readability before suppressing an available cold copy. The revised §2 rule calling a nonmatching cold member “bad” also needs to account for this case.

### R2 — BLOCKER: payload equivalence does not validate the cold header used by rebuild

Reference: `finding:w9-review3-snap2fix-r2`. Both trees; underlying header-validation gap predates these fixes. Locations: `crates/tidemark-store/src/cold.rs:40`–`:86`, `:228`–`:258`; `crates/tidemark-store/src/catalog.rs:384`–`:385`, `:411`.

`matches_hot` validates the header internally, then compares only decoded SQLite bytes. It never compares header metadata/root counters with the source snapshot. The zstd checksum covers the frame, not the preceding JSON. A valid numeric alteration in header `root.t_disk` therefore passes framing and byte-equivalence checks. Cleanup deletes hot, and header-only rebuild records the altered value even though opening the cold snapshot returns the original SQLite counter.

Probe `review_matching_payload_wrong_header_survives_cleanup` changed only header `root.t_disk` from 4096 to 5096, preserving the original compressed payload. `validate_cold`, direct catalog transition and cleanup all accepted it; hot was deleted. Rebuild reported 5096, while the cold reader still reported 4096. This is a deliberately corrupted-container test, not evidence that the normal compressor generates inconsistent headers.

The new comparator closes original B2's payload hole but cannot establish equivalence of the container's catalog-visible meaning. Check the duplicated header metadata/root against the trusted snapshot when resolving a pair; this does not require a format change. Header-only recovery cannot authenticate subsequently corrupted JSON under the current format, so a stronger guarantee requiring a format change needs a spec-change request. No regression currently covers this disagreement.

### B4 — BLOCKER: external SQLite sorting remains in the pinned candidate

Reference: `finding:w9-review2-snap2-b4`. **Pinned candidate only; fixed in integration.** Locations at `1415776`: `crates/tidemark-store/src/writer_v2.rs:166`, `:429`, `:487`–`:493`.

The exact candidate retains `temp_store=FILE` and bulk staging-index builds. Large inputs can spill through SQLite's environment/default temporary directory, outside Tidemark state, as in the original review's 204,800-instance probe. The writer source is byte-identical to base `60c210d` (blob `d248f99bf36ab881ecb3f1bc43a13d098511a6b1`), so the original defect is unchanged. It was not rerun against a separately compiled pinned tree here.

Integration replaces those sorts with clustered staging and incrementally maintained final indexes. Its external-scratch regression passed here with 200,000 directories and 500,000 link instances, observing both directory entries and live deleted-file descriptors. See the status table and coverage below.

### S3 — SHOULD-FIX: conversion still omits the space callback in the pinned candidate

Reference: `finding:w9-review2-snap2-s3`. **Pinned candidate only; fixed in integration.** Location at `1415776`: `crates/tidemark-store/src/writer_v2.rs:430` and `:483`–`:664`.

The unchanged writer calls conversion with only its cancellation callback. A space guard accepting input and switching to refusal before finish is never consulted while the second database is produced. This is the original S3 scenario and remains evident in the identical source blob. Integration's two conversion-refusal regressions passed here, including refusal at every reached checkpoint and removal of both temporary leaves.

## Original-finding recheck

“Fixed” below means the original scenario is addressed; it does not erase R1/R2. All named current tests ran successfully on `b953fe9`. Fail-before assessments come from comparing the old implementation with each test's assertions; this review did not substitute old source or independently repeat the handoffs' before/after build experiments.

| Original | Pinned `1415776` / integration `b953fe9` | Regression assessment |
|---|---|---|
| B1 | Fixed / fixed | `diff_and_explain_all_hot_cold_directions` exercises all four directions. Old ATTACH selects `.db.zst` and fails. The backing path is now separate; the durable filename remains in metrics output. |
| B2 | Original scenario fixed / fixed; R1/R2 remain | `valid_frame_with_wrong_payload_preserves_hot_and_catalog` uses a checksummed, exact-length zero-filled payload. Old transition accepts it and old cleanup deletes hot; current code rejects it and preserves hot. |
| B3 | Original scenario fixed / fixed; reverse corruption is R1 | `rebuild_prefers_hot_over_truncated_cold` directly catches old header-only suppression of good hot. |
| B4 | Unfixed / fixed | `conversion_never_uses_external_sqlite_scratch` repeats the spill scenario at larger input. `staging_leaf_and_readback_plans` checks clustered staging and all conversion query plans. The old private schema fails that test before the plan assertions. |
| S1 | Fixed / fixed | `cleanup_repairs_cold_only_stale_hot_pointer` recreates the post-crash state. Old cleanup leaves the missing-hot pointer. FULL-synchronous autocommit precedes hot unlink by inspection; no physical power-loss test was run. |
| S2 | Fixed / fixed | `cleanup_sweeps_v2_staging_leaf` fails on the old suffix filter. The writer's suffix was already correct; integration's child-process fixture pins its actual crash-left filename. There is no separate fail-before naming fix. |
| S3 | Unfixed / fixed | `space_guard_turns_to_refusal_after_last_input_row` and `finish_rechecks_space_after_input_and_each_conversion_checkpoint` catch the omitted conversion callback and check complete refusal cleanup. |
| S4 | Fixed / fixed | `descriptor_exhaustion_never_leaks_inflate_leaf` exercises isolated descriptor limits 4–23. The guard now owns the descriptor immediately after creation; the unguarded clone is gone. |

## Safety, format and coverage

- The B2 comparator is streaming: a 128 KiB comparison buffer, bounded decoder window, exact decoded-length limit, checksum/trailing-input checks and hot EOF check. It does not allocate a whole-snapshot buffer. It pins open descriptors during comparison, but does **not** verify unchanged inode metadata or pathname identity through later catalog update/unlink. External privileged in-place edits or replacement are therefore not covered by a mutation-safety guarantee. No compliant Tidemark writer causing that race was found: publication uses immutable leaves/NOREPLACE, and production callers hold the scan lock required by cleanup (the cleanup function does not acquire it itself). No concurrent-mutation experiment was run. This is a stated trust boundary, not an additional demonstrated production race.
- B1 retains the inflated leaf while ATTACH is active and detaches before dropping the old reader. The new backing accessor is crate-private and used only by ATTACH. External probe `review_metrics_reports_durable_cold_path` confirms `metrics_candidates` returns `.db.zst`, without leaking the inflate filename into that output. Existing diff/explain behavior tests also passed.
- Integration B4 leaves the published `V2_SCHEMA` and chunk codec unchanged. Nullable names retain NULL-before-name order and sequence suffixes preserve duplicates. Directory/file/instance reads follow staging primary keys; auxiliary tables stream directly. Required output indexes are created while empty and maintained on insertion. Source inspection and the query-plan test found no remaining populated-table index build or conversion read-back sorter. `temp_store=MEMORY` alone is not treated as proof of boundedness.
- Writer page caches remain 8 MiB each, chunk buffers remain capped at 8192 rows, and spans remain in SQLite. No whole-tree Rust map or new unbounded sort was found. The 200k/500k scratch probe does not establish the earlier review's 2M/10M memory bound or the §8 latency gate (2M directories, 200k notable files and 500k link instances). The additional four-staging-sizes-plus-16-MiB conversion reserve is a conservative estimate, not a shared reservation or a proof of every allocation.
- Integration S3 invokes the callback at conversion boundaries, each chunk and every 8192 plain rows. On refusal the output connection unwinds before writer Drop closes staging and removes both leaves. Every reached checkpoint in the small-chunk regression left its fixture directory empty. No real disk-full injection was performed.
- Existing no-follow/private-mode, corruption-length/trailing-data, cancellation, publication-collision, live-inflate-lock, direct-reader parity, raw v2 round-trip, shallow-row, duplicate-name and link-fixture tests remained present and passed. The new R1 behavior weakens the original corruption-recovery coverage. Bind-mount behavior and power-loss durability were inspected but not executed.
- Original N1 is accurately documented in §2: simultaneous cold opens still check capacity independently and may jointly overshoot the floor. The documentation change does not implement reservations; none were claimed here.

## Validation and evidence

1. `cargo test --offline -p tidemark-store -- --nocapture`: exit 0 on integration `b953fe9`, using pinned Rust 1.98.0 and `/tmp/tidemark-review-bd70555-target`. Cold suite: **14 passed**. Writer-v2 suite: **14 passed**. Aggregate completed harness summaries: **143 passed, 0 failed, 5 ignored**, including a nested subprocess result and doctests; this is not a count of distinct test functions. Cold implementation/test blobs are identical to the pinned candidate. The first launcher invocation failed before building because rustup's default home was unwritable; the successful run used the installed toolchain directly.
2. External Rust harness copied existing fixture helpers without modifying repository code. Four probes passed: three assert the R1/R2 observations, and one checks the durable metrics path. These are reproduced observations, not a passing correctness gate. Source and logs: `library/review-probes.rs`, `library/probe-build.log`, `library/probe-results.log` in this attempt's output directory.
3. `git diff --check 60c210d b953fe9`: exit 0. Scope, log and writer-blob evidence are in `library/scope.log`; Cargo transcript is `library/store-tests.log`.
4. Ran `cargo clean --offline` against the dedicated target (2778 files / 1.6 GiB removed) and removed the standalone probe executable. `library/clean.log` records cleanup. Only the requested review Markdown is changed in the repository.

No full workspace CI, Clippy, privileged commands/tests, equipped cargo-deny gate, uid-1000 gate, power-loss experiment or §8 latency/scale gate was run. No separate pinned-tree build was performed. The review receipt references R1, R2 and the original B4/S3 references because all four apply to the pinned whole-tree candidate; B4/S3 are explicitly resolved in the requested integration tree. This report and receipt are evidence proposals and claim no acceptance, integration or release decision.
