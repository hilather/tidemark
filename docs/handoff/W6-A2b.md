# Handoff — W6-A2b: Watcher, process-scan and alert services

- Branch: existing `hp-ec8cb5d92e0800caee114e41a04b19f1ccd84d109f8a611d88e613e7b4d96aa9`.
- Attempt: `attempt-ec8cb5d92e0800caee114e41a04b19f1ccd84d109f8a611d88e613e7b4d96aa9`; date: 2026-10-12.
- Status: service implementation delivered; acceptance blocked at the frozen-interface seams below.

## Needs from coordinator

This deliverable is an evidence candidate; acceptance remains incomplete at frozen-interface seams:

- Exact watcher windows (G4) are blocked. `Watcher::drain_completed_minutes` emits rows only, so an empty minute has no timestamp/loss record. `stats()` reports the cumulative overflow count at call time, not the preceding completed-minute boundary. Delayed polling cannot reconstruct boundary totals or distinguish an empty healthy bucket from a lost empty bucket. Add `Watcher::drain_minutes(&self) -> Vec<CompletedMinute>` with `CompletedMinute { started_at: i64, overflow_total_at_start: u64, overflow_total_at_end: u64, lossy: bool, truncated: bool, dirs: Vec<HotEntry>, persisted: Vec<HotMinute> }`, including empty minutes, at most 60 buckets, and top-1,000 directory rows. This permits bounded rolling aggregation and exact cumulative differences without claiming complete inventories. Currently **no WatchWindow measurements are published**, so watcher-backed rules remain not evaluable; diagnostics explain unsupported durations or the missing observation. No rounded/clamped windows or fabricated zeroes.
- The frozen state reader requires `WatchWindow.measured_at == view_at(now)` exactly. Minute publishers and an independent 30-second alert worker cannot guarantee matching wall seconds. A coordinator-owned state change should retain precise minute-boundary facts with a documented freshness interval and enforce supported durations. Proposed `LiveStateView::watch_window_at(&self, mount: &str, window: Duration) -> Option<i64>` exposes the observation time, with the existing AlertSource queries using its documented freshness. Arbitrary unsupported windows must remain unavailable.
- Procscan state cannot be invalidated. Add `LiveState::clear_procscan(&self)` (or `Publication::ProcscanUnavailable`) on reconfiguration/disablement/failure. The service drops its own obsolete collector, but API/Now views can still read the old state handle. Alerts mask disabled/error procscan data using config and component health, without duplicating collector state. Failure between scanner mutation and publication remains a shared-handle atomicity limitation.
- `ProcScanner::scan_once` is synchronous and uncancellable. Add `scan_once_cancelled(&mut self, now: i64, cancel: &AtomicBool) -> io::Result<ProcScanResult>` with cancellation checks during enumeration and aggregation to prove prompt shutdown for million-fd hosts. Current waits interrupt promptly, but an in-flight pass must finish.
- Alert reload rebuilds the engine when `AlertsConfig` changes, resetting firing/continuity state, including internal state-space continuity. Add `AlertEngine::reconfigure(&mut self, rules: &[AlertRule], renotify: Duration) -> Result<(), AlertError>` with documented state preservation by unchanged rule identity if continuity across threshold reload is required. Unrelated config reloads preserve the engine. Changed thresholds/rules apply immediately.
- Removed watcher component names remain in the frozen bounded component map. Repeated watch-set changes can eventually exhaust its 128-name cap; add `LiveState::remove_component(&self, name: &str)` or recycle components during watcher removal. The initial watcher count must also leave room for other services in the same component cap; per-list validation currently does not enforce that combined budget. Publication errors are returned, never silently dropped.

## What was built

Replaced all three service stubs with named synchronous worker threads using frozen `start` signatures. Watcher checks host namespace before any probe/mark, creates one collector per configured mount, publishes shared collectors for API/dirty-state use, drains completed top-1,000 minute lists, and persists only top 50 per minute/scope. It reports failed, recovering, overflow and persistence loss state, restarts only on watcher configuration changes, and releases state/service Arcs on restart, shutdown and loop errors. It never enables/drains foreground events or takes dirty roots; scheduler remains the sole scan-success dirty-clear owner.

Procscan immediately builds device mappings from bounded mountinfo, deduplicates devices, scans at the configured interval, publishes successful shared collector state and chunks phantom/writer rows to the writer's 20,000-row cap. It reports `fds_seen`, `writers_full`, `writer_evictions` and persistence drops; it reports no fabricated permission-error count. No Gate or regular-file-content reads were added. Reload applies interval, exclusions, writer tracking, enablement and watch mounts.

Alerts evaluate every 30 seconds (also at reload boundaries), use `LiveStateView` through a collector-disablement/window gate, publish active summaries and unavailable diagnostics, send every event to journald and optionally the library's bounded webhook worker, and queue events without blocking. The writer publishes history only after persistence. R2-10 unavailable-state semantics and internal state-space transitions delegate to AlertEngine and have service-level fake-source/recording-deliverer tests.

## Public interfaces

The only public items in owned source files are these unchanged signatures, one in each of `services::{watcher,procscan,alerts}`:

