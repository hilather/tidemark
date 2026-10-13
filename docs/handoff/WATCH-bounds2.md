# Handoff — WATCH-bounds2: watcher memory and lifetime fixes

## Needs from coordinator

Run the guest checks below on Rocky 8.8 and 8.10, and run CI on the merged candidate as an ordinary user with Unix socket binds and namespaces available. Whole-daemon RSS, fanotify shutdown/unmount, dependency advisories and real allocator exhaustion remain unverified. This handoff is an evidence candidate, not independent task acceptance.

- Attempt: attempt-45e84eaf7bec39644669ebb38bd58a95601f9cdb5c717e9dbfd0945ef9d11c2b
- Baseline: `7345c80b585933164cf7b55fd3fcd68badeff819`, including the nine WATCH-bounds commits after `5ca024e`.
- Date: 2026-10-13. Status: review and local validation in progress; final results recorded below before submission.

## Review and changes

Reviewed the nine retained commits against the six coordinator decisions, including the final three commits' budget/container/export, empty-pass health and fallback-root changes. Kept the retained implementation. No dependency, manifest, shared module, frozen contract, CLI wording or plan/security edits.

| Finding | Retained implementation and regression evidence |
|---|---|
| R3-03 | Shared 64 MiB admission includes ring map keys and row keys, unit maps, seen identities, dirty paths, acknowledgment roots, FID path/handle caches, immutable window rows and conservative container credits. Path clones retain exact-length vectors; copied Vec/String capacities are charged. `byte_budget_falls_back_to_root_and_releases_completed_maps` checks exact payload accounting (zero tolerance) over 70 rollovers, exact event/byte totals on memory fallback, and empty drained maps. `injected_long_keys_cross_default_budget_with_exact_root_totals` crosses the real ceiling. Cache, history and queued persistence byte tests cover downstream retention. |
| C3 | Existing-parent lookup precedes the count-cap check. Overflow clears parents and acknowledgment roots, invalidates scan roots independently once, and accepts later parents. `full_dirty_set_invalidates_root_once_and_recovers_over_sixteen_minutes` and `dirty_byte_admission_collapses_to_root_and_recovers` exercise recovery and exact reasons. |
| C6 | Shutdown holds the join mutex across signaling, joining and closing retained descriptors, so concurrent calls cannot double-join or return before teardown. Worker owns the fanotify group and its mount-descriptor clone. Retirement occurs before replacement in config, identity, disappearance and failure paths, including service error cleanup. `retained_view_does_not_delay_retirement_before_replacement` records stop/group-close/start order through the service retirement helper. `retired_watcher_transport_stops_while_an_old_state_view_is_held` exercises a real state view. Privileged normal-unmount regression is listed below. |
| C5 | A successful mount-table and namespace pass restores generic watcher health, including an enabled watcher with zero configured mounts; per-mount health remains independent. `mountinfo_emfile_retains_watcher_and_minute_history` injects mount-table failure, a successful reread and namespace recovery, retaining independent per-mount degradation. Empty-mount behavior was reviewed in the production pass; no separate end-to-end empty-mount regression is claimed. |
| N1 | Warm-up uses the same current-identity `rate_ready` history as rate availability, rather than daemon uptime or rule window. `one_minute_and_late_remount_rates_warm_for_five_minutes_of_current_identity` covers one-minute/one-hour rules, five-minute readiness, late reset and subsequent stale-sample fault. |
| C11 state | All must-happen waits are ten seconds. Reload runs on a controlled worker. Release and all joins precede assertions. `blocked_resolution_and_logging_leave_readers_free_and_reject_stale_generation` exercises all three phases. |

New changes in this attempt:

1. Output admission previously ranked into an allocated reference vector before reserving output storage. Replaced that pre-admission ranking with a no-allocation conservative upper bound: largest row payload cost times requested count. Ranking scratch is explicitly included in the output node reservation. Ranked copies use fallible vector reserves and fallible key/unit copies; unstable sorting avoids extra sort scratch. `output_admission_bounds_unequal_rows_before_ranking` checks the bound for unequal rows and multiple limits; all ring regressions passed. This is an allocation-order review fix; no allocator-failure injection is claimed.
2. Immutable window copies now copy unit strings fallibly. Added `concurrent_shutdown_joins_once_and_reclaims_retained_windows`: four shutdown callers, exactly one stop/close sequence, empty join/signal/mount slots, reclaimed window payload, no foreground-buffer restart, and repeated shutdown safety.

3. Added `emergency_root_headroom_preserves_sixty_undrained_minutes`: maximum 4,096-byte fallback root, actual default budget exhausted with 300 long distinct keys (below the completed-key count cap), sixty undrained minutes, exact event/byte totals and zero-tolerance payload accounting. Its initial fixture used 6,000 keys and wrongly expected totals to survive the separate completed top-1,000 truncation; that fixture failed. The corrected fixture isolates byte-budget headroom; both logs are retained.

Lock review: service shutdown does not hold the global state lock while joining. Worker processing never takes the join mutex. Window copies take their own data lock and the budget lock; they do not acquire watcher state. Mount-identity queries serialize with descriptor release and fail without kernel access after teardown. Other old-view queries may answer empty data and read clocks but do not reopen the group or mount. Read-only queries can create empty numeric minute buckets after retirement; retained window pathname payload is reclaimed.

