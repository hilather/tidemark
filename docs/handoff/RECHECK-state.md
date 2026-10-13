# Handoff — RECHECK-state: collector availability, sampler health and mount publication

## Needs from coordinator

Run the socket-dependent lifecycle tests in CI and the existing daemon VM gates on Rocky 8.8/8.10. Cargo-deny was not installed; its dependency/advisory audit remains pending on an equipped host (the existing W0-A skip behavior). Local validation results and any failures are recorded below; no acceptance or integration decision is claimed.

- **Branch:** precreated `hp-e8bab845a84b0353a5f4d0a948169e93ee5ebd27babb31bf93599b3e165566d3`
- **Attempt:** `attempt-e8bab845a84b0353a5f4d0a948169e93ee5ebd27babb31bf93599b3e165566d3`
- **Date:** 2026-10-13
- **Status:** implementation submitted as an evidence candidate; independent verification pending.

## What was built

R3 retains errors from the last due measurement across presence-only sampler passes, merging current absence and persistence diagnostics. Only a due successful measurement removes those collection errors. Missing configured sampler/watch mounts still contribute their existing `not mounted` reasons. A later successful mount publication includes measurement errors already observed on another mount in the same pass.

R4 snapshots configuration/generation, resolves exact mount identities outside the global state lock, then checks generation again before applying bounded in-memory changes. Transition logging runs after releasing the write lock, with the existing one-line-per-transition logger. Stale resolution results produce neither state changes nor logs.

R1 removes the sampler's pressure-error exceptions to unavailable publication, including mount-table collection failures. Process setup/scan failures also publish unavailable attempts; the last immutable report remains available for display. The daemon records successful process-report completion time through a new daemon-only publication variant. Alert use requires availability and age no greater than three current process-scan intervals. Process failure epochs participate in missed-unavailability replay, restarting pending/clearing rule continuity even when failure and recovery happen between alert ticks. Writer-queue congestion continues to preserve valid measurements.

## Shared-file edits

None. No dependencies, manifest changes, module declarations, frozen core types, wire types, config keys or public function signatures changed. New collection callbacks are internal injection seams. Existing process publication variants remain supported; legacy success publications take daemon time, and the service uses an explicitly supplied completion time.

## Acceptance criteria evidence

| Finding | Regression | What fails without the fix |
|---------|------------|----------------------------|
| R3 | `services::sampler::tests::failed_measurement_survives_presence_poll_until_success` | A real no-follow symlink collection failure at a 60-second interval is followed by a ten-second presence-only poll. Sampler health must remain degraded, with no sample, and the actual firstboot builder must remain unhealthy. Reload then schedules a real successful descriptor-checked measurement, restoring both health and firstboot. Without retained errors the presence-only health/report assertions fail. |
| R4 | `state::presence_lock_tests::blocked_resolution_and_logging_leave_readers_free_and_reject_stale_generation` | Barrier-blocked resolution and logging allow a concurrent reader to finish within 200 ms. Reload during blocked resolution must leave presence unpublished. Resolving under the lock blocks that reader; omitting the second generation check publishes stale presence. |
| R4 | `blocked_mount_transition_log_does_not_hold_state_lock` in `tests/w6a1.rs` | The real tracing writer blocks during actual `publish_mount_presence`; a concurrent reader finishes before the log writer is released. The original implementation times out the reader. |
| R1 sampler | `services::sampler::tests::emfile_collection_invalidates_firing_input_and_retains_history` | A real sampler service loop receives an injected EMFILE collection failure after a firing usage rule. AlertSource immediately returns unavailable, display history remains, and alert diagnostics record unavailable input. The old resource-pressure branch leaves the input available. |
| R1 process | `services::procscan::tests::emfile_invalidates_process_input_recovery_restarts_continuity_and_report_expires` | A real scanner collects an allocated deleted-open temp file, then its service receives injected EMFILE after a phantom rule fires. Input becomes unavailable while display survives. Failure/recovery between pending-rule ticks restarts continuity. A report completed at 110 is usable at 140 and unavailable at 141 with a ten-second interval. Without epochs pending evidence fires too early; without freshness the final input/diagnostic assertions fail. |
| Persistence-only congestion | Same process regression plus retained existing sampler/LVM congestion tests | A full process persistence queue reports `persistence_dropped=true` and degraded health while successful alert input remains available. |

## Tests

Validation uses the pinned 1.98.0 toolchain via `PATH=/home/brewerm/.cargo/bin:$PATH`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, and `CARGO_NET_OFFLINE=true`. Every long command is wrapped in `timeout 1500`. No privileged test was executed.