```rust
pub fn start(
    config: Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState,
    writer: super::tswriter::Sender,
    shutdown: crate::Shutdown,
) -> Result<crate::ServiceHandle, crate::DaemonError>;
```

No shared files, manifests, dependencies, library implementations, scheduler/firstboot code, contracts, plan or memory projections changed. Existing attempt branch retained; no branch creation, merge or push.

## Assumptions and deviations

- Exact watcher measurements are unavailable rather than approximated. Raw minute persistence and ordinary library-backed hot API remain usable as ranked activity lists, not complete inventories. The service reports the collector gap explicitly.
- Watchers are rebuilt for any watcher settings change (including exclusion changes); alert-only/schedule/procscan changes do not restart watchers. Failed startup is reported and retried on watcher config change, not in a tight probe loop.
- Procscan monitors `watch.mounts` independently of watcher enablement. Interval-only changes preserve its writer baselines. Mount paths must exactly match mountinfo entries; no symlink-following canonicalization is added. Duplicate devices have one canonical configured subject.
- Default rule generation remains owned by config loading, which already merges defaults. Tests may deliberately construct Config directly with an explicit rule list.
- Alert thresholds/rules/renotify changes rebuild the engine; webhook-only changes currently do so as well. HOSTNAME provides the webhook host label, with `unknown` fallback.
- Collector libraries own their existing finite caps. Watcher service retains at most configured 128 collectors; a drain is at most 60 x 3 x 1,000 rows, persisted selection at most 60 x 3 x 50. Procscan chunks existing bounded sample results; alert engine caps subjects/rules at 10,000 and uses at most two deliverers. Writer-full/disconnected conditions drop persistence and report degraded health.
- Webhook drop delegates to its library worker: one in-flight HTTP request can take its five-second timeout. No HTTP webhook delivery was exercised here.

## Acceptance evidence

| Criterion | Evidence | Status |
|---|---|---|
| Alert firing/resolving/renotify and unknown pause | `services::alerts::tests::transitions_delivery_persistence_and_unavailable_pause`, fake AlertSource and recording Deliver | Local test |
| State-space and bounded writer behavior | `services::alerts::tests::state_space_and_full_writer_do_not_block_delivery` | Local test |
| Unsupported watcher durations; disabled collectors | `services::alerts::tests::exact_window_gate_and_disabled_collectors` | Local test |
| Minute persistence is top 50 per scope | `services::watcher::tests::persistence_keeps_fifty_per_minute_and_scope_in_rank_order` | Local test |
| Disabled services, procscan live reload and shutdown | `disabled_services_reload_and_stop` | Local test |
| Bad mount does not publish measured zero | `procscan_bad_mount_reports_error_without_measurement` | Local test |
| Alert threshold applies live | `alert_threshold_reload_applies_without_restarting_service` | Local test |
| Actual watcher final Arc drop joins worker | `watcher_final_drop_joins_worker_on_vm` | Compile-only, pending VM |
| Exact G4 watcher windows and overflow samples | Frozen collector/state seams above | Blocked |
| In-flight procscan cancellation | Cancellation seam above | Pending coordinator |

## Pending VM

Owned `watcher_final_drop_joins_worker_on_vm`, behind `privileged` and `TIDEMARK_DEPLOY_TEST_VM=1`, starts a real watcher on the VM root mount, observes a published watcher and one additional `tidemark-watch` thread, disables and reenables the watcher through config reload (observing worker cleanup/recreation), requests shutdown, joins the service, and waits for the thread count to return to baseline with no retained watcher. No privileged test ran here. The other half's `daemon_reload_and_shutdown` remains untouched and pending its VM gate; this attempt compiles the whole privileged test set.

Unprivileged service tests use temporary config/socket paths, state deadlines and channel acknowledgements; no sleeps with assumed timing and no filesystem-dependent disk-size literals. The live library has no public fake ProcScanner source or Watcher factory; deterministic service coverage uses empty-device collection, bad-mount rejection and injected minute rows instead.

## Validation

Finish-check results and exact logs are recorded in the canonical attempt output `library/` and `report.md`. All cargo commands run offline against the host cache with Rust 1.98.0, `RUST_TEST_THREADS=2`, and `timeout 1500`. Privileged execution, exact watcher-window acceptance and the frozen-interface follow-ups above remain unresolved irrespective of local build results.


Final local results: format, workspace Clippy with warnings denied, workspace tests (516 passed, zero failed, 13 existing ignored), privileged-test compilation, and warnings-denied rustdoc exited zero. `tools/ci.sh` exited zero on the final source tree and repeated format/Clippy/tests/privileged compilation. Cargo-deny was explicitly skipped because it is not installed; dependency-policy validation and VM execution remain pending. Initial formatting/test-helper lint findings were corrected within owned files. All source changes precede final CI.

## Definition of Done evidence

- Owned paths only; no shared-file or dependency edits.
- Owner tags, public rustdoc, formatting and Clippy checks pass locally.
- Seven owned unprivileged service tests pass; one owned privileged test compiles, pending VM.
- Handoff, assumptions, signature inventory and exact frozen-interface requests provided.
- Full acceptance remains incomplete at the listed seams; this is not a verified task-success declaration.
