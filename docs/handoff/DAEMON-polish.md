# Handoff — DAEMON-polish: readable daemon output

## Needs from coordinator

- Update the unowned assertions in `crates/tidemarkd/tests/w6a1.rs:1600` and `:1601` from `Some(0.0)` to `None`. The assigned task requires missing rates before five minutes; that integration test encodes the earlier W6-mount-state zero-rate presentation. This worker cannot edit `crates/tidemarkd/tests/**`. Required workspace/CI outcomes are recorded below, including this expected incompatibility.
- Run unrestricted socket tests and privileged watcher tests on disposable Rocky 8.8/8.10 guests. Compile-only and socket-denied early returns are not guest acceptance.
- Decide separately whether aggregation caps should degrade component health. This patch deliberately preserves the existing state and alert availability.

Attempt: `attempt-c71b51263f6c86d59cf44dd02de399037ac3efdfada43633ffc7b2ad4bae63d9`; base `e44fae13e3dc3790cba4f3d5b667475126d97efb`. Date: 2026-10-13. Status: implementation and evidence candidate, independent verification pending. Existing attempt branch used; no branch creation, merge or push.

## Findings and changes, in requested order

### 3. Process inventory budgets

`resource_pressure` recognized raw ENOMEM but missed an `io::Error` carrying a typed error with `ErrorKind::OutOfMemory`. It now recognizes that kind as well as the original OS errors. The scan-service failure detail downcasts `tidemark_fs::procfs::InventoryBudgetExceeded`, preserving its descriptor count and accounted retained bytes. It publishes degraded and unavailable alert input, with the ordinary retry behavior intact.

Old detail: `proc inventory byte budget exceeded: <count> descriptors, <bytes> bytes retained` (component state error).
New detail: `process inventory over its memory budget: <count> descriptors, <bytes> bytes retained` (component state degraded).
Other pressure scan failures now have `process scan failed: <error>` instead of just `<error>`. Setup errors and non-pressure scan errors retain their original component detail.

Tests: `typed_memory_pressure_is_degraded`, `inventory_budget_detail_keeps_counts`.

### 1. Watcher caps and loss

The 200,000-entry distinct-identity table is shared by directory, unit and command scope. Each distinct file consumes three entries, so a single directory/unit/command burst counts 66,667 directory identities before admission stops while all 100,000 events continue counting. The old branch marked only lossy, missing the CompletedMinute truncation fact. It now sets truncated as well. Legacy lossy flags, persistence flags, lossy-minute counters, degraded state and unavailable exact hot windows remain unchanged. R4-7 explicitly describes cuts as truncation/loss; the correction adds the missing truncation fact rather than removing the compatibility loss flags. No artificial queue overflow is recorded.

Private per-bucket reason flags distinguish distinct-file, current-key, completed-key and requested-output caps, read failures and descriptor pressure. Queue overflow uses actual boundary counters. The daemon appends minute-qualified reasons to its existing component detail. Mixed cap and transport failures report both. At most eight causes are reported, each with its latest retained occurrence, bounding component detail size independently of repeated failures. UTC HH:MM and the epoch-minute timestamp remove timezone/date ambiguity. A distinct-file cap coinciding with transport loss says only `(per-file counts capped)`, avoiding a false claim that events were complete. Remaining generic loss is described as transport recovery or history gap, rather than falsely calling it kernel queue loss.

Old component detail: `mode=<mode> recovering=<bool> overflows_total=<count> read_errors=<count> persistence_dropped=<bool>; windows: <count> published`.
New detail: the same prefix, followed by `; <minute reasons>` when present. The reason text is:

- `truncated: distinct-file cap reached in the minute <HH:MM> UTC (timestamp <ts>) (events complete, per-file counts capped)`
- `truncated: current-key cap reached in the minute <minute>`
- `truncated: completed-key cap reached in the minute <minute>`
- `truncated: requested-output cap reached in the minute <minute>`
- `lost events: queue overflow in the minute <minute>`
- `lost events: read error in the minute <minute>`
- `lost events: descriptor pressure in the minute <minute>`
- `lost events: transport recovery or history gap in the minute <minute>`

