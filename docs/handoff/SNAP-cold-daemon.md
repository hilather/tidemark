# Handoff — SNAP-cold-daemon

## Needs from coordinator

- **Workspace gate blocker outside scope:** update `xtask/tests/w6c.rs:442` so the historical upgrade config need not contain new cold-default comments. `upgrade_fixture_options_and_drift` compares the old fixture byte-for-byte with the current packaged config (after only a metrics-path replacement). This assignment requires adding those comments and explicitly prohibits changing `tests/install/upgrade-config/`; neither that fixture nor the xtask test was edited.

- Independently run the uid 1000, umask 022, socket-enabled and GitHub ubuntu-24.04 gates. This report is an evidence candidate, not an integration or release decision.
- Reconcile the older 15 §2 sentence prohibiting retention removal of `.bad` leaves with this assignment's explicit requirement to remove a deleted scan's quarantine siblings automatically. This task implements the assigned behavior and documents it in the owned docs; `15-snapshot-v2.md` is outside scope.
- Privileged execution remains **pending VM**; only feature-enabled compilation is authorized here.
- Snapshot opens outside this task's owned CLI module retain their existing default inflate policy. Review configured-policy wiring in other CLI commands and daemon growth readers separately; no out-of-scope files were changed.

Attempt: `attempt-996e730d9f7b6a19fe0001dd45e34f6a889b62f01cef92bfa881a5ac3dd22453` · Date: 2026-10-17. Status: implementation candidate with the workspace gate blocker above.

Artifacts: `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-996e730d9f7b6a19fe0001dd45e34f6a889b62f01cef92bfa881a5ac3dd22453/library/`.

## What changed

Added default-enabled cold retention configuration (`cold_after_recent = true`, `cold_level = 9`) and dotted-field level validation (1 through 19), retaining unknown-field rejection. Packaged/example configs document commented defaults. Updated all eight existing `SnapshotRetention` literals, including two in cold-store tests discovered in addition to the six named in the assignment.

Post-scan retention, still under the scan lock, deletes selected snapshots, reconciles the catalog, and compresses retained complete snapshots outside the recent set. Every compression checks half the hot length against configured space policy. Publication follows store compression/fsync → FULL catalog update → trusted descriptor-relative hot unlink → directory fsync. Refusals and failures warn with scan ID and reason and do not fail the published scan. Cancellation is checked between files and passed into the streaming compressor. The preflight estimate uses a cold predecessor's uncompressed header length.

Both automatic/emergency deletion and CLI prune accept `.db.zst` and remove the selected scan's two quarantine siblings through trusted `StateDir` descriptors, then fsync. CLI list adds storage marker and stored/decoded byte sizes; JSON keeps old keys and adds `cold`, `file_bytes`, `uncompressed_bytes`. Prune previews/applies both deletion and compression and adds `compressed` scan IDs to JSON. Snapshot opens in the owned CLI module use the configured inflate space policy.

A quarantine NOREPLACE collision now logs hot/cold and attempted source/destination paths, preserves the pair and existing quarantine, and continues cleanup at the next entry.

## Tests and acceptance evidence

- `cold_retention_defaults_validation_and_unknown_fields`: default values, explicit overrides, invalid levels 0/20, valid boundaries 1/19, unknown-key rejection. `historical_upgrade_config_receives_cold_defaults` parses the untouched historical fixture and verifies the new defaults.
- `completed_scans_compress_calendar_representatives_and_preserve_totals`: fixed-clock five-scan tree, recent=2, three retained cold snapshots, two hot snapshots, separate root's newest hot, catalog targets/identities and root totals unchanged, disabled compression, subsequent cold/hot deletion and both quarantine siblings removed.
- `compression_space_refusal_keeps_hot_and_published_scan_succeeds`: post-publication ballast makes the compression space check refuse; all three completed scans remain hot and the newest scan succeeds.
- `following_scan_estimates_cold_predecessor_from_uncompressed_header`: chooses a real state cap that admits a compressed-byte estimate but refuses the decoded-byte estimate; the following scan refuses before publication and preserves the predecessor.
- CLI `list_sizes_prune_compression_dry_run_and_cold_deletion`: fixed-clock scan fixtures, text/JSON storage markers and exact sizes, dry-run preserves files, prune compresses three retained snapshots, all open, disabled compression is honored, cold show obeys the configured inflate guard, then prune deletes cold files and both quarantine siblings.
- `quarantine_collision_does_not_stop_later_cold_pointer_repair`: collision selected from actual directory iteration order, followed by cold-only stale-pointer repair within one cleanup. With the baseline `catalog.rs` temporarily restored, this test failed (exit 101: "later cold-only entry must be repaired in the same cleanup"); with the fix restored, all 25 cold tests pass. Evidence: `library/collision-before.log`, `library/focused-final.log`.
- Existing cold-store cancellation, crash recovery, corruption, trust, and reader-parity tests remain relevant to the streaming compressor used by both production callers.

