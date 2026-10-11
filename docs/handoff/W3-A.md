# Handoff — W3-A: Diff and explain

## Needs from coordinator

- SCR-1 (`docs/handoff/scr-W3-A/SCR-1.md`): `Snapshot` exposes neither its private SQLite connection nor its source file path. The normative ATTACH joins cannot be implemented from the assigned source paths. Please provide crate-private connection access and assign the SQL replacement. This attempt implements a bounded reader-API merge instead. SQL compliance and the five-second scale acceptance are **not done**.
- SCR-2 (`docs/handoff/scr-W3-A/SCR-2.md`): clarify historical missing-path resolution against the global realpath requirement. The interim CLI resolves an existing ancestor before appending missing byte names, rejecting ambiguous unresolved parent components.

- **Branch:** hp-0f6e272006cea2ec5a19dd3e5337e22a782c1be59dcd3c70cdfa33abde3a14d2
- **Agent/session:** attempt-0f6e272006cea2ec5a19dd3e5337e22a782c1be59dcd3c70cdfa33abde3a14d2
- **Date:** 2026-10-11
- **Status:** functional interim implementation; blocked for normative SQL/performance, awaiting coordinator decisions. Evidence candidate, not verified task completion.

## What was built

Implemented disk/reclaimable/file-count differences, presence classes, relative depths, checked signed deltas, raw-byte ties and absolute-growth ordering. Both inputs use immutable readers; subtree rows are reduced to path/depth/value records, sorted, and merged without per-directory cross-snapshot queries. Iterative explain follows a same-direction dominant chain, mirrors shrinkage, stops at ratio/minimum thresholds, and returns own charged byte changes and bounded child contributors.

The CLI selects explicit IDs or latest complete/nearest-before snapshots, reports actual age when the requested duration differs by more than 25%, and renders escaped tree-indented text or a `tidemark.diff.v1` JSON envelope carrying both diff and explanation. Invalid UTF-8 paths use mutually exclusive `path_b64`; no byte arrays leak into the report. Existing live query paths use realpath; removed targets have the narrowly documented ancestor fallback.

## Shared-file edits

None. No manifest, lockfile, reader, crate-root, shared module or core contract edits. No dependencies added. Existing `top --sort growth` is unchanged.

## Deviations from the spec

| Spec reference | Interim behavior | Why | SCR |
|---|---|---|---|
| 05 §5.1–5.2 ATTACH and SQL joins | Bounded reader subtree merge; explain joins direct children in a capped BTreeMap | Private reader connection and missing source path; owned scope excludes reader.rs | SCR-1 |
| 08 §1.1 realpath for all query targets | On ENOENT, realpath an existing ancestor, then append validated missing names | Removed snapshot targets usually no longer exist live | SCR-2 |
| Bounded queries | 1,000,000-row cap for each input and the union; error on excess, including when a smaller output limit is requested | Matches existing reader cap; no incomplete input truncation | No |
| Deep text output | Indentation capped at 64 levels, full escaped paths retained | Bounds output expansion at extreme depths | No |
| Offline validation | Pinned toolchain binary directory and private copied cargo cache `/tmp/tidemark-w3a-cargo` | Cargo absent from PATH; default home not writable; crates.io unavailable | No |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| Scan → mutate → scan → explicit-ID diff --explain names mutated directory | `end_to_end::scan_mutate_scan_diff_names_changed_directory`: invokes `tidemark_scan::run_scan` twice in walk mode, allocates growth, measures snapshot delta, checks text descent and exact measured JSON delta; third scan checks removed target CLI | Passed locally, unprivileged |
| Two 1M-directory snapshots, depth 3, diff under 5 seconds | `million_directory_depth_three`, ignored scale test with synthetic SnapshotWriter snapshots; measurements below | Not met |
| VM end-to-end story on XFS | `end_to_end::xfs_scan_mutate_scan_diff_names_changed_directory`, gated only by `privileged`; same scan/mutate/assertion helper on LoopXfs | pending VM; compilation checked |

## Tests

Store integration tests (`crates/tidemark-store/tests/w3a.rs`):

