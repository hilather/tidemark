# Handoff — STORE-catalog-paths: portable catalog snapshot paths

- Branch: `hp-46ca2f7195171c2ac1964ab64b4270d1ce79c363f0d3e1ad4e4b70e0007731a4`; attempt: `attempt-46ca2f7195171c2ac1964ab64b4270d1ce79c363f0d3e1ad4e4b70e0007731a4`; date: 2026-10-12.
- Status: implemented; unrestricted CI/VM and independent verification pending.

## Needs from coordinator

Re-run the five socket-dependent w6a1 readiness tests on an unrestricted CI host: `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. Each hit `w6a1.rs:27` (deadline exceeded), matching the sandbox limitation identified in the task. Independent verification and privileged VM execution remain pending.

## What changed

`Catalog` now stores snapshot file bytes relative to its own state directory on every write, including rebuild. Prefix comparisons remove dot components lexically, without canonicalization or symlink resolution. Outside paths and relative tails containing parent traversal return `StoreError::Invalid` before SQL mutation. All four read methods resolve relative values against the reading catalog's state directory. Existing absolute values remain unchanged when present; missing legacy absolute values use the last `snapshots` component's tail when that file exists locally, otherwise remain unchanged. The schema, public signatures, writer, and all production callers are unchanged. No dependencies or shared-file edits.

## Acceptance evidence

- `catalog_paths_are_relative_and_resolve_for_each_reader`: raw SQL bytes are relative for both record methods; plain and descriptor-anchored readers return their own openable paths through get/latest/nearest_before/list. Both non-UTF-8 slug and filename survive; real snapshots open through both aliases.
- `catalog_rejects_outside_and_parent_paths`: start and finish reject an outside sibling directory, an unanchored relative path, and parent traversal; no rows are inserted.
- `catalog_legacy_paths_and_rebuild_repair`: existing absolute path remains unchanged; dead `/proc/self/fd/999999/./snapshots/...` and repeated snapshots components recover correctly; unavailable legacy paths remain unchanged. Rebuild through an actual directory descriptor replaces a legacy row with relative raw bytes, readable through the plain catalog.
- `runtime::bound_storage_snapshot_readable_in_separate_process_without_sockets`: real bound-storage scheduled scan, plain catalog and Snapshot open, then an exact child test opens the same catalog and Snapshot in a fresh process. No sockets are used by this regression.
- CLI execution is optional through `TIDEMARK_CATALOG_CLI` because Cargo does not supply `CARGO_BIN_EXE_tidemark` to the daemon package. The local validation supplied the workspace-built binary and successfully ran `--config <missing-temp-config> --state-dir <state> --json snapshots list`, checking the JSON schema and scan id.

## Reader audit and test adjustments

Searched the entire workspace for file assignments, file assertions and catalog consumers.

| Reader / assumption | Result |
|---|---|
| CLI `cmd/top.rs` | File existence checks and Snapshot opens consume resolved catalog paths; compatible. |
| CLI `cmd/files.rs` | Delegates to snapshots selection; compatible. |
| CLI `cmd/diff.rs` | Opens/attaches both catalog paths directly; compatible. |
| CLI `cmd/snapshots.rs` | Selection/show open resolved files; prune uses its existing containment guard; JSON exposes the resolved path; compatible. |
| CLI `render/scan_progress.rs` | Actually renders `ScanOutcome.file` from writer output, rather than a stored catalog value; unaffected. |
| Scan `orchestrator.rs` previous estimates, retention, emergency pruning | Snapshot opens and existing canonical containment checks accept reader-resolved paths; compatible. |
| Daemon `scheduler.rs` | Current writer callback path remains openable; previous catalog path is resolved for Snapshot/diff; compatible. |
| Orchestrator test `assert_eq!(e.file.as_ref(), Some(&published))` | Tests an in-memory recorder failure lifecycle, without Catalog serialization; retained unchanged. |
| Store `catalog_selection_rebuild_and_nonutf8_file_paths` | Changed the non-UTF-8 fixture from `/state/…` to beneath its actual temporary state directory, as outside files are now rejected. Equality after reading remains correct. |
| CLI w2c `outcome_json_schema_and_text` `/state/a.db` | In-memory ScanOutcome rendering only; retained unchanged. |
| CLI w2c/w3a catalog fixtures and before/after equality, scan w2c lifecycle/pruning, daemon runtime catalog paths | Fixtures already lie under their catalogs and reads remain resolved; no adjustment required. |

Only the store fixture needed a part-4 adjustment. No orchestrator or CLI tests were changed.

## Proposed text for docs/plan/05-storage.md §4

> The scans.file column stores raw path bytes relative to the catalog state directory, with its existing SQL TEXT type. record_start, record_finish and rebuild_from_files strip the catalog state-directory prefix lexically after removing dot components; paths outside that prefix or relative tails containing parent components are invalid. No symlink resolution is performed when recording. list, latest, nearest_before and get return paths joined against the state directory used to open that Catalog. For compatibility, an existing legacy absolute path is returned unchanged; a missing legacy absolute path is resolved using the tail beginning at its last snapshots component if that file exists under the current state directory, and otherwise is returned unchanged. Rebuild replaces legacy names with relative paths. Public ScanEntry and Catalog signatures do not change.

## Assumptions

The assigned coordinator decision supersedes the older storage plan's omission of catalog path representation. Caller paths must use the same lexical state-directory name as their writing Catalog; different aliases are interoperable on reads, not accepted as equivalent write prefixes. Bare `snapshots/...` inputs are not under an absolute catalog directory and are rejected. Filesystem existence probes for legacy compatibility concern Tidemark's own state; this adds no scan-tree I/O. No baseline suite was executed before editing; baseline definition counts were measured with git show, rather than represented as prior passing evidence. No privileged tests were run.

## Validation

All long commands ran under `timeout 1500`, with the host Rust 1.98.0 toolchain, host cargo cache, `--offline` or `CARGO_NET_OFFLINE=true` for the CI wrapper.

| Check | Local result |
|---|---|
| `cargo fmt --check` | Passed; final CI fmt also passed. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed; final CI Clippy also passed. |
| `cargo test --offline --workspace --no-fail-fast` | Exit 101: 552 passed, 5 failed, 13 ignored across 59 result summaries, including doc tests. Only w6a1 failed, as listed above. |
| `cargo build --offline --workspace --features privileged --tests` | Passed; build only, pending VM execution. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Passed. |
| `tools/ci.sh` | Exit 1 at workspace tests: the same five w6a1 readiness failures. Source policy (140 owner tags), format and final Clippy passed. The wrapper stops before its privileged-build and cargo-deny stages; the privileged build was run independently above. |

Full logs and command exit codes are saved under this attempt's worker-output `library/`. The targeted catalog tests were rerun after the last store test edits and all five passed. The socket-free daemon test was rerun with the final CLI hook and passed.

Test definitions before/after: storage.rs 28 → 31 (unprivileged runnable storage tests 26 → 29); runtime.rs 11 → 13 (w6a2 unprivileged total 16 → 18). Five test definitions added, including the child-process reader helper. Focused catalog suite: 5 passed, 24 filtered. Socket-free daemon regression with CLI: 1 passed, 17 filtered; child exact reader additionally passed.

Guest proof command, as root in the VM: `cargo test -p tidemarkd --features privileged --test w6a3 operator::real_operator_status_scan_top_now_reload_cancel_and_sigterm`. This is the tidemarkd w6a3 operator gate requested in the task; it is a Cargo test invocation, not a daemon command-line flag. Privileged runtime proof remains pending VM.

## Review status

Implementation and local validation are evidence candidates, not a declaration of independently verified task success. No branch creation, merge, push, plan edits, production caller edits, or dependency changes.
