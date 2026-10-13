Commit reviewed: `1c8a0be4a30e2eee9ed498623a7d658b5b562ca7`.
Verdict: **NOT READY**.
Counts: **0 BLOCKER, 11 SHOULD-FIX, 1 NIT**. Recheck: **8 RESOLVED, 2 PARTLY RESOLVED, 0 NOT RESOLVED**. Findings are review proposals, not acceptance decisions.

The retained blind-review candidate is `5d3a437e2b509ff679cbb54f783b8ab25db595ef`, base `6a277e7f7d90266d139393d96c27695628f93027`. Its production source is identical to the checkout; differences are documentation and `crates/tidemark-cli/tests/w6a3.rs`. Exact-candidate test differences were inspected separately. Build results refer to the checkout, not an exact-candidate build.

## Recheck findings R1–R10

Source inspection of all ten is complete. Notes and handoffs are claims, not validation. Daemon source paths in this table are relative to `crates/tidemarkd/src/`; daemon `tests/` paths are relative to `crates/tidemarkd`; CLI-labelled source/test paths are relative to `crates/tidemark-cli`; xtask paths are repository-relative. Execution and guard evidence is recorded below.

| Finding | Disposition | Source and discriminating regression |
|---|---|---|
| R1 | RESOLVED | Sampler failures always publish unavailable attempts (`services/sampler.rs:148`, `:251`); process failures including typed inventory budget failures do likewise (`services/procscan.rs:117`); process freshness and failure epochs live in `state.rs:600`, `:1333` and `services/alerts.rs:85`. EMFILE sampler/process regressions exercise the real service loop without listeners. |
| R2 | RESOLVED | `api.rs` propagates Cancellation to `scheduler.rs:287`; checks surround blocking admission, reservations and enqueue; acknowledgment expiry cancels admitted work (`:352`). `request_expiring_behind_admission_locks_never_enqueues` and `short_request_deadline_cancels_admitted_scan_while_ack_is_held` are unguarded source-local tests. Cancellation is cooperative, not kernel preemption. |
| R3 | PARTLY RESOLVED | Presence-only passes retain last collection errors (`services/sampler.rs:137`, `:271`), covered by `failed_measurement_survives_presence_poll_until_success`. A new due multi-mount pass clears all old errors before every mount recovers (`:192`, `:228`); sibling finding below. |
| R4 | RESOLVED | Resolution outside the write lock (`state.rs:396`), generation recheck (`:410`), logging after unlock (`:435`). Blocked resolver/logger tests assert reader progress without sockets; their remaining synchronization weaknesses are discussed below. |
| R5 | RESOLVED | `firstboot.rs:74` throttles by injected elapsed time, independent of persisted wall timestamps. Wall-step/restart tests at `:203`, `:250` require no privileged facilities. |
| R6 | RESOLVED | Shared validation checks output/sidecar cross-product and parent identity (`lib.rs:1273`); exporter refuses nonregular destinations (`metrics.rs:633`). `tests/w6a6.rs:26` rejects check-config/reload collisions and preserves old config without sockets; live-socket preservation case is separately guarded. |
| R7 | RESOLVED | `xtask/tests/w6c.rs:239`, `w7c.rs:72` write advisories directly to stderr. Child-harness regressions at `:434`/`:189` assert successful-harness visibility and warning failure without sockets. Real ShellCheck remains separate. |
| R8 | RESOLVED | `services/watcher.rs:303` derives exact overflow endpoints independently of interior hot completeness. Missing-minute regression at `:669` expects overflow 2/5 and unavailable hot; replacement/duplicate/decreasing endpoints are covered at `:700`. No listener required. |
| R9 | PARTLY RESOLVED | Startup corruption recovery now waits for `scan.lock` (`lib.rs:1183`) and store checks retained exclusion (`catalog.rs:382`). Busy database/sidecar test at `lib.rs:226` asserts byte/inode preservation. However no state-wide writer lease spans the daemon's persistent writer lifetime or pre-API startup; scan exclusion alone cannot establish the recovery precondition. |
| R10 | RESOLVED | Snapshot mutations call definitive-absence-only deadline-bounded connection checks before and under scan lock (`cmd/snapshots.rs:70`, `:80`); scan helper classifies only missing/refused sockets as absence. Indeterminate transport coverage and remaining R9 ownership limitations are detailed below. |

## New findings

Unless a full crate path is given, daemon paths below are under `crates/tidemarkd/src/`. Evidence filenames refer to `library/` under canonical worker output `attempt-2cd12e24bff6534b086cadd2a06412ba65695ae25b8a737a7e4996e2d4269a59`. Probes assert defects, not successful fixes. Extracted-source probes are distinguished from complete-service or kernel execution.

### C1 — SHOULD-FIX — Recovery still lacks exclusion from persistent database writers

Reference: `finding:w7-correctness-recovery-writers`.

`lib.rs:1183` acquires only `scan.lock`; its guard ends when `startup_catalog` returns at `:1215`. Catalog/time-series connections survive without that exclusion (`:806`, `:836`), and the supervised writer starts at `:880-882`, before API ownership at `:991`. `crates/tidemark-store/src/catalog.rs:382` validates scan ownership but cannot enforce its stated requirement that other catalog/sample connections are closed. Neither `StateDir` nor the retained state directory descriptor is a writer lease.

**Observed:** `recovery-writer-exclusion.rs/.log` copies the exact startup/recovery decision and error-classification functions and links the actual store/scan crates. A returned startup Catalog and live TimeSeries allow another ScanLock acquisition. After deliberate corruption of this private fixture, a second startup recovery moves the original database inode and replaces it while those connections and an original file descriptor remain live. This proves missing writer exclusion; it does not simulate a full daemon or prove loss of an acknowledged sample. A second daemon starting while an idle daemon's database is corrupt can reach this path before discovering the existing API owner. Offline rebuild during pre-API startup has the same unprotected interval.

