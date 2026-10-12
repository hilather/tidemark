# NOT READY — 2 BLOCKER, 9 SHOULD-FIX, 0 NIT

Review target: **`bd69e06bb1d1aeda2e243b4e861a5c65a610a3a4`**, comparison base `031eb443a84992f447522e5ed641eac37a8ac21d`. All source locations refer to that candidate. This report is evidence for review, not an acceptance or integration decision.

The deliverable checkout starts at `d33369c3ade14757e4b6c31625bdbbcf2fede260`, which contains later fixes. Its daemon production sources match the candidate, but catalog code, tests and plan text differ. Inspection/builds use an unmodified candidate archive under the approved checkout's ignored `target/review-candidate`. Later catalog-path/packaging fixes are not credited to this candidate. Only this report changes in the repository.

Scope: lifecycle, state, scheduler, firstboot, six core services and their tests. API, CLI, metrics, scan and store dependencies were traced only where core behavior depends on them. No privileged test, VM connection, host service management, mount/LVM command or system-directory write was performed.

Artifacts referenced below are in `library/` under this attempt's canonical worker-output directory (`attempt-076eedab22672ca3f9e815e56b37b039ac0c179885359a70df803301cde29485`). Standalone probes link the actual candidate libraries without changing source. Assertions in these probes demonstrate existing defects, not successful fixes.

## Findings

### B1 — BLOCKER — Recorder waits outlive the shutdown deadline

Reference: `finding:w6-core-recorder-shutdown`.

**Location:** `crates/tidemarkd/src/scheduler.rs:367`, `:586`, `:1068`; `crates/tidemarkd/src/lib.rs:592`.

Recorder persistence allows eight seconds to enqueue plus eight seconds for acknowledgement; series persistence has three independent eight-second waits. None observes cancellation/deadline. Scheduler joins the worker indefinitely. Lifecycle gives producers nine seconds, then drops unfinished handles: in-process callers can return with workers alive, while executable exit kills them without establishing cleanup/flush completion.

Concrete scenario: a full writer queue becomes writable seven seconds after SIGTERM; the recorder still enqueues `ScanStarted` and waits another eight seconds for its acknowledgement. No blocked kernel syscall is required. `scheduler-backpressure.log` records ScanStarted enqueued 7000 ms after shutdown, scheduler still alive at 9200 ms, and join only at 15250 ms. The probe verifies the actual queued message. This is distinct from known LVM/procscan cancellation and forced-exit temporary cleanup limitations.

**Fix:** carry cancellation and one shared shutdown deadline into recorder operations; interrupt queue/ack waits, distinguish uncertain persistence from uncommitted work, and join workers before returning. Do not increase timeouts or detach. **Files:** `crates/tidemarkd/src/{scheduler,lib}.rs`, `crates/tidemarkd/tests/w6a2/{runtime,privileged}.rs`. Adapt both `Recorder` constructors (`launch` and source-local `recorder` helper). Extend `callbacks_wait_for_persistence_and_propagate_writer_error` and cancellation coverage with stalled enqueue/ack cases; no existing test requires detachment.

### B2 — BLOCKER — A valid root-list reload can stop the daemon

Reference: `finding:w6-core-root-reload-cap`.

**Location:** `crates/tidemarkd/src/state.rs:159`, `:239`; `crates/tidemarkd/src/scheduler.rs:1014`, `:890`.

Reload retains statuses for removed roots. Once historical statuses reach 128, publishing a newly configured root fails. The scheduler exits and its drop guard sets global shutdown, even though neither current configuration exceeds its bound.

**Reproduced:** start 128 `off` roots, replace with one different `off` root. `scheduler-inventory.log`: `replace_config=Ok(())`, `shutdown_after_valid_reload=true`, `Err(Unavailable("complete inventory exceeds 128 records"))`. Smaller changes accumulated over uptime can do the same. This is separate from the known watcher-component removal seam.

**Fix:** retire removed inactive configured-root statuses, preserve active scans until completion, and reserve current-root capacity against bounded manual history before accepting reload. Contain admission errors. **Files:** `crates/tidemarkd/src/{state,scheduler}.rs`, `crates/tidemarkd/tests/w6a1.rs`, `crates/tidemarkd/tests/w6a2/runtime.rs`. Extend `failure_health_survives_idle_other_root_and_reload_until_recovery`; add exact-capacity replacement and cumulative churn. Public signatures can remain unchanged.

### S1 — SHOULD-FIX — The single writer can exit while still reported healthy

Reference: `finding:w6-core-writer-supervision`.

**Location:** `crates/tidemarkd/src/services/tswriter.rs:112`; `crates/tidemarkd/src/lib.rs:384`, `:505`.

