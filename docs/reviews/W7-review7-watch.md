Commit at entry (`git rev-parse HEAD`): `6de8837a0b5c3e34b55411b3dfaf253c3fcdcbb6`.
Verdict: **READY FOR RELEASE CANDIDATE**.
Counts by class: **0 BLOCKER, 0 SHOULD-FIX, 0 NIT**. Four dispositions: **4 RESOLVED, 0 PARTLY, 0 NOT** (B1/B2 grouped).

Exact blind candidate: `717fb6a8ea8cb289674ba5b125a7abfeef2d2875`, base `67847fb2afb3ed8510a29e6bc8594e002b5e4d87`. The entry checkout and candidate have identical trees. The requested `4f2a6a9..HEAD` delta additionally contains the previous review report; code changes are the same four Rust files. This report is an evidence candidate, not release authorization.

## Four dispositions

All named in-repository unprivileged regressions below passed in this run and executed their assertions. None relies on a root, Unix-socket or user-namespace early-return guard.

| Prior item | Disposition | Evidence and discriminating regression |
| --- | --- | --- |
| F1: retained views show measured-empty history | **RESOLVED** | `crates/tidemark-live/src/watch/mod.rs:845-853` checks retirement, coverage and rows under one lock. Retirement is permanent and precedes reclamation (`:834`, `:1207-1221`). `crates/tidemarkd/src/state.rs:1226-1230` propagates any selected retired watcher to unavailable. `held_mature_view_is_unavailable_after_retirement` (`services/watcher.rs:1141`) requires twelve previously lossless 82-byte queries to become unavailable, replacement rows to appear, and held `now` to name retirement. |
| F2: dispatch loses history-start guidance | **RESOLVED** | `crates/tidemarkd/src/api.rs:267-273` parses once and uses the detailed unavailable error. `hot_dispatch_reports_startup_and_replacement_boundaries` (`:1003`) calls the real dispatcher without sockets and checks exact startup/replacement and retirement text. |
| F3: escaped names restart on reload | **RESOLVED** | `services/watcher.rs:640-679` compares raw mount paths and relevant exclusions; `state.rs:346-363` and `services/watcher.rs:585-603` map presentation labels back to configured raw identities for windows. `reload_preserves_raw_mount_history_and_windows` (`:1051`) checks backslash/newline mounts, actual history/window retention and relevant-exclusion retirement. |
| Prior B1 and B2 decisions | **RESOLVED / no regression found** | B1 retains delayed-time clamping and genuine rollback detection; `delayed_status_after_drain_does_not_rollback_history` (`services/watcher.rs:1423`) requires retained 82-byte lossy rows and no repeated delivery. B2 retains unchanged watcher Arcs/cursors, flushes eligible completed rows once, and discloses incomplete startup/replacement coverage; `reload_membership_preserves_unchanged_watchers_and_flushes_replacements` (`:1228`) checks those properties and `now` boundary expiry. Persistence here means queue handoff, not unconditional durable database storage. |

## Findings

No new in-scope finding. The dispositions and limitations below are review evidence, not release or VM-gate approval.

### Retirement and coverage reasoning

`query_at` holds the watcher mutex from the retired check through the coverage decision and row copy. A query before retirement can return its valid copied measurement; a query after retirement returns `Err(retired_at)` without advancing or rebuilding hot rows. The caller converts that into unavailable even when other selected mounts have rows. The combined-mount regression (`services/watcher.rs:830`) also requires retirement to take diagnostic precedence over another mount's startup gap.

For concurrent `now`, both directory and unit queries run before the retirement diagnostic (`state.rs:1068-1112`). If retirement suppresses either query, the later check observes the permanent marker. If retirement starts after that check, both row sets have already been copied. This is a source-ordering argument, not exhaustive concurrency model checking. The new barrier regression queries before retirement and after the worker has completed retirement/replacement; it does not pause a query inside the critical section.

The actual dispatcher regression requires `unavailable: watch history for /A starts at 00:01 UTC; no complete window yet`, then `unavailable: watch history for /A starts at 00:01 UTC; ask for a window of at most 1 minutes`. Its replacement repeats those checks with `00:03 UTC`; retirement reports `unavailable: watcher for /A was replaced at 00:02 UTC` and `00:04 UTC`. The grammatical plural is the tested text. The generic `available(...)` implementation would fail these exact assertions.