**Change:** introduce a state-wide ownership lease covering every catalog/sample writer for its lifetime, distinct from scan serialization. Acquire that lease before storage open/recovery; require exclusive writer absence for renames/recreation. Use one order, state ownership then scan ownership, across daemon startup, direct scans, prune and rebuild. Keep bounded contention and definitive endpoint checks. Files: `crates/tidemark-store/src/state_dir.rs`, `catalog.rs`, `timeseries.rs`; `crates/tidemark-scan/src/orchestrator.rs`; `crates/tidemarkd/src/lib.rs`, `services/tswriter.rs`; `crates/tidemark-cli/src/cmd/scan.rs`, `snapshots.rs`; tests in daemon `lib.rs`/`tests/w6a4.rs`, store `tests/crash_recovery.rs`, CLI `tests/w6a3.rs`. Revise `healthy_catalog_does_not_wait_for_scan_lock` only as needed to preserve healthy scan contention without allowing conflicting writer ownership. Add two-process tests holding a real writer during corruption recovery and before API bind, checking inode/sidecar preservation. No existing test asserts that live writers may be displaced.

### C2 — SHOULD-FIX — A multi-mount pass can permanently certify false sampler recovery

Reference: `finding:w7-correctness-sampler-health`.

`services/sampler.rs:192-194` clears all previous measurement errors at the beginning of a due pass. The first successful mount immediately publishes sampler `ok` (`:222-243`) before later previously failing mounts have recovered. `firstboot.rs:127-132` uses that state; `:84-85` permanently retains a matching healthy report.

**Observed:** `sampler-multimount-probe.rs`, `sampler-probe-module.rs`, and `.log` use the verbatim sampler service with injected two-mount measurements, actual LiveState, and actual included firstboot code. B fails; on the next pass A succeeds while B is barrier-blocked. Sampler becomes `ok`, firstboot becomes healthy, and B's alert input is still unavailable. After B fails again, the persisted firstboot report remains healthy. Only probe fixtures required an explicit private directory mode; the initial fixture-mode failure is retained separately. This is a real false certification path, not just a transient display inconsistency.

**Change:** retain outcomes per mount until that mount actually succeeds, or publish aggregate sampler recovery only after the entire due pass completes while interim sample updates retain previous health. Add the two-mount blocked/fails-again/succeeds sequence plus persisted followup assertion. Files: `services/sampler.rs` and its source-local tests; `firstboot.rs` for the persisted-report regression. Keep the existing single-mount presence-only test; no old expectation needs false recovery.

### C3 — SHOULD-FIX — Dirty-directory cap loses invalidation and continuously creates new loss

Reference: `finding:w7-correctness-dirty-cap`.

`crates/tidemark-live/src/watch/mod.rs:982-985` calls only `rings.overflow(at)` once 200,000 dirty parents are retained. It neither records the omitted parent nor invokes `State::mark_loss` (`:215-218`), which would make each affected root conservatively dirty. The cap branch also runs for an already-known parent. Continuing traffic therefore creates fresh lossy minutes indefinitely even without new dirty entries.

**Observed:** `watcher-dirty-cap-probe.rs/.log` compiles the exact dirty-cap branch, State methods, Rings and component health condition with minimal DTO adapters. At cap, `take_dirty` for a newly changed subtree returns empty with `loss_pending=false`; sixteen minutes of existing-parent traffic leave all fifteen health buckets lossy. Event-free expiry does work. This is extracted aggregation evidence, not fanotify execution.

**Change:** distinguish existing parents from new admission; collapse cap exhaustion into bounded conservative root invalidation, clearing/compacting retained parents so later ordinary traffic can recover. Label dirty-cap loss accurately instead of generic transport recovery. Files: `crates/tidemark-live/src/watch/mod.rs`, `watch/rings.rs` and their source-local tests. Add cap/new-parent/existing-parent/root-invalidation/recovery assertions; current one-directory processing test at `mod.rs:1545` misses the case.

### C4 — SHOULD-FIX — Startup lock errors erase retryable resource-pressure types

Reference: `finding:w7-correctness-startup-pressure`.

`lib.rs:1198` maps every non-Busy ScanLock acquisition error to `DaemonError::Store(error.to_string())`; `:1203-1206` also stringifies non-Store errors from `lock.open_catalog`. `retry_start` retries only typed pressure (`:645-687`). Consequently descriptor or memory pressure in these new lock/metadata opens becomes an immediate fatal startup error, unlike neighboring database opens.

**Observed:** the independent process-local `--pressure` mode of `recovery-writer-exclusion` sets only its own NOFILE limit to 64 and holds descriptors until EMFILE. The exact startup function returns `Store("...Too many open files...")`; the exact `daemon_pressure` classifier returns false. No host limit changes. This proves classification at the startup seam, not an actual daemon restart loop.

**Change:** preserve `ScanError::Io` as `DaemonError::Io`, route `ScanError::Store` through `startup_store_error`, and keep Busy as its distinct bounded refusal. Apply to both acquisition and writable-open branches. Files: `crates/tidemarkd/src/lib.rs`, with injected/isolated regression tests there covering EMFILE/ENOMEM, eventual retry success, and Busy expiry. Existing Busy sidecar-preservation expectations stay valid.

### C5 — SHOULD-FIX — Generic watcher health never recovers after coordinator failures

Reference: `finding:w7-correctness-watcher-health`.

`services/watcher.rs:455-474` publishes generic component `watcher=degraded` after mount-table errors; success returns without changing it. Namespace failures similarly publish generic degraded/error (`:125-147`). Normal successful passes only update `watcher:<mount>` (`:231`). `state.rs:790` retains components by exact name without expiry.

A transient EMFILE followed by completely successful collection leaves generic status degraded indefinitely beside healthy per-mount components. This is verified control flow, not a forced live-service failure. Firstboot's required component set excludes watcher, so this finding does **not** claim it blocks firstboot.

**Change:** recover the coordinator component after successful mount-table and namespace checks while leaving per-mount loss/absence independently degraded. Files: `services/watcher.rs` and its source-local tests. Extend `mountinfo_emfile_retains_watcher_and_minute_history` (`:616`) to execute recovery and assert the generic component becomes `ok`; its present success assertion only checks a returned table.

### C6 — SHOULD-FIX — Old state views keep the watcher transport alive across replacement

Reference: `finding:w7-correctness-watcher-lifetime`.

`services/watcher.rs:114-122` says old marks are released before replacement, but only removes the state/service Arc references. `state.rs:193-207` clones watcher Arcs into each LiveStateView. Shutdown/join is tied to final Watcher drop (`crates/tidemark-live/src/watch/mod.rs:669-677`, `:1011`); the mount descriptor is also retained in that object and its worker (`:305-315`, `:377`, `:407`, `:501`). A reader holding an old view can therefore keep the old group, mark, worker and mount descriptor alive while a new watcher starts at the same path.

