# Handoff — PURGE-lease: purge exclusion and lease-test child cleanup

## Needs from coordinator

The required workspace suite fails in the unchanged, out-of-scope `crates/tidemarkd/tests/w6a2s.rs:447`, `retired_watcher_components_and_errors_release_capacity`: expected one component, observed two. Unchanged `state.rs:230-233,379-388` publishes a sampler component when watch mount requirements change; the test counts all components instead of only the remaining watcher. The owning task/coordinator should make that assertion account for the sampler (prefer a watcher-specific count plus a sampler assertion). Both files are byte-identical to the assigned baseline, and the exact already-built default-feature test independently reproduces the failure. This direct LiveState fixture acquires no storage/scan lock. See `library/outside-scope-failure.*` and `outside-scope-provenance.log`. Do not widen this task's scope to repair it.

Independent review and ordinary-user CI with Unix sockets and user namespaces are pending. Run real systemd deployment acceptance on disposable Rocky 8.8/8.10 guests; privileged targets were compiled only here. This is an evidence candidate, not verification or release authorization.

## Changes by finding

**R4-02:** After stopping and checking the configured service, purge acquires exclusive writer ownership, then scan ownership. Each contention phase has a ten-second deadline with 25 ms polling. Contention returns `state directory in use by another tidemark process; nothing was removed` through the existing deployment guard/precondition path (CLI exit 5). No destructive traversal begins until both guards exist, and both remain alive through finalization. Unstage without purge follows its previous path.

Root `scan.lock` and `writer.lock` are excluded from recursive data removal. After all other entries have been removed, purge unlinks scan.lock, then writer.lock, rechecks the retained root against its parent/name, and attempts rmdir exactly once. New entries or replacement cause an error without another traversal. Such finalization failures may occur after old data has been deleted; the “nothing was removed” promise applies to acquisition contention, not finalization failures.

Every writer-lease acquisition now verifies identity after flock succeeds: fstat of the held file versus no-follow statat of the name through the directory retained before waiting, both link counts exactly one, and retained directory device/inode versus a newly verified open of its path, with nonzero retained directory link count. A detached/unavailable/replaced identity returns typed LeaseBusy/WouldBlock; dropping the local file releases ownership. ScanLock had the same acquisition weakness and now uses the same checker, returning ScanError::Busy. Existing public signatures are unchanged; the documented identity checker is additive.

**F5:** Each explicit spawn in the specified lease tests immediately enters an owning ChildGuard, including both racing-start children. Drop kills and waits on normal return and panic unwinding. Normal exit waits poll try_wait with a ten-second deadline. Existing explicit readiness files and the racing-start go barrier remain; all five daemon lease readiness/progress deadlines and the store/CLI readiness deadlines are now ten seconds. Controlled short Busy checks and their product deadlines remain unchanged. The guard is local to each owned test file to avoid modifying shared test-support outside this task's scope.

## Regression evidence

- `purge_refuses_live_writer_without_changing_bytes_or_inodes`: a real child holds writable Catalog and TimeSeries handles while fake-service purge runs; guard refusal preserves every fixture file byte and every file/directory inode. Removing writer exclusion makes this fail.
- `purge_refuses_busy_scan_without_removing_state`: held real scan ownership causes refusal with both lock files, marker and sentinel retained. Removing scan exclusion makes this fail.
- `idle_purge_removes_data_locks_and_directory`: real initialized storage and both locks disappear on idle purge. Leaving sentinel files behind makes this fail.
- `purge_waiter_refuses_deleted_inode_after_complete_purge`: a separate child opens the old lock descriptor while purge owns it, signals readiness outside state, waits for flock and invokes the production identity checker. It observes busy after complete purge, never valid ownership. Deleting the identity checker makes this fail.
- Store `waiter_on_deleted_lock_never_gets_ownership`: actual WriterLease reaches controlled flock contention before unlink/removal or directory replacement; after release, it returns WouldBlock. Removing the acquisition identity check makes this fail.
- `purge_finalization_never_traverses_new_entries_or_replacement`: a separate process creates an entry or moves/recreates the state directory between the last lock unlink and rmdir. Purge refuses and preserves the new sentinel. This exercises finalization refusal and sentinel preservation; retrying traversal would delete the new sentinel. The explicit identity-change check is also visible in the source, while a nonempty replacement independently makes rmdir fail.
- Store `detached_state_and_replaced_lock_refuse_ownership`: writer and scan descriptor identities refuse removed lock names or replaced directories, retaining typed busy errors.
- Store `child_guard_reaps_writer_when_an_assertion_panics`: an explicitly ready real writer is owned by a guard inside catch_unwind; the injected assertion panics, its process disappears from procfs and exclusive ownership becomes available. Omitting Drop cleanup makes this fail.
- Existing store crash-recovery suite, daemon library tests and CLI `rebuild_refuses_pre_api_writer_without_renaming` passed with the guards. The CLI still exercises its actual thirty-second busy refusal.

## Validation

All required commands ran with Rust 1.98.0, cached offline Cargo, four build jobs, two test threads, explicit umask 022 and timeout 1500. Logs and exit files are in the attempt's library.