- `classes_depth_missing_targets_and_limits`: grown, removed, new and symmetric shrunk classes; several depths; target missing on either side; both missing error; zero limit; disk/reclaimable/files metrics.
- `dominant_chain_minimum_and_symmetric_shrink`: dominant two-level chain, minimum stop, negative chain, invalid/nonfinite ratio validation.
- `spread_tie_bytes_and_own_files`: spread stop and child count, invalid-UTF-8 byte tie ordering for diff and descent.
- `own_only_zero_change_missing_target_and_read_only_snapshots`: own-only stop; zero change; missing targets; limit caps; byte-for-byte immutable state files after queries.
- `million_directory_depth_three`: 1M rows on both sides, depth 3, top 25, strict <5-second assertion; `#[ignore = "scale"]`.

CLI integration tests (`crates/tidemark-cli/tests/w3a.rs` and its directory):

- `reports::text_and_json`: insta text and JSON goldens with removal glyphs, shrink explanation, notices and invalid UTF-8 in rows/steps.
- `reports::files_are_counts`: file-count deltas render as counts.
- `catalog::since_age_notice_latest_complete_and_from_default_to`: synthetic catalog selects latest complete over newer partial, nearest-before, age notice, default --to, absent old snapshot/ID and configuration errors.
- `end_to_end::scan_mutate_scan_diff_names_changed_directory`: shared test-support fixture, measured allocations, walk scans and executable text/JSON/removal checks.
- `end_to_end::xfs_scan_mutate_scan_diff_names_changed_directory`: same story on VM-only LoopXfs; **pending VM**. No privileged operations executed locally.

CLI source unit test:

- `cmd::diff::tests::removed_target_resolves_existing_symlink_ancestor_before_normalization`: resolves `link/../gone` through realpath first; rejects an unresolved parent component.

No tests ignored pending other tasks. Existing scale tests remain ignored by ordinary workspace tests. No owner-external stub test required editing.

## Measurements

The initial point-lookup implementation measured 266.282081226 seconds for the diff (314.15 seconds including construction), then was replaced with a sorted merge. The final sorted merge measured **33.549742091 seconds** for the diff (95.38 seconds including construction), still failing the strict scale assertion. Evidence: `library/scale.log`, `library/scale-merge.log` and their exit files in the canonical attempt output. Final checks are recorded below. Measurements use pinned Rust 1.98.0, debug test profile, synthetic immutable snapshots in a temporary directory on the sandbox filesystem; concurrently running validation can affect timings. No production or VM performance is claimed.

## Known gaps and follow-ups

- Normative SQL ATTACH/read-only URI and child SQL join are absent: coordinator reader-access change needed, then replace interim reader-API merge.
- The under-five-second scale criterion remains unresolved; the test retains its strict assertion and remains ignored in ordinary runs.
- VM XFS execution and Wave 3 VM gate are pending; no VM exists in this attempt.
- Cargo-deny advisory/license auditing depends on an equipped host; see CI output.

## Notes for the reviewer

Public signatures and field lists are unchanged; enum derives were added for local metric reuse/class checks. No unsafe code, scanned-file content reads, filesystem crossing logic, service edits, uid gates or mutable snapshot queries were introduced. `own_delta` always compares `own_charged` bytes as the frozen field contract specifies, even for the file-count metric; text labels it explicitly as charged bytes.

Paths in merge and child contributor order use full raw path bytes (within one sibling level, equivalent to name-byte order). Snapshot roots must match. The stored integer and query bounds fail explicitly. Target absent from both snapshots is an error. Ratio must be finite and in (0,1].

## Definition of Done

- Only owned paths changed; no shared-file edits.
- Local checks and scale evidence recorded below; not independent verification.
- Privileged test written and compiled, pending VM.
- Public items documented; owner headers present; no production unwrap/expect or unexplained TODO.
- **Not all acceptance criteria met**: normative SQL, scale threshold and VM execution remain outstanding.

## Final local validation

| Command | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/fmt.log` and `.exit` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log` and `.exit` |
| `cargo test --offline --workspace` | exit 0 | `library/test.log` and `.exit` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0 | `library/privileged.log` and `.exit` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `library/doc.log` and `.exit` |
| `tools/ci.sh (CARGO_NET_OFFLINE=true)` | exit 0 | `library/ci.log` and `.exit` |

Workspace tests: 297 passed, zero failed, six scale tests ignored, aggregated across unit, integration and doc-test result summaries. W3-A adds nine ordinary tests (including its command unit test), one ignored scale test and one privileged-feature VM test. The explicit final scale run fails with exit 101 at 33.549742091 seconds. Privileged build is compilation only, pending VM. CI passes all available checks but skips cargo-deny because it is not installed. `library/contract-audit.json` confirms owned public function types and fields match the baseline. Whitespace and owned-path scope checks passed.