**Observed:** `watcher-view-lifetime.rs/.log` links actual LiveState and an injected watcher. After removing both service/state references, a Weak pointer still upgrades solely because the old view exists; dropping the view triggers final destruction. This proves ownership lifetime. The injected watcher has no kernel mark, so no real unmount or event misattribution is claimed. The source guarantees identity checking and new history, but does not guarantee prompt transport teardown. The ordinary-unmount consequence of retaining a mount directory fd is an inference pending VM; current VM tests use lazy unmount or stop the watcher before cleanup.

**Change:** separate service-owned transport resources from readable measurement snapshots; explicitly stop/join the old worker and release its group and retained mount descriptor before replacement, independent of reader lifetimes. Files: live `src/watch/mod.rs`; daemon `src/state.rs`, `services/watcher.rs`; daemon `tests/w6a2s.rs` and live `tests/w5a/privileged.rs`. Add a held-view replacement test and a VM normal-unmount/remount test checking marks/fds and no attribution to the retired identity. Preserve existing lazy-detach and exact-history reset tests; they do not assert the stronger teardown property.

### C7 — SHOULD-FIX — Guest watcher gate rejects valid mode and cannot pass its loss assertions

Reference: `finding:w7-correctness-watcher-guest-gate`.

Three verified mismatches in `tests/fault/03-fanotify-overflow.sh`:

- `:19` recognizes `fd_mount` and the two DFID-name modes; the new supported default `fd_filesystem` never satisfies readiness.
- `:146-153` demands component `ok` after 120 quiet seconds, but `services/watcher.rs:264-269` correctly retains degradation while any of fifteen minute buckets is lossy. The exact Rings/health probe observes degraded at 120 seconds and ok after the affected bucket expires.
- `:36` creates 100,000 distinct files while `tests/fault/check.py:88` requires no lossy history. The 200,000 shared scope/key/file tuple cap is consumed three times per identity (`watch/rings.rs:199-205`); a fast one-minute burst therefore truncates even with complete kernel event accounting and zero queue overflows. Existing distinct-cap tests assert 66,667 directory identities and truncation.

**Change:** accept `fd_filesystem`; separate transport recovery from retained history, checking current observations after 120 seconds and `ok` only after the actual affected bucket expires. For the no-loss workload, reuse identities or pace distinct identities below the cap while keeping the event-count workload, or explicitly assert aggregation truncation separately from transport loss. Files: `tests/fault/03-fanotify-overflow.sh`, `tests/fault/check.py`, `tests/fault/README.md`; update helper regressions in `xtask/tests/w7c.rs` to pin all accepted modes and the two recovery phases. Do not change production loss policy just to make the old test pass. Real shell/fanotify execution remains pending VM.

### C8 — SHOULD-FIX — Direct and daemon NoSpace error text still differs

Reference: `finding:w7-correctness-cli-refusal-parity`.

`crates/tidemark-cli/src/cmd/scan.rs:260-264` maps daemon NoSpace to a bare precondition; direct `run_scan` adds state-directory context at `:237-242`. Results are `scan: /: NoSpace` versus `scan: /: scan state: <dir>: NoSpace`, both exit 5. Checkout CLI `tests/w6a3.rs:966` and `:996` deliberately pin these different full lines. Source confirms the mismatch; both environment-sensitive runtime branches can return early here.

**Change:** use one human refusal formatter with the state-directory subject on both paths, preserving exit 5 and the frozen API error. Files: CLI `src/cmd/scan.rs`, `tests/w6a3.rs`. Replace the differing exact expectations with common equality and add an in-process mapping test so parity asserts without sockets or a real mount. The blind candidate has the same production mismatch but the earlier substring assertion; the checkout test change strengthens observations, not product parity.

### C9 — SHOULD-FIX — Human output still exposes Debug values and epoch timestamps

Reference: `finding:w7-correctness-human-formatting`.

`crates/tidemark-cli/src/render/now.rs:161-164` prints daemon LVM optional numbers using Debug (`Some(25.0)`, `None`); `render/symlinks.rs:30,35` prints Debug enum labels (`Dangling`). `cmd/watch.rs:98` prints raw `row.ts`. `crates/tidemark-live/src/watch/rings.rs:336-349` appends `(timestamp <epoch>)` to human detail. Namespace failure paths in daemon `services/watcher.rs:128,147` still print the Debug Result. Status/now render these detail strings unchanged except control escaping.

**Observed:** `cli-render-probe.rs/.log` calls actual compiled renderers without sockets and prints `Some(25.0) None None None`, `Dangling: 1`, and `(timestamp 1791888120)`. Foreground-watch and namespace-error cases are source evidence; no privileged event was induced.

**Change:** share ordinary percentage/unknown formatting with the direct LVM renderer, explicit lowercase labels, and UTC civil timestamps; format namespace success/failure by cause rather than Debug Result. Preserve JSON serializers and integer timestamps. Files: CLI `src/render/{now,lvm,symlinks}.rs`, `src/cmd/watch.rs`, `tests/w6a3.rs`, `tests/w3b.rs`, `tests/w3b/snapshots/w3b__symlinks_text.snap`; live `src/watch/rings.rs`; daemon `src/services/watcher.rs`. Update old epoch assertions in rings `:482` and CLI w6a3 `:1165`, retaining width/cause checks. Add no-socket present/missing-LVM and completed-watch-minute renderer tests; keep JSON goldens unchanged.

### C10 — SHOULD-FIX — Scoped tracing capture still has the candidate's process-global race in sibling tests

Reference: `finding:w7-correctness-tracing-test-race`.

The blind candidate correctly isolates `tests/w6a1.rs:1664` in an exact-test subprocess. The same global callsite-interest interaction remains in daemon library tests `mounts.rs:199-242` (`logger_emits_one_line_per_transition_and_device_changes_restart`) and `log_tests.rs:16-217` (`every_operator_message_formats_with_values_and_without_debug_fields`). Other tests in that binary call the same real sites without those scoped subscribers.

**Observed:** `tracing-sibling-probe.rs/.log` constructs the sole scoped dispatch, first logs the real mount site from a no-subscriber thread, then emits under the scoped subscriber: zero bytes captured. Rebuilding callsite interest under that subscriber yields 69 bytes. This reproduces the interference mechanism, not an assertion that this CI invocation happened to fail.

