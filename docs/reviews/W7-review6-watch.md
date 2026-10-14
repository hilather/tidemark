Commit at entry (`git rev-parse HEAD`): `4f2a6a971881bbe23a2ebba94d44e889fda0c51b`.
Verdict: **BLOCKED**.
Counts by class: **1 BLOCKER, 2 SHOULD-FIX, 0 NIT**. Prior dispositions: **3 RESOLVED, 1 PARTLY, 0 NOT**.

Exact blind candidate: `1ab6255de52e85a1364ae34df5ac2956ca19dc89`, base `8036325e35e401aeb6b4d1f996e11416b9151a88`. The checkout adds only the CLI-lvm2 renderer, its tests and its handoff; watcher findings apply to both trees. The requested checkout delta is `754ad83..4f2a6a9`. Findings and validation are evidence candidates, not release authorization.

## Four dispositions

| Prior item | Disposition | Source and discriminating regression |
| --- | --- | --- |
| B1: delayed tick/rollback clears history | **RESOLVED** | Delayed queries/drains/status and event observations clamp under the watcher lock (`crates/tidemark-live/src/watch/mod.rs:861,882,903,1122,1315,1325,1343,1388,1512`). Production fresh-clock observations sample under that lock (`:766,1115`); they still detect genuine rollback. `rings.rs:266-281` retains buckets, marks the affected interval lossy and preserves delivery watermarks. `services/watcher.rs:1218` (`delayed_status_after_drain_does_not_rollback_history`) barriers the production tick between drain and status, advances 179 to 180, checks all three scopes and 5/15/60-minute queries, three once-only queued rows and healthy status, then rolls back to 119 and requires retained 82-byte lossy rows. `rings.rs:1502` (`sixty_buckets_and_clock_rollback`) checks bounded retention and no repeated delivery after a large step. |
| B2: reload drops history/claims complete coverage | **PARTLY** | `services/watcher.rs:662-695` retains unchanged watchers for ordinary-name reorder/extension and unrelated exclusions; `state.rs:344-363` retains compatible immutable windows. Settings/identity retirement flushes eligible completed rows before shutdown (`watcher.rs:126-127,617-634,683-685`). `watch/mod.rs:825-834,864-871` and `state.rs:1218-1235` mark pre-start nonempty answers lossy and make empty ones unavailable; `state.rs:1081-1090` adds a `now` boundary until sixty-minute coverage. The new reload regression (`watcher.rs:1023`) checks Arc/ring retention, replacement quality, queue counts, repeat retirement and boundary expiry; combined-mount coverage is checked at `:818`. Remaining gaps are F1–F3 below. |
| S1: normal-budget enqueue rejects emergency aggregate | **RESOLVED** | `services/watcher.rs:756-799` moves already-exported rows into bounded messages without copying keys or demanding normal credits again. `production_tick_moves_emergency_aggregate_through_full_normal_admission` (`:935`) fills normal admission and requires one lossy root aggregate with two events/62 bytes and no duplicate. `rejected_writer_queue_keeps_persistence_drop_signal` (`:880`) requires the existing drop signal on rejected enqueue. |
| S2: unmount test detaches cleanup worker | **RESOLVED** for owned, killable subprocess cleanup | `crates/test-support/src/xfs.rs:18-40` kills and reaps on deadline/polling failure; cleanup uses it at `:167-179`. `crates/tidemarkd/tests/w6a2s.rs:797-804` joins before propagating failure. The unguarded imported `cleanup_deadline::deadline_tests::stalled_cleanup_is_killed_and_reaped_before_error` (`xfs.rs:206`) requires `TimedOut`, completion within two seconds, disappearance of the child's PID and a successful command. It discriminates unbounded waiting in the helper; removing its guest cleanup wiring requires the guest test. Uninterruptible kernel tasks cannot be given a hard reap/join deadline by this code. |

All named unprivileged disposition regressions above passed in this run's CI and actually executed their assertions; none has a socket/namespace/root early-return guard. These are synthetic event/clock/channel checks, not kernel watcher evidence. Final command results are recorded below.

## Findings

### F1 — BLOCKER — A retained view presents retired history as measured empty

Reference: `finding:review6-retired-view-empty`. Applies to both reviewed trees.

`crates/tidemarkd/src/state.rs:1221-1222` consults startup coverage and then queries a mutable watcher without considering retirement. Replacement calls `shutdown()` before removing the watcher (`services/watcher.rs:703-707`). Shutdown clears rows (`crates/tidemark-live/src/watch/mod.rs:1163-1179`), but `rings.rs:244-250` leaves the old `history_start`. A view already captured by API dispatch (`api.rs:260`) retains that Arc, still judges a mature window covered, recreates empty buckets on query and returns successful `Some([])`.

