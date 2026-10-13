# Handoff — W6-watch-windows: exact watcher alert windows

## Needs from coordinator

Run `hot_growth_exact_minute_fires_on_loop_xfs` in disposable Rocky 8.8 and 8.10 guests, and run socket-dependent lifecycle/API scenarios in unrestricted CI. Privileged execution and independent acceptance are pending. This deliverable is an evidence candidate, not a verified success or integration decision. See Validation for local outcomes, dependency-audit limits and the intermittent unchanged metrics staging-lock test failure.

- Attempt: `attempt-646cd1fc8c536047e2c796ec1d484a7b783411c62bbd99ccded0a808baeacda6`.
- Base: `8e967edc6c8057f15909bea9f0e9554c1d508502`; retained canonical attempt branch, no new branch, merge or push.
- Date: 2026-10-13. All five implementation parts are addressed; guest verification remains pending.

## Per-part changes and regression evidence

1. **Exact publication.** The watcher service uses `drain_minutes(1000)`, removes `persisted` rows before retaining facts, and keeps at most 60 completed facts per watcher identity. Persistence still selects the first 50 rows per minute/scope and uses the existing bounded writer. Each completion publishes every distinct enabled rule duration for that mount, only for integral minutes 1–60. Rule-only reload republishes retained history without refreshing its boundary timestamp. Watcher replacement, namespace/mount-table failure and removal discard old history and state windows. Publication rejects a stale observed watch/rule configuration.

   A window requires exactly n consecutive facts. Overflow totals must be nondecreasing within minutes and chain between minutes; the result is end minus start. Missing interior facts make overflow unavailable, following this task's explicit decision. Loss and truncation alone do not invalidate a known overflow difference. Hot requires healthy, untruncated, overflow-free facts and rows; it merges raw keys with saturating events/files/signed deltas, sorts signed bytes descending and raw key ascending, and returns unavailable when the 10,000-key union cap is exceeded. Healthy empty minutes yield `Some(empty)`. Cross-minute `top_unit` is deliberately `None`: minute leaders cannot reconstruct library candidate weights; the ordinary hot API/now attribution remains unchanged.

   `exact_consecutive_minutes_loss_gaps_overflows_and_cap` checks filling, signed merge/counts, empty minutes, missing timestamps, loss, truncation, counter differences/inconsistency, 60-fact retention and the union cap. `injected_watcher_minutes_publish_through_real_reader_and_engine` drains a real injected watcher and proves hot growth fires/resolves; synthetic overflow boundary facts then make watch-lossy fire/resolve through the same real engine/reader. These assertions catch fabricated zeros, skipped-minute acceptance and missing publications.

2. **Freshness.** `LiveStateView::watch_window_at` documents and enforces age 0–90 seconds inclusive, rejecting future observations and unsupported durations. Both AlertSource window readers use it. The injected-watcher regression asserts availability at +90, unavailability at +91, and no future-window use. The existing `alert_source_unavailable_empty_and_measured` fixture now uses a supported two-minute duration and expires at age 91 rather than age 1.

3. **Health and warmup.** Watcher details report `windows: <n> published`. Alert health reports the unavailable-rule count and first three sorted names, and `now.degraded` retains every fault diagnostic. Healthy initial incomplete watcher history carries private warmup evidence; known loss, truncation, gaps, stale facts, failed/recovering watchers and disabled collectors are not warmup. Startup's partial minute is excluded, so filling n minutes requires the next boundary plus n full minutes. Once this initial interval ends, missing facts are faults. Initial insufficient filesystem rate samples for `time_to_full` also report warmup while a fresh filesystem sample exists and the requested window has not elapsed. Warmup stays visible in diagnostics/now as `warming up`, without fault wording or degraded alert health.

   `initial_incomplete_history_warms_but_loss_and_gap_do_not` (also checks i64 clock extremes without overflow), `warming_health_fault_names_and_now_diagnostics`, and `rate_window_warmup_requires_a_healthy_sample_and_expires` check those distinctions, the full now fault list and the three-name summary. They fail if warmup is treated as a fault or unavailable/stale data is treated as healthy.