Batch rollover calls `flush_fs(...)?`, bypassing normal failure publication and exiting the worker. Lifecycle supervises other handles but checks the separately retained writer only at shutdown. Storage errors or a writer panic can leave `tswriter=ok` with persistence disconnected.

**Reproduced:** gate the initial worker receive, enqueue 20,000 filesystem rows and one more, then trigger a checked store error using an out-of-range gauge. `services-probe.log` records `Err(Store("integer outside storage range: 18446744073709551615"))` and `writer_health_after_exit=ok`. This is explicit fault injection, not a claim that real statvfs returned that gauge; disk-full/write errors follow the same propagation branch. Missing lifecycle supervision is source-verified.

**Fix:** route rollover errors through normal health/recovery handling and supervise writer exit/panic independently of ordered writer shutdown. **Files:** `crates/tidemarkd/src/services/tswriter.rs`, `crates/tidemarkd/src/lib.rs`, `crates/tidemarkd/tests/w6a1.rs`. Add rollover recovery and worker-death tests; retain `writer_batch_flush_shutdown_and_ack` and `repeated_gauge_keys_and_restart_do_not_poison_writer`. No payload change needed.

### S2 — SHOULD-FIX — Failed collectors continue alerting from stale measurements

Reference: `finding:w6-core-stale-alert-input`.

**Location:** `crates/tidemarkd/src/services/sampler.rs:79`; `crates/tidemarkd/src/services/alerts.rs:166`; `crates/tidemarkd/src/state.rs:674`.

Sampling failure changes health without marking that mount's measurement unavailable. The alert wrapper delegates filesystem methods unchanged; `fs_sample` accepts arbitrarily old samples. Thus failed collection continues supplying hysteresis evidence, contrary to `07` §6.2. LVM likewise retains its last report while alert input checks enablement only (`alerts.rs:194`, `state.rs:722`). Cached display data and currently evaluable measurements need separate availability.

**Reproduced:** a 95% sample at time 0, then sampler degradation with no further sample, emits Firing at 10 and Renotify at 30. At 86,400 the reader still returns timestamp 0 (`services-probe.log`). This uses the real state and alert engine; production filesystem delegation is identical. It does not re-report the known thin-pool inventory diagnostic.

**Fix:** track per-source/per-mount collection availability; retain display history but return None to alerts after failure, restarting continuity on recovery. Persistence congestion or another mount's failure must not hide a valid measurement. **Files:** `crates/tidemarkd/src/state.rs`, `crates/tidemarkd/src/services/{sampler,lvm,alerts}.rs`, `crates/tidemarkd/tests/{w6a1,w6a2s}.rs`. Add successful→failed→recovered tests through the actual wrapper. Fake-source unavailable and retained-LVM-display tests remain valid.

### S3 — SHOULD-FIX — Procscan holds the published reader lock during collection

Reference: `finding:w6-core-procscan-reader-lock`.

**Location:** `crates/tidemarkd/src/services/procscan.rs:86`; `crates/tidemarkd/src/state.rs:592`, `:605`.

Service and state share the same `Arc<Mutex<ProcScanner>>`. `/proc` enumeration holds this mutex; phantom/writer readers and phantom alert evaluation synchronously lock it. A slow successful pass therefore blocks previously available reports, contradicting short critical sections and the under 50 ms report requirement. This reader-availability defect is additional to the known cancellation/invalidation seam.

**Reproduced:** holding that exact published mutex prevents the real phantom reader returning for 100 ms; release immediately permits its result (`state-probes.log`). This proves synchronization behavior, not real procfs performance.

**Fix:** keep the mutable collector private and publish bounded immutable completed phantom results and writer history, preserving arbitrary positive writer windows. Coordinate the shared publication surface. **Files:** `crates/tidemark-live/src/procscan.rs`, `crates/tidemarkd/src/state.rs`, `crates/tidemarkd/src/services/procscan.rs`, `crates/tidemark-live/tests/w5b/unit.rs`, `crates/tidemarkd/tests/w6a1.rs`. All current `Publication::Procscan` constructors are the service and `scanner_measured_empty` / `phantom_measurement_uses_allocated_byte_oracle`; adapt each and add concurrent blocked-collector coverage.

### S4 — SHOULD-FIX — LVM retry backoff uses seconds instead of minutes

Reference: `finding:w6-core-lvm-backoff`.

**Location:** `crates/tidemarkd/src/services/lvm.rs:11`, `:90`, `:102`.

Default failures retry after 1, 2, 4, …, 300 seconds, capped by the healthy interval. `07` §1 requires 5, 10, 20, …, 60 minutes. This initially increases command frequency up to 300-fold and never backs off beyond normal cadence. Writer congestion also triggers recollection as a collector failure.

**Reproduced:** `services-probe.log` prints `[1,2,4,8,16,32,64,128,256,300,300,300]` for the default 300-second interval.