All long commands used `timeout 1500`, installed Rust 1.98.0 and offline host cargo cache. Final format, Clippy, privileged-feature test compilation, and rustdoc with warnings denied passed. Workspace tests completed with summed harness summaries **1092 passed, 1 failed, 14 ignored** (91 completed summaries, including nested child harnesses; not a distinct-function count). The sole remaining failure is the out-of-scope `xtask::upgrade_fixture_options_and_drift` assertion described at the top.

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/fmt-final.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0; CI also reached tests after Clippy | `library/clippy-final.log`, `library/ci-final.log` |
| Focused config/scan/store/CLI integration | 38 passed in focused run (config 9, scan 3, store 25, CLI 1); added historical fixture case also passed in final workspace | `library/focused-final.log`, `library/tests-final.log` |
| CLI exact text sizes after strengthened assertions | 1 passed | `library/cli-final.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 101; only xtask historical drift test fails | `library/tests-final.log`, `library/test-counts.json` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compiled only, pending VM execution | `library/privileged-build-latest.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `library/docs.log` |
| `tools/ci.sh` | exit 1; source checks, fmt and Clippy pass; tests fail only on the same historical drift assertion | `library/ci-final.log` |

Initial local checks caught three needless borrows in the new scan tests and a comment mismatch between the documented and fixture TOML; both were corrected. The example comparison subsequently passed (`library/config-example-fixed.log`) and the final workspace run passed every core test. Initial fmt/Clippy/CI attempts selected rustup wrappers under a read-only worker home; corrected runs set `RUSTUP_HOME=/home/brewerm/.rustup`. Original transcripts are retained. No tests were skipped to hide the upgrade drift failure. CI stopped at its test phase; cargo-deny audit results are not established.

## Before/after output

List column sequence before/after (alignment spaces depend on row widths):

```text
before: ID ROOT MODE STATUS DISK
after:  ID ROOT MODE STATUS DISK STORAGE FILE BYTES UNCOMPRESSED BYTES
```

Previously `snapshots list` ended at `DISK`; now it also contains `STORAGE`, `FILE BYTES`, and `UNCOMPRESSED BYTES`, with `cold` for compressed files. List JSON retains `file`, `t_disk`, and every existing field and adds the boolean and two byte sizes. Prune text now reports `would compress <id>` / `compressed <id>` as well as deletions; JSON adds `compressed: [<scan IDs>]`. The CLI regression checks these outputs and compares sizes directly with file metadata and checked cold headers.

## Assumptions and scope

- The task explicitly authorizes the two config-contract additions. No manifest, schema, reader, cold codec, or shared module file changed; no dependencies added.
- Added a documented scan entry point accepting an injected UTC clock so integration tests exercise the real orchestrator deterministically. Existing `run_scan` still supplies the system clock.
- There is no standalone `snapshots remove` subcommand; cold removal is exercised through prune's existing `remove_snapshot` helper. Quarantine removal applies to siblings of selected scans, not unrelated orphan evidence.
- Existing CLI goldens do not capture snapshot-list rows or prune compression output; no existing golden files were changed. New coverage uses semantic text and additive JSON assertions.
- No branches were created, merged, rewritten or pushed. No privileged tests were executed, and no host configuration was changed.

## Handoff status

Only the assigned paths changed. No manifests, shared module declarations or dependencies changed. Implementation and owned regression coverage are submitted as an evidence candidate; full workspace/CI acceptance requires the coordinator's out-of-scope upgrade-test adjustment. Privileged execution, uid 1000/umask 022/socket-enabled testing, and GitHub runner validation remain pending independent gates.