B1 still clamps delayed observations under the watcher lock (`watch/mod.rs:898,919,940,1158-1160,1356-1369,1384-1392,1429-1432,1553-1555`). Fresh clock observations intentionally detect real rollback and sample under that lock (`:767-774,1151-1153,1297-1299`). `rings.rs:266-281` retains buckets and delivery watermarks while marking affected minutes lossy. The injected rollback tests establish the transition; no host clock was changed.

For F3, raw `Path`/`PathBuf` identity is used for watcher membership, relevant exclusion prefixes, immutable-window retention and requested alert-window matching. Escaped labels are mapped back to raw configured paths at their boundaries; escaping is also used consistently for component labels (`state.rs:369-377`, `services/watcher.rs:691-694`). No new identity map or contract change was introduced. This disposition addresses the reported reload defect, not a claim that every preexisting path-label consumer throughout the daemon was rewritten.

### Persistence, memory, shutdown and consumers

Reorder/extension retains the actual watcher Arc and its delivery cursor. Both identity and settings retirement flush eligible completed minutes through the same cursor before retirement (`services/watcher.rs:126-127,620-637,684-685`). The B2 regression requires three scope rows and no repeat after another retirement. This establishes once-only handoff of those rows to the bounded queue, not unconditional exactly-once SQLite durability. Full/disconnected queues and writer errors remain lossy. The existing flush-before-join sequence does not test a real event or minute boundary racing the final drain; the diff does not change that ordering.

`retire_at` releases the state guard before entering `shutdown`; shutdown serializes callers with the thread mutex, marks direct shutdown too, signals and joins the worker, releases the mount descriptor, then clears retained rows/windows (`watch/mod.rs:833-835,1200-1221`). Retained window handles become unavailable (`:225-227,393-404`). Daemon shutdown still joins API/producers before clearing watchers and flushing/stopping the writer (`tidemarkd/src/lib.rs:1393-1417`). The new state is one optional timestamp; it preserves the 64 MiB watcher credit, bounded mount/window counts, sixty-minute history, 8 MiB service history and bounded writer channel/messages. No new unbounded collection was found; this is source review, not an RSS measurement.

Metrics use the repaired hot fallback and omit lossy rows (`metrics.rs:432-479`); reclaimed immutable windows return `None` (`state.rs:1450-1453`). An immutable window may be copied between retirement marking and reclamation; it remains a previously completed measurement. After reclamation it is unavailable, never a synthetic zero series. CLI `render/hot.rs:34-50` preserves JSON quality and the text LOSSY column. `render/now.rs:256-275,349-355` prints row quality and degraded explanations. The shared error mapper (`cmd/now.rs:53-70`) preserves daemon unavailable text and method context. Relevant operator-guide and clap/man-generation source statements were inspected; no new false operator claim caused by this delta was identified.

### Test discrimination and cleanup

| Test | What would fail if the behavior were removed; facility/timing limits |
| --- | --- |
| `held_mature_view_is_unavailable_after_retirement` | All twelve held queries must become unavailable; fresh queries must contain 82 bytes; held `now` must name replacement. A joined thread and a two-phase barrier order retirement. No privilege/socket/namespace guard. A panic before the second barrier can strand the peer until process timeout. It does not assert omitted `now` rows or fresh-row quality. |
| `hot_dispatch_reports_startup_and_replacement_boundaries` | Generic dispatch errors, missing startup guidance or missing retirement handling fail exact text/code assertions. Real public dispatcher, injected clocks, no transport or facility guard. |
| `reload_preserves_raw_mount_history_and_windows` | Display-string membership or exclusion comparison fails backslash/newline cases; lost requested-window mapping fails the initial window publication. Reorder/extension must retain Arc, hot rows, nonempty minute history and window timestamp; a relevant exclusion must retire/invalidate them. No facility guard. |
| `combined_mount_window_returns_known_rows_and_names_the_incomplete_mount` | Requires available combined rows to be lossy, uncovered empty windows to be unavailable, coverage to recover and retirement to win diagnostic precedence. No facility guard. |
| Existing B1/B2 regressions named above | Clamp removal, rollback clearing, restart-all reload, omitted flush, duplicate delivery or missing coverage labels fail explicit assertions. B1 barriers/join really run; its failure path can strand a peer until timeout. B2 `now` also invokes real-clock stats, so its expiry assertion proves warning removal rather than a fully frozen-clock snapshot. |
| `concurrent_shutdown_joins_once_and_reclaims_retained_windows`; `retained_view_does_not_delay_retirement_before_replacement` | Actual eventfd/synthetic-worker tests require one teardown, released descriptors, reclaimed window payload and replacement after release. All normal-path threads are joined; no fanotify/socket/namespace guard. |
| `incomplete_startup_hot_windows_are_omitted_until_covered` | Requires no premature hot series and three covered series; real metrics renderer, no facility guard. |