**Observed:** `library/review6-retired-view-probe.rs/.log` links unchanged production crates and mirrors the actual public retirement ordering. Before retirement, all nine scope/window combinations contain one lossless 82-byte row. After retirement, the held view returns `Some([])` in all nine; the current empty view returns `None`. Publishing an empty replacement leaves the same distinction. Held `now` has empty directory/unit lists, `lossy_minutes_15m=0` and no history-start warning. The probe executes actual assertions without sockets, namespaces or fanotify. This deterministically demonstrates the method interleaving; a concurrently scheduled API request was not needed or run.

**Exact change requested:** give retired watchers an explicit unavailable lifecycle state and make lifecycle/coverage/rows one atomic query decision under the watcher lock; have `LiveStateView::hot` propagate that unavailability and `now` expose retirement/loss instead of measured-empty lists. Merely clearing `history_start` leaves a race between the separate coverage and query calls. Preserve existing transport release and bounded-memory reclamation. Files: `crates/tidemark-live/src/watch/mod.rs`, `crates/tidemarkd/src/state.rs`, `crates/tidemarkd/src/services/watcher.rs` (regression). Add a barrier test that captures a mature view, retires/replaces its watcher, then requires retained hot/now to be unavailable or retain explicitly incomplete rows in all scopes/windows. Existing `retained_view_does_not_delay_retirement_before_replacement` checks resource release, not those results.

### F2 — SHOULD-FIX — The history-start explanation is not wired into the API

Reference: `finding:review6-hot-unavailable-detail`. Applies to both reviewed trees.

`crates/tidemarkd/src/api.rs:267` still calls generic `available(...)`, whose error is hardcoded at `:204`; it never calls the new `LiveStateView::hot_unavailable_detail` (`state.rs:1238`). An empty startup/replacement window therefore fails visibly but does not explain its coverage boundary or admissible shorter window.

**Observed:** the same probe obtains `watch history for /A starts at 02:01 UTC; ask for a window of at most 0 minutes` from the state view, then calls the real public API dispatcher and receives `ok:false`, code `unavailable`, message `unavailable: measurement unavailable`. This is not silent complete data and is therefore SHOULD-FIX under the requested classification.

**Exact change requested:** in `crates/tidemarkd/src/api.rs`, parse `HotParams` once and use `DaemonError::Unavailable(view.hot_unavailable_detail(&p))` when `view.hot(&p)` is `None`. Add a socket-free dispatch regression for empty startup and replacement windows that requires the boundary text. The existing view-only assertion does not catch missing dispatch wiring.

Known issue: Empty hot windows that predate watcher startup or replacement return a generic unavailable error without the history start or shorter-window guidance.

### F3 — SHOULD-FIX — Display-escaped mount names fail reload membership matching

Reference: `finding:review6-escaped-mount-retention`. Applies to both reviewed trees.

`crates/tidemarkd/src/services/watcher.rs:679` compares `PathBuf::to_string_lossy()` with watcher keys created using `RawPath::display_lossy()` (`:162-164`). For the valid absolute path `/data\archive`, the display key contains the literal escape `\x5C`; the strings differ. A watch-list reorder or extension therefore retires that unchanged watcher. `relevant_exclusions` (`:637-645`) also treats the escaped display string as a filesystem path; `state.rs:351` uses the mismatched membership comparison for windows. Config validation accepts these absolute names (`crates/tidemark-core/src/config.rs:809-817`); escaping is defined at `core/path.rs:253-270`.

**Observed/inferred boundary:** `library/review6-render-path-probe.rs` uses the real TOML parser and RawPath display conversion, then asserts that the exact copied membership predicate rejects this configured mount (`library/render-path-probe.log`). Its consequent retirement follows directly from the inspected helper; no real oddly named mount was created. New startup coverage discloses the loss, so this is SHOULD-FIX rather than a silent-data blocker.

**Exact change requested:** retain raw `PathBuf`/`RawPath` identity for membership and relevant-exclusion comparisons, applying display escaping only when producing labels; use the same identity mapping for retained windows. Files: `crates/tidemarkd/src/services/watcher.rs`, `crates/tidemarkd/src/state.rs`. Extend the reload regression with a backslash-containing mount, reordered/extended watch lists and a relevant exclusion change; require unchanged Arc/history for membership-only changes and retirement for the actual exclusion change.

Known issue: Reloading watch settings restarts otherwise unchanged mounts whose names contain backslashes or control characters, losing live history until the advertised window fills.

## Persistence, memory, shutdown and consumers

The once-only ring delivery cursors survive rollback and unchanged-watcher reload; retirement flush uses the same `drain_minutes_at` cursor as normal ticks. The replacement test receives exactly three scope rows for its eligible minute and zero on the repeated retirement. This verifies **at-most-once handoff to the queue**, not unconditional exactly-once database durability: full/disconnected queues and writer failures remain lossy. Flush occurs before worker shutdown; an in-flight event or newly completed minute after that cut is not covered by these tests or a final post-join drain. No real worker/retirement boundary-crossing or SQLite persistence integration was exercised. The source-visible ordering is verified; the possible boundary race is an inference, not an observed duplicate/drop in this run.