| Required command | Exit/result |
|---|---|
| `cargo fmt --check` | 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0; compilation/lint only |
| `cargo test --offline --workspace --no-fail-fast` | 101; only the out-of-scope watcher component-count assertion failed |
| `cargo build --offline --workspace --features privileged --tests` | 0; compilation only |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | 1; source policy, formatting, Clippy and strict-feature regression passed; workspace tests failed on the same out-of-scope assertion, so CI did not reach its later build/deny stages |

workspace-tests harness summaries: 918 passed, 1 failed, 14 ignored across 85 summaries. ci harness summaries: 920 passed, 1 failed, 14 ignored across 86 summaries. These include nested subprocess helpers and capability guard returns, not that many distinct exercised scenarios. All new purge/lease/cleanup parent assertions passed. Focused top-level results: purge 12 passed (including two helper entrypoints), store lease unit 1 passed, crash-recovery 11 passed, daemon library 110 passed, CLI rebuild refusal 1 passed. The out-of-scope test independently reproduced exit 101 on the already-built default-feature binary. Shell syntax checks passed for all fault/install shell entrypoints; none was executed. cargo-deny and ShellCheck are absent, and no advisory/license or real shell lint acceptance is claimed.


## Pending guest/ordinary-user checks

Run on prepared disposable Rocky guests only; these commands were not executed here:

```sh
export CARGO_NET_OFFLINE=true TIDEMARK_DEPLOY_TEST_VM=1
umask 022
timeout 1500 cargo test --offline -p tidemark-cli --lib purge -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemark-store --test crash_recovery -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemarkd --lib -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemark-cli --test w6a3 -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemark-cli --features privileged --test w4b privileged::deploy_unstage -- --exact --nocapture --test-threads=1
```

Existing privileged `deploy_unstage` is unchanged and compiled with the new purge behavior. No new privileged-only regression was necessary: the new concurrency cases use actual SQLite/flock and temporary state plus fake service management without privileged syscalls. Real service stop/disable, SELinux and Rocky/XFS behavior remain pending VM. On an ordinary-user runner with sockets/namespaces available, run `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh --require-deny` at umask 022; observe guards rather than interpreting compilation or early returns as exercised scenarios.

## Assumptions and limits

- Busy purge is a deployment precondition, using existing exit 5; no CLI flag or error-mapping signature changed.
- The bounds cover flock contention, not an absolute deadline for arbitrary blocked filesystem/SQLite calls. Shared leases do not promise writer fairness.
- Compliant Tidemark processes coordinate through the locks. Hostile same-uid metadata replacement and external SQLite writers bypassing the protocol remain outside the existing root-only coordination boundary. Final statat/rmdir are separate syscalls, not an atomic compare-and-remove primitive.
- Normal paths are reopened through trusted ancestry. For the existing caller-vouched `/proc/self/fd/<n>/.` anchor, the checker compares that anchor and rejects an unlinked directory; the anchor does not reveal an original configuration pathname after a rename. No StateDir contract was changed.
- The workstation is uid 0 with restricted capabilities and refuses socket binds/namespaces. Cargo uses cached dependencies with --offline; no dependency was added. Initial rustfmt setup required pointing RUSTUP_HOME at the installed toolchain location; final formatting is checked with that setup.
- Only owned paths changed. No manifest, module declaration, state_dir.rs, core contract, plan/security document or memory projection changed. No branch creation, merge, push, host service/storage commands or privileged runtime occurred. Memory receipts were empty; no update was acknowledged without application.

## Proposed plan text (not applied)

After service stop, `deploy unstage --purge-state --yes` acquires exclusive writer ownership before exclusive scan ownership, waiting at most ten seconds per contention phase. Failure retains all state and reports the busy-state precondition with exit 5. Purge removes data before scan.lock and writer.lock (writer last), then revalidates directory attachment and attempts final rmdir once. New entries or replacement stop finalization without another traversal. Every successful writer/scan flock must validate held-versus-named lock identity through its retained state directory, single-link lock ownership and linked directory attachment before reporting ownership.

Lease subprocess tests synchronize on explicit readiness and use at least ten-second scheduling/progress bounds; children are owned immediately after spawn by guards that kill and reap on all exits, including panic. Controlled product Busy budgets remain short.

## Proposed security text (not applied)

Stopping the service does not exclude direct administrator writers. Purge requires writer then scan exclusion before deleting state, retains both lock names until destructive traversal is finished, and unlinks writer.lock last. Waiters on unlinked or replaced lock inodes must refuse ownership after flock; directory identity and lock link-count checks prevent obsolete lock descriptors from authorizing work. A recreated namespace or new entries at final removal are preserved without a second traversal. These guarantees apply to cooperating Tidemark processes under the existing trusted, root-only state policy.

## Definition of Done evidence

- [x] Only owned paths changed; no shared-file edits or new dependencies.
- [x] Final fmt and default/privileged Clippy clean; public helper documented.
- [x] New unprivileged process/identity regressions pass; privileged targets compile.
- [x] Per-finding behavior, limitations, proposed text and pending guest commands recorded.
- [ ] Workspace suite/CI green: blocked by unchanged out-of-scope assertion described at the top.
- [ ] Real service/ordinary-user/guest acceptance and independent review pending.
