# Handoff — SAMPLER-reload: generation-safe sampler health

## Needs from coordinator

Update `crates/tidemarkd/tests/w6a2s.rs:447` in `retired_watcher_components_and_errors_release_capacity`: assert one remaining watcher plus the degraded sampler component (two components total), and verify retired watcher names/errors are absent. Its old total-component count of one conflicts with the coordinator-required sampler invalidation on watch-set changes. This file is outside the assigned write scope, so it was not edited. The workspace suite currently fails only this assertion.

Run the ordinary-user socket/readiness suite on an equipped CI host and the existing privileged daemon reload/shutdown test in a disposable guest. Local environment limitations and final check results are recorded below; this note is evidence for review, not independent verification.

## What was built

F1: `LiveState::publish_component_for_generation` checks generation and updates health under one write lock. All sampler health-only publications use it, including final pass health, in-progress invalidation and mount-table failures. Existing public signatures stay unchanged.

`replace_config` compares sampler and watch mount sets by native path order, preflights component capacity, and invalidates sampler health under the config write lock when either set changes. The detail is `waiting for a complete pass under the new configuration`. This happens before configuration/generation publication and before the caller can publish the deployment hash. Mount reordering and unrelated configuration changes preserve health. Firstboot production behavior is unchanged.

## Acceptance criteria evidence

- `services::sampler::tests::reload_during_pass_waits_for_new_mount_and_persisted_firstboot` drives the actual sampler loop through bounded collector/completion barriers. It first persists a healthy old hash, blocks the next pass on A, reloads adding B, and reads actual JSON for the new hash before and after stale-pass completion. Both remain unhealthy. It fails B's first current-generation measurement and confirms degraded health/unhealthy JSON, then recovers B and confirms healthy health/JSON only after complete-pass publication. Removing final generation checking fails the stale-completion assertion (negative control exit 101). The pass is already degraded while blocked, so this service-loop scenario alone does not discriminate reload invalidation.
- `state::presence_lock_tests::reload_health_tracks_mount_sets_and_rejects_stale_passes` verifies interval-only/reordering preservation, watch-set invalidation, stale-generation rejection and current-generation publication. Removing reload invalidation fails this test with `ok` instead of `degraded` (negative control exit 101).
- Existing fixed two-mount recovery/persistence test remains intact.

## Shared-file edits and dependencies

None. Only sampler.rs, state.rs and this handoff change. No dependency added; firstboot.rs was not changed.

## Assumptions

The existing reload caller publishes deployment metadata after successful `replace_config`; invalidation within that call therefore precedes the new hash. Mount requirements are sets, so list ordering alone does not invalidate health. Trusted mount administration and identity reconciliation retain their existing limitations. The regression injects mount-table metadata and collectors without real mounts or sockets, as authorized; it validates the service loop and persistence helpers rather than a full API reload.

## Proposed plan and security text

Plan 07 §1 / wave-6 reload: “Sampler aggregate health belongs to the configuration generation captured at pass start. Reject all health publications from superseded passes atomically. Changes to sampler or watch mount sets invalidate aggregate sampler health before the new deployment hash becomes visible; only a complete current-generation pass may restore it.”

Security: “Reload cannot certify newly required mounts using aggregate health from a superseded sampler pass. Generation checking and health publication share a state write lock. Mount-set changes invalidate health before deployment-hash publication. This adds no filesystem operations, content reads, privileges or external writes.”

## Tests and limitations

Targeted offline sampler suite: 6 passed, 0 failed. Offline reload filter: 4 passed, 0 failed (includes both new regressions). All long commands use `timeout 1500`; full validation uses explicit `umask 022`, pinned Rust 1.98.0, offline cached dependencies and four build jobs. A readable registry cache was copied to `/tmp/sampler-cargo`; direct pinned toolchain binaries avoid rustup attempting to create a home outside writable paths.

Workspace validation so far:

| Check | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| Same Clippy command with `--features privileged` | exit 0 |
| `cargo test --offline --workspace --no-fail-fast` | exit 101; 900 passed, 1 failed, 14 ignored across 84 summaries |
| `cargo build --offline --workspace --features privileged --tests` | exit 0, compilation only |
| `tools/ci.sh` with offline environment | exit 1; same sole component-count failure, source checks/fmt/Clippy passed |

The one workspace failure is the out-of-scope existing component-count assertion described above, observed `left: 2`, `right: 1`. The five environment-limited readiness cases did not fail this run; passing guard returns do not establish socket/namespace behavior. Summary counts include nested helpers and do not imply every scenario was exercised. Privileged runtime was not executed, no host service/storage commands were run, and fault/install scripts were syntax-checked only. This task adds no privileged tests because the required interleaving is deterministically covered without kernel privileges.

Pending guest integration (existing test, not run here):

```sh
umask 022
CARGO_NET_OFFLINE=true timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture --test-threads=1
```

Equipped ordinary-user CI: `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh --require-deny`, with Unix sockets/user namespaces available. Real cargo-deny, advisory/license acceptance and Rocky guest runtime remain separate gates.


CI summary: 900 passed, 1 failed, 14 ignored across 84 summaries. CI stops after the failed workspace test command, before later deny/privileged-build phases; the explicitly requested privileged build passed independently. Neither cargo-deny nor real ShellCheck is installed. `bash -n` for fault/install shell entrypoints passed without executing them. No canonical memory updates were pending (empty receipts at both checks).


Negative controls were performed after full validation: removing only final-pass generation checking fails the sampler regression; removing reload invalidation fails the state regression. Removing invalidation alone leaves the blocked-pass sampler scenario passing because in-progress health is already degraded; both tests are needed. Committed sources were restored byte-for-byte and the reload-filter suite passed again (4 passed).