**Fix:** implement default 300/600/1200/2400/3600-second retries, define nondefault cadence consistently, separate persistence congestion, reset on collection recovery. **Files:** `crates/tidemarkd/src/services/lvm.rs`, `crates/tidemarkd/tests/w6a1.rs`. `lvm_disabled_and_backoff` explicitly asserts the incorrect one-second/four-second behavior and must change; add sequence/recovery/backpressure tests in the service module. No signature change needed.

### S5 — SHOULD-FIX — Wall-clock corrections change elapsed operational intervals

Reference: `finding:w6-core-wall-clock-intervals`.

**Location:** `crates/tidemarkd/src/scheduler.rs:217`, `:927`, `:970`, `:1028`; `crates/tidemarkd/src/services/tswriter.rs:96`, `:157`.

Retry/poll deadlines and firstboot elapsed waiting use Unix time; hourly compaction uses `clock()+3600`. Forward corrections retry early; backwards corrections suspend polling/compaction until old deadlines. Daily windows need wall time, but operational intervals do not (AGENTS rule 12).

**Reproduced:** failure at wall 100 sets retry 130. Advancing wall immediately to 3700 causes another failure in 94 ms elapsed, then retry 3760 (`scheduler-retry-step.log`). Source also establishes that rollback 100 → 0 after `schedule_at=101` suppresses ordinary refresh until 101 and prolongs firstboot's two-minute wait.

**Fix:** separate injectable wall and monotonic clocks for calendar/history versus operational deadlines/runtime cadence/compaction. Persisted restart catch-up can retain wall timestamps. **Files:** `crates/tidemarkd/src/scheduler.rs`, `crates/tidemarkd/src/services/tswriter.rs`, `crates/tidemarkd/tests/w6a1.rs`, `crates/tidemarkd/tests/w6a2/{runtime,schedules}.rs`. Preserve wrappers or update every caller: scheduler `start`, runtime `Harness::with_clock`, writer `start`, `compaction_uses_injected_clock`. Adapt `retry_sequence_uses_injected_time_and_caps_at_one_hour`, `scheduled_failures_back_off_with_injected_clock_and_reload_resets`, and `busy_keeps_fixed_deferral_and_does_not_increment_failure_attempts` to advance monotonic separately. Shared Unix `Clock` need not be repurposed.

### S6 — SHOULD-FIX — Config parsing rereads instead of parsing the hashed bytes

Reference: `finding:w6-core-config-bytes`.

**Location:** `crates/tidemarkd/src/lib.rs:863`.

`load_config` reads bounded bytes, reopens `/proc/self/fd/N` through `Config::load`, then rereads and compares against its first buffer. In-place A → B → A changes can parse B while first/last comparison succeeds and hash identifies A. The fd prevents pathname replacement, not content mutation. The available `Config::from_toml_bytes` is unused, missing R3-6. This interleaving is source evidence; no timed config-write exploit was executed.

**Fix:** parse `Config::from_toml_bytes(&bytes)` and hash that same buffer, removing redundant reopen/rewind. **Files:** `crates/tidemarkd/src/lib.rs`, `crates/tidemarkd/tests/w6a1.rs` or existing source-local config tests. Helper signature and `run_args`, logging, `run_controlled`, reload callers remain unchanged; no core/deploy edits needed. Add a captured-byte parse/hash regression.

### S7 — SHOULD-FIX — A late sample resurrects a removed mount

Reference: `finding:w6-core-stale-generation`.

**Location:** `crates/tidemarkd/src/state.rs:169`, `:204`; `crates/tidemarkd/src/services/sampler.rs:57`, `:68`.

Reload prunes old mounts, but a sampler pass holding old config can publish afterwards. Publication checks neither generation nor membership, reintroducing the removed mount. Future passes never visit it, and eviction occurs only on another sample for that key, so it persists indefinitely.

**Reproduced:** prune `/old` by reloading `/new`, then publish the old in-flight result: effective config contains `/new`, `fs_samples()` contains `/old` (`state-probes.log`). Source establishes the service's possible interleaving.

**Fix:** atomically check generation/current mount when publishing, normally discarding stale passes. **Files:** `crates/tidemarkd/src/state.rs`, `crates/tidemarkd/src/services/sampler.rs`, `crates/tidemarkd/tests/w6a1.rs`. Add barrier-controlled reload/sampling coverage; `lifecycle_reload_and_shutdown` currently changes interval only. A dedicated helper preserves generic `Publication::Fs`; if its payload changes instead, update the sampler and all constructors in `tests/w6a1.rs`, `tests/w6a2s.rs`, and `tests/w6b.rs`.

### S8 — SHOULD-FIX — Internal state-space alert exceeds publication capacity

Reference: `finding:w6-core-alert-cap`.

