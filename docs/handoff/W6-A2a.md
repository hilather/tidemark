# Handoff — W6-A2a: Scheduler, scan control and first-boot report

## Needs from coordinator

This is an implementation and validation evidence candidate, with unresolved integration gaps:

1. **Growth cannot be invalidated through the frozen state interface.** `Publication::DirGrowth` accepts only a complete vector; there is no unavailable/removal publication. The scheduler requests 10,001 diff rows, refuses to publish any prefix over the 10,000 inventory cap, reports a degraded `post_scan` diagnostic and retains watcher dirtiness. If that root already has growth facts, those old facts remain accessible to `AlertSource`; this does **not** satisfy R2-10's unavailable requirement. Needed addition: `Publication::DirGrowthUnavailable { root: String, reason: String }`, removing every growth depth for that root and exposing the reason. Use it before processing a new complete comparison and on missing predecessor, diff failure, cap/depth failure or partial publication. No state, live-library or contract file was edited to work around this gap.
2. **Series acknowledgements cannot report persistence errors.** The scheduler writes only through `Message::DirSeries` and waits on the writer's `Message::Flush` barrier. The barrier has a unit acknowledgement; writer health reports errors, but the recorder cannot reliably return the particular series write's persistence error. Needed addition: `Message::DirSeriesAck { root: RawPath, scan_id: String, ts: i64, rows: Vec<DirRow>, ack: crossbeam_channel::Sender<Result<(), DaemonError>> }`. Start/finish callbacks already wait for their existing error-bearing acknowledgements. Retention itself remains in `run_scan`, including its existing writable catalog/rebuild operations; the scheduler adds no second writer connection.
3. **Metrics implementation and real dirty clearing need W6-B.** Calls use the unchanged `SnapshotMetrics::refresh(&self, &Snapshot, u32) -> Result<(), DaemonError>` and publication interfaces. The current W6-B stub fails, so a complete scan persists and growth can publish, but metrics publication and dirty clearing remain pending that implementation. Successful cache refresh precedes publication/health publication and then dirty clearing. Existing shared caches are refreshed in place so later root scans preserve the other roots' cached data. Tests inject a successful refresh at this private seam; they do not assert that B remains a stub.
4. **Forced-exit stale temporary cleanup is missing.** Ordinary cancellation drops the snapshot writer, deleting its own `.db.tmp`; this is tested. Neither current daemon startup nor `run_scan` removes a temporary snapshot left by a process killed during an uninterruptible index/sync. Needed scan-owner helper: `cleanup_stale_tmp(state_dir: &Path) -> Result<usize, ScanError>`, invoked inside `run_scan` after acquiring its host lock, using bounded, no-follow state-directory operations. The scheduler deliberately never acquires that lock before `run_scan`. The daemon lifecycle owns its ten-second deadline; the scheduler logs before waiting for cancellation and does not claim an absolute kernel/SQLite shutdown bound.
5. **Privileged runtime and whole-daemon integration remain pending VM and downstream services.** The watcher, process-scan, alerts, API and metrics files are untouched. Their work cannot be evidenced by this scheduler-half run.
6. **Pinned storage paths need durable catalog-path mapping.** The worker follows A1 and passes `LiveState::storage_path()` to `run_scan`; with bound storage this is `/proc/self/fd/<fd>`. Code inspection shows that `run_scan` constructs `ScanEntry.file` from that path and its retention rebuild likewise uses that path. Those process-local file names cannot serve another CLI process or a restart reliably. The owned walk tests use unbound temporary storage; a real pinned-storage scan/restart has not been runtime-tested here. Needed coordinated library surface: `run_scan_with_storage_path(cfg: &Config, storage_path: &Path, req: &ScanRequest, recorder: &dyn ScanRecorder, cancel: &AtomicBool, progress: &(dyn Fn(&Progress) + Sync)) -> Result<ScanOutcome, ScanError>`, preserving the config's logical state directory for persisted artifact identities while using the separate pinned path for operations, and `Catalog::open_mapped(storage_path: &Path, logical_state_dir: &Path, rw: bool) -> Result<Catalog, StoreError>` (or equivalent owner design) for correct path mapping during rebuild and recording. This is a source-inspection finding, not an executed reproduction; no frozen interface or library was changed to hide it.

- Branch: existing attempt branch `hp-31d546de5cf64ed74fa0e19f0349ed9e6dcfc276c3358b3533817299d9e4aebf`; no branch creation, merge or push.
- Attempt: `attempt-31d546de5cf64ed74fa0e19f0349ed9e6dcfc276c3358b3533817299d9e4aebf`.
- Date: 2026-10-12. Status: implemented scheduler half with the gaps above; independent verification pending.

