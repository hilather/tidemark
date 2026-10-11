# Handoff — W3-A-finish: streamed snapshot diff and explain

- Attempt: `attempt-9dafb14deac142dfe86eba12a43388f3ed7994cacce668a775c5de1539b47974`
- Branch: `hp-9dafb14deac142dfe86eba12a43388f3ed7994cacce668a775c5de1539b47974`
- Date: 2026-10-11
- Status: implementation and local evidence candidate; independent verification pending.

## What changed

Replaced the interim subtree vectors/sorted merge with the `05` §5.1 LEFT JOIN / UNION ALL / NOT EXISTS query on `Snapshot::conn()`. The old `Snapshot::file()` is percent-encoded into a bound immutable/read-only ATTACH URI; query cursors stream only the selected output, and statements are dropped before DETACH. A guard also detaches on errors and unwinding. During the query, connection-local read-only memory mappings are bounded at 1 GiB per database (2 GiB total address space); the previous main-connection mapping limit is restored on success, error and unwinding. This avoids small-page-cache churn during random hash-index probes without changing snapshot files. Exact BLOB paths accompany path-hash matching. All runtime query values are bound; interpolated column/filter/order fragments come exclusively from closed internal choices. SQLite PRAGMA settings cannot use bind placeholders, so mapping settings use the typed rusqlite PRAGMA API with a fixed constant and a previously read integer, never caller input.

Explain now joins direct children by their snapshot-specific parent ids in the same attachment, sorts symmetrically for shrinkage, and obtains the complete same-direction contributor count with a SQL window aggregate. It retains at most `max(top, 1)` child rows per level, plus the bounded explanation steps. The subtree/union/child input caps have been removed. The existing documented million-row output and step bounds remain; the snapshot schema and public contracts are unchanged.

SCR-2's accepted fallback was already implemented. Retained realpath of the original live path, then longest-existing-ancestor realpath on ENOENT, followed by byte-name appends; unresolved `..` is rejected. Expanded its regression evidence for invalid UTF-8 names, dot/trailing separator handling, and multiple unresolved parent cases. No other command's resolution changes.

## Shared-file edits and dependencies

None. Only owned files changed; no manifests, lockfile, reader, crate roots, core types, plan files or dependencies changed. Text/JSON renderer and existing CLI goldens are unchanged.

## Decisions and deviations

The task instruction explicitly accepts SCR-1 and SCR-2, superseding their retained “Pending” handoff lines. Each side uses its own target depth to produce relative depths and widened absolute depth bounds. This preserves relative-depth behavior without u32 addition overflow. SQL counts zero-direction parents as having zero same-direction contributors even when children cancel each other out.

SQLite executes the joins/sort/window work; Rust no longer holds entire input subtrees or child maps. Existing reader `temp_store=MEMORY` behavior is unchanged. No claim of a hard process-memory limit for SQLite's scratch work is made.

## Acceptance evidence

| W3-A acceptance item | Evidence | Status |
|---|---|---|
| `tidemark diff /fixture --from A --to B --explain` on a fixture mutated between scans names the mutated directory | Retained `end_to_end::scan_mutate_scan_diff_names_changed_directory`: real walk scans, measured allocation delta, executable text/JSON explanation, then removed-target CLI query | Met locally; owned-test exit 0 |
| Diff of two 1M-directory snapshots, depth 3, returns in <5 s on dev box | `million_directory_depth_three`, still `#[ignore = "scale"]`; now actual depth-3 preorder trees, exactly 1M rows per input, renamed leaf makes union 1,000,001 rows | Met locally in optimized profile (2.194837574 s); debug profile does not meet it (9.367672107 s) |
| XFS scan/mutate/diff VM story | Retained `end_to_end::xfs_scan_mutate_scan_diff_names_changed_directory`, gated by `privileged` | pending VM; compile evidence below |

## Tests

Retained all ordinary W3-A tests and CLI goldens: classes/metrics/depth/missing targets/limits, dominant chain and thresholds, symmetric shrink, raw-byte ties, own-file stopping, read-only snapshot bytes, catalog selection and age notice, file-count rendering, and scan-to-diff executable story.

Added:

- `attachment_uri_bytes_hash_collision_and_error_cleanup`: URI-sensitive and non-UTF-8 state path, deliberately colliding hashes with different full paths, repeated calls, and a malformed own-state snapshot path that makes conversion fail after attachment; repeated errors remain conversion errors rather than stale-attachment failures.
- `path_join_ignores_preorder_ids_and_depth_addition_does_not_wrap`: reordered ids, non-root target, maximum relative depth, and explanation with zero requested spread rows.
- Unit `query_restores_mapping_limit_and_detaches_on_error_and_unwind`: verifies read-only main/attachment, restores the original mapping limit and detaches after injected errors, unwind, and successful queries.
- `zero_parent_delta_does_not_count_opposing_children_as_spread`: cancellation between positive/negative child deltas with zero parent delta, including top zero.
- CLI `missing_target_keeps_bytes_and_rejects_unresolved_parents`: raw names, dot/trailing separators and unresolved parent rejection. Existing symlink/parent-before-realpath regression retained.

## Measurements

