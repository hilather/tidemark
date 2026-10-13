# Handoff — WATCH-fd-budget

## Needs from coordinator

Run the socket-dependent root binary test and the revised fault script in disposable Rocky 8.8/8.10 guests. Run shellcheck at default severity and cargo-deny in equipped CI. Local validation is an evidence candidate; guest descriptor peaks, event loss accounting and watcher performance remain unmeasured. See the validation table below for sandbox limitations.

Attempt: `attempt-c1ce88396586f949fb6e87c2af4943d3748744ca40ff9e0dd0181049fe44339d`. Base: `7388c1016b17448ed4c261a3b088518eb9bfd07f`. Work remains on the supplied attempt branch; no branch creation, merge or push.

## Changes by decision

1. **Bound descriptors.** Fd mode allocates a 6,144-byte transport buffer, with each read receiving only the configured 24-byte-record batch slice. The process-wide setting is clamped to 16–256 records and consulted before every read, including existing workers. FID transport keeps its existing 64 KiB buffer because FID records have variable-length handle/name information and carry no event descriptors. Collected records retain their RAII descriptors and close serially, including filtered/error records. No sys or fdmode adapter change was needed.

2. **Limit and budget.** Startup uses rustix getrlimit/setrlimit to raise the soft NOFILE limit to the hard limit and logs effective limits once. A refused set logs a warning and retains the actual limit. Watch configuration changes retire existing workers before recomputing the process-wide batch from `(soft - 512) / configured mounts`, clamped to 16–256. A low limit logs the needed `256 * mounts + 512`, mount count and reduced batch once per watch configuration. Disabled watchers reserve no batches. The configured count is conservative when some mounts are absent or cannot start.

3. **Contain pressure.** Accept classifies EMFILE, ENFILE, ENOMEM, ENOBUFS and ECONNABORTED as transient. The actual accept seam returns a retry rather than an error to the shutdown branch. Backoff grows from 50 to 800 ms, warns at most once per minute, publishes degraded health, and observes shutdown in 20 ms slices. A successful accept or idle WouldBlock after recovery clears degradation. Other accept errors retain the fatal path.

   Scheduling catalog opens and latest reads, including initial scheduling inventory, publish degraded and continue on resource pressure. Scan-thread spawn pressure releases its active/inventory reservation and replies with an error without stopping the scheduler. Resource I/O returned by completed jobs is contained. Non-resource scheduler errors keep existing behavior.

   The watcher mount-table seam retains exact watcher Arcs and minute histories during resource errors, without authorizing a new mark. A transient namespace-check failure also retains existing marks and retries. Successful mount-table reads continue to reconcile replacements/removals normally; other failures retain existing retirement behavior. The sampler retains prior samples and identities on transient mount-table/statvfs errors. Process scanning retains its previous report and collector after a resource scan failure. Config changes still invalidate incompatible collectors/reports.

   The writer retains its stores and bounded pending filesystem rows during transient storage errors, reports degraded and retries on its normal pass. Invalid rows still drop so later valid batches recover. If a failed flush leaves the pending batch full, incoming rows are rejected rather than exceeding the cap. Other message types keep their existing error-bearing acknowledgements or loss/degraded behavior; this does not promise replay of failed Hot/Phantom/Writers inserts. A resource failure on final shutdown flush is reported degraded rather than turning SIGTERM into a resource-error exit. The writer supervisor reports resource spawn/reopen failures degraded and retains its existing bounded backoff/restart behavior. Metrics already contained export/cache errors as degraded and retried on the next tick; its source needed no change.

   Lifecycle supervision retries resource failures from service constructors, initial configuration/state-directory opens, catalog/history and time-series initialization. Retries publish component degradation, warn at most once per minute, and observe shutdown with 50–800 ms backoff in 20 ms slices. Catalog recovery remains restricted to the original typed corruption predicate. A temporary startup namespace error no longer permanently disables the watcher.

4. **Count loss.** Existing read-error recovery already clears cached paths/handles, increments read_errors and marks rings/dirty roots lossy for all four resource errnos. After each successful fd-mode read, a bounded procfs descriptor count with an 8 KiB stack getdents buffer (no per-entry heap allocation) checks the effective limit before processing/closing its batch. A full table, or a resource failure opening the descriptor inventory, marks loss and increments read_errors without inventing queue overflows. The inventory's own descriptor makes the last remaining slot conservative. The count visits at most the effective soft limit; an unlimited limit skips it. Existing completed-minute boundaries and cumulative overflow semantics remain intact.

5. **Attribution.** No attribution rewrite: the existing watcher lookup converts a failed `/proc/<pid>/stat` read to unknown/unattributed and invalidates that PID without propagating an error. It retains WATCH-perf's one-second validated-cache reuse; exact process-scanner validation remains unchanged. Attribution failure alone neither marks transport loss nor stops a worker.

## Fatal-error audit

