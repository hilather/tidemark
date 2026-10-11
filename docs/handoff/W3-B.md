# Handoff — W3-B: Link topology, regions, symlink analysis

## Needs from coordinator

- Decide [SCR-001](scr-W3-B/SCR-001.md), which requests a crate-private connection accessor in `tidemark-store/src/reader.rs`. The reader currently exposes only bounded vector queries. W3-B's owned paths cannot provide the required streamed rows without this shared-file change.
- Follow up with the streamed implementation and repeat the million-set performance gate. The interim report measured **85.079075606 s**, failing the 10-second timing assertion. This task is not ready to claim all acceptance criteria.

- **Branch:** `hp-17ba9c76717caa7870863e0d32577a0e9397d8c508318e91cc8b5d0a789a8f11`
- **Agent/session:** Codex; attempt `attempt-17ba9c76717caa7870863e0d32577a0e9397d8c508318e91cc8b5d0a789a8f11`
- **Date:** 2026-10-11
- **Status:** functional implementation; blocked on streaming/performance completion. Evidence candidate, not independent verification or release approval.

## What was built

`Regions` selects raw-byte glob matches or relative-depth directories, constructs laminar preorder intervals and parent pointers, and finds the deepest containing region by binary search and iterative ancestor lookup. Crossing reports distinguish unmatched instances inside the scan from undiscovered links outside it, aggregate every region pair once per set, and retain deterministic largest results. Reports include set totals, crossing totals, pinned subtrees, largest sets with LCA paths, and shared directories. Pinned bytes count each incomplete inode once in every containing subtree, including the report root.

Symlink analysis uses safe Rust/rustix, a bounded iterative single-thread traversal, component-by-component `O_NOFOLLOW` directory opens, `O_NOATIME` with the `EPERM` fallback, mountinfo boundaries and device/mount-id checks. It charges the gate before directory opens, getdents refills, metadata calls and readlink calls. Only explicit target checks follow links through `stat`; no regular-file contents are read. Cancellation and breached caps fail explicitly. The additive `analyze_matching` helper applies CLI example filters before the limit without changing the frozen `analyze` signature or `SymlinkOpts` fields.

Both commands select their established roots/snapshots and render escaped text or schema-wrapped JSON with byte-preserving path/target fields. Links includes snapshot metadata, and bulk `--show-paths` resolves names through gated `read_dir_entries`, marking live or gone entries. Existing public signatures, CLI arguments and core contracts were preserved.

## Shared-file edits

None. No dependency, manifest, crate-root, plan, test-support or memory-projection edits.

## Deviations from the spec

| Spec reference | Interim behavior / interpretation | Reason | SCR |
|---|---|---|---|
| W3-B streamed region/link rows | Directory and set queries return capped vectors; instances are fetched one set at a time, not as one global instance collection | `Snapshot.conn` is private and the public methods return vectors; shared reader edits are outside scope | SCR-001 pending |
| W3-B million-set gate | Explicit synthetic timing test failed at 85.079075606 s in debug | Per-set queries and vector materialization remain; no passing release or full CLI timing measurement | SCR-001 |
| `symlinks --dangling` | Requires explicit `--check-targets`, with a configuration error otherwise | Lexical-only analysis cannot diagnose dangling targets; no new flag was introduced | No |
| Inside/outside classification with target checks | Membership remains lexical after target stat; different device, missing target, loop and unreadable results override that class | The specified stat operation supplies no canonical target path; secondary same-filesystem symlink escapes are not resolved for membership | No; interpretation of the stated stat-only algorithm |
| Pinned directories | Aggregate once per incomplete inode per containing subtree, including ancestors | Consistent with the subtree meaning of pinned bytes and `03` union semantics | No |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| `tidemark links /data --boundary-depth 2` on a 1M-set synthetic snapshot in <10 s | `million_set_report_timing`: one million crossing sets counted correctly; library report interval 85.079075606 s; timing assertion failed (`library/scale.log`, exit 101) | **Not met**; full CLI/release gate not claimed |
| `crossing_sets` and `Regions` have rustdoc examples | Two `no_run` examples in `links.rs`; workspace doctests and documentation check | Passed locally |
| Nested/depth regions and brute-force containment | `nested_globs_and_depth_are_relative`, `globs_match_non_utf8_path_bytes_and_literal_separators`, `links::tests::intervals_match_bruteforce` | Locally exercised |
| Scanned hardlink web | `scanned_hardlink_web_crossings_and_paths`: scan → CLI report; 49 crossing sets, 1,274 instances, two incomplete sets; bytes measured with `st_blocks` | Locally exercised |
| Golden worked example | `link_semantics_basic_golden`: inode 100 crosses a/b (4,096 bytes), 102 is incomplete (8,192 bytes), 103 remains in c; totals and LCA checked on synthetic data | Locally exercised |
| Symlink fixtures, lexical and checked classes | `symlink_loops_lexical_and_checked`, `targets_preserve_bytes_and_links_are_never_descended`, `checked_stat_uses_original_target_before_dotdot_normalization` | Locally exercised |
| Cross-filesystem target and mount exclusion | `other_filesystem_target_and_mount_boundary_pending_vm` | **Pending VM**, compile only |
| Same-device bind mount exclusion | `same_device_bind_mount_is_skipped_pending_vm` | **Pending VM**, compile only |

