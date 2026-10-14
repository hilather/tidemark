# Handoff — WATCH-rc

## Needs from coordinator

- **B2 API error text remains blocked by file ownership.** In `crates/tidemarkd/src/api.rs`, the `"hot"` dispatch currently calls the generic `available(view.hot(&params(request)?))`, which hardcodes `measurement unavailable`. Parse `HotParams` once, call `view.hot(&p)`, and on `None` return `DaemonError::Unavailable(view.hot_unavailable_detail(&p))`. The additive state method supplies the required `watch history for <mount> starts at HH:MM UTC; ask for a window of at most <n> minutes` text. No core/wire changes are needed. Add an API regression for an empty startup/replacement window. `api.rs` is outside this attempt's allowed paths; it was not edited. Thus B2 is not fully delivered at the wire boundary.
- Run ordinary-user CI with Unix socket binds and user namespaces available. This sandbox runs as uid 0 and denies those facilities; guarded passing tests may not execute their central assertions.
- Run the existing privileged daemon ordinary-unmount regression on both Rocky guests, with the commands below. Privileged execution is prohibited here.

Branch: canonical attempt `hp-cc85f849e94b1db7c5b36c6cadfa074592710056b9b43317fddeb46584ba82e7`. Status: implementation/evidence candidate; independent verification pending. No branch creation, merge, push or release operation.

## Per-finding changes and regressions

### B1 — delayed timestamps and real rollback

Status reads clamp saved service time to the newest watcher observation under the watcher lock. The production `stats()` reads its clock after taking that lock. Delayed drains, queries, and event observations after metadata/attribution likewise clamp under the lock; queue-overflow and transport-error secondary observations do too. Worker polling and explicitly injected `stats_at` retain the ability to observe a real clock rollback. A tick's saved time is used for its drain; its subsequent status observation cannot undo a newer worker observation. Diagnostics use the latest serialized observation.

`Rings::advance` retains completed facts and delivery watermarks on rollback, marking the affected retained interval lossy instead of clearing the deque. Queries/status use the newest retained minute while wall time catches up. Replay events accumulate in the retained current minute with incomplete quality; already emitted timestamps are not emitted again. Dirty roots still invalidate through the existing rollback observation rule. The ring remains bounded to sixty buckets.

`delayed_status_after_drain_does_not_rollback_history` pauses the production tick after drain/enqueue and before status publication with a two-party barrier. The worker seam advances 179 to 180; all three scopes and 5/15/60-minute queries must compare equal, three writer rows must arrive once, and subsequent drain cannot repeat them. An injected rollback to 119 keeps the 82-byte rows and marks them lossy. Original stale status/clear behavior fails row equality and retention. `sixty_buckets_and_clock_rollback` requires retained bounded buckets, totals and once-only delivery after a large backwards step.

### B2 — reload retention and history coverage (API follow-up above)

Service reconciliation uses configured mount membership and per-mount settings. Reordering/extending the list preserves existing watcher Arcs, minute history, immutable windows and delivery state. Exclusions unrelated to a mount do not restart it; relevant exclusions are compared by set membership. Changes to event selection, hot depth, LRU size or relevant exclusions retire affected watchers. Mount identity transitions flush undelivered completed persistence rows before shutdown, then start independent history. Settings changes use the same bounded flush-before-retire path. Queue rejection is logged; it is not retried after the once-only cursor advances.

Each watcher records its first fully observed minute, excluding the startup partial minute consistently with completed-minute delivery. Hot windows predating that boundary return existing rows with `lossy=true`; an empty such window returns `None`. The additive `LiveStateView::hot_unavailable_detail` produces the required start-time/admissible-window message, awaiting API dispatch integration. `now` adds `watcher <mount>: history starts at HH:MM UTC` until all advertised windows through sixty minutes are covered. Both directory and unit now lists use the view's injected time and carry incomplete quality. No CLI changes are needed to preserve the existing lossy rows or degraded strings; the API dispatch change is needed for the exact unavailable text.

`reload_membership_preserves_unchanged_watchers_and_flushes_replacements` uses the daemon service reconciliation with injected watchers and real `LiveState::replace_config` calls. It checks identical rows in all scopes/windows after reorder and extension, Arc identity, unchanged persistence counts, relevant-setting retirement, identity retirement's once-only flush, independent replacement rows, lossy quality, history-start diagnostics and their expiry, and unavailable empty startup/replacement windows. Retiring every watcher on config inequality fails the Arc/query assertions; missing flush fails the persistence count; missing coverage fails quality/degraded/unavailable assertions. The identity test invokes the service's same flush/retire functions without kernel mount operations; actual mount identity detection remains guest coverage.

### S1 — emergency aggregate persistence