4. **Thin-pool diagnostic.** Inspection found a cached-alert timing seam, not a missing inventory mapping: LVM publishes an available report immediately, but alerts formerly reevaluated only every 30 seconds or on config changes. Therefore the startup `LVM inventory unavailable` diagnostic could remain while `now` already displayed thin data; by inspection it clears at the next periodic tick if the report stays available/fresh. This was not independently observed on the supplied real Rocky daemons.

   A bounded revision counter changes on successful/unavailable LVM publications, watcher insertion/removal and exact-window publication. The alert worker polls revision/config/shutdown at most every 50 ms and reevaluates on changes, retaining the 30-second periodic evaluation for freshness/hysteresis. Capture precedes evaluation, so a publication racing evaluation cannot be lost. No subscription queue or HTTP was added. `lvm_publication_refreshes_alert_diagnostics_before_periodic_tick` starts the real alert service with no inventory, publishes tracked thin-pool data and requires healthy evaluation within the existing ten-second deadline, shorter than the former tick. Without the input wakeup it times out.

5. **Remaining seams.** `remove_component` removes health and error history; reload also removes retired watcher names even when startup failed before creating a watcher handle. Watcher configuration restarts retire previous per-mount names. Daemon validation reserves 17 component slots for lifecycle/services/post-scan names (including a firstboot diagnostic reserve) and checks the 1,024 distinct supported window-pair cap before startup/reload. Rule-only reload retains still-requested observations and evicts retired pairs.

   `AlertEngine::reconfigure` validates replacements before mutation, remaps state by unchanged name and full `AlertRule` definition, and preserves pending, firing, clearing, notification, internal state-space and injected-clock continuity. Changed/removed definitions lose their state; global renotify is updated. The daemon calls this method on alert reload. `retired_watcher_components_and_errors_release_capacity` churns through twice the component cap using both explicit removal and failed-start reload retirement. `reconfigure_preserves_identity_definition_continuity_and_internal_state` checks reorder, pending/firing/clearing continuity, atomic invalid replacement, removed state and internal alert preservation. `alert_reload_keeps_unchanged_firing_rule_without_duplicate_event` checks real service reload and absence of a second firing notification for the unchanged rule. Rebuilding the engine fails these continuity/event assertions.

   CORE-fix-3 already supplied immutable generation-bound process reports and publication clearing, and its predecessor supplied cancellable process scans. Those implementations and their tests were retained; procscan/scheduler/API sources were not edited.

## Tests updated and audit

Searched the entire workspace for WatchWindow/freshness, unavailable-window diagnostics, LVM inventory diagnostics, alert health and watcher detail assertions before changing behavior. The only old behavior fixture changed is `crates/tidemarkd/tests/w6a1.rs::alert_source_unavailable_empty_and_measured` (90-second duration → 120 seconds; age-1 expiry → age-91 expiry). Existing unavailable-before-publication, ordinary hot/now, mount-presence and collector freshness assertions remain. The five readiness tests already have socket probes in this baseline; their startup/reload/readiness behavior was read and preserved.

The CLI's old diagnostic/detail constants are arbitrary wire/render fixtures, not assertions against the running watcher service; they remain valid text-rendering coverage and are outside this task's write scope. The metrics window fixture already uses a supported duration. New tests were added only in owned alert unit files, watcher/alert service unit modules and `w6a2s.rs`.

Source marker counts across tracked Rust files: 726 before, 736 after, net nine unprivileged tests and one privileged test. Suite inventories: live library 66→67, daemon library 49→54, unprivileged w6a2s 4→7, w6a1 remains 29. Initial focused execution before adding these regressions passed 66 live-library and 49 daemon-library tests, but already contained implementation edits; no clean-base full workspace run was performed. Full final runtime totals are recorded below; baseline full totals are not claimed measured. Socket/root early returns do not prove operator scenarios ran.

## Pending VM: exact guest commands

Run as root inside each disposable Rocky guest, on the candidate checkout:

```sh
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s hot_growth_exact_minute_fires_on_loop_xfs -- --exact --nocapture --test-threads=1
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s watcher_follows_mount_arrival_detach_and_remount_on_vm -- --exact --nocapture --test-threads=1
timeout 1500 cargo test --offline --workspace --no-fail-fast
```

The new test uses the owned LoopXfs fixture, watcher and alert services, a one-minute/one-byte-per-second hot rule, and waits for an evaluable idle window with `alerts=ok` before a child writes 1 MiB. It requires firing within three minutes and retains/join-shuts workers before fixture teardown, including assertion failures. No privileged test ran here; only compilation/linting is permitted. Existing full packaged-unit end-to-end gates should also run in the coordinator's guest process.

