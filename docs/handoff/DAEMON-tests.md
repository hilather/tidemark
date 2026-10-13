# Handoff — DAEMON-tests: sampler recovery and test synchronization

## Needs from coordinator

Run socket-dependent readiness and lifecycle assertions on an ordinary-user runner allowing Unix socket binds and namespaces, and perform the guest commands below. The five w6a1 readiness bodies return early on denied Unix binds in this sandbox; a separate nocapture run confirmed all five deferrals. These are environment limitations, not passing scenario coverage. Independent verification and integration remain pending. The state.rs part of C11 and other findings are outside this task's scope.

- **Attempt:** attempt-921f3b0a72d8bb8151aab0b0491f7d7d7db87e2cd11c11cc22de8d023dbef505
- **Branch:** precreated hp-921f3b0a72d8bb8151aab0b0491f7d7d7db87e2cd11c11cc22de8d023dbef505
- **Date:** 2026-10-13
- **Status:** implementation and validation evidence candidate; not an independent acceptance decision.

## Changes and discriminating tests

**C2:** The sampler keeps a bounded map of measurement errors keyed by configured mount path. Only that mount's successful measurement removes its error; removal from configuration retires its entry. Mount-table errors invalidate each configured mount and retain errors until collection succeeds. A due pass invalidates existing sampler health before collecting. On the initial pass, absent sampler health already prevents firstboot certification; its first health publication accompanies the first measurement, avoiding a degraded status before any sample exists. Successful samples remain immediately available with conservative in-progress health, and only the completed pass publishes aggregate recovery. An interrupted shutdown pass stays degraded. Persistence queue failure and absent watch-only mounts still degrade the final aggregate. No change to public signatures, core types, state.rs or firstboot policy was required.

`services::sampler::tests::two_mount_recovery_waits_for_complete_pass_and_persisted_firstboot` executes the real sampler loop with injected mount tables and measurements. A succeeds and B fails; on the next pass A succeeds while B is held at its collection barrier; B fails again; on the final pass B succeeds. It checks sampler health and the actual serialized `deploy/firstboot.json` both while B is blocked and after each completed pass. It uses real `LiveState`, `firstboot::write` and `Followup::poll`, with injected monotonic followup timestamps ten seconds apart. The successful A measurement is checked during blocked B. The final report is healthy only after the whole successful pass. Firstboot's historical-finalization behavior remains intact; its implementation needed no edit.

**C10:** Both capturing tests launch an exact-test subprocess with a distinct child-only environment marker. The parent checks successful exit and that exactly one child test executed. Scoped capture stays in the child; other tests cannot poison its process-wide callsite interest cache. Real mount logging, four transition lines, two missing transitions, one device change and one present message remain asserted. Operator message tests retain their existing real helper calls and formatting checks. Their copied macro assertions still prove spelling/formatting rather than production reachability.

Discriminating tests: `mounts::tests::logger_emits_one_line_per_transition_and_device_changes_restart` and `log_tests::every_operator_message_formats_with_values_and_without_debug_fields`. Without isolation these assertions can lose output when sibling calls register shared callsites without a subscriber; the controlled cache reproduction and mechanism are documented in `docs/handoff/TEST-flaky2.md` and review C10. Five full library-binary repetitions (excluding the ten-second presence poll) ran these capturing tests concurrently with sibling callers during workspace compilation: 460 parent tests, ten isolated child captures, zero failures. This stress evidence does not prove every possible schedule.

**C11 (owned sampler portion):** A private completion hook runs after final health publication. Bounded completion/resume channels hold the worker after a pass so assertions inspect a completed, stable state. The single-mount presence regression now observes both failed collection completion and presence-only completion; it no longer counts mount-table entry or assumes completion after a 100 ms sleep. Reload recovery is also observed after completion. All positive collection/progress/receive bounds in sampler tests are at least ten seconds. Remaining 10 ms sleeps pace polling of actual observable state; none establishes a pass phase. The presence-only test deliberately waits up to fifteen seconds for the service's ten-second presence cadence.

`services::sampler::tests::failed_measurement_survives_presence_poll_until_success` detects clearing the failed outcome on a presence-only pass after the completion handshake. The existing EMFILE and injected mount appearance/disappearance regressions retain their alert invalidation/history/persistence assertions with ten-second scheduling bounds. Out-of-scope inventory entries were left for their owners.

## Shared-file edits and contract changes

None. Only sampler.rs, mounts.rs, log_tests.rs and this handoff changed. No dependency, module declaration, public signature, config key, schema, metric or API change. No changes to firstboot.rs were necessary because the persisted regression exercises its existing helpers through the corrected sampler input. No state.rs change is needed for C2.

## Assumptions

- A due measurement pass is conservatively unhealthy until all its configured present mounts have been attempted: initial absence of sampler health is unhealthy, and subsequent passes explicitly degrade previous health before collection. This prevents firstboot from certifying an incomplete current pass.
- Errors for absent configured mounts remain until measurement recovery or configuration removal, alongside the existing absence diagnosis.
- The existing ten-second followup throttle and permanent retention of a genuinely healthy historical firstboot report are intentional contracts.
- Test-only injected mounts need no actual mounts or filesystem mutation; measurements are injected, while state and firstboot persistence are real. Private temporary state fixtures explicitly use mode 0700.
- No user questions, delegation, branch creation, merge, push, privilege escalation, host service/mount/LVM mutation or memory projection edit was performed. Canonical memory brief/input/receipts had no pending update to apply.

