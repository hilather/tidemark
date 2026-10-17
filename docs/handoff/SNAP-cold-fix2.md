# Handoff — SNAP-cold-fix2

## Needs from coordinator

- Independently review this evidence candidate and run the uid 1000 / umask 022 / sockets-enabled and GitHub ubuntu-24.04 gates. No integration or verified-success decision is claimed here.
- CLI follow-up: `snapshots list` is catalog-only and does not expose quarantine leaves. Explicit prune visits catalog entries and `remove_snapshot` currently accepts only `.db`, so it neither sweeps `.bad` siblings nor accepts a cold catalog target. CLI is outside this task's scope. State purge's descriptor-based recursive traversal does remove quarantine leaves. Physical state usage already counts their allocated blocks without filtering extensions (`space_guard::state_usage`), so they remain charged against the state cap.
- The CI wrapper skipped cargo-deny because it is not installed. Run advisory/license checks on an equipped coordinator host.
- Privileged execution remains **pending VM**; feature-enabled tests are built only. Power-loss behavior is based on FULL-synchronous commit ordering and directory fsync inspection, not physical crash testing.

## What changed

Hot validation opens the existing immutable read-only reader, checks SQLite `quick_check`, supported user_version, filename/scan_id equality and root presence. Cold validation uses the existing space-guarded `.inflate` lifetime guard and bounded decoder, checks the entire payload and SQLite validity, and compares the header's user_version, all meta key/value pairs and serialized root against the decoded snapshot. No reader API changed; the existing reader's supported SQLite versions remain authoritative.

Cleanup requires byte equality and header equality against a valid hot snapshot before durably repointing cold and unlinking hot. On mismatch it checks both members independently. A sole valid member becomes the catalog target before the invalid sibling is renamed to `.db.bad` or `.db.zst.bad` using NOREPLACE and directory fsync. Two valid but different members, two invalid members, and inconclusive validation preserve both leaves and the catalog row with a warning naming both paths. Operational I/O, SQLite resource failures and space refusal are inconclusive; generic decoder I/O errors that cannot be distinguished from operational failures are conservatively preserved as well. Existing quarantine collisions never overwrite earlier evidence. Startup/rebuild never open `.bad` leaves as snapshots.

The unreleased cold magic is now `TMCOLD02`. JSON is followed by its u64 little-endian xxh3_64 checksum; the frame begins at `20 + M + 8`. Header-only reads verify the checksum before parsing, and `TMCOLD01` is rejected. Frame decoding requires the zstd content-checksum flag as well as validating the checksum, exact decoded length and no trailing bytes. The metadata helper and 1 MiB header bound are shared by compression and semantic comparison.

When both members exist, rebuild prefers hot only after the hot predicate passes. Legacy hot-only rebuild behavior is retained, including historical filenames; existing out-of-scope storage tests require it. Otherwise a cold member with a valid checksummed header is recorded. Both fallback cold and cold-only rebuild remain **header-only**, as explicitly allowed; neither validates the full frame. Filename identity is checked for these cold rows too. §2 records the format and pair decision table.

## Shared-file edits and scope

`crates/tidemark-store/Cargo.toml` adds only `xxhash-rust.workspace = true`, inheriting the same workspace features as tidemark-core, to checksum the cold JSON. Cargo.lock adds only that existing package to tidemark-store's dependency list; no new package is resolved. No module declarations, state-dir trust rules, reader/writer APIs, CLI code or other task files changed. Generic leaf trust checks already support quarantine names. The format change is explicitly authorized by this task; no additional contract change was inferred.

## Regression evidence: before and after

All ten added tests were executed with baseline cold/catalog sources restored temporarily, retaining the dependency and test additions. Each failed with exit 101. Baseline sources were then replaced byte-for-byte with the fixed sources before final validation. Logs are `library/before-final.log`, `library/before-final-status.log`, and `library/final-cold.log` in this attempt's worker output; earlier probes remain in `before.log` and `before-additional.log`.

| Regression | Baseline observation | Fixed expectation |
|---|---|---|
| `review_healthy_cold_corrupt_hot_deleted` | Healthy cold vanished | Healthy cold survives, corrupt hot quarantined, catalog → cold |
| `review_rebuild_hides_healthy_cold_behind_corrupt_hot` | Rebuild returned 0 | Rebuild records cold |
| `review_matching_payload_wrong_header_survives_cleanup` | Direct transition accepted mismatched header | Recomputed checksum cannot hide semantic mismatch; preserve hot and quarantine cold |
| `cold_only_rebuild_rejects_header_corruption_and_old_magic` | Corrupted JSON accepted | Header-only rebuild rejects changed JSON and old magic |
| `inconclusive_pairs_preserve_both_bytes_and_catalog` | Different valid cold deleted | Both-valid-different and neither-valid preserve exact bytes and catalog |
| `hot_identity_and_root_are_required_for_rebuild` | Wrong scan identity cataloged | Wrong identity and missing root skipped |
| `cold_frame_must_include_checksum` | Unchecksummed frame accepted | Frame rejected |
| `quick_check_rejects_corruption_outside_meta_and_root` | Unrelated corrupt table cataloged | Quick-check corruption skipped while direct meta/root open remains possible |
| `quarantine_collision_never_overwrites_previous_evidence` | Hot removed | Both published members and earlier quarantine preserved |
| `space_refusal_is_inconclusive_for_pair_cleanup` | Cold removed after space refusal | Both published leaves and catalog preserved |