**Change:** run each capture assertion in its own exact-test subprocess as the repaired w6a1 test does; serializing only subscriber installers does not exclude other callers. Files: `crates/tidemarkd/src/mounts.rs`, `log_tests.rs`. Retain real-callsite and transition-count assertions. The store warning test is already process-isolated; other process-global patterns are inventoried below.

### C11 — SHOULD-FIX — A lock regression can deadlock its regression test; positive waits remain fragile

Reference: `finding:w7-correctness-test-synchronization`.

`state.rs:1519` gives publisher entry two seconds and `:1525` gives reader completion 200 ms. In phase 2, `:1527` synchronously calls `replace_config` before releasing the blocked publisher (`:1529`). If resolution is accidentally moved under the global write lock, the reader timeout is captured but the test then deadlocks: replacement waits for the lock and publication waits for release retained by the replacement thread. The outer CI timeout, not the regression assertion, ends the run.

The single-mount R3 regression also increments its pass counter at mount-table entry (`services/sampler.rs:429-431`) then sleeps 100 ms (`:471`); this does not prove the presence-only pass finished. A delayed regressed pass can leave the old degraded value through the assertion.

**Change:** always release/join workers before asserting failed progress, and move the phase-2 reload to a controlled worker so cleanup remains possible under broken locking. Use an explicit end-of-pass barrier for sampler. Raise must-happen scheduling waits to at least ten seconds while retaining intentionally short product deadlines through controlled entry. Files: `crates/tidemarkd/src/state.rs`, `services/sampler.rs`; the inventory below identifies other source/test files needing corresponding deadline/handshake edits. Preserve stale-generation and lock-held-until-reader-completion assertions. No product lock change is requested.

### N1 — NIT — Rate warmup diagnostics use daemon age instead of measurement history

Reference: `finding:w7-correctness-rate-warmup-detail`.

`state.rs:879-884` classifies warmup by daemon uptime less than the requested window, while actual rate readiness requires at least 300 seconds of samples from the current identity (`:1118-1128`). With a one-minute time-to-full window and only 60 seconds of healthy history, rates are correctly absent but `services/alerts.rs:175-195,225-235` reports a fault and degrades the alerts component. A late remount has the same diagnostic issue.

**Observed:** `rate-warmup-probe.rs/.log` uses the actual exported alert service and obtains `alerts=degraded`, `window rate unavailable or nonfinite` at 60 seconds. There is no false numeric rate or false firing.

**Change:** derive expected warmup from the same per-identity sample readiness and mandatory five-minute minimum, still treating failed/stale collection as a fault. Files: `state.rs`, `services/alerts.rs` tests, extending `rate_window_warmup_requires_a_healthy_sample_and_expires` with one-minute and late-remount cases. Its current one-hour test should remain but must not encode daemon uptime as a substitute for sample readiness.

## Lifecycle and behavioral cross-checks

### R2 admission, cancellation and records

API `dispatch_controlled` calls `start_for_request`; scheduler admission checks the absolute request budget before/after active/failure/state locks, after inventory reservation and immediately before enqueue (`scheduler.rs:287-352`). The atomic request phase arbitrates explicit cancellation against admission; admitted scan requests remain cancellable (`api.rs:99-141`). An expired reservation is released. After enqueue, acknowledgment waiting checks at most each 50 ms and again on receipt, sets the scan cancellation flag, and reports uncertain outcome (`scheduler.rs:356-395`). This repairs the demonstrated long-blocking late-admission path; it is cooperative cancellation with small check-to-operation scheduling intervals, not an atomic transaction with a wall clock.

For recording, `Recorder::scan_started` checks cancellation before persisting, persists the running row before replying (`scheduler.rs:656-685`), and does not abandon persistence merely because the request expires (`:453-497`, stopped uses daemon shutdown/deadline). `run_scan` checks cancellation before metadata phases and work. If cancellation precedes recording, no scan work is allowed to proceed to a successful unrecorded scan; if recording succeeded, `recorded` finalizes failed work (`crates/tidemark-scan/src/orchestrator.rs:1118-1144`). Lost writer acknowledgment remains honestly uncertain; a queued start can commit while its acknowledgment is lost, requiring startup abandoned-row repair. This is a known persistence limit, not a promise of zero running rows after all I/O failures.

New admission tests directly exercise API dispatch and hold real command reply senders; they do not run a real scan through that expiry. Complementary socket-free scheduler/outcome tests are `tests/w6a4.rs:177` (cancelled row and following scan), `:204` (noncancel failure), `:215` (shutdown-interrupted outcome repaired), plus `scheduler.rs:1665` callbacks/persistence and stalled queue/uncertain acknowledgment tests. No new test alone proves every admission-to-persistence interleaving. Listener transport tests add coverage only where bind exists.

### R9/R10 ownership and wait ordering

| Operation | Ownership held during mutation | Ordering and outcome |
|---|---|---|
| Daemon startup, corrupt/missing database | ScanLock held through recovery/cleanup, then dropped; no state writer lease | Read-only probe, nonblocking ScanLock attempts up to 30 monotonic seconds, then `Store("state directory in use by another tidemark process")`. This is not pressure, so startup ends before producers/API. Wait does not poll shutdown within those 30 seconds. |
| Daemon startup, healthy database | May open writable catalog with no ScanLock when busy | Cleanup is skipped. `healthy_catalog_does_not_wait_for_scan_lock` asserts this under one second. Writer opens/start precede API socket ownership. C1 identifies the uncovered persistent-writer interval. |
| `snapshots prune` / `rebuild-catalog` | Retained ScanLock spans the operation | Deadline-bounded absent-daemon connection check, nonblocking scan acquisition, second connection check. Rebuild uses checked recovery; prune ordinary writable catalog. Neither check issues an API request that waits for the scheduler. |
| Direct scan | ScanLock around recovery preflight, released; `run_scan` acquires its own lock | Two absent-daemon checks surround initial acquisition. Actual run uses ordinary catalog open, not recursive recovery; no self-acquisition of the same lock. A daemon may still start between absence checking and operation without a shared writer lease. |
| API ownership | Separate per-socket lock | Acquired after database/writer startup, without retaining startup ScanLock. It prevents duplicate socket ownership but does not guard earlier shared-state mutation or different sockets targeting the same state. |