## Tests

All newly added tests:

- Store integration (`tests/w3b.rs`): `link_semantics_basic_golden`, `nested_globs_and_depth_are_relative`, `top_does_not_limit_totals`, `globs_match_non_utf8_path_bytes_and_literal_separators`.
- Store unit property test: `links::tests::intervals_match_bruteforce`.
- Store scale: `million_set_report_timing`, `#[ignore = "scale"]`; explicitly executed and failed its timing assertion. It is not ignored for privilege or silently reported as passing.
- FS integration (`tests/w3b.rs`): `symlink_loops_lexical_and_checked`, `targets_preserve_bytes_and_links_are_never_descended`, `cancellation_and_caps_fail_explicitly`, `checked_stat_uses_original_target_before_dotdot_normalization`, `filtered_examples_apply_limit_after_classification`.
- CLI integration (`tests/w3b.rs`): `scanned_hardlink_web_crossings_and_paths`, `symlink_filters_preserve_total_counts`, `bulk_show_paths_marks_live_and_gone`, `text_and_json_report_snapshots` (four committed insta snapshots).
- CLI renderer unit: `render::symlinks::tests::target_encoding_preserves_relative_bytes`.
- Privileged FS tests: `other_filesystem_target_and_mount_boundary_pending_vm`, `same_device_bind_mount_is_skipped_pending_vm`, both behind `#[cfg(feature = "privileged")]`, neither ignored — **pending VM**.

No tests assert stub behavior. No tests in another task's files were changed or ignored.

## Measurements

The ignored synthetic test constructs 1,000,000 two-instance complete link sets with five directories and depth-2 boundaries through `SnapshotWriter`. Snapshot construction is outside the measured report interval. Debug report time was **85.079075606 s**; total test time including setup was 112.56 s. Hardware visible to the sandbox: Intel Core i7-8750H @ 2.20 GHz, 12 logical CPUs, x86_64. This is not a VM result, a release-build measurement, or evidence that the CLI performance gate passed.

## Validation

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/fmt.log`, `.exit` |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log`, `.exit` |
| `cargo test --workspace` | exit 0; 305 passed, 0 failed, 6 scale tests ignored | `library/test.log`, `.exit` |
| `cargo build --workspace --features privileged --tests` | exit 0; compilation only | `library/privileged.log`, `.exit` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | exit 0 | `library/doc.log`, `.exit` |
| `tools/ci.sh` | exit 0; cargo-deny unavailable and skipped | `library/ci.log`, `.exit` |
| Explicit million-set scale execution | exit 101; timing assertion failed | `library/scale.log`, `.exit` |

Commands use the pinned Rust 1.98.0 toolchain directory on PATH, a private host-cache copy at `/tmp/tidemark-w3b-cargo`, and `CARGO_NET_OFFLINE=true`. Full logs and exit files are in this attempt's canonical worker-output `library/`. No privileged runtime operations were executed. Cargo-deny availability is reported by CI separately from passing the checks it could run.

## Known gaps and follow-ups

- Streamed rows, flat intermediate storage and the required performance gate remain unfinished; decide SCR-001 and implement that follow-up before considering W3-B complete. Current candidate/directory/per-set instance vectors are bounded by the reader's 1,000,000-row cap and reject larger queries explicitly.
- Target membership stays lexical even with checks. A target that escapes through another same-filesystem symlink can be classified InsideRoot; `stat` still checks the original, unnormalized target and reports errors/device changes correctly. A stronger canonical membership algorithm would need a specified gated target-resolution primitive.
- EPERM fallback is implemented but lacks an independently forced fallback test; host UID/permission assumptions were avoided.
- VM execution of the cross-filesystem and bind-mount tests remains pending. No privileged acceptance item is claimed.
- Snapshots with `link_instances = none` are rejected for topology reports with a message requesting `scan.link_instances = all`.
- Live bulk resolution is a moment-in-time view and propagates unreadable-directory errors rather than incorrectly labeling them gone. Path output is capped at one million entries and 64 MiB of path bytes; crossing samples/pairs and report maps have explicit caps.

## Notes for the reviewer

Global totals are independent of top/limit. An incomplete set is `found < nlink`, preserving the normative race tolerance. Crossing `None` denotes instances unmatched by regions inside the snapshot, not hypothetical missing external links. Pin aggregation visits ancestors iteratively and deduplicates each set's contribution. Globs operate on raw bytes with literal separators. No unsafe code was added.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits.
- [x] Final local checks recorded above; scale performance gate separately failed.
- [x] Privileged tests written behind the feature and listed as pending VM.
- [x] Public items documented; owner headers present.
- [ ] All acceptance criteria met — streaming/performance remain open.
- [x] No unexplained TODOs, `unwrap`s or `expect`s outside tests.