**Location:** `crates/tidemarkd/src/state.rs:232`; `crates/tidemarkd/src/services/alerts.rs:150`; `crates/tidemark-live/src/alerts/mod.rs:292`.

Engine capacity allows 10,000 configured rule/subject states plus a separately stored internal state-space alert. State rejects more than 10,000 active summaries. Publication at that valid boundary exits the alerts worker after notification attempts but before updating active state.

**Reproduced:** a complete 10,000-directory growth inventory plus space refusal produces 10,001 events/active summaries, then state rejects it (`state-probes.log`).

**Fix:** reserve an additional active-summary slot for the internal alert, keeping configured-state and diagnostic limits separate. **Files:** `crates/tidemarkd/src/state.rs`, `crates/tidemarkd/src/services/alerts.rs`, `crates/tidemarkd/tests/w6a1.rs`; also `crates/tidemark-live/src/alerts/mod.rs` if exporting a shared capacity constant. Add engine-to-publication boundary coverage. Existing rejection of 10,001 diagnostics should remain; it does not require rejecting valid active summaries including the internal alert.

### S9 — SHOULD-FIX — Reload accepts a schedule rejected by its consumer

Reference: `finding:w6-core-reload-schedule-validation`.

**Location:** `crates/tidemarkd/src/lib.rs:660`; `crates/tidemarkd/src/scheduler.rs:79`, `:998`; `crates/tidemark-core/src/config.rs:741`.

Core validation accepts positive `Duration`; scheduler additionally requires rounded seconds to fit `i64`. Reload swaps config after only core/bounds validation. Scheduler then fails and initiates shutdown instead of preserving the old config.

**Reproduced:** `every 10000000000000000000s` has no core issues, is accepted by replacement, then produces `Invalid("interval too large")` and global shutdown (`scheduler-large-schedule.log`). Production reload uses these same validations without parsing the scheduler representation.

**Fix:** validate scheduler representation/windows before replacement, consistently for startup/reload/check-config. **Files:** `crates/tidemarkd/src/lib.rs`, `crates/tidemarkd/tests/w6a1.rs`, `crates/tidemarkd/tests/w6a2/schedules.rs`. Reuse existing parsers without changing frozen keys/types. Add a reload rejection test asserting unchanged config/generation and continued operation; existing invalid-config cases remain valid.

## Lifecycle, scan trace and safety assessment

Startup validates config, verifies/creates eligible private state directories, retains the checked fd, preflights storage and initializes completed-scan history. Only recognized corruption triggers recovery; recovery filenames are logged. Namespace failure disables the watcher with an error. Collector source failures generally degrade; fatal path/storage/spawn/API-bind failures request cleanup of started services. API bind precedes READY. An active socket refuses a second instance, but after producer startup; no earlier state-directory singleton guard was found. Concurrent different-socket instances and recovery contention were not executed and are not promoted to new findings.

SIGHUP/API reload share the callback. State/socket relocation is refused. Services observe generations at boundaries; relevant config changes rebuild watchers/procscan/alerts as described in their handoffs. Running scans keep admitted config. Core-invalid config preserves the previous one; S9 is a consumer-validation exception. Log-filter reload is neither implemented nor among the listed live-reload settings. Other service exits are detected by lifecycle polling, but writer supervision is absent (S1).

Scheduled admission creates a configured `RawPath` with mode/throttle overrides absent. API admission preserves raw request bytes and overrides. `launch` copies config, substitutes operational state_dir **`/proc/self/fd/<retained-fd>/.`**, selects requested mode before configured Auto/Walk/Bulk, and constructs `ScanRequest` with `threads=None`, no extra excludes/annotations and `keep_partial=false`. Requested throttle maps gentle/normal/max; absent override leaves configured root selection to orchestration.

`run_scan` alone owns host locking, gate creation, space guard and retention. The daemon does not pre-lock. The final dot fixes the original no-follow ENOTDIR while retaining the verified fd. Scanner component-relative opens enforce no-follow, charging attempts/retries and O_NOATIME fallback. No new scanned-file-content read was found. Gate cancellation does not cancel recorder waits (B1).

`scan_started` captures the predecessor while holding the host lock and waits for persistence. `scan_finished` waits for completion persistence, computes growth before predecessor deletion, refreshes/publishes metrics, then takes dirtiness for that root. Series follows, then orchestrator retention. Known unavailable-growth invalidation and unit-only Flush acknowledgement gaps remain; these are not new findings. Manual conflicts return Busy; scheduled roots are reconsidered serially with no unbounded queue.

Firstboot requires staging, waits for observations or the deadline, and atomically writes `deploy/firstboot.json` through the shared retained-handle helper, syncing file and parent. Output is capped at 1 MiB. Healthy requires each core component exactly once and ok. Same-hash suppression and no rewrite on reload are implemented; one-shot error/no-reload-rewrite limitations are disclosed in W6-A2a. S5 affects the elapsed deadline. Duplicate helper inclusion and SQLite leaf-substitution limitations remain known.