`07` §3.3 says current-key eviction marks approximation, completed lists are capped at 1,000, and any included truncated/lossy minute makes hot alert data unavailable. `02` §9 explicitly describes queue-overflow degradation; neither section explicitly decides the component state of an aggregation cap. That remains a contract question. Existing component health is preserved here.

The additive `Watcher::health_detail(&self) -> String` exposes diagnostics without changing any existing callable signature or the frozen CompletedMinute fields. Existing owner headers are preserved. Diagnostics examine only the bounded ring; no new syscall, metadata write or file-content read occurs.

Tests: `distinct_file_cap_marks_truncation_and_preserves_event_totals` uses the injected constructor with 100,000 identities and asserts events 100,000, files 66,667, truncated/lossy and zero overflow. `distinct_cap_detail_and_alert_window_remain_unavailable` exercises the daemon's production component formatter and minute-window reader. `diagnostics_distinguish_caps_from_each_transport_loss` verifies concurrent reasons and the minute; `diagnostics_retain_only_latest_occurrence_per_cause` checks repeated failures remain compact. Existing watcher window/alert tests continue to check unavailable loss/truncation and exact overflow boundaries.

### 2. Self-contained journal messages

All daemon INFO/WARN/ERROR calls were audited, along with the three watcher-library calls emitted by the daemon. Values needed by an operator now appear in the message as well as structured fields. Optional descriptor limits display a number or `unlimited`; structured string fields use Display rather than debug formatting. The namespace error previously emitted only a structured reason, whose text included a debug Result; it now emits plain reason text. The associated disabled component detail changes from `host mount namespace unavailable: <Debug Result>; watcher disabled` to `host mount namespace unavailable: <plain check failure>; watcher disabled`. Panic payloads render their String/&str content or `non-text panic payload`. Paths added to scan messages use `RawPath::display_lossy()`.

API requests are logged after handler completion with method, peer credentials, success/failure and elapsed milliseconds. The log does not wait for successful delivery to the socket. Shutdown and failed handler-thread spawn keep the existing early-return behavior and log a failed request before returning, preserving the original request audit coverage.

Every changed message template is listed below. Angle brackets denote runtime values; structured fields remain in addition. Unchanged daemon ready/stopping, mount transition and cancellation-wait messages already carry the necessary facts.

| Old message | New message |
|---|---|
| `effective descriptor limit` | `effective descriptor limit: soft <soft>, hard <hard>` |
| `could not raise descriptor soft limit` | `could not raise descriptor soft limit: <error>` |
| `descriptor budget reduced; minimum batch may exceed available reserve` | Same prefix plus `: soft <soft>, needed <needed>, mounts <mounts>, batch <batch>` |
| `startup resource pressure; retrying` | `startup resource pressure; retrying <component>: <error>` |
| Structured reason only, no message | `host mount namespace unavailable: <plain check failure>; watcher disabled` |
| `config reload` | `config reload: accepted` |
| `config reload rejected` | `config reload rejected: <error>` |
| `firstboot followup failed` | `firstboot followup failed: <error>` |
| `service implementation pending` | `service implementation pending: <component>` |
| `service degraded at shutdown` | `service degraded at shutdown: <component>: <error>` |
| `keeping previous config` | `keeping previous config: reload rejected: <error>` |
| `time-series writer failure` | `time-series writer failure: <error>` |
| `boot timestamp unavailable` | `boot timestamp unavailable: <error>` |
| `Tidemark first-boot report published` | `Tidemark first-boot report published: healthy <bool>, version <version>, config sha256 <hash>` |
| `API request` | `API request: <method> from uid <uid> pid <pid>, <ok or failed> in <milliseconds> ms` |
| `API socket cleanup failed` | `API socket cleanup failed: <error>` |
| `API lock cleanup failed` | `API lock cleanup failed: <error>` |
| `API accept degraded; retrying` | `API accept degraded; retrying: <error>` |
| `scan started` | `scan started: root <path>, scan id <id>` |
| `post-scan publication incomplete; dirty state retained` | Same prefix plus `: <error>` |
| `scan failed` | `scan failed: root <path>, mode <mode>, source <scheduled or operator>, attempt <attempt>, retry timestamp <timestamp>: <error>` |
| `scan completed` | `scan completed: root <path>, scan id <id>` |
| `scan worker panicked during scheduler cleanup` | Same prefix plus `: <plain panic reason>` |
| `scan inventory publication rejected; scheduler continuing` | Same prefix plus `: <error>` |
| `scheduled scan capacity rejected; scheduler continuing` | Same prefix plus `: <error>` |
| `metrics exporter degraded` | `metrics exporter degraded: <error>` |
| `watcher started` (watch library) | `watcher started: mount <path>, mode <mode>` |
| `fanotify poll failed` (watch library) | `fanotify poll failed: <error>` |
| `fanotify read failed` (watch library) | `fanotify read failed: <error>` |