## What was built

Implemented local schedule/window parsing, DST-aware due-time calculation, overdue catch-up and one active scan across scheduled and API work. A bounded scheduler queue and worker pass cancellation to `run_scan`, which owns locking, the gate, space admission and retention. Recorder callbacks wait for writer persistence; the actual predecessor is read during `scan_started` while the orchestrator holds its lock, and growth/cache/dirty handling runs inside `scan_finished` before series and retention. Every root's initial status is populated before firstboot can become ready, including when the first scan will occupy the host slot. Firstboot waits for component observations or the injected two-minute deadline, then uses the existing safe build/write helpers once during startup. Live schedules/windows/excludes apply at the next admission; an active scan retains its admitted configuration.

## Shared-file edits

None. No manifests, lockfile, module declarations outside the owned test entry, library crates, other service files, plans, AGENTS.md or memory projections changed. No dependencies were added.

## Final public interfaces

Existing callable signatures are unchanged:

```rust
pub enum Schedule { Off, Every(i64), Daily(u32) }
pub struct Window { /* private local minute endpoints */ }
pub struct ScanControl { /* private shared admission/control */ }
impl Clone for ScanControl;
impl Default for ScanControl; // disabled/unavailable control
pub fn parse_schedule(text: &str) -> Result<Schedule, DaemonError>;
pub fn parse_window(text: &str) -> Result<Window, DaemonError>;
pub fn next_run(schedule: &Schedule, window: Option<&Window>,
    last_complete: Option<i64>, now: i64) -> Result<Option<i64>, DaemonError>;
impl ScanControl {
    pub fn start(&self, params: &ScanStartParams) -> Result<ScanStartResult, DaemonError>;
    pub fn cancel(&self, params: &ScanCancelParams) -> Result<ScanCancelResult, DaemonError>;
    pub fn status(&self, params: &ScanStatusParams) -> Result<Vec<ScanStatus>, DaemonError>;
}
pub fn start(config: Arc<Config>, state: LiveState, writer: tswriter::Sender,
    shutdown: Shutdown) -> Result<(ScanControl, ServiceHandle), DaemonError>;
```

Added a documented explicit-zone seam:

```rust
pub fn next_run_in_timezone<T: chrono::TimeZone>(schedule: &Schedule,
    window: Option<&Window>, last_complete: Option<i64>, now: i64,
    tz: &T) -> Result<Option<i64>, DaemonError>;
```

`Schedule::Every` stores elapsed whole seconds; `Daily` stores local minutes since midnight. Construct schedules through the parsers. Start returns the actual orchestrator scan ID after its start row is acknowledged. Busy is represented by the frozen `DaemonError::Scan("Busy".into())`; W6-A3 must map that to wire `busy`. Manual roots, including byte paths, use the orchestrator defaults when unconfigured. Status uses exact UTF-8 root labels and the SCR-002 escaped fallback for byte-only roots.

## Assumptions and limits

- Daily DST gaps run at the first valid minute after the requested wall time, with a documented three-hour gap cap. Folds use the first occurrence and do not rescan on the second occurrence after a same-day completion. Windows are half-open; equal endpoints mean the full day. Window search is bounded to three elapsed days.
- Positive fractional `every` durations round up to the next whole second because the frozen due-time clock uses Unix seconds. Arithmetic/date overflow returns an error.
- Manual scans bypass automatic quiet windows. Admission reserves the state's 128-root inventory for configured roots as well as observed manual roots. A single command/active slot prevents simultaneous starts; acknowledgement timeout cancels that request.
- Supervisor polling is 50 ms; idle catalog scheduling and firstboot probes happen at most once per injected wall-clock second. Failed/busy roots receive a 30-second retry boundary; this bounded per-root operational map does not prevent another root from scanning. No measurement state is duplicated outside LiveState.
- Post-scan growth is capped at 10,000 rows and depths 0 through 128, additionally subject to LiveState's 1,024 root/depth-pair cap. Every published depth inventory is complete; any failure leaves dirtiness uncleared and reports a component diagnostic. Prior-growth invalidation still requires the interface above.
- A firstboot write error is reported once; this scheduler does not retry unsafe-state failures or rewrite the startup report on config reload. Read/write eligibility and no-follow handling are delegated to the existing firstboot helpers.
- Series rows remain bounded to 20,000 and use the writer connection. A read-only query for the root row at the exact series second detects an already persisted observation, including after restart; the first observation wins. The root row is part of the orchestrator's largest-first series selection.
- Catalog ordering for scans starting in the same whole second is the existing `(started_at, scan_id)` ordering. IDs have a non-chronological suffix, so rapid manual scans can have a latest/retention order different from physical completion order. This library behavior is unchanged. The retention regression seeds an explicitly older catalog timestamp through the writer rather than waiting for a wall-clock tick or depending on a random suffix.
- Firstboot readiness expects `sampler`, `lvm`, `procscan`, `alerts`, `tswriter`, `scheduler`, `api`, `metrics` and the configured canonical `watcher:<mount>` names. Disabled/error reports count as observations; an aggregate disabled/error `watcher` observation also covers lifecycle namespace refusal or a disabled service.
- No privileged operations, reporting LVM commands, host metadata modifications, network browsing, service enablement, PostgreSQL integration or `tidemark` group work ran here.