Owned state facts are cloned under one read lock; data and health publish under one write lock after admission checks. No explicit SQLite I/O or channel send was found under that global lock. Collector handles remain mutable (S3 and the known atomicity seam). Normal watcher removal retains a service Arc until after removing state ownership. Final-handle destructor latency still needs runtime evidence. No new production unwrap/expect panic, unsafe block outside sys, depth-recursive traversal or unbounded daemon collection was found. Config/state/report and bounded procfs reads are distinct from forbidden scanned-file contents.

### Bounds and channels

| Inventory/channel | Bound and handling |
|---|---|
| Time-series message queue | 64; accepted batches ≤20,000 rows; ordinary producers try_send and report drops |
| Scheduler command queue | 1, plus one active worker; manual conflict Busy |
| API start reply, scheduled dummy reply, scan start/finish ACK, series Flush ACK, lifecycle Flush ACK | Each bounded(1) |
| API dependency | 32 connection/handler permits; dispatch response bounded(1); independent API internals otherwise out of scope |
| Webhook dependency | 1,000-event deque, one worker |
| Mount/root/watcher/component/error maps | 128 principal entries each; historical key retention causes B2 and known watcher seam |
| FS rings | ≤7,201 samples/mount; two-hour eviction on publication; same-second replacement; rollback clears |
| LVM | ≤128 mapped mounts, ≤10,000 VGs |
| Growth/window facts | ≤1,024 keys each, ≤10,000 rows/key; growth fetch ≤10,001, depth ≤128 |
| Active alerts/diagnostics/history | 10,000 each; history drops oldest; active mismatch S8 |
| Degraded reasons | 128 distinct strings |
| Sampler persistence | ≤128 rows/pass; first whole-second persisted gauge wins |
| Watcher drains | ≤60×3×1,000 source rows/collector; top 50/scope/minute persisted, ≤9,000 selected |
| Procscan persistence | Collector-owned finite caps; chunks ≤20,000 rows |
| Config/firstboot output | 1 MiB; firstboot boot timestamp procfs read ≤16 MiB |

Cardinality bounds do not establish the memory budget: cloned views, large growth inventories and many collectors still need measured RSS acceptance.

## Requirements in scope

Known gaps remain visible as gate context without duplicate findings. Names/methods are assessed at this candidate, not at the newer deliverable baseline.