`log_tests.rs` pins all 28 daemon message templates and the three watcher-library templates to production source and formats fixture values through the real tracing subscriber into a buffer, checking the rendered text and rejecting `Some(` and quotes. It also invokes the actual descriptor-limit, API-request and descriptor-raise-failure logging helpers. A precise source scan checks daemon log-call structured fields for debug `?` formatting. This is formatting coverage, not forced runtime execution of every error branch. Other libraries (store, fs, alert delivery) can emit their own messages and are outside this task’s daemon/watch call-site audit and write scope. Existing mount-transition captured-log tests are retained.

Integration tests were searched for changed detail/message strings. Old scan and lifecycle substrings are preserved; the incompatible early-rate assertions are listed at the top. No unowned integration test was edited.

### 4. Collecting rates

The five-minute history gate already existed for alert/metrics sources and all requested windows. `now` bypassed the gate's missing result with `Some(0.0)` for both 15-minute and one-hour rates. It now publishes the direct optional fitter results: no rate and no time-to-full until five minutes of elapsed sample history, then the usual minimum-three-sample slope for each window. The injected regression covers 66, 299 and 300 seconds and 1m/5m/15m/1h source windows.

Old JSON: `rate_15m: 0.0`, `rate_1h: 0.0` during collection. New JSON: both null. `rate: collecting <mount>` remains in degraded reasons.

CLI inspection: `crates/tidemark-cli/src/render/now.rs:226` already renders a missing 15-minute rate as `unknown`. No CLI correction is needed for this finding and no CLI file was changed. The older W6-mount-state handoff explicitly describes/proposes zero display rates; the assigned DAEMON-polish instruction supersedes that presentation choice, without changing optional wire types.

Test: `now_rates_are_missing_until_five_minutes_for_every_window`.

### 5. Disabled metrics

After preserving generation/interval validation, an empty textfile path now publishes disabled with a reason immediately and skips cache reconciliation/export work until configuration enables the exporter. Enabled success still says exported. This prevents a leftover exporter detail or irrelevant cache failure from appearing as disabled metrics health.

Old disabled detail: `exported`. New disabled detail: `not configured: metrics textfile path is empty`.
LVM’s vague disabled detail `configuration` now says `not configured: lvm disabled by config`. Other disabled component details already explain disabled-by-config, no watch mounts, implementation pending or namespace failure; no other stale `exported` detail exists.

Tests: `disabled_exporter_reports_why_without_reconciling_snapshots` starts the actual disabled service and checks its published component before joining it. `disabled_lvm_reports_why_without_collecting` checks the LVM reason and that disabled collection/mount reads never run.

### 6. Writers unavailability

The text originated in the daemon's generic `available` helper. The writers dispatch now uses process-specific details. Initial/reconfiguration waiting, disabled configuration and failed inventory scans are distinguished; memory-budget failures keep their counts. The generic helper remains unchanged for other methods outside this item.

Old response: `unavailable: measurement unavailable`.
New responses:

- `unavailable: process scan has not completed yet`
- `unavailable: process scan not configured: disabled by config`
- `unavailable: process scan failed: <component reason>`
- `unavailable: process inventory over its memory budget: <count> descriptors, <bytes> bytes retained`

