Commit at entry (`git rev-parse HEAD`): `754ad83afaabaf38644234cd1e60384c7171c810`.
Verdict: **BLOCKED**.
Counts at checkout: **2 BLOCKER, 2 SHOULD-FIX, 1 NIT**. Exact blind candidate: **2 BLOCKER, 4 SHOULD-FIX, 1 NIT**, including C1/C2 repaired only in the checkout. Prior findings: **4 RESOLVED, 2 PARTLY, 0 NOT**. Findings are review proposals, not acceptance decisions.

The blind-review receipt names exact candidate `599679cc82279aca4dced857ee26fbe092d32bce`, base `30545227dbc4b663fc8973ecdea817edd253a2b8`. This report distinguishes that candidate from the checkout, which contains later product/test changes. Production watcher and sampler fixes are identical at both. Evidence paths beginning `library/` refer to this attempt’s external output directory (`attempt-f6fcf8b2438ab7133b5d75276ac3b6bd375a7f0782f966acd5daa62e0b3bfbd8`), not repository files. Line references default to the checkout; exact-candidate references are explicitly prefixed. The requested correctness delta is `e0a8c2e..754ad83a`. Findings and check results are evidence proposals, not release authorization.

## F1–F6 recheck

| Finding | Status | Evidence and discriminating regression | Execution boundary |
|---|---|---|---|
| F1 sampler reload | RESOLVED | `crates/tidemarkd/src/state.rs:805-817` guards health by generation under the state lock; `:379-390` invalidates changed mount requirements. `services/sampler.rs:458` (`reload_during_pass_waits_for_new_mount_and_persisted_firstboot`) blocks a pass across reload and checks persisted firstboot; `state.rs:1479` rejects stale health. | Ordinary CI; passed here with actual assertions, no socket/namespace prerequisite. |
| F2 live history | PARTLY | `crates/tidemark-live/src/watch/rings.rs:888-950` preserves maps while advancing independent delivery watermarks. `every_scope_retains_history_after_both_delivery_paths` at `:1134` checks retained query equality; daemon `second_ticks_keep_live_history_and_mapping_loss_expires` exercises the production tick. The destructive-drain defect is repaired; B1/B2 below prevent the requested lifecycle-wide retention guarantee. | Ordinary CI; passed here with actual assertions using synthetic clocks/events. |
| F3 output budget | PARTLY | `rings.rs:826-868` emits exact scalar root sums under emergency admission and `:598-605` records query-budget loss. `exhausted_output_keeps_exact_aggregate_and_expiring_reason` at `:1168` requires +71/-9 = 62 bytes via both drains and an expiring reason; `emergency_headroom_covers_full_history_and_aggregate_export` at `:1022` retains simultaneous reservations. Core export is repaired, but S1 loses that aggregate in daemon enqueue. | Ordinary CI; passed here with actual assertions, no kernel facility guard. |
| F4 planned unmount | RESOLVED for missing guidance | `docs/operator-guide.md:385`, `docs/runbooks/watcher-degraded.md:48-54` explain mount pinning and teardown. `crates/tidemarkd/tests/w6a2s.rs:713` requires daemon SIGTERM exit and ordinary unmount. | Real daemon/mount assertion is privileged guest-only, not run here; one negotiated mode. Synthetic retained-view shutdown assertions passed here; their kernel counterpart remains guest-only. |
| F5 test lifecycle | RESOLVED for named children | Immediate kill/wait guards in daemon `lib.rs:147-180`, store `tests/crash_recovery.rs:10-43`, CLI `tests/w6a3.rs:40-72`; positive readiness now ten seconds. Store `child_guard_reaps_writer_when_an_assertion_panics` at `:343` asserts PID disappearance and reacquired recovery lease. | Ordinary CI; passed here including actual panic cleanup, no facility guard. |
| F6 compaction witness | RESOLVED | `tests/fault/compaction.py:70-98` requires the intended live daemon, a disjoint newly compacted sentinel and unchanged prior rows. `xtask/tests/w7c.rs:312` rejects no-compaction and malformed/dead/replaced-daemon fixtures through the real helper. | Real helper assertions passed here in ordinary CI and the extracted negative-control fixture; actual hourly workload is guest-only. |