| Method/service/contract | Implemented, deviates or missing |
|---|---|
| run_args/main_entry/check-config | Basic validation/errors, journald or stderr implemented; downstream validation mismatch S9 |
| load_config | Bounded no-follow open implemented; R3-6 deviation S6 |
| prepare_state/path validation | Shared eligibility, safe creation, retained handles implemented |
| run signals | SIGTERM/SIGINT shutdown, SIGHUP reload and unregister wired |
| run_controlled startup/recovery | Ordering/corruption-only recovery implemented; real notification transport pending |
| reload_callback/replace_config | Generations, relocation refusal implemented; B2/S7/S9 and known collector seams |
| protect_config | Literal escaped scan exclude and watch/procscan state prefixes implemented |
| join_until/ordered shutdown | API, scan cancellation, producers, writer barrier ordering implemented; B1/S1; known LVM/procscan bounds |
| parse_schedule | off/every/daily and checked arithmetic implemented; S9 validation mismatch |
| parse_window | Half-open/wrapped windows; equal endpoints full day implemented |
| next_run/timezone seam | Never-scanned due, restart catch-up, DST gaps/folds implemented/tested; elapsed runtime defect S5 |
| ScanControl::start | Byte-preserving single admission, Busy, bounded reply implemented |
| ScanControl::cancel | Exact-root cancellation and no keep-partial implemented; callback wait gap B1 |
| ScanControl::status | Published statuses/root filtering implemented; historical capacity B2 |
| Recorder::scan_started | Predecessor capture and error-bearing persistence ACK implemented |
| Recorder::scan_finished | Growth→metrics→own-root dirty ordering implemented; known stale-growth invalidation gap |
| Recorder::dir_series | Bounded rows before retention; known unit-Flush error gap |
| firstboot::needs_report | Staging/readability and same-hash suppression implemented |
| firstboot::build | Components, health, timestamps and next scans implemented |
| firstboot::write | Safe atomic private report implemented; guest boot/ownership validation pending |
| LiveState publish/component/view_at | Owned facts/health atomic and bounded; S3/S7/S8 at collector/generation/boundary seams |
| LiveStateView::status | Components/modes/errors/uptime implemented; S1 supervisor omission |
| LiveStateView::now | Dependency trace only; LIVE-hot assembly deliberately excluded; S3 can block consumers |
| hot | Collector ranked-window interface present; LIVE-hot work excluded |
| watch_stats | Shared collector counters/modes exposed |
| clear_watch_dirty | Root-specific take after successful metrics publication |
| lvm_report | Shared reduced DTO, sorted mounts/VGs and retained report implemented |
| phantom | Measured/unavailable distinction implemented; S3 and known invalidation gap |
| writers | Arbitrary positive windows delegated to collector; S3 and known invalidation gap |
| active_alerts | Bounded sorted summaries; S2/S8 |
| alert_diagnostics | Bounded sorted unevaluable reasons; known missing producer diagnostics retained |
| alerts_history | Current-run persisted events only, known R3-L3 |
| fs_samples | Bounded latest gauges; removed-mount deviation S7 |
| AlertSource::fs_sample | Latest prior sample returned; unavailable-input deviation S2 |
| AlertSource::fs_rate | Exact fractional-duration filtering/shared regression; availability S2 |
| AlertSource::hot_dirs | Missing/stale facts None; alert wrapper restricts minute windows; known absent producer |
| AlertSource::watch_overflows | Exact observation lookup present; boundary producer missing, known gap |
| AlertSource::phantom_bytes | Disabled/error procscan masked; S3 blocking lock |
| AlertSource::lvm | Tracked inventory supplied when present/enabled; failed collection availability S2 |
| AlertSource::dir_growth | Per-depth inventory; known stale invalidation gap |
| AlertSource::scan_age | Observed never-scanned roots, windowed cadence ≥24 h implemented |
| LiveStateView::firstboot_inputs | Bounded component/watch/sample/next-scan facts supplied |
| LiveStateView::state_space_refused | Optional startup/scan space-guard fact exposed |
| LiveStateView::scans | Bounded published statuses; B2 historical saturation |
| LiveStateView::degraded | Bounded sorted retained reasons plus current nonhealthy components |
| LiveState::config | Shared effective config and monotonic generation; no subscription queue |
| LiveState::bind_storage | Retains checked directory fd |
| LiveState::storage_path | Directory-form fd path fixes ENOTDIR |
| LiveState::remove_watcher | Removes collector handle before restart; component removal remains known gap |
| LiveState::clear_watchers | Removes collector handles during shutdown |
| snapshot_metrics_handle | Actual shared cache used; no production caller found for legacy always-None snapshot_metrics() |
| Sampler service | Immediate statvfs/noatime/fallback/nonblocking persistence; S2/S7 |
| LVM service | Read-only collector/disablement; S2/S4; cancellation known |
| Writer service | Single bounded writer, batching, ACKs, compaction; S1/S5 and known Flush seam |
| Watcher service | Namespace refusal, collectors, completed drains, diagnostics; exact windows/perf/removal gaps known |
| Procscan service | Device mapping, intervals/reload, batching, fds/writer diagnostics; S3; cancellation/invalidation known |
| Alerts service | 30-second monotonic wait, library hysteresis/delivery; S2/S8; rebuild/reset seam known |
| Metrics names/exporter | Other reviewer owns full contract audit; core cache refresh/order traced only |
| RPM/systemd/tmpfiles/config/dashboard/rules | Other reviewer; newer packaging fixes not credited to candidate |

## Acceptance and gate checklist

| Requirement | Result | Evidence/limit |
|---|---|---|
| Unsafe state refused before artifacts; private state creation | PASS for inspection/local refusal tests | w6a1 unsafe/nonprivate/symlink cases; actual ready path blocked by sandbox |
| Root-only, no group/startup uid gate | PASS for core inspection | Shared ownership/file policy; full API peer enforcement belongs to API reviewer |
| Descriptor-bound scans publish snapshots | PASS locally | Both candidate bound-storage runtime regressions pass; real socket dispatch fallback noted below |
| Never-scanned/windows/DST/single active scan | PASS on covered paths | Parser/runtime tests; S5 wall-step exception |
| Invalid reload retains old config; valid reload keeps service | FAIL | B2/S9 reproductions, S7 late publication |
| Workers supervised/joined within shutdown bound | FAIL | B1 measured 15.25 s; S1 silent writer; known LVM/procscan seams |
| Growth and series precede retention | PASS for order, data semantics incomplete | Recorder/orchestrator trace and retention tests; known growth/Flush gaps |
| Exact watcher-rule windows | FAIL — known gap | No WatchWindow producer; deliberately not re-reported |
| Unavailable rules never fire/resolve/renotify | FAIL | S2 reproduction |
| Bounded queues and inventories | PASS for finite bounds; FAIL for capacity transitions | B2/S8 |
| No scan symlink/content traversal; gate ownership | PASS for core wiring | Delegates to established scanner/sys helpers; not a full scanner re-audit |
| Firstboot safe/once/correct config identity | FAIL for identity guarantee | Safe writer present; S6; reboot readiness pending |
| Both Rocky 8.8/8.10 end-to-end suites | NEEDS-HUMAN | Compile-only in this attempt |
| RSS ≤150 MiB after 1 h idle, within 20% after scan | NEEDS-HUMAN | No daemon soak |
| 24 h flat RSS/fds, continuous metrics, clean logs | NEEDS-HUMAN | No soak; workload/metrics gate remains |
| Every API method/metric, RPM install/upgrade/erase | NEEDS-HUMAN / other reviewer | Outside core ownership |
| wave-6-done tag | NEEDS-HUMAN | Not created; this review grants no release approval |