No absence-of-fix mutation run was performed; discrimination above follows the assertions and inspected production paths. Feature-gated guest tests are compilation-only here. No new child process is introduced by the patch's tests.

## Validation and limits

All local commands used Rust 1.98.0, `CARGO_HOME=/tmp/tidemark-review7-cargo` (a private copy of the populated host registry), `CARGO_NET_OFFLINE=true`, and explicit `umask 022`. The sandbox ran as uid 0. Long commands were bounded by `timeout 1500`; no privileged tests were executed. Supporting files are in this attempt's external `library/`, under `attempt-4555fd20248cb952b45e01c21fda1b18d60697c13ea29f3ff6f7811224756e5a`.

| Command/check | Observed result | Evidence |
| --- | --- | --- |
| `timeout 1500 tools/ci.sh` | Exit 0: 150 owner tags, forbidden-symbol scan, format, Clippy with warnings denied, strict-ancestry subset, workspace tests and privileged-feature compilation. Cargo-deny explicitly skipped because absent. | `library/ci.log`, `ci.exit`, `ci-summary.json` |
| Separate `timeout 1500 cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only. | `library/build-privileged.log`, `build-privileged.exit` |
| External consumer probe, offline Cargo and `timeout 1500` | Exit 0; all assertions executed. | `library/consumer-probe/probe.rs`, `consumer-probe/Cargo.toml`, `consumer-probe.log`, `consumer-probe.exit` |
| `timeout 1500 cargo clean` | Exit 0; removed 35,018 files / 20.3 GiB before submission. | `library/clean.log`, `clean.exit` |
| Candidate/checkout identity | Same tree `818fa5b00476c92af138c847e33be8e49dc0f651`; full committed-tree diff empty. | `library/candidate-tree-diff.txt` |

The external consumer probe links the unchanged production crates, with the same dependency versions as the workspace lockfile. It checks twelve lossless mature queries; a retained immutable alert window; populated `now` directory/unit rows before retirement and their omission with a retirement diagnostic afterward; omitted retired metrics; twelve fresh replacement queries including quality flags; CLI lossy markers at widths 80/100/200 and unchanged integer/quality JSON. It uses no socket, namespace or kernel watcher guard. Its fixture minute is initialized from wall time because the existing `now`/metrics stats deliberately sample real time; observation times are then injected. It is not a concurrent scheduling stress test.

CI output contains 87 successful summaries reporting 948 passes, zero failures and 14 ignored tests. These are raw totals including nested subprocess summaries, the strict-ancestry subset and facility-guarded passes; they do not represent 948 independently complete acceptance cases. All specifically relied-on in-repository tests in the table above ran their assertions here. The five `w6a1` socket-readiness guards (`tests/w6a1.rs:530,673,730,843,920`), other Unix-socket/user-namespace guards and root-sensitive permission checks are not credited as transport or ordinary-user evidence merely because Cargo reports success. Cargo-deny advisories/license checking remains pending an equipped host.

## For the VM run

Pending VM. Inside each disposable, prepared Rocky 8.8 and 8.10 guest, from the reviewed checkout with cached dependencies, run as root:

```sh
export CARGO_NET_OFFLINE=true
umask 022
timeout 1500 cargo build --offline --workspace --features privileged --tests
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s daemon_sigterm_releases_loop_xfs_for_ordinary_unmount -- --exact --nocapture
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
```

Record watcher mode/readiness, real event delivery, reload continuity, SIGTERM exit, ordinary unmount and cleanup results. A guarded skip is not acceptance. Separately, on an equipped ordinary-user host allowing Unix sockets and user namespaces, run `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh --require-deny` at `umask 022`. No guest was contacted here.

## What was not checked

No real fanotify negotiation, cross-namespace event delivery, mount identity replacement, XFS ordinary unmount, LVM, Rocky runtime, service/deploy operation or privileged cleanup was executed. No host clock was changed. No performance/RSS/scale/soak measurement, sustained concurrency stress, model checking or new database-failure injection was performed. Transport-level CLI errors are source-reviewed, not exercised through a socket here. Manuals are generated from clap; installed man pages were not tested. This is the targeted WATCH-rc2 delta review with relevant candidate-tree dependencies, not a renewed full architecture/security or plan-drift audit.

Canonical memory brief/input/receipts were read; no pending receipt required adaptation. Only this review report is changed in the repository. No implementation, memory projection, branch, merge, push or release operation was performed.