No newly unbounded collection was found. `state.rs:864-871` bounds configured watch mounts by 128 subject slots minus 17 reserved slots. Retained watchers reuse their existing Arcs and 64 MiB allocation-credit accounts, rather than allocating replacement histories for ordinary reorder/extension. `MinuteHistory` remains limited to sixty facts and 8 MiB of directory rows charged to the same account (`services/watcher.rs:351-409`); immutable watcher window handles are capped at 1,024 (`watch/mod.rs:1051-1070`). The writer channel remains 64 messages; this watcher producer caps each Hot message at 64 KiB, bounding its queued ownership to about 4 MiB. These are allocation/count bounds, not measured RSS or a global 64 MiB daemon limit. Long-key enqueue and retained-history budget regressions passed with actual assertions.

Shutdown retains the prior ordering: stop/join watcher workers and release descriptors before replacement, then join producer services before flushing/stopping the writer (`tidemarkd/src/lib.rs:1393-1417`). The retained-view resource test passed and proves synthetic worker-stop/group-close ordering, but F1 shows that successful reclamation is not sufficient query correctness. No new production privileged operation or regular-file-content read was found in the diff.

Alert windows retain unchanged immutable observations during compatible reload; changed settings/identity remove their windows. `MinuteHistory::facts` still requires consecutive healthy completed minutes, while `watch_window_at` rejects future or more-than-90-second-old observations. This path was inspected; the new reload test does not seed a populated `MinuteHistory` or publish an immutable window before reordering. Its empty history at `services/watcher.rs:1037-1038` means those two retention guarantees are supported by source inspection, not that test's assertions.

Metrics now omit lossy hot rows (`metrics.rs:444,466`) because the frozen metric has no quality label. `incomplete_startup_hot_windows_are_omitted_until_covered` (`:799`) passed without facility guards: it requires no hot series before coverage and three scope series afterward. It covers the direct hot fallback; it does not populate the directory alert-window preference or exercise replacement concurrency.

CLI inspection and the direct renderer probe confirm that `render/hot.rs:34-50` preserves JSON quality and prints LOSSY, and `render/now.rs:263,275,349-355` prints `[lossy]` for directory/unit rows and the degraded history boundary. The probe asserts these at widths 80/100/200 and checks JSON flags, without sockets. The shared command error mapper (`cmd/now.rs:53-70`) preserves daemon unavailable text and method context; therefore F2 originates at dispatch. The real socket CLI error tests cannot establish transport behavior in this sandbox. CLI-lvm2's two renderer tests passed with actual assertions for GiB/MiB, widths and unchanged integer JSON; no regression was found in that extra checkout change. Relevant operator-guide text and the help/man generation source were inspected; no additional false operator statement caused by this delta was identified.

## Tests and execution limits

| Changed test or test group | What removal would break; what actually ran here |
| --- | --- |
| `delayed_status_after_drain_does_not_rollback_history` | Removing timestamp clamping fails healthy status; clearing rings fails equality/82-byte retention. Passed; its real two-party barrier and joined thread ran. An assertion/panic before releasing the second barrier can strand the test thread until process exit; the outer command timeout bounds the run. |
| `sixty_buckets_and_clock_rollback` | Clearing on rollback fails the retained sixty buckets, totals, loss and once-only assertions. Passed; deterministic ring clocks, no kernel prerequisite. |
| `reload_membership_preserves_unchanged_watchers_and_flushes_replacements` | Restart-all fails Arc identity and ring queries; omitted flush fails queue counts; omitted coverage fails lossy/None/history-label checks. Passed, with the populated-window coverage limitation above. Its `now` path still calls real-clock `stats()` (`state.rs:1151`), so the expiry assertion proves warning removal, not an entirely frozen-clock end-to-end snapshot. No old-view assertion catches F1. |
| `combined_mount_window_returns_known_rows_and_names_the_incomplete_mount` | Dropping known combined rows, omitting lossy quality or naming the wrong incomplete mount fails. Passed; synthetic watchers, actual assertions. |
| `production_tick_moves_emergency_aggregate_through_full_normal_admission`; `rejected_writer_queue_keeps_persistence_drop_signal`; adjusted long-key enqueue test | Reintroducing normal-credit copying, dropping the loss signal, replaying rows or exceeding message bounds fails. All passed; actual channels and reservations, no database/kernel watcher. |
| `incomplete_startup_hot_windows_are_omitted_until_covered` | Removing quality filtering emits three premature series; never recovering emission fails the covered count. Passed; actual renderer assertions. |
| `cleanup_deadline::deadline_tests::stalled_cleanup_is_killed_and_reaped_before_error` | Unbounded wait fails timing/timeout; failure to reap fails PID disappearance. Passed with a real `sleep` subprocess and success command. The 30 ms deadline/<2 s expectation may fail under extreme scheduler stalls; no fixture mount command ran. |
| Changed `daemon_sigterm_releases_loop_xfs_for_ordinary_unmount` | Removing owned teardown can fail guest cleanup; only compilation is claimed here. It is feature-gated and was never executed. |
| CLI-lvm2 unit/integration renderer tests | Restoring integer size cells fails GiB/MiB assertions; changing JSON units fails integer equality. Both passed and have no socket guard. |