## Build/test evidence

All builds used exact candidate sources, pinned Rust 1.98.0, a temporary host registry copy at `/tmp/tidemark-review-cargo`, `CARGO_NET_OFFLINE=true`, and `RUST_TEST_THREADS=2`. Long commands use `timeout 1500`. Logs are in the attempt's `library/`. Before cleanup, blob hashing matched all 458 tracked candidate files with zero mismatches (`source-identity.json`).

| Command | Result |
|---|---|
| tools/ci.sh (offline environment) | Exit 1. Source policy (140 owner tags), fmt and Clippy passed. Workspace tests stopped at w6a1: 13 passed, 5 failed. `ci.log` |
| cargo test --offline -p tidemarkd --test w6a2 --test w6a2s --test w6a3 -- --nocapture | Exit 0: 16+3+9 passed. Socket skips/fallbacks below; `daemon-remaining.log` |
| cargo build --offline --workspace --features privileged --tests | Exit 0; compile-only. `privileged-build.log` |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | Exit 0. `rustdoc.log` |
| Standalone candidate probes | All four binaries compiled and exited 0, asserting observed defects; logs/sources retained |
| cargo clean | Exit 0; removed 23,950 files / 13.7 GiB, including temporary candidate build tree. `clean.log` |

The five readiness failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. They hit readiness deadlines in this socket-denied sandbox; the remaining transport tests explicitly report Unix bind EPERM. CI is **not green**. Cargo-deny was not reached; no dependency audit is claimed. Daemon library tests ran in CI: 22 passed. CI fail-fast did not execute later workspace targets/doctests; the focused follow-up covers the remaining named daemon suites, not those unrelated targets.

`services-probe.log` establishes writer failure with healthy status, stale-alert firing/renotification and the LVM sequence. `scheduler-inventory.log`, `scheduler-retry-step.log`, `scheduler-large-schedule.log` establish the reload/clock defects. `state-probes.log` establishes removed-mount resurrection, reader blocking and alert capacity mismatch. `scheduler-backpressure.log` records enqueue at 7000 ms after shutdown, still running at 9200 ms, and join at 15250 ms. No code patch or privileged fault injection was required.

Tests that return early on socket denial count as passed in Rust. The bound-storage runtime test has an injected-root direct-dispatch fallback; real-binary/operator tests skip their transport behavior. Procscan service reload coverage uses no tracked devices. Fake alert-source None tests do not cover the stale real wrapper. The privileged reload test observes accepted/rejected log text, not changed alert firing; its scan checkpoint is a memory-admission log. Idle-scanner tests do not exercise concurrent latency. The LVM backoff test asserts the wrong values. None of these establishes production RSS, fd stability, interference or queue-pressure shutdown.

Known issues intentionally not counted again: candidate catalog process-local snapshot paths (fixed only in the later checkout), absent WatchWindow/hot publication and top_unit, watcher syscall/CPU work, stale-growth invalidation, unit-only series acknowledgement, uncancellable LVM/procscan, old procscan handles, alert-engine reset on changed rules, watcher-component removal, current-run alert history, duplicate deploy helper and SQLite leaf races, forced-exit temporary cleanup, bulk-size semantics, CLI rendering and packaging defects. Relevant handoffs and R3 decisions were read; unresolved assumptions remain unresolved.

## For the VM run

Run these commands as root in disposable Rocky 8.8 and 8.10 guests, from the exact candidate with its offline cache. Set the VM variable only in those guests. These are proposed checks; none ran here.

```sh
export CARGO_NET_OFFLINE=true RUST_TEST_THREADS=2 TIDEMARK_DEPLOY_TEST_VM=1
timeout 1500 cargo test --offline -p tidemarkd --test w6a1 -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a2 -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a3 operator::real_binary_scheduled_scan_publishes_then_exits_cleanly -- --exact --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a3 operator::real_operator_status_scan_top_now_reload_cancel_and_sigterm -- --exact --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s watcher_final_drop_joins_worker_on_vm -- --exact --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture
```