Baseline writers/phantom display APIs intentionally retain the last completed report after failures/expiry; alert inputs separately become unavailable. This patch preserves that behavior, so no new report-age rejection or `report older than <n> s` path is introduced. There is no CLI-origin text to correct for this finding.

Test: `writers_unavailable_names_the_process_scan_and_failure` dispatches the actual API for initial, budget failure, ordinary failure and disabled states.

## Assumptions and boundaries

- The task’s explicit daemon source search defines the log audit scope; owned watcher-library calls were included too. Store/fs/alert-delivery dependency logs require their separate owners for a process-wide journal guarantee.
- No worker questions were sent. UTC minute labels, additive diagnostics, preserved lossy compatibility and retained display reports are the choices documented above.
- Caps keep their existing lossy/degraded behavior; this work adds factual reason/truncation reporting and does not decide a new state policy.
- Only owned paths changed. No manifests, dependencies, frozen core types, config keys, CLI flags, metric names, API methods or external-call signatures changed. The test-only `mod log_tests` declaration in daemon lib.rs is the only added module declaration.
- No privileged test was executed, no system directory/service/mount/LVM operation was performed, and no regular-file content scan was added. Existing feature-gated watcher tests are compiled only, pending VM.
- Formatting expanded existing compact watcher service code as part of required cargo fmt; those surrounding changes are formatting only.
- Socket listener tests can return early in this sandbox; their harness passes do not establish transport acceptance. Dependency advisory checks depend on locally installed cargo-deny and available data.

## Proposed plan text (not applied)

For `07` §3.3: “The distinct-file identity table holds 200,000 scope/key/file tuples shared across the three scopes. Reaching this bound truncates per-file counts while event counters continue. Completed facts set truncated, preserve legacy lossy flags, and keep affected exact hot windows unavailable. Status identifies the bound and UTC minute separately from queue overflow, read error and descriptor pressure. Component state policy for caps remains separately decided.”

For `07` §2 / W6-mount-state presentation: “Filesystem rates require at least five minutes of in-memory history for the current mount identity and at least three samples in the requested fit window. During collection, now rate fields are None, time-to-full is None, and rate-dependent metrics/alerts remain unavailable.”

For W6 logging/API: “Every daemon INFO/WARN/ERROR message includes its operator-relevant values in journal MESSAGE using Display and byte-path display helpers. API request completion includes method, uid, pid, result and elapsed milliseconds. Typed memory-budget failures are degraded with descriptor/byte counts. Disabled components name why, and writers with no report names the pending/failed/disabled process scan.”

## Guest observation commands — pending VM

On the already provisioned disposable guest, with the root-only installed CLI:

```sh
sudo tidemark status
sudo tidemark hot --json --window 15m
sudo journalctl -u tidemarkd --since '-5 minutes' --no-pager
sudo tidemark now --json
sudo tidemark now
sudo tidemark writers --window 5m
```

1. Repeat the operator's controlled 100,000-distinct-file single-minute burst under a watched disposable directory; status should name the distinct-file cap/minute, hot retain the legacy lossy flag and 66,667 per-minute files, event counter include all writes and overflows remain zero. Use injected cap/window tests for deterministic alert availability evidence; do not infer real queue completeness if guest overflow counters increase.
2. Start the packaged daemon and issue status; journal MESSAGE should contain both descriptor limits and the completed API request's credentials/result/duration. Exercise a disposable missing scan root to inspect scan failure values.
3. Use the long-target synthetic budget fixture from AUDIT-procfs-budget, or its coordinator-controlled guest memory-budget experiment; status should show degraded and retained descriptor/byte counts. No host stress experiment was run here.
4. Inspect now after the first samples and after five minutes on one unchanged mount identity: early JSON rates null/text unknown; later fitted rates appear. Remount/restart history-reset tests remain the coordinator's guest responsibility.
5. On a guest config with the metrics textfile path empty, status must say not configured. Reenable via the established config reload workflow to check exported success.
6. Query writers before the first successful process scan, then after a controlled injected/guest scan failure without any prior completed report; compare the pending/failure reasons. Retained completed display reports remain available as documented.

## Validation

