# Handoff — CORE-fix-3: collector availability and immutable process reports

## Needs from coordinator

Run the five documented socket-restricted readiness scenarios in unrestricted CI and the existing privileged daemon gates in disposable Rocky guests. This is implementation and validation evidence for review, not a verified acceptance or integration decision. Final command results are recorded below; no privileged test was executed on this workstation.

## Per-finding changes

### S2 — unavailable collectors cannot supply alert evidence

Filesystem availability is separate from its bounded display/rate history, per mount. Production sampler publications identify their configuration generation and mount, and publish successful or unavailable attempts independently of aggregate component health. Failed mount-table reads invalidate every configured sampler source; statvfs failure invalidates only that mount. LVM similarly publishes successful/unavailable attempts while retaining its last display report. AlertSource accepts only the last-successful available source, at an age from zero through three exact configured intervals, inclusive. Persistence congestion and another mount's failure do not invalidate a successful source. Disabled LVM stays unavailable to alerts.

Bounded failure epochs preserve interruptions even if a collector fails and recovers between alert ticks. The daemon first evaluates affected inputs as unavailable, then evaluates the completed current view at the same logical timestamp; delivery, diagnostics and publication happen once for the combined events. Other sources remain evaluable. A gap longer than three collection intervals also breaks continuity on recovery. Pending/clearing evidence restarts through the existing normative engine behavior; unavailable active alerts remain active without notifications.

The landed W6-mount-presence sampler now clears only that mount's in-memory ring and current availability on appearance/disappearance/identity changes, through a generation-checked state method. Thus an absent mount has no current sample and rates cannot span a remount; persisted history is untouched. This closes that note's sample/rate seam, without changing its unrelated watcher-mark or current-now-degradation follow-ups.

Regressions: `services::alerts::tests::collector_failure_expiry_and_recovery_through_enabled_source` reproduces 95% at time zero, failed collection, ticks at 10/30 and query at 86,400, then fresh recovery and restarted pending evidence. It also tests another mount, persistence health, inclusive expiry and LVM display retention/recovery. `failure_between_alert_ticks_restarts_continuity` uses the real daemon evaluation helper to cover hidden failure/recovery and a hidden freshness gap. Existing sampler presence, sampler-full-queue and LVM fixture/full-queue tests gained assertions for cleared samples, retained valid inputs and failed LVM input respectively.

### S7 — obsolete sample passes cannot resurrect removed mounts

`Publication::FsAttempt` checks its captured generation and current mount membership under the same state write lock that replaces configuration. An obsolete pass returns without changing samples, availability or component facts. Generic synthetic `Publication::Fs` stays available to existing consumers; the production sampler uses the guarded variant. The sampler's transition reset and failure publication are also generation checked.

Regression: `late_sampler_pass_cannot_resurrect_removed_mount` blocks a producer after it captures configuration, replaces `/fixture` with `/new`, then releases both its successful and failed publications. It also rejects a current-generation publication for an unconfigured mount. No `/fixture` sample returns.

### S8 — one reserved internal alert slot

Exported the existing engine capacity as `MAX_ALERT_SUBJECTS` without changing it. Active publication permits 10,000 configured summaries plus at most one internal `state_space` / `state` summary. The diagnostic limit remains 10,000, and a further configured summary is still rejected.

Regression: `internal_alert_has_reserved_publication_slot` evaluates the real engine over 10,000 growth subjects plus space refusal, confirms 10,001 events and active summaries, and publishes them. It separately rejects 10,001 diagnostics and a further configured summary. Existing `reject_incomplete_over_cap_inventory` remains valid and unchanged.

### S3 — private mutable collector, immutable completed reports

The service owns `ProcScanner` directly; no scanner mutex is reachable from published state. `ProcScanner::report()` snapshots completed phantom totals and bounded writer histories into a `ProcReport`, shared through Arc. State readers clone that Arc and compute reports without synchronization with collection. Legacy `phantom`, `writers`, `scan_once` and `take_samples` signatures are preserved for the CLI. Scanner and report use the same window calculation: arbitrary positive durations, including subsecond and irregular windows, select the retained baseline and report its actual elapsed span. Histories retain the existing 10,000 writer / 60 sample bounds.

Production process publications are generation checked. Configuration disablement or any process settings/watch-mount change clears the published report immediately; collector construction/pass failures publish unavailable. Successful subsequent passes publish completed immutable reports, including measured empty. Interval-only reload retains private collector history, as W6-A2b specified, while clearing its prior published result until the next pass.