The original B2 test `valid_frame_with_wrong_payload_preserves_hot_and_catalog` still exercises a valid checksummed frame containing invalid SQLite. Its assertion now explicitly requires `.db.zst.bad` rather than deletion. The added space-refusal regression uses a signed-domain declared length exceeding available capacity; it uses no real-time assertion or actual large allocation and requires the pair/catalog to remain intact.

```text
Baseline: ten separate runs each FAILED. 0 passed; 1 failed
Fixed cold suite: ok. 24 passed; 0 failed; 0 ignored
```

## Assumptions and limits

- Rebuild preserves the pre-existing hot-only filename behavior; the task specifically tightens selection when both members exist. Strict identity and quick-check regressions therefore use pairs.
- Published snapshot leaves are immutable and production cleanup callers retain the existing scan lock. No new guarantee is made for a privileged actor replacing/editing paths during validation and later rename/unlink.
- Comparison and the protected catalog transition may each stream the frame; mismatch recovery may also inflate once for independent cold validation. This favors positive evidence over avoiding additional bounded sequential I/O.
- A resource failure does not prove invalidity. Inconclusive pairs are retried on later cleanup; no temporary file or quarantine is read as a snapshot during rebuild.
- Header checksum detects accidental changes; it is not an authentication mechanism. A cold-only rebuild intentionally trusts a checksum-valid header, even if an actor deliberately recomputes its checksum. Full cleanup validation checks header/payload consistency.
- Concurrent cold opens retain independent capacity checks; shared reservations remain outside scope. No privileged tests were added because the changes are container/catalog logic; the existing cold bind-mount test remains pending VM.
- No wave subsection was assigned for this follow-up; the task's two reviews, prior handoff and §2 were read with repository conventions and the relevant playbook/handoff requirements.

## Validation

All long commands use `timeout 1500`, offline host cargo cache, and the installed pinned Rust 1.98.0 toolchain. Initial launch attempts failed before compilation because the worker default rustup home was read-only and its default cargo cache lacked base64; setting RUSTUP_HOME and CARGO_HOME to the host caches resolved both. An initial Clippy run identified an unused Snapshot import; subsequent preservation of legacy hot-only rebuild uses that import. An initial workspace run exposed two existing storage tests using `valid.db` for hot-only rebuild. Strict hot validation is now limited to pairs in rebuild, as item 4 specifies; the two predicate regressions now include corrupt cold siblings. Final checks below supersede these initial runs.


| Command | Final observed result | Evidence log |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/final-fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `library/final-clippy.log` |
| `cargo test --offline -p tidemark-store --test cold` | exit 0; 24 passed, 0 failed, 0 ignored | `library/final-cold.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 0; 1087 passed, 0 failed, 14 ignored | `library/final-workspace.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; build only | `library/privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `library/docs.log` |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | exit 0; 1089 passed, 0 failed, 14 ignored | `library/ci.log` |

Test totals sum completed harness summaries, including nested subprocess summaries; they are not counts of distinct test functions. CI reported `source checks: 153 owner tags; forbidden-symbol scan passed` and skipped cargo-deny because it is absent. The initial failed Clippy/workspace runs remain in the earlier logs for transparency; the final direct checks above and corrected-tree CI supersede them. No privileged tests were executed.

`git diff --check` passed. Build cleanup and final scope/status evidence are recorded in `library/clean.log` and `library/scope.log` before submission. Independent acceptance remains with the coordinator.

## Definition of Done evidence

- [x] Only owned paths changed; manifest/lock edits and their reason are listed above.
- [x] Final format, workspace Clippy, workspace tests and all available CI checks passed locally.
- [x] Existing privileged tests compiled; cold bind-mount execution remains pending VM.
- [x] New crate-internal items have rustdoc; no unsafe code or production unwrap/expect was added.
- [x] Pair resolution, rebuild fallback, checksum, semantic-header and B2 quarantine criteria have regression evidence.
- [x] Assumptions, compatibility choice, CLI gaps and validation limits are recorded.
- [ ] Independent uid 1000, GitHub, equipped cargo-deny and privileged VM gates remain with the coordinator.