## Memory envelope

Let `B = 64 MiB`, `E = min(B/64, 1 MiB)`, `L = watch.lru_entries`, `W = enabled watcher count`, `M = 60` minute buckets, `R = 4096` root bytes. Normal shared admission is at most `B-E`; emergency admission is at most `B`. This covers pathname/key payload and conservative node/scratch credits, not only lengths. Root fallback headroom requires at most `M*(2R + 512 + 4096) + R`, about 754 KiB at the root ceiling, below `E`. Optional command/unit scopes are dropped rather than double-counted in directory scope. Current/completed caps remain 20,000/1,000 per scope; distinct identities and dirty parents remain capped at 200,000, handle cache at 100,000, acknowledgments/windows at 1,024. Count limits cannot bypass shared byte admission.

Numeric FD size history is outside the pathname budget. A conservative envelope for its hash growth overlap and FIFO capacity slack is `S(L) = 128*(L+1) + 64*(2L+1) + 4096` bytes: hash entries contain two u64 pairs; queue records contain three u64s. Credits exceed hash growth overlap at the load factor and doubled queue-capacity slack. Worker transport/event scratch and default thread stack can conservatively be allowed another 3 MiB per watcher. Therefore watcher subsystem envelope is `W*(B + S(L) + 3 MiB) + 64*64 KiB` for the shared persistence queue. Default `W=2, L=200000` gives less than 236 MiB, below the 256 MiB target and far below the 2 GiB unit limit. Default watcher options add tiny fixed path/event vectors; user config is already capped at 1 MiB. This is a structural estimate, not measured allocator RSS or an envelope for arbitrary `W`/`L` configurations.

Dirty takes move paths instead of cloning the parent set. Tree-node credits cover the response vector during extraction; copied fallback roots are admitted separately. Ranking and drain exports are charged through construction, then ownership transfers out of the watcher. Daemon history/window consumers separately admit their retained capacity; history has an additional 8 MiB cap. Persistence chunks are at most 64 KiB and its existing 64-message queue adds at most 4 MiB. Caller-owned returned vectors do not carry a lease after method return; arbitrary callers retaining unlimited query responses are not included in the watcher envelope. Generic state-view snapshots, other collectors and writer payload types need their own bounded-structure review; this task does not certify whole-daemon peak memory.

## Proposed documentation text (not applied)

Plan: Each watcher admits at most 64 MiB of retained path/key allocations and conservative container credits across dirty tracking, all aggregation copies, seen identities, FID caches, minute history and immutable window views. Dirty admission failure invalidates scan roots conservatively once and marks the affected minute lossy. Key admission failure aggregates exact event and signed-byte totals under the watched mount's directory key and marks truncation. Completed persistence drains release key maps. Shutdown joins and releases kernel resources independently of reader-view lifetimes.

Security: Metadata names cannot grow watcher pathname storage beyond the shared capacity budget. Count caps remain defense in depth. Dirty-set exhaustion does not permanently poison later minutes. Allocation admission and conservative fallback preserve directory event/byte totals; per-file and optional attribution details may be incomplete. Configuration-dependent numeric caches, caller-owned query results, concurrent API consumers, other daemon structures and allocator RSS require separate envelope/guest measurements.

## Assumptions and limits

- Use only the assigned attempt branch; no merge, push, new branch or dependency fetch.
- Pinned Rust 1.98.0, offline cache copied into `/tmp/watch-bounds-cargo`; no host service, filesystem, namespace, LVM or system-directory modification.
- Capacity equality tests reflect the pinned allocator's exact-length Vec/String copies. Container charges are intentionally conservative, not equality measurements of allocator internals. B-tree node allocation still uses the standard library's infallible internal allocator; fallible admission is not an OOM-proof process guarantee.
- Privileged runtime prohibited here. The live crate's normal-unmount test holds the same watcher Arc that daemon views retain; the independent daemon test holds an actual LiveStateView.
- Sandbox uid/socket/namespace guards can produce passing early-return tests; test counts do not imply those central assertions ran.

## Guest commands — pending VM

Run only inside disposable prepared Rocky 8.8 and 8.10 guests:

```sh
export CARGO_NET_OFFLINE=true
umask 022
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a privileged::shutdown_releases_mount_for_ordinary_unmount_while_old_reader_survives -- --exact --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemarkd --test w6a2s retired_watcher_transport_stops_while_an_old_state_view_is_held -- --exact --nocapture
```

The first regression uses ordinary `umount` through LoopXfs cleanup, with no lazy/forced flags. Fanotify mode negotiation, namespace attribution, descriptor lifetime, kernel stalls, remount behavior and RSS/FD soak remain guest-only evidence.

## Validation

Initial targeted suites: 292 passing test results, zero failures (includes guards and empty suites); ring follow-up: 15 passed; concurrent-shutdown regression: 1 passed. Commands use `timeout 1500` and offline dependencies. Final required check results and aggregate counts will be appended after completion.