## Findings

### B1 — BLOCKER — A delayed status tick mistakes its saved timestamp for a clock rollback

Reference: `finding:review5-stale-tick-clears-history`. Present at both commits; the stale status timestamp is introduced in this delta.

`crates/tidemarkd/src/services/watcher.rs:226` captures one wall time for a tick; `:266` drains with it, and `:279,299` reuse it for `stats_at` after history publication and persistence work. Suppose the tick captures second 179 and drains. The watcher worker advances normally to second 180 (`crates/tidemark-live/src/watch/mod.rs:748-755`) while the service is delayed. Resuming the tick calls `stats_at(179)`. `State::observe_time` (`mod.rs:410-418`) treats this ordering as rollback, and `rings.rs:265-270` clears all retained minute maps. No clock adjustment or event is required. Before this delta, component publication called `watcher.stats()` with a fresh time.

**Observed:** `library/watcher-lifecycle-probe.rs/.log` links unchanged production watcher and state code. It reproduces the exact drain/advance/status call ordering: all-scope history containing 82 bytes becomes zero. `LiveStateView::hot` returns successful `Some([])`, so there is no row to carry a loss flag (`state.rs:1180-1203`). Separate watcher status records one lossy minute, but the hot result silently says no activity. This is a deterministic method-order probe, not a scheduling reproduction in a running fanotify daemon.

**Recommended change:** do not reuse an earlier tick clock for a later mutating status observation; sample under the watcher state lock, and distinguish actual wall-clock rollback from stale concurrent timestamps. Retain completed query rows when handling late observations, preserving the once-only delivery watermark. Add a barrier regression between tick drain and component publication, advance the worker to the next minute, resume, and require exact directory/unit/command queries plus no repeated persistence rows. Files: `crates/tidemarkd/src/services/watcher.rs`, `crates/tidemark-live/src/watch/mod.rs`, `crates/tidemark-live/src/watch/rings.rs`. The sequential `second_ticks_keep_live_history_and_mapping_loss_expires` test does not cover this interleaving. The analogous timestamp captured before event metadata/attribution is an older exposure, also relevant to the fix.

### B2 — BLOCKER — Reload replacement silently discards still-current hot history

Reference: `finding:review5-reload-clears-history`. Present at both commits. The lifecycle reset predates the delta; the new retention repair does not cover this explicitly requested boundary.

Any `WatchConfig` inequality restarts **every** watcher (`crates/tidemarkd/src/services/watcher.rs:74-92`), including merely reversing the same mount list. Shutdown clears ring rows and immutable window storage (`crates/tidemark-live/src/watch/mod.rs:394-406,1129`); the service deletes `MinuteHistory`, creates a fresh watcher and publishes `ok` (`services/watcher.rs:192-208`). Readers consult only the new watcher (`state.rs:1055-1067,1105-1120,1180-1203`), with no historical store fallback or query warmup indicator. A still-configured unchanged filesystem therefore loses the previous five/fifteen/sixty-minute activity before expiry.

**Observed:** the linked lifecycle probe seeds 82 bytes in all three scopes, verifies their presence before and after persistence drain, reverses the configured watch list, and mirrors the production shutdown/remove/publish replacement sequence. All three `hot` results and both `now` directory/unit lists become empty; watcher and sampler health remain `ok`, and `lossy_minutes_15m=0`. The `now` diagnostics have no history-loss/warmup explanation. An old retained state view also loses the rows. This uses real public state/watch methods with a synthetic replacement, not actual kernel watcher negotiation or API reload.

**Recommended change:** compare mount membership independently of order and retain unaffected watchers; separate bounded per-mount query/delivery history from transport ownership so a same-identity replacement preserves admitted facts through expiry. For changed identity or incompatible aggregation settings, explicitly mark affected query windows unavailable/warming instead of publishing measured-empty data. Flush eligible undelivered facts before retirement, retaining delivery identity across a same-minute replacement. Add a service reload regression with old activity, an unchanged mount, a reordered/extended mount list, `hot`/`now` in every scope/window, and persisted row counts. Files: `crates/tidemarkd/src/services/watcher.rs`, `crates/tidemark-live/src/watch/mod.rs`, `crates/tidemark-live/src/watch/rings.rs`, `crates/tidemarkd/src/state.rs`, `crates/tidemarkd/src/metrics.rs`, `crates/tidemarkd/tests/w6a2s.rs`. Never merge history across different mount identities.