No unbounded two-process lock-order cycle was found in these paths: acquisition is nonblocking or waits at most the stated 30 seconds; the CLI's second connection check is bounded to 11 seconds and requires no service reply. Avoiding a permanent deadlock does not establish correct writer exclusion. Busy sidecar/inode tests and store `checked_recovery_refuses_wrong_or_busy_scan_lock` genuinely exercise locks without sockets. CLI `scan_invalid_endpoint_never_writes_state` (`tests/w6a3.rs:431`) and `catalog_mutations_preserve_existing_corrupt_state_on_uncertain_endpoint` (`:1084`) assert ENOTDIR refusal and byte/inode preservation here. Backlog test `:476` requires bind; mode-000 permission test `:721` can skip as root and needs ordinary-user CI. These distinguish R10's transport fix from C1's remaining ownership defect.

### Watcher modes, windows and resources

Mode negotiation installs one preferred rich/filesystem mark, or mount fallback, rather than both (`crates/tidemark-live/src/watch/mod.rs:318-348`, `:708-725`). Mode/fallback tests assert the fallback closure is not called after success. Foreign event mapping uses one handle identification, a temporary reopen only for foreign provenance, then one path resolution; metadata remains from the original event descriptor (`watch/mod.rs:851-878`, `fdmode.rs:115-145`). Processing funnels each accepted event into one event increment and three aggregation scopes (`mod.rs:955-980`). Root fallback counts unmapped once and retains total/unit/command counts; exclusions still apply.

Source-local mapping/provenance and synthetic processing tests assert here. However `unmapped_event_keeps_totals_and_root_directory` directly calls `unmapped_path` and `record`, so removing the fallback wiring from `process` could leave that test passing. The real namespace/bind/chroot exact-three-event regression is privileged and skips when filesystem marks are unsupported. That proof remains VM work.

Identity transitions remove old publications/history and verify the opened descriptor matches the observed mount (`services/watcher.rs:114-188`). C6 prevents crediting the stronger claim of transport release before replacement. Existing `tests/w6a2s.rs:335-337` uses lazy unmount; `tests/w5a/privileged.rs:493-495` stops the watcher before cleanup. Neither establishes ordinary unmount while a watcher is active. No kernel event attribution across an actual remount was executed here.

Loss detail has at most eight latest causes and a fifteen-minute bucket cutoff (`watch/rings.rs:300-363`); per-mount health uses current retained loss, recovering, failed and current persistence-drop state, not cumulative read/overflow totals (`services/watcher.rs:264-279`). Finite losses age out by themselves. C3 creates new losses continuously; C5 concerns a different generic component. A retention-gap `drain_minutes` result can be locally lossy without updating retained bucket detail (`rings.rs:411`); this diagnostic limitation is source-observed, not a separate alert-availability defect. Hot input still requires every complete healthy, untruncated minute; overflow input requires consistent exact S/E totals. Duplicate/decreasing boundaries and replacement histories cannot reuse old counts. Alert window publication uses the final fact's exact end and freshness limits, including future/stale rejection.

The descriptor budget remains `clamp((soft - 512)/mounts,16,256)` (`lib.rs:619-640`). During filesystem-mode processing all event batch descriptors can coexist with **one extra reopened descriptor per processing watcher** (`watch/mod.rs:854-862`). That extra slot is not subtracted from the batch; it borrows the generic 512 reserve, in addition to fixed group/mount/eventfd resources. Existing batch/budget unit tests do not prove reserve sufficiency under simultaneous foreign events. This is bounded source accounting, not an observed EMFILE exhaustion result; require explicit extra-slot accounting or measured reserved headroom in the guest low-limit test.

### Rates, health and CLI boundaries

Filesystem rates for all requested windows require three usable points and at least five minutes of retained history from the current mount identity (`state.rs:1118-1128`, `:1306-1321`). `now` now uses the optional fitter result directly (`:986-990`); alert time-to-full, metrics rate and time-to-full consumers call that same source. The unguarded `now_rates_are_missing_until_five_minutes_for_every_window` tests 60/300/900/3600-second requests and the 299/300 boundary; metrics `tests/w6b.rs:155-182` checks omission. No numeric early-rate regression found. N1 concerns classification only.

Every inspected filesystem/process failure invalidates alert availability at publication, including typed inventory-budget failure; failure epochs replay a missed failure before recovered evidence. Process success has inclusive three-interval freshness, rejects future completions, and only true completed reports restore availability. Display history intentionally survives. LVM's common failure branch also invalidates; persistence-only queue congestion preserves valid measurements. The typed inventory detail test itself checks formatting/classification; the service EMFILE test plus common typed-error control flow establish the budget-failure path, not a separately injected full budget service run.

Firstboot uses monotonic reevaluation, with up to ten seconds of healthy-recovery lag. `deploy verify` reads persisted firstboot (`crates/tidemark-cli/src/deploy/mod.rs:558-578`) and may return exit 6 during that lag; `--wait` polls every five seconds (`:499-503`), default zero. This is consistent with “not healthy yet”; a historical finalized firstboot report is not current service health. C2 is the verified false-finalization case.

CLI exit mapping 1–6 (`crates/tidemark-cli/src/lib.rs:58-79`) survives added anyhow contexts. Command context names operation/subject (`cmd/mod.rs:56-109`); final diagnostic escaping preserves only renderer-owned validation/hint line breaks (`lib.rs:177-194`). Hostile configuration/deployment tests check control escaping. Changed JSON branches retain core serializers and field shapes; early optional rate numbers are now null as required, not new keys. No exhaustive proof of every dependency/clap diagnostic is claimed. C8/C9 are concrete remaining output gaps.

### Daemon-ending paths after these changes