Debug-profile depth-3 scale run: **24.401457198 s** for diff; **106.00 s** test runtime including both snapshot constructions. Exit 101 at the unchanged strict five-second assertion. Unlike the interim cap-limited implementation, the 1,000,001-row union completes and returns 25 rows. Evidence: attempt output `library/scale.log` and `scale.exit`.

Before mapping, the optimized run also failed at **36.76167909 s** (53.57 s total test runtime, plus 4m44s initial compilation). Evidence: `library/scale-release.log` and `.exit`.

With bounded mappings, the debug run measured **9.367672107 s** (80.55 s total test runtime), still failing the strict threshold. Evidence: `library/scale-mmap.log` and `.exit`.

The final optimized run measured **2.194837574 s**, passing the unchanged strict <5-second assertion with 25 results from a 1,000,001-row union. Total test runtime **10.50 s**, wall time including incremental build **26.914 s**. Command: `cargo test --offline --release -p tidemark-store --test w3a million_directory_depth_three -- --ignored --nocapture`. Evidence: `library/scale-mmap-release.log` and `.exit`. Ordinary debug tests continue to ignore this scale test. The acceptance is met in the production optimized profile only; no claim of a passing debug-profile threshold is made.

Final validation is recorded below. These are local sandbox timings on temporary snapshots, not VM or production benchmarks. Rust 1.98.0; host cargo cache copied into a private writable `/tmp` cargo home; all cargo commands offline.

## Known gaps from W3-A

- Normative diff ATTACH and explain child SQL joins: implemented, replacing the interim reader merge and map.
- Subtree input cap: removed; scale regression exercises a larger union.
- Five-second timing: met locally in optimized profile; debug remains above five seconds.
- VM XFS execution / Wave 3 VM gate: pending VM; no privileged or destructive host operation executed.
- cargo-deny advisory/license audit: not reached because CI stops at unrelated compilation errors.

## Local validation

| Command (all cargo work offline) | Exit | Evidence |
|---|---:|---|
| `cargo fmt --check` | 0 | `library/fmt.log`, `.exit` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 101 | `library/clippy.log`, `.exit`; unrelated W1-B compile errors |
| `cargo test --offline --workspace` | 101 | `library/test.log`, `.exit`; unrelated W1-B compile errors |
| `cargo build --offline --workspace --features privileged --tests` | 101 | `library/privileged.log`, `.exit`; unrelated W1-B compile errors |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | 1 | `library/ci.log`, `.exit`; unrelated W1-B/W2-A compile errors |
| Crate-wide Clippy/test/privileged build for `-p tidemark-store -p tidemark-cli` | 101 each | `library/target-{clippy,test,privileged}.log`, `.exit`; unrelated W2-C CLI compile errors |
| `cargo clippy --offline -p tidemark-store -p tidemark-cli --lib --bins --test w3a -- -D warnings` | 0 | `library/owned-clippy.log`, `.exit` |
| `cargo test --offline -p tidemark-store -p tidemark-cli --lib --bins --test w3a` | 0 | `library/owned-test.log`, `.exit` |
| `cargo build --offline -p tidemark-store -p tidemark-cli --features privileged --test w3a` | 0 | `library/owned-privileged.log`, `.exit`; compilation only, pending VM |
| `cargo test --offline -p tidemark-store` | 0 | `library/store-test.log`, `.exit`; all store unit/integration/doc tests |
| Optimized final scale run | 0 | `library/scale-mmap-release.log`, `.exit`; 2.194837574 s |

The selected library/binary/W3-A test run has 66 passed, zero failed, 1 ignored (scale). It includes the mapping cleanup unit test, both missing-path unit tests, all retained store/CLI W3-A ordinary tests and goldens, and the real scan/mutate/scan executable story. The scale test remains ignored in default test runs and was explicitly executed separately.

The full store test suite also passed: 49 passed, zero failed, 4 scale tests ignored.

Out-of-scope build failures, left untouched as required:

- `crates/tidemark-fs/tests/w1b/main.rs:15`: `WalkConfig` initializer missing `max_record_bytes` (E0063).
- Same file at lines 599 and 726: pattern string literals for `WalkError::Limit` whose payload is now `String` (E0308).
- `crates/tidemark-scan/tests/w2a/main.rs:553`: `WalkConfig` initializer missing `max_record_bytes` (E0063), reported by CI.
- `crates/tidemark-cli/tests/w2c/main.rs:807` and `:813`: debug formatting of `Vec<ScanEntry>` when `ScanEntry` does not implement `Debug` (E0277).

CI does not reach cargo-deny because Clippy fails in those other tests. No complete-workspace-green or advisory/license-audit claim is made. Broad checks require those owners' fixes and revalidation.

## Definition of Done evidence

- Owned paths only; no shared-file/dependency edits. Scope and whitespace checks performed before committing.
- Format and owned-target Clippy/tests/privileged compilation passed locally.
- The two W3-A acceptance items have local evidence, with the timing profile explicitly qualified.
- XFS execution and the Wave 3 VM gate remain pending VM.
- Full workspace/CI is blocked by the unrelated compile failures above.
- Public contracts/rustdoc retained, source owner headers present, no unsafe code or production unwrap/expect introduced.
- This handoff and attempt report are evidence candidates; independent verification/integration remain with the coordinator.