| Command | Exit | Observed result |
|---------|------|-----------------|
| `cargo fmt --check` | 0 | completed |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | 0 | completed |
| `cargo clippy --workspace --all-targets --features privileged --offline -- -D warnings` | 0 | completed |
| `cargo test --workspace --no-fail-fast --offline` | 0 | 750 passed, 0 failed, 14 ignored across emitted test summaries |
| `cargo build --workspace --features privileged --tests --offline` | 0 | completed |
| `tools/ci.sh` | 0 | 750 passed, 0 failed, 14 ignored across emitted test summaries |

All six required commands returned zero on the final source. CI reported `source checks: 146 owner tags; forbidden-symbol scan passed` and `ci: all available checks passed`; it explicitly skipped cargo-deny because the executable was absent. Initial full-run failures were the process fixture enabled/clock assumptions described below; corrected fixtures and the repeated full run passed. Earlier logs are retained under `library/*-first-pass.log` and `w6a1-corrected.log`.

Targeted pre-commit observations: R3 sampler tests **3 passed**; R4 resolver tests **1 passed** and actual tracing-writer integration **1 passed**; final daemon unit suite **68 passed, 0 failed**. The final focused sampler rerun has **4 passed** and `w6a1` has **30 passed, 0 failed**. Full transcripts and machine-readable command results are under this attempt's worker-output `library/`.

Privileged tests: existing `daemon_end_to_end` and `daemon_reload_and_shutdown` — **pending VM**. This patch adds unprivileged deterministic regressions rather than new privileged operations. Guest commands, for each disposable Rocky 8.8 and 8.10 VM:

```sh
cargo xtask vm-test --host <vm> --filter daemon_end_to_end
cargo xtask vm-test --host <vm> --filter daemon_reload_and_shutdown
```

Also run `cargo test --offline -p tidemarkd --test w6a1` in CI with Unix listener binding available. The new blocked-writer test needs no listener and runs locally.

## Proposed plan text (not applied)

For `07` §1 and the collective W6-A service behavior: “Sampler health retains the outcome of its last due measurement between presence polls. Current mount absence and persistence diagnostics are merged into that outcome; a presence-only poll cannot establish measurement recovery.”

For `02` §5 and W6-A LiveState: “Mount-presence publication resolves paths and emits transition logs outside the LiveState lock. Apply resolved in-memory facts only if their captured configuration generation remains current.”

For `07` §4/§6.2: “Every failed filesystem/process collection makes its alert input unavailable, including descriptor and memory pressure. Retain display history and retry. Process reports carry daemon publication completion time for alert freshness, with an inclusive three-interval limit. Failure epochs replay missed unavailability before recovered data, restarting pending/clearing continuity. Persistence-only queue congestion does not invalidate successfully collected data.”

## Assumptions and deviations

- Completion time is taken in the daemon when publishing the completed process report, as authorized; `tidemark-live` is unchanged.
- The inclusive three-interval age boundary uses the same `fresh` helper as filesystem/LVM samples. Future timestamps are unavailable.
- Process config/watch changes retain the existing policy of clearing display reports; collection failures alone retain history.
- The R3 recovery test uses accepted reload to make the next measurement due instead of waiting another full minute. Presence-only behavior is tested at the actual ten-second poll with an unchanged 60-second interval.
- Existing process-report oracle/invalidation fixtures explicitly enable process collection and use the new completion-time publication with `view_at(1)`; the default `view()` clock is filesystem/startup time, not wall time. Their initial enabled/time assumptions failed under the new alert gate and were corrected.
- Errors are injected at internal callbacks in the actual service loops; no host descriptor limits are changed. The process success path uses the actual metadata-only scanner on a real deleted-open temporary file.
- No user questions, branches, pushes, merges, privileged execution, mount/LVM/service commands or writes to system configuration were performed.
- No memory projection was edited; attempt brief/input and receipts were read through the canonical API, with no pending updates reported.

## Notes for the reviewer

Display and alert availability intentionally differ after a failed process collection. An already firing alert remains retained when evidence is unavailable; unavailability pauses continuity and must not fabricate a resolution. The pending-rule regression demonstrates epoch replay independently of the retained firing rule. All channels in the new tests are bounded, and reader tests release their blocked publisher before asserting timeout outcomes.

## Definition of Done evidence

- [x] Only owned paths changed; no shared-file edits.
- [x] Public additions documented, existing ownership headers retained; no unsafe added.
- [x] R3, R4 and R1 regression evidence recorded; separate implementation commits made in requested order.
- [x] Handoff and proposed plan text written without editing `docs/plan`.
- [x] Local fmt, both Clippy configurations, workspace tests, privileged test build and CI available checks passed.
- [ ] Guest acceptance and independent task verification — coordinator responsibility; see validation results above.