| Path | Current behavior and assessment |
|---|---|
| Configuration/validation/state trust, supplied-vs-loaded mismatch | `lib.rs:777-809`, validation/load helpers. Refuse before producers; appropriate for invalid/unusable setup. Output collisions now reject startup/reload before commit. Security boundary correctness belongs to the other review. |
| Signal setup | `lib.rs:737-765` unregisters earlier handlers and returns on registration failure; appropriate. |
| Storage startup/recovery | `lib.rs:806-843`, `:1168-1215`. Typed pressure retries, storage/schema failures end. New Busy recovery wait ends after 30 seconds with explicit ownership refusal; appropriate bounded refusal, but C1 exclusion and C4 pressure erasure remain. It can delay signal response by the wait duration. |
| Service/API construction after writer start | `lib.rs:880-1001`, startup cleanup below. Nonpressure constructor failure stops/joins prior services and writer; pressure normally retries. API ownership/bind/security refusal prevents readiness. |
| Runtime API accept | `api.rs:830-901`: retryable transient/pressure errors back off; fatal listener errors set shutdown. Per-client framing/credentials/handler/write/spawn failures stay connection-local; listener-thread panic is observed as a failed service. |
| Scheduler initialization, loop error or unwind | `scheduler.rs:1175-1179` exit guard, `:1243-1287` initial catalog and job result, later schedule/publication errors. Persistent storage/invariant/worker errors can end scheduler and set shutdown; ordinary Scan/Busy/cancellation/NoSpace and recognized pressure are contained. |
| Command channel closure / explicit shutdown | Scheduler receive and cleanup (`:1302`, `:1472` onward); OS flags in `lib.rs:740-744`. Orderly stop is appropriate; scheduler guard also fails closed on control-owner loss. |
| Process budget/collection failure | `services/procscan.rs:117-136`, `lib.rs:693-704`: unavailable and retry; pressure/budget failures are degraded, ordinary setup/scan errors are error. Neither ends daemon. Other sampler/LVM collection failures similarly remain local. |
| Watcher namespace/mount/transport/mode failure | `services/watcher.rs:93-197`: degrade/disable/remove/retry as applicable. Publication/reconcile errors can end this service; lifecycle records it (`lib.rs:1047-1057`) and continues. C5 can leave stale health without ending anything. |
| Writer runtime exit | `services/tswriter.rs` supervisor restarts it; persistent failures remain visible. It is not a blanket daemon exit. |
| Shutdown join/final publication/flush failure | `lib.rs:1059-1165`: cancellation/deadline broadcast, all owned threads joined, errors reported. Deadline expiry does not detach a blocked kernel/SQLite/LVM operation; no universal ten-second termination guarantee. |
| Abort/OOM/unhandled main panic | Still process-ending Rust/OS paths, outside recoverable Result handling. Bounded application inventories do not prove infallible allocation. |

## Test quality and environment inventory

The exact blind candidate changes only `crates/tidemarkd/tests/w6a1.rs` and its handoff. Its subprocess isolation, single retained writer blocking only its first write, error cleanup before assertions, and ten-second reader bounds preserve the lock-ownership regression. A slower reader cannot pass while the publisher retains a wrongly shared lock, because release remains after the reader result. No architecture defect was found in that test-only diff; C10/C11 are remaining siblings in the candidate tree. Checkout differences from it are docs plus the CLI NoSpace test already distinguished under C8.

Passing helper entry points without their opt-in environment are not scenario execution. Socketpair tests assert here, but whole listener/fake-daemon bodies can return success before central assertions. The CI workflow runs normal user tests (`.github/workflows/ci.yml:60-79`), so mode-000 permission assertions normally execute there, while root-only listener pressure/privileged tests still need a root-capable guest. This worker is uid 0, shell umask 022; Unix bind/user namespaces are restricted. Fixture modes are explicitly reported where they affect probes.

Additional weak evidence found:

- `api.rs:1150-1172` checks a shutdown flag never passed to the accept-pressure seam; that particular survival assertion is vacuous. Its error/backoff checks remain useful. Wire the real owner or restrict the claimed property.
- `tests/w6b.rs:281-297` initializes reads before publication and only requires reads > 0; it does not require overlap with an atomic rename. Add a controlled publication/read handshake while retaining all-value assertions.
- `log_tests.rs` copies many message macros after searching production strings. Those tests preserve spelling but can pass if an actual call becomes unreachable; real descriptor/API helper calls are stronger.
- FUZZ-timing's scaling ratio is conditional on the small case taking at least 1 ms, and some seams include SQLite setup or immediate parse failure. These are coarse guards, not complexity proofs. The socket-client fuzz entry can skip its parser assertions when bind is denied.

The complete search evidence is `timing-search.txt` and `process-global-search.txt`; the following compact inventory excludes duration values used only as data windows. Suggested file edits for C11 extend to these named files when correcting their positive scheduling bounds; short protocol budgets should retain semantic tests with explicit entry/cleanup synchronization.