All long commands use timeout 1500, Rust 1.98.0 and cached offline dependencies. No new dependency was added. Full logs and machine-readable exit/count summaries are in this attempt's external output `library/`. Final outcomes are appended below. A late workspace run exposed an existing 100 ms socket-pair fixture timing race in `cooperative_reload_exits_on_deadline_and_shutdown`: BrokenPipe at its first request write, before dispatch/logging. The owned test now allows one second; its three-second read limit still detects missing deadline cancellation. Production timeouts are unchanged. The failing full-run evidence and subsequent focused results are retained.


### Recorded command outcomes

Counts below sum reported harness result lines, including subprocess harnesses; they are not a count of unique declarations or proof of socket/VM execution. Final source follow-up logs are explicitly distinguished from the earlier full run.

| Command | Exit | Evidence/counts |
|---|---:|---|
| `cargo fmt --check` | 0 | `library/submission/fmt.log`; completed |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `library/submission/clippy.log`; completed |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0 | `library/submission/clippy-privileged.log`; completed |
| `cargo test --offline --workspace --no-fail-fast` | 101 | `library/submission/workspace-tests.log`; 797 passed, 2 failed, 14 ignored |
| `cargo build --offline --workspace --features privileged --tests` | 0 | `library/submission/build-privileged.log`; completed |
| `tools/ci.sh` | 1 | `library/submission/ci.log`; 688 passed, 1 failed, 14 ignored |
| `cargo test --offline -p tidemarkd -p tidemark-live --lib` | 0 | `library/submission/targeted.log`; 164 passed, 0 failed, 0 ignored |

After the test-only one-second socket-pair budget adjustment, final fmt, both workspace Clippy modes, feature-enabled test compilation and targeted libraries were repeated:

| Check | Exit | Evidence |
|---|---:|---|
| fmt | 0 | `library/submission/followup-fmt.log` |
| clippy | 0 | `library/submission/followup-clippy.log` |
| clippy-privileged | 0 | `library/submission/followup-clippy-privileged.log` |
| build-privileged | 0 | `library/submission/followup-build-privileged.log` |
| targeted | 0 | `library/submission/followup-targeted.log` |

The targeted final result is 164 passed, zero failed (76 live + 88 daemon unit tests). Twelve in-source regressions were added: three live and nine daemon; one existing owned API timing fixture was adjusted. All new regressions passed. The API-only timing follow-up passed 15/15 (`api-timing-followup.log`).

The last standalone workspace run had 797 passed, two failed, 14 ignored: the unowned early-rate assertion and the pre-adjustment 100 ms cooperative reload fixture. The preceding full run had 798 passed, only the early-rate failure and 14 ignored (`library/final/workspace-tests.log`). The subsequent CI run had 688 passed, only the early-rate failure and 14 ignored, then stopped. The timing fixture passes in final focused runs and the final CI daemon harness (88/88). No claim that a completely clean standalone workspace run was obtained is made. No full standalone workspace rerun after the test-only timing adjustment was performed.

CI source policy reports 147 owner tags and a clean forbidden-symbol scan; format/default Clippy passed before CI's expected assertion stop. CI's later privileged-build and deny stages were not reached; privileged build and Clippy were completed separately. No dependency advisory freshness result is claimed. No privileged test was executed; existing real watcher tests remain pending VM.

Earlier iteration logs (`library/` and `library/final/`) are retained separately. The first workspace run also exposed the metrics validation-order mistake; it was corrected, the affected existing w6b test passed, and the later workspace w6b harness passed 13/13. Final compiler/lint/test evidence is under `library/submission/`, with the follow-up distinction above.

## Definition of Done evidence candidate

- Owned source paths and this handoff only; no manifest/dependency/frozen-type/CLI/plan edits.
- Final fmt and both Clippy modes pass; privileged tests compile only.
- All twelve new regressions and both affected timing/metrics follow-ups pass.
- Standalone workspace/CI clean gates remain blocked by the explicit unowned early-rate expectations; the prior timing failure and follow-up evidence are retained transparently.
- Guest execution, unrestricted socket coverage, independent verification and the separate cap-state policy decision remain pending.
- Handoff contains findings, every changed text template/detail, assumptions, guest observations and proposed plan wording.
