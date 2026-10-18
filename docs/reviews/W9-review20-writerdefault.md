VERDICT: READY
BLOCKER 0 / SHOULD-FIX 1 / NOTE 0

Reviewed commit `a86b75f36362a14db66a2590c615d44f33fb5b64`, against `7a56325ec8039c362471cd4c216eb598839f39f1`: the two `SNAP-writer-default:` commits ending at that candidate. The supplied checkout is `0debe0626778c1fed335fca1490bb0f56a64c37e`; its later reader-hardening changes are excluded. Tests use an unchanged archive of the exact candidate. Read the writer-default handoff and snapshot-v2 contract, including §3 and §9. READY means no demonstrated blocker in this review; the error-classification defect below remains.

### SHOULD-FIX: conversion space refusal loses the NoSpace category

Reference: `finding:writer-default-conversion-nospace`.

Location: `crates/tidemark-scan/src/orchestrator.rs:1448`, especially the fallback at line 1459. Switching `finish_snapshot` to `SnapshotWriterV2` makes conversion-time `StoreError::Space` reachable in ordinary scans, but the adapter returns `ScanError::Store` instead of `ScanError::NoSpace`.

Reproduction against the exact candidate: complete a small walk scan; set `retention.max_state_size` to 16 MiB for the next scan; after its preflight passes, have the recorder create a private 32 MiB state leaf in `scan_started`, simulating concurrent state growth. With only six directories and one file, the 100,000-input-row checkpoint is not reached. The v2 conversion checkpoint (`writer_v2.rs:455–459`) refuses the write. The public `run_scan_with_clock` result is:

```text
Store(Space("maximum state size would be exceeded"))
```

The probe confirms that the prior snapshot survives, the new catalog lifecycle is failed with no file, and neither `.db.tmp` nor `.db.stage.tmp` remains. The defect is error classification, not cleanup or data loss.

Concrete downstream effects follow from the existing dispatch branches: `tidemark-cli/src/cmd/scan.rs:291–304` maps this to Runtime, whose exit code is 1 (`src/lib.rs:60`), instead of the documented NoSpace exit 5. The daemon only publishes `StateSpaceRefused(true)` for `ScanError::NoSpace` (`tidemarkd/src/scheduler.rs:895`), so this refusal misses that state-space signal. These downstream effects were established by source inspection, not by running the daemon or CLI. The NoSpace contract is recorded in `docs/plan/05-storage.md:340` and `08-interfaces.md:445`. No claim is made about a synchronous daemon API failure: `scan.start` has already acknowledged the scan before conversion.

Map `StoreError::Space(_)` to `ScanError::NoSpace` at the scan publication boundary and cover this conversion-time path. V1 already had an analogous staging-row classification gap; this finding concerns the newly activated v2 finish-time refusal.

### Evidence and coverage

- Source trace confirms `schema_version = 2`, the v2 writer's SQLite `user_version = 2`, first-scan `dirs_estimate × 64`, and unchanged previous-snapshot `uncompressed_length × 5 / 4` rounded up. The existing input-row space callback remains installed; v2 additionally checks conversion boundaries.
- Independent real scans with `series_depth` 0, 1 and 5 verified raw SQLite metadata and the deepest shallow row: `plain_depth` 2, 2 and 5 respectively. All had `user_version = schema_version = 2`.
- The writer preserves directory counters and side tables; file IDs intentionally become clustered row numbers under §9. Growth-series rows still come directly from the unchanged aggregation sink. Cancellation, non-replacing publication and abandoned-stage cleanup retain their existing control paths.
- With Rust 1.98.0, `cargo test --offline --manifest-path target/review-candidate/Cargo.toml --target-dir target/review-build -p tidemark-scan -p tidemark-store --lib --tests` exited 0: scan 89 passed / 4 ignored; store 227 passed / 8 ignored. Coverage includes real-scan v2 publication and cold retention, killed-scan recovery, v1/v2 mixed diff in hot/cold combinations, reader equivalence for files/links/errors/ncdu, cancellation checkpoints, conversion-space refusal and staging cleanup. Ignored tests were not run; child-process subtests are not double-counted.
- `cargo test --offline --manifest-path target/review-candidate/Cargo.toml --target-dir target/review-build -p tidemarkd --test w6b` exited 0: 15 passed. These daemon fixtures deliberately retain v1 snapshots and cover metrics restart, depth changes and cached snapshot rows; they do not independently exercise all daemon growth behavior with v2.
- `cargo clean --offline --manifest-path target/review-candidate/Cargo.toml --target-dir target/review-build` exited 0 after testing.

Supporting evidence is in this attempt's worker-output `library/`: `scan_probe.rs`, `scan-probe.log`, `scan-store-tests.log`, `daemon-w6b-tests.log`, `test-summary.json`, `cargo-clean.log`, and `validation-context.txt`. The probe output has SHA-256 `5fb290ef53f6536c41cde5baed9620ca1a51129e540ec91bb221bf3e445b2059`. The probe is external to the repository and links the unchanged candidate libraries; no production code or repository tests were edited.

### Limits

The new `writer_default.rs` test constructs its v1 reference from directory rows already decoded from v2. It therefore checks reader agreement, not an independent before/after scan oracle, and does not itself compare files, links, skips, errors or annotations. Existing aggregate, writer, reader and mixed-format diff tests provide complementary coverage. Exhaustive scan-level equivalence for every side row and every partial/degraded condition was not established.

No privileged commands, VM/XFS/LVM tests, physical ENOSPC experiment, ignored scale tests or latency benchmark were run. Full-workspace CI and clippy were not rerun. Space refusal was injected through ordinary state allocation. This is a review evidence candidate, not a release or integration approval.