## Proposed plan and security text

For `docs/plan/waves/wave-6-daemon-packaging.md` sampler/firstboot behavior: “Retain each configured mount's measurement failure until that mount succeeds or leaves configuration. Publish measurements as they arrive, but publish sampler recovery only after the complete due pass succeeds. Presence checks alone cannot clear measurement failures. Firstboot certification must not use an incomplete sampler pass.” This is proposed wording; no plan files were edited.

For `docs/security.md`: “Firstboot sampler health is conservative during an incomplete multi-mount pass. A successful sibling measurement cannot certify recovery of a failing mount. This correction changes health aggregation only; it adds no content reads, unsafe code, privilege, listener, external command or filesystem mutation outside Tidemark-owned state.” No security file was edited.

## Guest-only and unrestricted-runner checks (pending)

Run in disposable Rocky 8.8 and 8.10 guests with cached dependencies; record kernel, uid, umask, guards and actual assertions. No privileged runtime, fault scripts or install scripts were run here.

```sh
export CARGO_NET_OFFLINE=true
umask 022
timeout 1500 cargo test --offline -p tidemarkd --lib -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a1 -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2 privileged::daemon_reload_and_shutdown -- --exact --nocapture --test-threads=1
```

The source-local sampler regression needs neither root nor a VM; the guest adds real listener/service/mount integration coverage. Actual staged-install/reboot verification needs the coordinator's disposable guest and prepared RPM/mount fixtures; follow the existing install harness there and check that `deploy verify --wait` stays unhealthy until genuine full recovery. Current status and historical firstboot certification are separate observations.

## Validation evidence

All Cargo commands used Rust 1.98.0, the populated `/home/brewerm/.cargo` cache, offline mode, explicit `umask 022` for the workspace run and `timeout 1500`. Full command transcripts and exit files are under this attempt's external output `library/`. Counts include nested child summaries and guarded tests; a successful early return is not scenario coverage.

## Controlled regression evidence

- Restoring the baseline sampler production body while retaining only the private completion seam and new tests made the two-mount test fail immediately on premature `sampler=ok` (exit 101, `c2-negative-control.log`). The original production bug is directly rejected; the hook only observes completion.
- Injecting error clearing on a non-due presence pass made the synchronized single-mount test fail after observing that pass's completion (exit 101, `c11-negative-control.log`). Its expected degraded state was actually `ok`; no scheduling sleep was used to establish completion.
- Restoring the final source passed all five sampler tests (exit 0, `restored-sampler.log`). Neither fault injection is committed.
- The first full workspace run passed. The first `tools/ci.sh` run exposed `full_channel_sampling_is_nonblocking` observing pre-measurement degraded health and unwrapping missing sample history (`library/ci.log`, exit 1). The in-scope correction defers the initial health publication until its first measurement, while immediately invalidating previous sampler health on later passes. Both sampler and full w6a1 tests passed afterward. Final validation is separately retained under `library/final-validation/`; the original failure is not replaced or hidden.

| Final check | Observed result |
|---|---|
| `timeout 1500 cargo fmt --check` | Exit 0 |
| `timeout 1500 cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| `timeout 1500 cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | Exit 0 |
| `timeout 1500 cargo test --offline --workspace --no-fail-fast` | Exit 0; 857 passed, 0 failed, 14 ignored across 80 summaries |
| `timeout 1500 cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only |
| `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh` | Exit 0; all available checks passed; cargo-deny explicitly skipped because not installed |
| Final concurrent library repeats | 5/5 binaries passed; 460 parent-test results, ten isolated capture executions; ten-second presence test excluded from repeats and covered separately/full suite |
| Restored focused sampler suite | 5 passed, 0 failed |
| w6a1 nocapture guard audit | 30 passing names, five socket-denial early returns; those five readiness bodies deferred |
| Fault/install script syntax | Seven separate `timeout 1500 bash -n <script>` checks exit 0; no scripts executed |
| Scope/whitespace | Only owned paths changed; `git diff --check` exit 0 |

CI counts: 857 passed, 0 failed, 14 ignored across 80 summaries. Counts include nested helpers, empty suites and guard returns. Neither ignored scale tests nor guard returns establish scenario acceptance. Advisory/license freshness, privileged runtime, actual XFS/fanotify/LVM/service/reboot behavior and unrestricted listeners/namespaces remain unverified here. No claim of independent verified task success is made.

## Completion record

Four focused code commits cover C2, C10, C11 and the C2 initial-health ordering correction discovered by CI. This note is committed separately. Final cleanup, clean-status confirmation and submission receipt are recorded in the external `report.md` and `library/`; acceptance remains with the coordinator. No memory proposal was needed, and no update was acknowledged without application.