Persistence enqueue takes ownership of exported rows and moves them into bounded messages instead of cloning path/key strings against normal watcher admission. The exported emergency aggregate therefore needs no second normal-budget copy. This is ownership transfer of already constructed output; new vector slots remain bounded by message row/byte limits. The existing 64-message channel and 64 KiB conservative per-message charge still bound queued ownership to 4 MiB. Oversized rows, allocation failure, and full/disconnected queues retain the `persistence_dropped` signal. Delivery cursors still advance once.

`production_tick_moves_emergency_aggregate_through_full_normal_admission` fills normal admission, runs the production tick over +71/-9 events, requires exactly one lossy root aggregate with two events and 62 bytes in the writer queue, and requires no repeat on the next tick. The former normal-credit clone path returns zero rows and fails the count. `rejected_writer_queue_keeps_persistence_drop_signal` rejects enqueue through a zero-capacity channel and requires the existing signal without replay. The long-key enqueue regression still requires every message's conservative byte charge to fit its bound.

### S2 — bounded owned cleanup

Loop-XFS cleanup commands use three-second deadlines, null stdio, kill and wait on expiration or polling failure. Setup/reporting commands retain their existing behavior. The guest daemon test receives its cleanup result, always joins the cleanup worker, and only then propagates timeout/unmount failure. The fifteen-second parent diagnostic bound covers command deadlines and bounded Drop retries; it is not permission to detach the worker.

`cleanup_deadline::deadline_tests::stalled_cleanup_is_killed_and_reaped_before_error` runs outside privileged execution by including the deadline helper from the owned fixture source. It spawns `sleep 30`, uses a 30 ms deadline, requires a timeout within two seconds and disappearance of its `/proc/<pid>` entry, and checks successful command completion. Unbounded wait fails the deadline assertions. No mount, loop setup or filesystem formatting is involved. Actual kernel unmount behavior remains pending VM; a kernel task stuck uninterruptibly cannot be made reapable by userspace deadlines alone.

## Validation

Required checks are running; final results and counts will replace this paragraph before submission. Evidence transcripts and exit codes are under the attempt output `library/`.

## Guest commands — pending VM

Only inside disposable prepared Rocky 8.8 and 8.10 guests:

```sh
export CARGO_NET_OFFLINE=true
umask 022
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s daemon_sigterm_releases_loop_xfs_for_ordinary_unmount -- --exact --nocapture
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
```

Retain the daemon test's selected watcher mode and ordinary-unmount result. Real fanotify modes, handle metadata timing, mount identity replacement, shutdown descriptor release and filesystem cleanup are guest-only evidence. Compilation does not establish these runtime results.

## Proposed plan and security text (not applied)

Plan, `07` §3.3: “Completed query facts and delivery cursors survive wall-clock rollback; affected retained minutes are incomplete and emitted timestamps never repeat. Concurrent delayed observations cannot be mistaken for rollback. Reload preserves unchanged mount watchers independently of list order and flushes completed persistence rows before replacing a mount identity or aggregation configuration. A new watcher advertises its first complete minute; older requested windows return incomplete rows or an unavailable response with the history boundary. Now reports that boundary until every advertised window is covered.”

Security/quality: “Normal and emergency export ownership transfers into the bounded writer queue without an extra pathname clone. Queue rejection remains explicit degraded persistence loss. Query retention and all count/byte caps remain bounded. Guest fixture cleanup owns subprocess termination and reaping and joins cleanup workers before propagating failures. No privileges or content reads are added to production watcher code.”

## Assumptions and limits

- The start boundary is the first full minute after construction, matching the existing drain contract; a startup partial minute cannot certify a complete window. The `now` boundary line covers all fixed hot windows through sixty minutes.
- Global settings that affect every watcher (events, depth, LRU) replace all affected mounts; exclusions are limited to ancestor/descendant paths relevant to each mount. No histories merge across replacements.
- No public signature used outside scope changed. Added watcher/state seams have rustdoc. No dependencies, manifests, shared module declarations, CLI, core types, metric names, plan or security files changed.
- Pinned Rust 1.98.0 with `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, offline mode and explicit `umask 022`. Each long command uses `timeout 1500`. No dependency fetching, privileged test execution, host mount/LVM/service changes or system-directory writes.
- No user questions were issued. Canonical memory brief/input/receipts were read; no pending delivery was reported and projections were not edited.
- Regression assertions discriminate the removed behavior; absent-fix negative controls were not separately executed. Reported pass counts include tests that return early for unavailable facilities and nested summaries, rather than unique fully exercised cases.

## Shared-file edits and completion evidence

None. Focused commits cover B1, B2, S1 and S2, with a follow-up tightening production tick/empty-start coverage. All changes remain within assigned paths. Final clean status, cargo-clean result and submission receipt are recorded in the external report/library. B2's exact wire error message, equipped-host dependency audit, ordinary-user CI and guest runtime acceptance remain coordinator work; no verified task success is claimed.