| Test file/group | Positive waits below10s and assumed-phase sleeps | Runs/asserts here? |
|---|---|---|
| daemon state.rs presence_lock_tests | :1519 entry2s; :1525 reader200ms; phase2 reload-before-release can deadlock on the regressed implementation | Yes, no socket/subscriber; real state lock with injected resolver/logger |
| live tests/w5b/unit.rs immutable report | :295 entry5s, :307 reader50ms | Yes, no socket |
| daemon services/sampler.rs | EMFILE :321 collect5s/:352 progress3s; mount presence :546 progress5s/:570 receive5s; failure persistence :471 assumes pass completed after100ms | Yes, unguarded source-local collector regressions |
| daemon services/procscan.rs | EMFILE :207 command5s/:226 progress5s | Yes, injected collector |
| daemon metrics.rs / tests/w6b.rs | disabled metrics :770 progress5s; w6b :31 lock retry2s; atomic :297 only requires read count>0 | Yes; does not prove read/rename overlap |
| daemon lib.rs recovery/join tests | :234 child ready5s; :252/:280 elapsed<1s; :315 join5s; :456 worker sleeps50ms to outlive expired deadline | Yes, temp-only subprocess; short sleep here deliberately simulates lag |
| daemon scheduler.rs admission tests | :1596/:1600/:1615 receive1s, :1626 elapsed<1s; injected admission budgets100/150ms can expire before spawned thread is scheduled | Yes, actual API dispatch without listener |
| daemon scheduler.rs recorder tests | :1700/:1709 ACK8s; :1751/:1796/:1799/:1848/:1855-56 entry1s; :1761/:1862 result500ms | Yes; bounded channels hold target phase, but scheduled receive must fit small wall budget |
| daemon API unit pair tests | pair helper :1121 read3s; :1236/:1253 elapsed<1s; :1264 read1s; :1307 budget1s + :1312 entry1s; :1412 budget50ms then :1421 sleep80ms | Yes where UnixStream::pair works; no bind needed. Request write can race initial idle deadline |
| daemon tests/w6a3 listener tests | :144/:204/:282/:286/:295/:297/:338/:359 read/entry3s; :172 elapsed2s; :208 entry3s; :478 elapsed1s; :550 read5s; :214 sleep80ms; :291 sleep850ms against700ms idle cutoff | No central assertions in denied-bind sandbox; normal CI with Unix bind yes |
| daemon tests/w6a5 low descriptor | :61 socket read5s | Early bind guard here; socket-capable root runner needed |
| daemon tests/w6a2/runtime.rs | :838/:875 sleep150ms as assumed scheduler boundary | Yes, no socket for these scheduler cases |
| CLI tests/w6a3 mock server | :67/:1225 accept5s; :80/:1239 read3s; :1046 read5s | Early socket guard here; unrestricted CI yes |
| CLI live_rows.rs / tests/w7a/browser.rs | live_rows :214 sleep100ms before cancel/:222 elapsed2s/:224 lock2s; browser :465 sleep100ms then :472 completion2s | Yes, temp scan; sleep does not establish worker has entered throttle |
| FS tests/w1d/main.rs | :189 and :368 progress1s; :198 elapsed100ms; :324 receive2s/:330 elapsed200ms; :410/:469 sleep20ms | Yes, nonprivileged gates; sleep-based phase assumptions remain |
| FS tests/w1b/main.rs | :281 sleep30ms then :285 elapsed<1s; :393 sleep1ms and :702 sleep5ms for mutation workloads | Yes; workload pacing distinguished from handshake |
| FS tests/w4a/lvm.rs | command timeout100ms plus :105 elapsed2s/pidfile assertion; output-cap timeout3s; privileged :145 collection<2s | Shell fixture local yes; LVM guest-only part not executed |
| FS src/throttle.rs tests | :602/:610/:636/:643/:648 2s progress/result; :828 receive3s; :790/:793/:817/:820 sleeps50/700/100/800ms around cadence/cancel | Yes; injected Gate tests; synchronized rate assertions should use controlled phase |
| scan tests/w2c + orchestrator tests | w2c :442 cancel<2s; orchestrator :1469 cancel<2s/:1500 lock2s; :1867 sleep2200ms deliberately crosses progress tick; tests/w2b :329 sleep2300ms | Yes; actual cancellation lock tests local, bulk variants retain feature restrictions |
| CLI deploy/tests.rs | :435 verify polling20ms/<1s; :613 subprocess wait20ms/<1s | Local test fixtures, not host deploy; no VM acceptance |
| live watch/mod.rs synthetic | :1666 shutdown-ready retry_wait<100ms | Yes, test-support injection; does not establish guest performance |
| live tests/w5a/privileged.rs | :56 FD event1s/:115 stop-join<1s/:168 FID event1s/:231 thread discovery2s/:470 event5s; child sleeps100ms+2s; filesystem-mode :476 sleep100ms | Compiled only; needs root+fanotify+XFS guest |

| Process-global state/capture pattern | Classification and exact affected tests |
|---|---|
| tracing callsite interest cache | **Remaining siblings:** daemon mounts.rs:199 logger_emits_one_line_per_transition_and_device_changes_restart, scoped default :219; log_tests.rs:16 every_operator_message_formats_with_values_and_without_debug_fields, scoped default :41. Real mount/API log sites are also called by peer tests without those defaults. Deterministic real-site cache probe reproduced suppression. |
| tracing integration blocked sink | **Fixed by exact candidate:** daemon tests/w6a1.rs:1664 blocked_mount_transition_log_does_not_hold_state_lock has child-only env marker passed with Command::env and exact-test subprocess. Current process env is not mutated. |
| StateDir WARNED and CONTEXT + tracing capture | state_dir.rs:140 one-shot warning, :100 namespace context OnceLock; readable_storage_warns_once :377 already runs exact child process and captures at :427. policy_table supplies explicit inputs. No additional unisolated assertion against these globals found. |
| live FD_BATCH atomic | watch/mod.rs:24 global, set only by daemon budget configuration; tests found pure fd_budget assertions rather than asserting an unstable global value. No conflicting test write identified. |
| env variables | CLI privileged w4a.rs:214-215 lvm_thin_pool_usage_tracks_writes and :273-274 lvm_absent_is_na; FS privileged w4a/lvm.rs:133-134 lvm_maps_mounts_to_lvs mutate process-wide LVM executable overrides (all assign same installed paths). They are not child-confined; distinct-value race not demonstrated. Prefer Command::env or dependency-injected executable paths for test isolation. |
| service / directory-open globals | CLI w4b privileged SERVICE_TEST mutex serializes service tests; FS getdents OPEN_LOCK serializes internal descriptor operation. No new race asserted. |
| signal registrations / CLI logging init | Library run installs global subscriber and command signal callbacks, but search found CLI tests invoke command binaries, not crate::run/main_entry in one test process. No global init race asserted for those tests. |
| Other mutations search | No test set_current_dir, umask, setrlimit, set_uid/set_gid global mutation found; Command::current_dir/env are child-local. DAC test sets Command UID/GID, also child-local and may skip on restricted sandbox. |


## For the VM run

Run only inside disposable Rocky 8.8 and 8.10 guests with the pinned compiler, cached dependencies and the project's root-only access setup. Commands below are proposed, not executed here. Record kernel/mode/uid/umask and every skip; a harness success that returns before assertions is not guest evidence. C7's current fault gate is expected to fail on the identified modes/policies until corrected.

```sh
export CARGO_NET_OFFLINE=true TIDEMARK_DEPLOY_TEST_VM=1
timeout 1500 cargo test --offline -p tidemarkd --lib -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemarkd --test w6a1 --test w6a2 --test w6a3 --test w6a4 --test w6a5 --test w6a6 --test w6a7 --test w6b -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemark-cli --test w6a3 --test surface -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a3 -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
timeout 1500 cargo test --release --offline -p tidemark-live --features privileged --test w5a privileged::watcher_sixty_second_load_and_ten_minute_fd_stability -- --exact --ignored --nocapture --test-threads=1
timeout 1500 bash tests/fault/02-snapshot-crash.sh
timeout 1500 bash tests/fault/03-fanotify-overflow.sh
```

Require filesystem-mark mapping on supported Rocky 8.10 and explicit mount-fallback coverage on 8.8. Add the normal-unmount/held-view/remount case described in C6; the existing lazy-detach test cannot substitute. Exercise foreign-namespace event reopen under the low-NOFILE daemon budget, verify complete event counting, cap reasons and expiry, and distinguish 120-second transport recovery from fifteen-minute history expiry. Keep the three-event namespace/bind/chroot test single-threaded; replace fixed settling sleeps with observable completion before claiming once-only delivery.