- API accept: errors other than WouldBlock, Interrupted, the four resource errnos and ECONNABORTED still set shutdown and return an error. These indicate a broken/invalid listener or an unexpected transport failure rather than temporary allocation pressure.
- Scheduler: channel disconnection or shutdown ends the scheduler through its existing exit guard. Worker panics, non-resource job errors outside the already-contained Scan category, invalid scheduling/path representations, publication invariant/cap failures, and nontransient catalog errors still return errors. Continuing with corrupt storage or invalid inventory would fabricate scheduling facts. Ordinary scan failures still report/retry per the existing scheduler policy.
- Lifecycle initialization: invalid configuration, unsafe paths/ownership, lock conflicts, malformed nonrecoverable catalog/storage, non-resource service startup errors and invariant/cap failures remain fatal. Resource startup errors are retried; failed limit raising alone is never fatal. Corruption-only recovery preserves the original artifacts and predicate.
- Running sampler/procscan/watcher/metrics: metadata resource errors are contained in their own loops. Remaining propagated errors are validated-path/configuration or LiveState publication/cap/invariant errors. They do not directly set shutdown; supervision reports an exited handle, and cleanup joins and reports its error. Unchanged alerts/LVM use their existing error containment; lifecycle retries their constructor resource errors without changing their owned files.
- Time-series writer: ordinary persistence failures remain health/ACK errors, while the supervisor restarts unexpected worker exits/panics. Supervisor publication failures remain errors. A non-resource final flush failure remains an error; a resource final flush reports degradation. No pressure branch sets global shutdown.
- Cleanup: a returned resource errno is logged as degradation and does not fail the lifecycle join. Panicked joined workers, service-returned non-resource errors, a missed shared shutdown deadline and publication failures can still make the final result fail. Thread handles remain retained and joined, following CORE-fix-1. Uninterruptible kernel/SQLite calls and allocator aborts are outside recoverable errno handling.

## Acceptance evidence

- `api::tests::twenty_resource_accept_failures_back_off_then_recover`: production accept seam, twenty EMFILE results followed by a client, capped delays, one warning, degraded callbacks, successful reset and rejection of a genuinely fatal error.
- `scheduler::tests::catalog_descriptor_pressure_keeps_scheduler_pass_alive`: injected EMFILE returns a contained pass with degraded state; a subsequent real catalog open succeeds.
- `services::watcher::tests::mountinfo_emfile_retains_watcher_and_minute_history`: exact Arc and history survive injected failure and the next successful table read.
- `tests::descriptor_limit_raise_equal_and_refused`: injected get/set functions exercise raising, equal limits and EPERM, without modifying the test process limit.
- `tests::per_mount_descriptor_budget_reserves_headroom`: 256/128/32/16 budgets, unlimited and disabled cases.
- `tests::requested_shutdown_preserves_first_startup_attempt`: preserves pre-requested-shutdown cleanup and aborts only resource retries. The existing w6a4 repair tests pass after correcting this regression.
- `tests::resource_exit_does_not_fail_lifecycle_join`: a joined EMFILE worker does not turn cleanup into a resource-error exit.
- `tests::service_start_resource_pressure_retries_without_shutdown`: repeated injected ENOMEM constructors then success, retained shutdown flag and degraded evidence.
- `tests::sqlite_extended_pressure_codes_exclude_corruption`: extended SQLite NOMEM/IOERR/CANTOPEN classes are contained; corruption, constraints and read-only errors are excluded.
- `watch::tests::fd_reader_drains_five_thousand_in_bounded_batches`: production read-buffer seam drains 5,000 fake fixed-size events at batches 256, 32 and 16.
- `watch::tests::descriptor_limit_counts_and_unreadable_pressure_are_lossy`: limit/full/below/unlimited and inventory resource-error detection seams.
- Existing watcher transport retry/loss/backoff/descriptor-closure tests, writer invalid-row recovery, mount identity, exact windows, shutdown and corruption recovery tests retained.
- `w6a5::low_descriptor_limit_binary_serves_twenty_status_requests`: real shell hard/soft limit 64, one sampler mount and no watcher, twenty root status replies and exit 0 on SIGTERM. The temp directory is 0700; socket denial prints one line and returns here, and non-root CI also reports pending root execution. No real-binary survival claim is made locally.
- Revised fault script: three active 100,000-write bursts, status requests during each writer (100 ms pauses between requests), alive checks, event delta or overflow growth, lossy facts when events are short, and new post-burst activity. A fresh daemon under hard/soft limit 256 runs a fourth burst. Each uses its own cumulative baseline. Requests use the harness's existing 20-second timeout; this proves eventual responses, not a strict 100 ms response latency. Pending VM.

## Assumptions and proposed plan text