## Assumptions and proposed plan text

- This task's explicit requirement that all overflow-window minutes be consecutive takes precedence over LIVE-minutes' more permissive endpoint-only proposal. The consumer conservatively rejects a missing interior minute, with no library/core/plan change.
- Warmup applies to initial daemon collection, including the excluded partial startup minute. A watcher restarted after that initial interval is unavailable until its new history fills; old observations never cross identities. No promise of exact cross-minute unit attribution is made.
- The public injected watcher factory has no injected drain clock or overflow/loss transport. Healthy collector drains use events positioned relative to a captured real minute boundary; all subsequent window/engine/reader clocks are injected. Loss/truncation/counter/gap cases use explicit CompletedMinute facts. No library watcher API was changed.
- Input wakeups run on the existing alert thread and preserve collector failure-epoch replay. The 50-ms poll is an operational scheduling bound subject to thread scheduling, not an instantaneous atomic combined LVM/alert publication guarantee.
- No dependency, manifest, lib.rs wiring, core contract, CLI, plan, HTTP or process-scan change. Prior mount-presence and now assembly behavior is retained. All writes outside the checkout are only this attempt's evidence output or `/tmp`; no privileged/destructive host command was executed.

Proposed coordinator-only plan text: “The daemon retains 60 completed-minute facts per watcher identity, persists their original rows and publishes supported configured windows at their exact latest boundary. This consumer requires consecutive facts for overflow and healthy untruncated facts for hot data, with a 10,000-key union cap. Window observations are usable for 0–90 seconds inclusive. Initial incomplete windows/rates report warming up without fault health. LVM/window publication wakes alert evaluation, unchanged rule definitions preserve hysteresis across reload, and retired watcher health/error entries are removed.” Do not conflate this proposal with an applied contract acknowledgment.

## Validation

All Cargo commands use pinned Rust 1.98.0, host CARGO_HOME/RUSTUP_HOME, offline mode and `timeout 1500`. Logs and exact exit codes are retained in this attempt's worker-output `library/` directory. The first required-check run passed all available stages; it is retained under `library/first-required-checks/`. A final run includes the overflow-safe warmup boundary arithmetic and extreme-clock assertions. Outcomes are appended below. Parts not implemented: none; privileged execution and independent verification remain unresolved.


| Final-source check | Exit | Evidence |
| --- | ---: | --- |
| `cargo fmt --check` | 0 | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `clippy.log` |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0 | `clippy-privileged.log` |
| `cargo test --offline --workspace --no-fail-fast` | 101, then 0 | `workspace-tests.log`, `workspace-recheck.log` |
| `cargo build --offline --workspace --features privileged --tests` | 0 | `privileged-build.log`; compilation only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 | `rustdoc.log` |
| `tools/ci.sh` with offline environment | 0 | `ci.log` |

The first full required run passed 677 tests, zero failed, 14 existing ignores. After the extreme-clock refinement, one repeat reported 676 passed / 1 failed / 14 ignored: the unchanged `w6b::temporary_live_writer_and_nonregular_are_refused` test's recovery write at w6b.rs:424 returned OS error 11 (`WouldBlock`) immediately after dropping the staging-file lock. Its standalone exact recheck passed (`metrics-lock-recheck.log`). Final-source CI then passed 677 / 0 / 14, and the full workspace recheck with `RUST_TEST_THREADS=2` passed 677 / 0 / 14. This intermittent lock failure is retained as evidence; no metrics source or test was modified. Its underlying cause has not been independently established and deserves coordinator follow-up.

Both required-check batches passed formatting, both Clippy configurations, privileged compilation and warnings-denied rustdoc. CI source policy, formatting, Clippy, tests and privileged compilation passed. CI explicitly skipped cargo-deny because it is not installed; dependency-policy auditing is pending on an equipped host. Socket readiness/operator cases can return early in this sandbox, so their passing harness counts are not guest/transport evidence. No privileged test was executed.

Commits separate alert-engine reconfiguration from the coupled daemon publisher/reader/health/wakeup/removal changes, followed by this handoff. All changes are in owned paths. No dependency or shared module wiring edits. Final cleanup and submission receipt are recorded in the canonical attempt report outside the repository.