Regressions: `immutable_report_keeps_exact_windows_while_collector_blocks` compares legacy/report window results, blocks an injected collection source, and requires completed phantom/writer reads within 50 ms. It proves the old report is unchanged after a new completed pass. `process_publication_is_independent_of_collector_and_clears_on_invalidation` holds the private collector mutex while real LiveState readers return within 50 ms, and checks failed/changed/disabled/stale-generation publications and `now` unavailability. The actual service test `disabled_services_reload_and_stop` now goes through enabled success, invalid reconfiguration, error/unavailable, and disablement. Both old constructors in `scanner_measured_empty` and `phantom_measurement_uses_allocated_byte_oracle` now publish `scanner.report()`.

## Regression checks against reintroduced defects

The canonical output's `library/mutation-checks.py` saves and restores each affected source in a `finally` block. Each mutation compiled and exited 101 at a test assertion, not a build failure:

| Finding | Reintroduced defect | Failing regression / artifact |
| --- | --- | --- |
| S2 | Bypass filesystem availability and age gates | `collector_failure_expiry_and_recovery_through_enabled_source`; `mutation-S2.log` |
| S2 continuity | Omit missed-interruption evaluation | `failure_between_alert_ticks_restarts_continuity`; `mutation-S2-continuity.log` |
| S7 | Omit generation/membership guard | `late_sampler_pass_cannot_resurrect_removed_mount`; `mutation-S7.log` |
| S8 | Count the internal alert against configured capacity | `internal_alert_has_reserved_publication_slot`; `mutation-S8.log` |
| S3 | Share a reader mutex with the mutable collector and hold it during collection | `immutable_report_keeps_exact_windows_while_collector_blocks`; `mutation-S3.log` (50 ms receive timeout) |

These are controlled mutations of the defect mechanisms, not a claim to have rerun the entire review commit. All sources were restored before final checks.

## Existing tests inspected and updated

Workspace searches covered `Publication::Fs`, `Publication::Procscan`, AlertSource filesystem/rate calls, and real alert-service constructors. Updated:

- `w6a1::scanner_measured_empty` and `phantom_measurement_uses_allocated_byte_oracle`: immutable constructor.
- `w6a1::full_channel_sampling_is_nonblocking`: current input remains alert-available despite persistence degradation.
- `w6a2s::alert_threshold_reload_applies_without_restarting_service`: its real-time service previously consumed a timestamp-zero sample; the fixture now supplies current Unix time so it tests reload with a fresh measurement.
- `w6a2s::disabled_services_reload_and_stop`: invalidation after prior successful process measurement.
- `services::sampler::tests::injected_mount_appears_then_disappears_without_sampling_parent`: disappearing mount's current samples are removed.
- `services::lvm::tests::fixture_reports_and_failures` and `full_queue_does_not_recollect`: failure makes alert input unavailable while display survives; queue congestion preserves a valid measured-empty input.

The five readiness tests were read: interval-only lifecycle reload, binary notify/stop, private state startup, corrupt catalog recovery and catalog startup. Their assertions do not depend on accepting stale alert data or sharing a process collector. They retain their unrestricted-host assertions. Frozen API/metrics/CLI tests and LIVE-hot assembly were not changed.

## Validation

All long commands run under `timeout 1500`, offline against `/home/brewerm/.cargo`, with pinned Rust 1.98.0, `RUSTUP_HOME=/home/brewerm/.rustup`, four build jobs and two test threads. No dependency was added. Initial Clippy invocation needed the host Rustup home; that environment error was corrected before the recorded final run.

| Command (all under timeout 1500) | Exit | Evidence |
| --- | ---: | --- |
| `cargo fmt --check` | 0 | `fmt-final.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `clippy-final.log`; repeated after all source changes |
| `cargo test --offline --workspace --no-fail-fast` | 101 | `workspace-tests-final.log`: 624 passed, 5 failed, 13 ignored |
| `cargo build --offline --workspace --features privileged --tests` | 0 | `privileged-build-final.log`; compile only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 | `docs-final.log` |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | 1 | `ci-final.log`; stops at the same five readiness failures |

Only the five documented `w6a1` readiness cases failed: `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. They hit their existing readiness deadlines in this Unix-socket-restricted sandbox. All other workspace targets and doctests ran successfully. The final workspace run includes 44 daemon unit tests, 22 passing / 5 failing w6a1 tests, four w6a2s tests and 71 live-library unit tests. Actual binary/operator tests that return early for socket/root restrictions still need unrestricted execution. `cargo deny --version` exited 101 because cargo-deny is not installed; dependency-policy auditing remains unavailable.