- If `soft < 512 + 16 * mounts`, the reserve and minimum-batch requirements cannot both hold. Keep the required 16-event minimum, warn with the full needed count, and start anyway. No refusal or silent reserve guarantee is claimed.
- SQLite can discard the underlying OS errno. Use its actual extended numeric code (masked to the primary code), retrying NOMEM, IOERR and CANTOPEN conservatively. This also contains some permanent I/O/open faults; degradation remains visible and corruption is not reclassified. No rusqlite dependency was added.
- Procfs inventory failure under resource pressure is conservatively lossy. Its sampling cannot prove the descriptor table was never briefly full and recovered before observation; the fixed batch and shared reserve prevent the reported fanotify-induced fill when the budget is satisfiable. Multiple unrelated descriptor consumers can still exhaust any finite limit.
- The procfs count adds metadata work per transport read. Guest WATCH-perf CPU/syscall measurement must be repeated; no performance attainment is claimed here.
- Public called signatures, core types, CLI/config keys, metrics and API methods are unchanged. The only public addition is the documented process-wide batch setter; no dependencies, manifests, shared module declarations, unsafe code, plan files or memory projections changed.
- Startup always performs its first attempt, including when shutdown was requested before entry; resource retries observe cancellation. Retries do not grant permission to bypass unsafe-path or storage validation.

Proposed plan text (not applied): “Fd-mode fanotify reads hold at most 256 event descriptors per watched mount. Raise NOFILE soft to hard at daemon start and reserve 512 descriptors for other components; reduce all watcher batches from the effective limit and configured mount count, with a minimum of 16 and a warning when the budget is unsatisfiable. Recoverable resource errnos degrade and retry rather than stopping daemon service loops. Read errors and observed descriptor saturation mark the minute lossy and increment read_errors; only kernel overflow records increment queue-overflow totals. Attribution failure remains best effort.”

## Guest commands

On a disposable root Rocky guest, after building binaries with its configured dependency cache:

```sh
timeout 1500 cargo test --offline -p tidemarkd --test w6a5 -- --nocapture
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 bash tests/fault/03-fanotify-overflow.sh
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --test-threads=1
shellcheck tests/fault/03-fanotify-overflow.sh tests/fault/common.bash
```

Privileged tests were compiled only on this workstation. No fault script, sudo, mounts, LVM, service enablement or host system-directory writes were performed. Existing privileged tests remain pending VM.

## Validation

Every long command uses `timeout 1500`; Cargo uses cached dependencies with `--offline` / `CARGO_NET_OFFLINE=true`, pinned toolchain and explicit CARGO_HOME/RUSTUP_HOME. The final source checks are under the canonical attempt output's `library/submission/`. Earlier runs remain separately under `library/` and `library/final/`; they are not used as final acceptance evidence.

| Command | Final outcome | Evidence |
|---|---|---|
| `cargo fmt --check` | 0 | `submission/fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `submission/clippy.log` |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0 | `submission/clippy-privileged.log` |
| `cargo test --offline --workspace --no-fail-fast` | 0; emitted summaries total 722 passed, 0 failed, 14 ignored | `submission/workspace-tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | 0; compile only | `submission/privileged-build.log` |
| `tools/ci.sh` | 0; all available checks passed, deny unavailable | `submission/ci.log` |
| `bash -n` for burst script and shared harness | 0 | `script-syntax.log` |
| Python AST parse of check.py | success | `script-syntax.log` |
| shellcheck | unavailable; pending equipped CI | not run |

Test counts are sums of emitted harness summaries, including subprocess helper runs, rather than a claim of 722 unique executed scenarios. Final daemon library 64/64, live library 69/69, recovery integration 5/5, and binary test 1/1 (explicit sandbox early return). Twelve ordinary test definitions were added: nine daemon library, two live library, one binary integration. No privileged tests ran. Socket/root scenarios using existing early returns require unrestricted coordinator execution even when their harness reports a pass. The five readiness failures described in the task did not occur in this final run; this does not establish real Unix-socket readiness.

Development checks caught two regressions introduced here: initially retaining invalid filesystem rows broke the writer's existing rollover recovery test; this was corrected to retain only transient failures. An intermediate CI/workspace run then caught pre-requested-shutdown startup skipping catalog cleanup; the helper now always performs its initial attempt, with a new regression test, and both existing recovery assertions pass. Their failing intermediate logs are retained rather than attributed to the baseline or sandbox. Final required checks run after these corrections.

`checked-source-hashes.json` records the checked source/script bytes. CI source policy passed with 146 owner tags, format/Clippy/tests and privileged compilation passed, and cargo-deny was explicitly skipped because it is not installed. No advisory/license audit pass is claimed. Guest performance, real root low-limit API execution, shellcheck and four-burst fault execution remain pending.

## Definition of Done evidence

- Owned paths only; no dependencies, manifests or module declarations changed.
- Format and both Clippy configurations pass.
- Final workspace tests pass; sandbox/root early-return limits are explicit.
- Privileged targets compile; existing privileged runtime gates and new fault coverage are pending VM.
- Public setter documented, original source owner headers retained, new test owner header present; no unsafe or new production unwrap/expect.
- Per-decision behavior, fatal-error audit, proposed plan text and assumptions supplied.
- Independent review, integration and real-guest acceptance remain with the coordinator.