Absence-of-fix mutation runs were not performed. Discrimination above follows each assertion and its inspected production path. The external lifecycle/API probe asserts the observed defects, so its exit 0 means the bad behavior was reproduced, not that the product meets acceptance criteria. The external renderer probe initially used an invalid null PhantomSummary fixture; that harness-only error was corrected before its successful run.

All commands used pinned Rust 1.98.0, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_NET_OFFLINE=true`, and explicit `umask 022`. The process ran as uid 0. Evidence under `library/` belongs to this attempt's external output directory, `attempt-dd6bf6162471818d113f7e689a13b3a0196670fb4daccf3725b7f33104f50beb`; it is not an additional repository edit.

| Command/check | Observed result | Evidence |
| --- | --- | --- |
| `timeout 1500 tools/ci.sh` with offline environment | Exit 0: source scan (150 owner tags), fmt, Clippy with warnings denied, strict-ancestry subset, workspace tests and privileged-feature test build passed. Cargo-deny explicitly skipped because absent. | `library/ci.log`, `ci.exit`, `ci-summary.json` |
| `timeout 1500 cargo build --offline --workspace --features privileged --tests` | Exit 0; separate required compilation, no privileged execution. | `library/build-privileged.log`, `build-privileged.exit` |
| Lifecycle/API defect probe | Exit 0; all assertions ran and reproduced F1/F2. | `library/review6-retired-view-probe.rs`, `review6-retired-view-probe.log`, `probe-commands.sh` |
| Renderer/path probe | Exit 0; actual renderer/config assertions, F3 membership mismatch observed. | `library/review6-render-path-probe.rs`, `render-path-probe.log`, `probe-commands.sh` |
| `timeout 1500 cargo clean` | Exit 0 before submission. | `library/clean.log`, `clean.exit` |

CI contains 87 test summaries reporting 945 passes, zero failures and 14 ignored tests. These totals include nested summaries and facility-guarded successes, not 945 independently complete acceptance cases. Unix-socket/user-namespace tests and mode-000 permission checks may return early here; no socket, namespace or ordinary-user permission assertion is credited solely from its reported pass. The specific unguarded tests/probes relied on above did execute; the feature-gated daemon unmount test only compiled. Dependency license/advisory auditing remains unperformed because cargo-deny is absent.

## For the VM run

Pending VM. Run from the reviewed checkout inside each disposable prepared Rocky 8.8 and 8.10 guest, as root for the privileged tests, with cached dependencies:

```sh
export CARGO_NET_OFFLINE=true
umask 022
timeout 1500 cargo build --offline --workspace --features privileged --tests
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s daemon_sigterm_releases_loop_xfs_for_ordinary_unmount -- --exact --nocapture
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
```

Record selected watcher mode, startup readiness, SIGTERM exit, ordinary unmount and cleanup results; a guarded skip is not acceptance. Separately run `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh` at `umask 022` as an ordinary user on an equipped host that permits Unix sockets/user namespaces. After F1/F2/F3 are repaired, run their added socket-free regressions and the full checks again. No guest was contacted here.

## What was not checked

Real fanotify negotiation/modes, cross-namespace events, mount identity replacement, XFS ordinary unmount, LVM, Rocky/glibc runtime, privileged cleanup and host/service operations were not executed. No system-clock change was made; rollback was injected. No performance/RSS/load/scale testing, sustained reload race stress or new writer/database-failure injection was performed. This is a targeted review of the requested delta and its relevant candidate-tree dependencies, not a renewed full security/architecture audit or a plan-drift review. Manuals are generated from clap (`xtask/src/man.rs`); no installed manual was tested.

The checkout was used for builds, not a second full build of the earlier exact candidate. `library/watch-source-diff.txt` is empty (exit 0), proving byte-identical `tidemark-live`, `tidemarkd` and `test-support` trees at both commits. The CLI change is checkout-only. Canonical memory brief/input/receipts were read; no pending receipt required adaptation. Only this review file is changed in the repository. No implementation, memory projection, branch, merge, push, release or privileged-host change was made.