All logs and machine-readable counts/results are in the canonical output's `library/`. The initial format/Clippy pass preceded a five-line preservation of sampler error health for an empty inventory; CI and the final explicit format/Clippy repeat checked the final source. That follow-up introduces no measurement behavior change for a populated sampler. No source changes followed the final sampler commit. CI passed source policy, format and Clippy, then stopped at the same five readiness failures; its later privileged-build/deny stages were not reached. The independent privileged build passed, and cargo-deny was confirmed absent.

## Pending unrestricted CI and Rocky commands

On an unrestricted host, run `timeout 1500 cargo test --offline -p tidemarkd --test w6a1 -- --nocapture` and `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh`. The socket-dependent real API/operator scenarios remain covered by `timeout 1500 cargo test --offline -p tidemarkd --test w6a3 -- --nocapture` on a root/socket-capable host. Existing daemon stderr assertions in those operator scenarios remain intact.

Disposable Rocky guests only: `timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture` and `timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture`; or the coordinator's existing `cargo xtask vm-test --host <vm>` gate. The existing mount-presence watcher VM regression remains pending unchanged: `TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s watcher_follows_mount_arrival_detach_and_remount_on_vm -- --exact --nocapture`. Privileged tests were compiled only here. Synthetic blocked-source tests establish synchronization, not real `/proc` latency or the whole-daemon performance budget.

## Proposed plan text (not applied)

For 07 §6.2 and Wave 6: “Alert measurements are separate from display history. Filesystem availability is per configured mount; LVM report availability belongs to its reporting collector. Only the last successful available attempt, no older than three exact configured collection intervals, supplies alert inputs. Failure, intervening freshness gaps, and mount identity transitions break pending/clearing continuity even when recovery occurs between alert evaluations. Persistence congestion and another mount's failure do not hide valid input. Absent mounts have no current filesystem sample; identity transitions discard their in-memory rate baseline while persisted history remains.”

For Wave 6 LiveState: “Sampler publication checks captured configuration generation and current mount membership atomically; obsolete results are discarded. Configured alert summaries remain capped at 10,000 with one separate reserved slot for the internal state-space alert; diagnostic capacity is unchanged. A process scanner stays private to its service. Shared state contains immutable completed phantom results and bounded writer history, supporting arbitrary positive writer windows. Process disablement, settings/mount reconfiguration or failure clears current publication, and successful recovery publishes a new completed report. Readers never wait for process enumeration.”

## Assumptions, bounds and remaining uncertainty

- No user questions were issued, branches created, merges/pushes performed, memory projections edited, external messages sent, dependencies added or privileged/destructive commands run. The supplied branch was retained.
- Failure epochs saturate at u64; inventories remain bounded by existing mount limits. At the inclusive freshness boundary the sample remains evaluable; future-dated observations are unavailable until the injected clock reaches them.
- LVM is one report-producing collector; a reporting failure invalidates its entire alert inventory, while individual statvfs failures stay isolated. Display reports remain retained.
- Generic synthetic publications retain their existing role; all production sampler/process paths use generation-bound attempts. No core DTO, configuration key, CLI flag, metric or schema changed.
- Process report copying happens outside the publication lock; histories are bounded but a report can still be large. This fixes waiting on a mutable collector, not unrestricted serialization/copy/real procfs latency budgets or in-flight collector cancellation.
- Owned paths only. No shared `lib.rs`, manifest/module declaration, scheduler, API, metrics, watcher, core, CLI or plan edit. LIVE-hot assembly is untouched.
- Findings intentionally not implemented: none among S2, S7, S8 and S3. Independent CI/guest verification and unrelated handoff gaps remain outstanding.
- Test source inventory (tracked Rust files in crates/xtask/tests): 679 at the supplied base, 685 after changes, six new unprivileged tests. Base counts are source inventory, not an independently executed baseline. Runtime totals are 624 passed / 5 failed / 13 ignored including doctests. Corresponding baseline runtime inventory is inferred as six fewer tests, not separately executed. Daemon unit inventory is 42→44, w6a1 24→27, and live-library unit inventory 70→71. Early socket/root returns are not proof those scenarios executed.