The first two must exercise readiness/notification/bound-storage assertions without socket skips. The scheduled binary must produce a real `.db` and exit 0 after SIGTERM. The operator test must query that snapshot from a separate CLI; the known catalog-path issue remains in this exact candidate and can refute that check. Reload/shutdown must finish within 10 seconds without `.db.tmp`; it still does not cover the stalled-writer B1 case. Watcher final-drop must join. End-to-end must see cross-unit hot activity and thin-pool data; known absent live publication remains an impediment at this candidate. These results must be attached separately for each kernel/guest.

To repeat the source-based probes in a guest, copy the four `.rs` evidence artifacts to a directory, set `TIDEMARK_REVIEW_PROBES` to it, build candidate unprivileged libraries, and link/run as follows. Run before enabling the privileged build variant so the dependency directory has one relevant daemon rlib:

```sh
timeout 1500 cargo build --offline --workspace
export TIDEMARK_REVIEW_DEPS="$PWD/target/debug/deps"
python3 - <<'PY'
import os, pathlib, subprocess, tempfile
src = pathlib.Path(os.environ['TIDEMARK_REVIEW_PROBES'])
deps = pathlib.Path(os.environ['TIDEMARK_REVIEW_DEPS'])
with tempfile.TemporaryDirectory() as tmp:
    root = pathlib.Path(tmp)
    clean = root / 'deps'
    clean.mkdir()
    for pattern in ('*.rlib', '*.so'):
        for path in deps.glob(pattern):
            (clean / path.name).symlink_to(path.resolve())
    specs = {
        'scheduler-probe': ['tidemarkd','tidemark_core','tidemark_store','crossbeam_channel'],
        'scheduler-backpressure-probe': ['tidemarkd','tidemark_core','tidemark_store','crossbeam_channel'],
        'services_probe': ['tidemarkd','tidemark_core','tidemark_live'],
        'state-probes': ['tidemarkd','tidemark_core','tidemark_live','tidemark_fs','parking_lot'],
    }
    for name, crates in specs.items():
        cmd = ['timeout','1500','rustc','--edition=2021',str(src/(name+'.rs')),
               '-L','dependency='+str(clean),'-o',str(root/name)]
        for crate in crates:
            libs = list(clean.glob('lib'+crate+'-*.rlib'))
            assert len(libs) == 1, (crate, libs)
            cmd += ['--extern', crate+'='+str(libs[0])]
        subprocess.run(cmd, check=True)
    for case in ('inventory','retry-step','large-schedule'):
        subprocess.run(['timeout','1500',str(root/'scheduler-probe'),case], check=True)
    subprocess.run(['timeout','1500',str(root/'scheduler-backpressure-probe')], check=True)
    subprocess.run(['timeout','1500',str(root/'services_probe'),str(root)], check=True)
    subprocess.run(['timeout','1500',str(root/'state-probes')], check=True)
PY
```

On this candidate, each probe prints the defect described above and exits 0. After fixes, replace the defect assertions with desired behavior: valid reload remains alive, unsupported reload is refused before replacement, a wall correction does not consume retry delay, shutdown joins within the bound, failed persistence reports unhealthy/recovery, unavailable collectors do not advance alerts, obsolete mounts remain absent, readers return completed data during collection, and all valid active alerts publish.

For RSS/fd observations, assume the dedicated guest's normal gate daemon is already running with its recorded test configuration and a disposable fixture root. The following observer performs a 24-hour series of 15-minute commands, each under the 1500-second command limit. It does not modify the daemon or fixtures:

```sh
export TIDEMARK_REVIEW_PID=$(pgrep -xo tidemarkd)
for segment in $(seq 1 96); do
  timeout 1500 sh -c '
    for sample in $(seq 1 15); do
      date -u +%FT%TZ
      awk "/VmRSS:/ {print}" /proc/$TIDEMARK_REVIEW_PID/status
      find /proc/$TIDEMARK_REVIEW_PID/fd -mindepth 1 -maxdepth 1 -printf . | wc -c
      sleep 60
    done
  ' >> tidemark-core-soak.log || break
done
```

For the one-hour idle acceptance use the first four segments without a mutating workload. For a separate 24-hour gate run, the gate operator must supply and record its fixture mutation workload and hourly scan configuration; the candidate's `xtask gen-fixture` is still a stub, so it is not presented as a working generator command. Confirm idle RSS ≤153,600 KiB, post-scan RSS returns to ≤1.2×baseline, no sustained fd/RSS growth, continuous metrics and no unexplained daemon errors. This observer alone cannot establish workload or metrics continuity.

Not checked: real syscall latency/overhead, writer/LVM children under a full system, HTTP webhook transport, mount/bind/ownership races, reboot/install behavior, one-hour/24-hour memory budgets, and actual hot-directory attribution. The corresponding acceptance remains NEEDS-HUMAN; compilation, synthetic probes and socket skips do not close it.