Run two-process writer/recovery/CLI contention tests after C1, including shutdown during startup wait; assert no database/WAL/SHM replacement under live ownership and no orphaned acknowledged scan after request expiry. Re-run real process inventory budget/recovery with bounded guest fixtures and measure RSS; the synthetic allocation envelope is not a measured whole-daemon peak.

For install/reboot, use the existing prepared 0.1.0 and changed-default 0.1.1 RPMs and a persistent disposable XFS fixture. The external guest harness must supply these variables and restore the mount after reboot:

```sh
: "${TIDEMARK_INSTALL_RPM:?absolute initial RPM}"
: "${TIDEMARK_INSTALL_UPGRADE_RPM:?absolute upgrade-fixture RPM}"
: "${TIDEMARK_INSTALL_MOUNT:?persistent disposable XFS mount}"
TIDEMARK_INSTALL_PHASE=install timeout 1500 bash tests/install/install.sh
# After the external harness reboots and restores the same mount:
TIDEMARK_INSTALL_PHASE=verify timeout 1500 bash tests/install/install.sh
```

Require `deploy verify --wait` to move from unhealthy to healthy only after genuine complete sampler recovery, preserving firstboot wall timestamps through wall-clock corrections. Inspect status for current health separately from historical firstboot certification. On an equipped ordinary-user runner, run `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh --require-deny` with cargo-deny/advisory data and real ShellCheck installed. No SSH, privilege, mount, LVM, service or system-directory mutation was attempted in this review.


## Validation and limits

All required long commands ran under `timeout 1500`, offline with the pinned Rust 1.98.0 binaries, `CARGO_HOME=/tmp/tidemark-w7-review3-cargo` copied from the readable populated registry/git cache, `CARGO_NET_OFFLINE=true`, four build jobs and two test threads. No production/source/test modification or dependency fetch was needed. The checkout was `1c8a0be4a30e2eee9ed498623a7d658b5b562ca7`; only this report is changed. These are evidence candidates, not release or VM acceptance.

| Check | Observed result | Evidence |
|---|---|---|
| `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh` | Exit 0: source policy, fmt, workspace Clippy with `-D warnings`, workspace tests, privileged-test compilation passed. Cargo-deny explicitly skipped because absent. | `ci.log`, `ci.exit` |
| CI test summaries | **845 passed, 0 failed, 14 ignored across 79 summaries**; includes nested helper processes, empty suites and successful early returns, not 845 distinct fully exercised scenarios. No full-suite rerun was used to replace a failure. | `ci-counts.json` |
| `timeout 1500 cargo build --offline --workspace --features privileged --tests` | Exit 0, explicit independent command after CI; compilation only. | `privileged-build.log`, `.exit` |
| `RUSTDOCFLAGS='-D warnings' timeout 1500 cargo doc --offline --workspace --no-deps` | Exit 0, warnings denied. | `rustdoc.log`, `.exit` |
| Selected already-built default-feature tests with `--nocapture` | Six invocations exit 0. They expose **21 deferred/guard notices**: five daemon readiness, eleven daemon listener, one low-fd daemon, one metrics live socket, daemon/direct NoSpace, and one child-identity/DAC case. Central guarded assertions did not run. | `guard-coverage.log`, `.json` |
| R1–R10 socket-free regressions | Actual CI log records collector EMFILE/epochs/freshness, single-mount presence health, resolver/log lock tests, firstboot clock/restart, output collision/reload, ShellCheck harness, exact windows, Busy recovery preservation and ENOTDIR no-mutation assertions passing. R3 timing weaknesses and missing R9 writer-lease case remain findings. | `ci.log` test names; table above |
| Finite defect probes | Each final probe exits 0 asserting the stated defect: recovery writer exclusion and startup EMFILE classification; multi-mount firstboot false finalization; dirty cap/window health; view-held watcher lifetime; actual CLI Debug/epoch output; tracing callsite suppression; short-window rate diagnostic. | `recovery-writer-exclusion.log`, `sampler-multimount-probe.log`, `watcher-dirty-cap-probe.log`, `watcher-view-lifetime.log`, `cli-render-probe.log`, `tracing-sibling-probe.log`, `rate-warmup-probe.log` and corresponding source |
| Real ShellCheck / cargo-deny | Not available; stub harness proves output/exit handling, not real script diagnostics. No advisory/license freshness claim. | CI and source inspection |
| Final scope/cleanup | `git diff --check` passed; only declared review file is changed. `timeout 1500 cargo clean` exited 0, removing 27,924 files / 17.7 GiB. | `cargo-clean.log`, `.exit` |

All unguarded R1–R10 tests named in the table actually asserted in this run. In particular socketpair admission/handler tests, source-local watcher windows, copied-shellcheck child harnesses, state/readers and real file/flock tests are not attributed merely from handoffs. R10's ordinary-user mode-000 branch, socket backlog checks, live socket collision test and end-to-end lifecycle still need their indicated runner. The explicit guard check observed NoSpace's direct branch stop at the mount-identity precondition and the permission test's child UID/GID change fail with EINVAL; their passing names do not establish those outcomes.

The multi-mount probe's first attempt failed its newly added private-state fixture mode before reaching persisted firstboot assertions; that log is retained (`sampler-multimount-probe-fixture-mode-failure.log`). Explicit 0700 fixture setup corrected the probe, with unchanged production source. This does not conceal a product-test failure. Extracted-source probes compile production bodies with small adapters; they are not full-daemon or kernel reproductions. Probes linked to compiled crates use the default-feature build made during CI.

Not checked: privileged runtime, real Unix listeners in this sandbox, namespaces, normal unmount/remount, kernel fanotify/XFS/LVM behavior/performance, post-corruption acknowledged-transaction loss, packaging/install/reboot on Rocky, SELinux acceptance, dependency advisories/licenses, real ShellCheck, RSS/FD soak, Grafana/promtool, or every possible API/persistence scheduling interleaving. Security boundaries assigned to the separate review were not independently re-audited. Plan drift was not used as a finding; required notes were read as claims, with operator/test behavior judged from files. No PostgreSQL work was reviewed or proposed. No host configuration, service, mount or LVM mutation, branch creation, merge, push, memory projection edit, or external message was performed.
