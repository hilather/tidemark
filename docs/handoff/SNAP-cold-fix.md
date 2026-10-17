# Handoff — SNAP-cold-fix

## Needs from coordinator

- Independently review this evidence candidate and run the uid 1000 / umask 022 / socket-enabled and GitHub ubuntu-24.04 gates. This worker does not claim verified integration success.
- Privileged execution remains **pending VM**. Feature-enabled tests are compiled only here. Power-loss ordering is established from the FULL-synchronous commit path, not a physical power-failure experiment.
- Concurrent cold opens have independent space checks and may jointly overshoot the floor (§2); no reservations were added.

## What changed

`Snapshot` now retains both its durable catalog filename and its SQLite backing filename. The sole ATTACH site (`diff::with_old`, shared by diff and explain) uses the backing filename. Metrics still report the durable filename. Grepping store sources found no other `snapshot.file()` SQLite opens; no extra explain file was needed.

Cold/hot pair resolution streams the entire decoded cold frame against the hot source through a bounded 128 KiB comparison buffer, checks end-of-file on both sides, and retains the original frame/length/trailing-data checks. `point_to_cold` also requires equivalence whenever a hot sibling exists, protecting the normal compressor caller before it unlinks. Cleanup removes a non-equivalent cold copy with a warning, restores the hot catalog pointer, and keeps the hot file. Rebuild prefers hot whenever both leaves exist; cold-only rebuilding remains header-only.

Catalog transitions set `synchronous=FULL` on the connection executing the update, before the autocommit update returns and before cleanup unlinks hot. Cleanup repairs cold-only stale hot pointers after validating the frame and opening the SQLite payload. The inflate cleanup guard owns the output descriptor immediately after exclusive creation; flock setup and decompression use that descriptor without cloning it. Abandoned v2 `.db.stage.tmp` leaves use the existing temporary cleanup trust, no-follow, mount and scan-lock rules.

Only owned paths changed. No shared module declarations, manifests, dependencies, public contracts, v2 writer files or other task files changed. §2 documents the recovery/equivalence rules and the requested concurrency limitation.

## Regression evidence: before and after

The six added integration tests in `crates/tidemark-store/tests/cold.rs` were run with the original four implementation files restored temporarily from the attempt baseline, then with the fixes restored. Logs: `library/before.log`, `library/before-cleanup.log`, `library/cold.log` in the worker output. No original-source edits remain.

| Finding | Regression | Observed original behavior |
|---|---|---|
| B1 | `diff_and_explain_all_hot_cold_directions` | `NotADatabase`, `file is not a database` |
| B2 | `valid_frame_with_wrong_payload_preserves_hot_and_catalog` | Catalog transition accepted zero-filled SQLite-sized frame; separate cleanup probe failed `Snapshot::open(&hot).is_ok()` after hot deletion |
| B3 | `rebuild_prefers_hot_over_truncated_cold` | Selected `.db.zst` instead of readable `.db` |
| S1 | `cleanup_repairs_cold_only_stale_hot_pointer` | Pointer remained at missing `.db` |
| S4 | `descriptor_exhaustion_never_leaks_inflate_leaf` | Child at descriptor limit 7 left one inflate leaf |
| S2 cleanup | `cleanup_sweeps_v2_staging_leaf` | Planted staging leaf remained |

```text
Original implementation with new regressions:
test result: FAILED. 8 passed; 6 failed; 0 ignored
Fixed implementation:
test result: ok. 14 passed; 0 failed; 0 ignored
```

The comparison test runs diff and explain in hot/hot, hot/cold, cold/hot and cold/cold directions against identical fixtures and checks zero deltas. The descriptor test runs in an isolated child and probes soft limits 4 through 23, restoring the original limit before inspecting leaves. It requires at least one refusal and zero leaked leaves at every boundary. No clock-dependent assertions or sockets were added.

## Assumptions

- Choose removal plus logging of a bad cold pair member, rather than `.bad` quarantine. Published sources are immutable as in the existing contract. Failure to prove equivalence never authorizes hot removal.
- The normal compressor does **not** get a comparison exemption: the catalog transition re-decodes and byte-compares its published output before caller unlink. Cleanup currently compares once to decide pair recovery and again through that protected transition API. This is bounded extra sequential I/O, favoring safety over avoiding a second pass.
- Cold-only repair checks both full framing and SQLite readability; an open refused by the existing space policy remains available for a subsequent repair/rebuild when capacity permits.
- Rebuild selects the hot form in duplicate pairs and still applies its existing readability checks; cleanup resolves duplicates. No capacity reservation mechanism was introduced.
- There was no assigned wave section for this follow-up beyond the explicitly assigned review, prior handoff and §2. Those were read before coding, together with repository conventions and relevant storage/link/testing/playbook material.

## Validation

All builds use pinned Rust 1.98.0, host cargo cache, offline networking, and `timeout 1500` for each long command. The initial invocation found cargo absent from PATH; reruns explicitly added `/home/brewerm/.cargo/bin`. Final results and counts follow below. No privileged tests were run; the existing cold bind-mount regression remains pending VM.

| Command | Observed result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo test --offline -p tidemark-store --test cold` | exit 0; summed completed harnesses: 14 passed, 0 failed, 0 ignored (includes child harnesses) |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline --workspace --no-fail-fast` | exit 0; summed completed harnesses: 1070 passed, 0 failed, 14 ignored (includes child harnesses) |
| `cargo build --offline --workspace --features privileged --tests` | exit 0 |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 |
| `tools/ci.sh` | exit 0; summed completed harnesses: 1072 passed, 0 failed, 14 ignored (includes child harnesses) |

`git diff --check` passed. CI reported `source checks: 153 owner tags; forbidden-symbol scan passed`. The CI wrapper skipped cargo-deny because it is not installed; advisory/license validation remains pending on an equipped coordinator host. uid 1000, socket-enabled, GitHub and privileged VM execution remain coordinator gates; no physical power-loss test was performed.