### S1 — SHOULD-FIX — Emergency export cannot pass the daemon's normal-budget enqueue

Reference: `finding:review5-emergency-persistence-drop`. Present at both commits.

`rings.rs:826-868` successfully constructs the exact root aggregate using emergency headroom. On return its temporary reservations end. `crates/tidemarkd/src/services/watcher.rs:688-697` then demands normal credits through `Watcher::reserve_key_bytes` (`watch/mod.rs:1043-1045`) before copying it into the bounded writer queue. At the normal 61 MiB ceiling this fails even with an empty queue. The minute was already marked drained (`rings.rs:857,917`); releasing memory later cannot recover the row. This is explicit degraded persistence loss, not a silently exact result, hence SHOULD-FIX.

**Observed:** `library/watcher-persistence-budget-probe.rs/.log` links actual crates and copies the production enqueue/filter functions verbatim. It holds normal admission full, exports one lossy row with two events and +71/-9 = 62 bytes, then observes `persistence_dropped=true`, zero queued messages, and zero retry rows after releasing the reservation. No real database writer or kernel watcher is involved.

**Recommended change:** carry/move the bounded emergency aggregate into the writer queue with admission that survives the handoff; avoid requiring unavailable normal scratch for the one-row fallback. Preserve bounded queue failure signaling and once-only delivery, and add a production-tick regression requiring one exact aggregate received and no duplicate on the next tick. Files: `crates/tidemarkd/src/services/watcher.rs`, `crates/tidemark-live/src/watch/mod.rs`, `crates/tidemark-live/src/watch/budget.rs` (and `watch/rings.rs` if the reservation travels with exported ownership).

Known issue: Under watcher memory pressure, exact aggregate rows can be dropped before persistence; watcher status reports `persistence_dropped`.

### S2 — SHOULD-FIX — The new unmount test times out without owning cleanup to completion

Reference: `finding:review5-guest-cleanup-timeout`. Present at both commits; source-verified only.

`crates/tidemarkd/tests/w6a2s.rs:789-795` moves the loop-XFS fixture into a cleanup thread, then panics on a ten-second receive timeout before joining. That detaches the thread. Its fixture helper executes unbounded `Command::output` for cleanup (`crates/test-support/src/xfs.rs:19-23,112-116`). If guest unmount stalls, the asserted deadline does not kill/reap that subprocess or complete loop cleanup. The daemon child itself has a guard; this concern is the subsequent cleanup worker.

**Recommended change:** make guest fixture cleanup own timeout-controlled subprocesses, kill/reap them on expiry, and always join the cleanup worker before propagating failure; add an injected stalled-command regression outside privileged execution. Files: `crates/tidemarkd/tests/w6a2s.rs`, `crates/test-support/src/xfs.rs`.

Known issue: A timed-out ordinary-unmount guest test can leave its cleanup process and loop fixture alive after the test reports failure.

### C1 — SHOULD-FIX at exact candidate only — The retired-watcher CI assertion rejects the required sampler invalidation

Reference: `finding:review5-candidate-component-count`.

At `599679cc:crates/tidemarkd/tests/w6a2s.rs:447`, `retired_watcher_components_and_errors_release_capacity` still requires exactly one total component. The sampler fix inserts its required degraded waiting component during each watch-set change (`state.rs:379-387`), so the correct result has one watcher plus one sampler. **Observed:** `library/candidate-watcher-component-probe.rs/.log` copies the exact candidate test body and links the byte-identical state implementation; it fails with actual 2, expected 1 (exit 101). This is an unguarded ordinary CI assertion, not an unavailable-socket skip.

**Recommended change:** adopt the checkout's watcher-only count, expected sampler state, and bounded/current-error assertions at `crates/tidemarkd/tests/w6a2s.rs:447-486`. File touched: that test file only. The checkout check is recorded with CI below; its repair cannot be credited to the earlier candidate.