## Acceptance evidence

| Criterion | Evidence | Status |
|---|---|---|
| Parse schedules/windows; restart catch-up; wait for window | `schedules::parsing_and_catchup`, `due_scan_waits_and_window_wraps`, `invalid_clock_is_fallible` | Local tests |
| Both DST transitions and explicit zones | `daily_gap_runs_at_first_valid_minute_and_fold_once`, `windows_across_both_transitions`; explicit Eastern 2026 transition fixture and UTC | Local tests |
| Second scan refused; CLI host lock respected | `runtime::second_start_refused_cancel_and_shutdown_leave_no_snapshot`, `held_host_lock_refuses_start` | Local tests |
| Scheduled walk and live reload | `runtime::scheduled_walk_and_reload_from_off`; complete fixture snapshot and measured `du_disk` oracle; `successful_short_cadence_has_no_failure_backoff`, `failed_root_does_not_starve_another_due_root` | Local tests |
| Byte-preserving manual root | `runtime::manual_byte_root_uses_frozen_request_and_status_contract` | Local test |
| No partial snapshot/tmp on cancellation; lock released | Cancellation test above; bounded worker state checkpoints | Local test; forced exit gap above |
| Callback waits for persistence and returns error | `scheduler::tests::callbacks_wait_for_persistence_and_propagate_writer_error` | Local test |
| Growth before cache publication; series coalescing | `growth_precedes_metrics_publication_and_series_coalesces_across_restart`; successful private cache-refresh injection | Local test; real W6-B pending |
| Growth/series before retention | `predecessor_survives_growth_and_series_then_retention_removes_it` | Local test |
| Firstboot component/deadline readiness and one report | `firstboot_waits_for_components_or_deadline_and_writes_once`, `firstboot_has_every_roots_next_due_before_first_admission` | Local tests |
| Actual signal reload/shutdown gate | `privileged::daemon_reload_and_shutdown` | Compile-only; pending VM |
| Truncated growth is unavailable | No prefix is published; unavailable invalidation requires frozen-interface addition | Blocked as described above |

## Pending VM

`daemon_reload_and_shutdown`, gated by `privileged` and `TIDEMARK_DEPLOY_TEST_VM=1`: launches the real daemon on temporary fixture paths with LVM/watcher/procscan disabled, sends SIGHUP after changing an alert threshold and observes accepted reload, sends another SIGHUP with invalid config and observes rejection, then sends SIGTERM during a throttled walk and asserts successful exit within ten seconds and no `.db.tmp`. This proves reload acceptance through lifecycle logs; actual alert firing against the changed threshold requires the later alert-service implementation/whole-daemon gate. No privileged test was executed on this workstation.

## Validation

All Cargo checks run offline with Rust 1.98.0 and the host cache; long commands use `timeout 1500`. Workspace test execution uses `RUST_TEST_THREADS=2`. After the final startup-ordering fix, format, workspace Clippy, workspace tests (509 passed, zero failed, 13 existing ignored), privileged-test compilation and warnings-denied rustdoc all exited zero. `tools/ci.sh` also exited zero; it skipped cargo-deny because the executable is not installed. Command logs are recorded in the canonical attempt output `library/checks.txt` and report. Cargo-deny and privileged VM execution remain pending.

## Definition of Done evidence

- Owned paths only; no shared-file/dependency edits.
- Public items documented and owner headers present; no unsafe code or production unwrap/expect.
- Five scheduler source-local tests and eleven owned integration tests pass locally.
- Privileged test written; compilation and workspace finish-check outcomes recorded in the attempt report.
- All unresolved acceptance/integration items are listed under Needs from coordinator; this note makes no verified-success claim.