Known issue: The exact blind candidate fails the watcher-retirement CI test because the assertion has not been updated for sampler reload health.

### C2 — SHOULD-FIX at exact candidate only — The watcher fixture requires Python 3.7 on a Python 3.6 target

Reference: `finding:review5-candidate-python36`.

`599679cc:xtask/tests/w7c.rs:244` invokes `subprocess.run(..., capture_output=True, text=True)` in the real watcher fixture. These keywords were added in Python 3.7; Python 3.6 forwards unsupported arguments to `Popen` and raises `TypeError` before the helper assertions. This is verified from the candidate call and the [CPython 3.6.8 implementation](https://raw.githubusercontent.com/python/cpython/v3.6.8/Lib/subprocess.py), with the additions documented in [Python's subprocess version notes](https://docs.python.org/3/library/subprocess.html#subprocess.run). No Python 3.6 interpreter was available locally. Artifact: `library/candidate-test-defects-evidence.txt`.

**Recommended change:** adopt checkout `xtask/tests/w7c.rs:244`, using `stdout=subprocess.PIPE`, `stderr=subprocess.PIPE`, and `universal_newlines=True`. File touched: `xtask/tests/w7c.rs` only. The exact candidate remains affected; checkout fixtures execute successfully on the local interpreter and parse with the 3.6 grammar.

Known issue: The exact blind candidate's watcher helper regression aborts on Rocky's Python 3.6 before checking watcher behavior.

### N1 — NIT — Daemon LVM sizes still use raw integers

Reference: `finding:review5-daemon-lvm-units`. Present at both commits; direct formatting is repaired only in the checkout.

`crates/tidemark-cli/src/render/now.rs:158-159,173` still renders LV/VG sizes with decimal integers and a raw `bytes` label; the direct renderer now uses the shared binary size formatter. Numeric values remain correct. The daemon regression `tests/w6a3.rs:1364` checks percent formatting without asserting present size values.

**Recommended change:** use the shared size formatter in these daemon cells and VG line and assert GiB/MiB examples without changing integer JSON. Files: `crates/tidemark-cli/src/render/now.rs`, `crates/tidemark-cli/tests/w6a3.rs`.

## Behavior, memory and test evidence

The per-scope retention repair is real for an uninterrupted watcher: both drains preserve maps and share the `drained`/`rows_drained_until` delivery watermark; `next_minute` independently controls completed facts. Rollover drops distinct-file tracking, caps each completed scope at 1,000 rows, and expires buckets at sixty minutes. The first partial startup minute is deliberately excluded from the completed-facts API; the legacy drain can deliver it. Exact persistence is **at most once at the ring boundary**, not an unconditional database-delivery guarantee: B1/B2, S1, bounded queue rejection and persistence errors limit it. Actual clock rollback clears history but retains delivery watermarks and marks loss; forward jumps beyond retention visibly skip minutes. Existing rollback tests assert no repeated timestamps, not preservation of pre-step query rows.

All query readers use retained rings: API `hot` and `now` directory/unit sections call `Watcher::hot`; metrics unit/command scopes do too (`crates/tidemarkd/src/metrics.rs:458-473`). Directory metrics prefer completed alert-window facts, then fall back to `hot` (`:436-456`). There is no persistence-store reconstruction on these readers. Unrelated reloads leave the watcher intact; changed watch configuration/replacement has B2. Mount-identity retirement intentionally starts separate history; it must not be presented as a continuous measured window.

Unmapped events now call `rings.unmapped(at, errno)` on their event minute (`watch/mod.rs:1262-1266`, `rings.rs:678-690`), setting both lossy and truncated flags while preserving numeric event/byte totals. Per-minute counts and the latest errno are bounded; `detail` and health loss counts filter the latest fifteen minutes. The lifetime counter does not keep health degraded. `injected_foreign_reopen_maps_or_reports_badfd` checks successful mapping, EBADF, ESTALE and missing provenance with actual descriptor metadata, persisted/completed flags and expiry; the daemon second-tick regression checks unavailable directory-alert data followed by recovery. Those are unguarded synthetic regressions, not real cross-namespace fanotify behavior. B1's stale timestamp handling can still disturb which retained minutes survive.

The new all-scope retention test has a coverage weakness: its first directory iteration performs the drain before unit/command pre-drain values are captured (`rings.rs:1147-1155`), so a unit-only clearing regression could escape. It also compares equality without independently asserting each expected total. The review probe asserts nonzero 82-byte totals in every scope before **and after** drain; that probe is external review evidence, not a CI regression. B1/B2 recommend the missing lifecycle/reader assertions.

### Memory arithmetic

The 64 MiB limit is conservative shared allocation **credits per watcher**, not measured process RSS. `watch/mod.rs:279-281` fixes 67,108,864 bytes and a 4,096-byte mount root; `budget.rs:42-53` reserves 3,145,728 bytes for emergency use, leaving 63,963,136 normal bytes. The locked combined payload/node check rejects every reservation beyond its applicable ceiling. Measured type sizes on this build: `HotEntry=80`, `HotMinute=112`.

- Sixty fallback root rows cost at most `60 × (2 × 4096 + 512 + 4096) = 768,000` credits (`rings.rs:11-12,464-489`).
- Production `display_lossy()` can expand a byte to four ASCII characters (`tidemark-core/src/path.rs:255`), so sixty simultaneous completed fallback exports cost at most `60 × (4 × 4096 + 2 × 4096 + 3 + 4 × 80 + 4 × 112 + 256) = 1,536,180` credits (`rings.rs:829-835`). Combined with fallback roots, 2,304,180 bytes leaves 841,548 of the emergency partition, before small fixed root/response overhead. The saturation test uses threefold replacement UTF-8 labels rather than the production fourfold escaped form; the corrected arithmetic still fits.
- Filling all nominal completed key caps is impossible inside this budget: `59 × 3 × 1000 × 512 = 90,624,000` row-node credits before keys, units or the current minute. Normal admission therefore must truncate earlier, as it does. “Sixty-minute history fits” means the admitted/truncated history, not every nominal key cap at once.
- Each ring has at most sixty buckets; each row's unit-leader counters cap at eight. Daemon `MinuteHistory` caps sixty facts and 8 MiB of directory rows and reserves them against the same normal account (`services/watcher.rs:333-396`); immutable windows and their scratch also share it. The window handle list caps at 1,024. Watcher persistence batches cap 64 KiB and the writer channel caps 64 messages, up to another 4 MiB for this producer, separately bounded from watcher credits (`watcher.rs:655-657`).

No unbounded history was found in these paths. These calculations do not prove allocator behavior, whole-daemon RSS, numeric-size-cache cost, arbitrary reader-held return objects, or performance under a sixty-minute production workload. Evidence and constants are also retained in `library/watcher-budget-arithmetic.txt`.

### Sampler and first boot

All health-only sampler publications reject stale generations under the same state write lock. Mount-set changes invalidate before the new deployment hash; unrelated settings and reorderings preserve already measured equivalent requirements. The existing interleaving regression checks actual firstboot JSON through stale completion, B's first failure, and a complete successful current pass. A historical healthy report for an unchanged hash remains historical by design; newly added required mounts cannot inherit it through this path.

`library/sampler-empty-unrelated-probe.rs/.log` runs a copied byte-for-byte production sampler loop with actual linked state/writer types, synthetic mount metadata/collector and bounded barriers. It observes: unrelated reload retains healthy equivalent requirements; unrelated reload during a blocked pass rejects stale completion then the new pass recovers; removing all mounts clears old errors and becomes healthy after an empty pass. Assertions execute without guards. This is service-loop evidence, not a socket API reload or full firstboot integration run. It supports the source behavior at `sampler.rs:162-170,230-245,298-304`; mount-table I/O failure can still legitimately keep even an empty pass degraded.

### Small changes and daemon-ending paths

Second-daemon refusal translates only lock-acquisition `WouldBlock` (`crates/tidemarkd/src/api.rs:655-656`); other error and exit behavior is unchanged. `held_socket_lock_explains_refusal_without_binding` tests the text mapper and `socket_lock_excludes_same_socket_reuses_crash_file_and_preserves_replacement` tests real flock exclusion without binding. The latter does not assert the new wording, so neither alone proves that deleting the production mapper call fails a test.

Recovery's eight-name absence check (`crates/tidemark-store/src/catalog.rs:512-542`) matches the later preservation loop. Any real database/sidecar still goes through `create_subdir` before rename/sync (`:548-584`); no introduced missing-directory failure was found. The empty-start/restart and empty-then-real-recovery tests exercise both branches in temporary directories without privilege guards.

Build identity uses environment → Git → `.tidemark-build-id` → unknown precedence per field in both binary `build.rs:25-44` and `xtask/src/rpm.rs:237-263`; remote commands carry host identity (`vm_test.rs:173-185`). Tests exercise the pure resolution functions and generated commands, not installed RPM identity. The script runner preserves nonzero status with `pipefail` and checks exit status before `SKIP:` (`vm_test.rs:147`); its fake-command regression explicitly requires `SKIP:` plus exit 7 to fail. No false successful SKIP was found.

The production daemon-ending changes inspected are generation-checked sampler publication, API lock refusal wording, the long-version entry path, recovery's artifact-free branch, and post-acquisition scan-lock validation. No additional exit/hang path was found in that delta. Invalid ownership/schema/config errors continue to refuse startup; typed resource pressure is retried. Service publication errors still propagate to their existing owners. Strict shutdown joins and arbitrary blocked filesystem calls retain their earlier limitations; no universal shutdown deadline was measured here.

### Changed test and script boundaries

Named F5 lease children are guarded immediately after spawn; readiness is ten seconds while controlled Busy/refusal checks retain shorter semantic limits. The deliberate-panic cleanup regression checks that the child PID disappears and a new exclusive lease is immediately obtainable. Exact child-helper entrypoints that return without their environment marker are not standalone evidence. S2 is a separate newly introduced guest cleanup lifetime gap.

Compaction's helper requires the original live PID/start time, a newly seeded disjoint 600-row witness compacted to exactly two expected rows, unchanged serialized historical columns and a single SQLite snapshot for comparison. The unchanged-database negative control now fails. It proves the oracle discriminates progress, not that a real daemon completes hourly compaction. Shell deadlines and the script's printed estimate correctly allow roughly 183 minutes for one cycle and 244 for the default two; setup/cleanup add time.

All eight shell files checked (seven entrypoints plus `common.bash`) pass `bash -n`. Nineteen Python blocks physically under `tests/` parse using the local interpreter's Python 3.6 grammar; both embedded xtask helper fixtures execute against real helper files. Source API inspection found no remaining 3.6-only compatibility problem at checkout. This is not execution under Python 3.6.8. The candidate-only incompatible keyword call is C2. Real ShellCheck and guest scripts were not run. Artifacts: `library/scripts-python-review.log`, `library/second_compaction_requires_new_exact_rows_and_live_daemon.log`.

## For the VM run

Run these from a root shell in the checkout on prepared disposable Rocky 8.8 and 8.10 guests, after addressing B1/B2. They are recommendations, not commands executed by this reviewer. Retain selected fanotify mode, strict ancestry/build identity, actual test assertions and script logs; do not interpret a SKIP as acceptance. Each kernel supplies its negotiated mode; the daemon unmount test does not force both modes.

```sh
export CARGO_NET_OFFLINE=true TIDEMARK_DEPLOY_TEST_VM=1
umask 022
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s daemon_sigterm_releases_loop_xfs_for_ordinary_unmount -- --exact --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-cli --features privileged --test e2e -- --nocapture --test-threads=1
```

For the slow compaction witness, build binaries first and permit the actual three-hour workload. Unlike local build/check commands, its outer guest allowance must exceed 1,500 seconds:

```sh
TIDEMARK_FAULT_KEEP=1 TIDEMARK_FAULT_SLOW=1 TIDEMARK_FAULT_CYCLES=1 timeout 14400 bash tests/fault/04-compaction-crash.sh
```

The script needs the existing fault VM preparation and environment in `tests/fault/README.md`; do not run it on a production or workstation host. One cycle has kill, recovery and second-compaction phases, each enclosed by 3,800-second helper deadlines. Require the new sentinel/PID evidence, not only a PASS banner.

On an equipped ordinary-user CI host with Unix sockets, user namespaces and cargo-deny installed:

```sh
umask 022
CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh --require-deny
RUSTDOCFLAGS='-D warnings' CARGO_NET_OFFLINE=true timeout 1500 cargo doc --offline --workspace --no-deps
```

## Validation

Commands ran on the checkout at entry, with only this report being added. Environment: uid 0, explicit `umask 022`, pinned Rust 1.98.0 binaries, four Cargo jobs, copied writable registry at `/tmp/tidemark-review5-cargo`, and `CARGO_NET_OFFLINE=true`. Every long Cargo/check command has `timeout 1500`; no dependency fetch was attempted.

| Command | Result and evidence |
|---|---|
| `timeout 1500 tools/ci.sh` | Exit 0. Owner/forbidden-symbol checks (150 owner tags), fmt, all-target Clippy, strict-ancestry unit checks, workspace tests, and privileged-feature test compilation passed. Cargo-deny explicitly skipped because it is not installed. `library/ci.log`, `ci.exit`. |
| `timeout 1500 cargo build --offline --workspace --features privileged --tests` | Exit 0, compile only. `library/privileged-build.log`, `privileged-build.exit`. |
| `RUSTDOCFLAGS='-D warnings' timeout 1500 cargo doc --offline --workspace --no-deps` | Exit 0 with warnings denied. `library/rustdoc.log`, `rustdoc.exit`. |
| Standalone review probes | Lifecycle/reset and saturated persistence probes reproduce B1/B2/S1; empty/unrelated sampler probe passes; exact-candidate component-count probe intentionally fails (C1). Sources, method descriptions and logs retained in `library/`. |
| Shell/Python checks | Eight `bash -n` checks; 19 Python 3.6-grammar parses; actual watcher/compaction fixture executions pass. No Python 3.6 runtime or ShellCheck execution. |
| Scope/cleanup | `git diff --check` passes; only the requested report is changed. `cargo clean` exited 0 before submission (`library/cargo-clean.log`); build artifacts removed. |

CI reports 937 passes, zero failures and 14 ignored across 87 harness summaries, including nested child summaries and guarded no-op successes. These are not 937 distinct exercised scenarios. The F1/F2/F3/F5 named regressions and F6 real helper assertions have no unavailable-facility return and executed here. Product recovery, direct LVM, identity resolver/generated command, false-SKIP, and real flock exclusion assertions also executed. Synthetic retained-view tests exercised eventfd/thread shutdown without a fanotify mount. Test helper entrypoints that do nothing without their child marker are not credited separately.

The new root namespace case explicitly returns on uid 0 (`crates/tidemark-cli/tests/rules.rs:259`), so it did not assert ancestry here. The changed second-UID/sticky-parent e2e fixture and daemon ordinary-unmount case are privileged-feature tests compiled only. Socket/readiness/API and user-namespace assertions guarded by unavailable facilities are not credited from a harness `ok`; this sandbox cannot bind Unix sockets or create user namespaces. No guest, privileged workload, installed RPM or real second-daemon API listener was exercised. `library/ci.log` retains every reported test result.

## What I did not check

No VM was contacted, and no privileged fault/install/unmount test was run. Real Rocky 8.8/8.10 behavior, both fanotify modes, cross-namespace mapping, kernel SIGTERM/unmount completion, actual hourly compaction, Python 3.6 runtime, real ShellCheck and cargo-deny audit remain pending. There is no whole-daemon RSS, CPU/scale/soak benchmark or physical clock-step experiment. The deterministic B1/B2 probes do not exercise a running kernel watcher or socket reload; their production call ordering is source-verified. Exact candidate was not fully rebuilt: current-tree checks and the isolated candidate failure are explicitly distinguished.

Packaging strictness, purge correctness and lease security belong to the other review; only their changed lifecycle/test seams relevant here were inspected. I did not re-review unrelated old code or plan/code drift, change product code, modify memory projections, create a branch, merge, push, or perform host service/storage operations. Canonical memory receipts were empty at both checks; no update needed acknowledgment. This report and its receipts are evidence candidates for independent disposition.
