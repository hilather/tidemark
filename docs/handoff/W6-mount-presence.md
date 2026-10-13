# Handoff — W6-mount-presence: exact configured mount presence

## Needs from coordinator

**Incomplete acceptance: do not treat this candidate as the complete product fix.** The owned services detect transitions, but `LiveState` exposes no supported way to remove/reset an individual filesystem series. `Publication::Fs` only clears a ring for clock rollback; fabricating rollback samples or temporarily replacing the configuration would corrupt observations or reload semantics. Please add an internal state publication/method that atomically records current configured mount presence and identity, clears that mount's ring on appearance/disappearance/identity change, and removes its stale sample when absent. Then call it for each sampler transition before publishing new measurements. Persisted history must remain intact. The required no-rate-across-remount assertion is pending this seam; currently an existing series can still span a remount.

`LiveStateView::now` ignores component degradation and its `Publication::Degraded` list is append-only. Please have current mount presence contribute `mount not present: <path>` to `NowReport.degraded`, removing the reason upon recovery/config removal. Appending permanent degraded strings from these services would leave recovered mounts falsely degraded, so this candidate does not do that. The exact `now` degraded acceptance assertion is pending this seam. This task expressly excludes changing `state.rs` and asks to describe needed publications here.

Watcher::start owns its fanotify group and opened mount descriptor privately. Its public API provides neither the descriptor nor its mount ID. Cheap verification of the actual mark requires a library-owned identity accessor/start check from the concurrent live task. No substitute descriptor is presented as proof of the mark. The residual check-to-mark race must be evaluated in the guest.

- Branch: precreated canonical attempt branch; no branch creation, merges or pushes.
- Agent/session: attempt-76574fbfdc6b2e3b550e3c077aa993f989789a39d3cbdd8f9ab77e93ea88e89e.
- Date: 2026-10-12 local.
- Status: partial implementation; state dependencies above remain unresolved.

## What changed

Added `mounts.rs`, using `tidemark_fs::mount` for parsing, exact mount-point comparison and later/topmost overmount selection. Identity includes mount ID and device major/minor. Production resolution canonicalizes existing configured paths as deployment preflight does; missing paths fall back to shared RawPath lexical normalization. Pure reconciliation accepts an injected parsed table and emits transition records only when presence/identity changes. Configured inventories bound its retained map.

The sampler reads one bounded mount table per pass, skips absent entries entirely, and reports `not mounted: <path>` in degraded health. It checks presence at most ten seconds apart while preserving the configured measurement interval, and samples immediately after a transition or reload. It verifies the mount ID and device of the opened sampling descriptor using bounded fdinfo plus fstat before statvfs. Identity transitions clear the service's persistence-key deduplication bookkeeping, **not the inaccessible LiveState rate ring**. A private mount-table source and clock seam lets unit tests inject arrival/disappearance without real mounts or binary test hooks.

The watcher checks one mount table every existing one-second pass. Absent entries get `watcher:<mount>` degraded with `not mounted`. Identity changes remove both the state and service handles before starting a replacement; failed starts are retried. Namespace refusal remains enforced. Settings reload and shutdown release handles as before. Initial/changed/missing identities are logged through transition vectors; steady passes emit no transition logs.

Scheduled and requested scans both run the presence guard immediately before `run_scan`, entering the existing reply/error, retry, logging and scheduler-health path on refusal. Component-prefix matching blocks a covered root and leaves unrelated roots eligible. The guard error includes `mount not present: <path>`.

Sampler aggregate health also includes absent watch-only entries, even when watching is disabled, because existing firstboot health considers sampler/scheduler/api/tswriter and does not gate on watcher components. All successful sample publications preserve already-known missing-mount degradation so firstboot cannot observe an intermediate healthy sampler while another configured mount is missing. No configuration, schema or wire fields were added.

## Shared-file edits

`crates/tidemarkd/src/lib.rs`: declares the documented `mounts` module only. No manifest or dependency changes. Public existing service signatures are unchanged. Scheduler edits are limited to pre-scan presence/error plumbing.

## Tests and part 4 audit

Workspace search: `rg -n 'sampler.mounts|watch.mounts|mounts=' --glob '*.rs' crates/*/tests`, plus searches of daemon/CLI test configuration and service startup calls.

- `w6a1.rs` shared `write_config` fixture now resolves the real mount containing its temporary directory via mountinfo. Its scan root stays `temp/mount`. This preserves real sampling in `full_channel_sampling_is_nonblocking`, lifecycle reload/shutdown, binary readiness, eligible-state startup, corrupt catalog recovery and startup catalog tests. Fixture consumers that only publish synthetic state remain synthetic; their contract/rate assertions need no mount injection. The new missing-mount test explicitly replaces sampler.mounts with the temporary scan directory, asserting no rows or persistence and unhealthy firstboot.
- `w6a3/operator.rs`: both real-binary operator scenarios now sample the actual containing mount and retain temporary scan roots. The successful-reload rule deliberately targets the unsampled temporary root, making its continued existence observable through unevaluable diagnostics. Invalid reload now asserts ok, reloaded=false, warning naming unknown_config_key, and continued visibility of the prior rule. Waits for the first rule evaluation before attempting the invalid reload.
- `w6a2/privileged.rs`: real-binary VM reload/shutdown fixture now samples the containing mount, uses it for its usage rule, and keeps the scheduled scan root temporary. Its old plain-directory sampler entry would now prevent scan admission.
- `w6a2s.rs`: procscan's nonexistent path intentionally remains a negative procscan test; it starts no sampler/watcher. Disabled-service tests have empty watcher entries. Existing privileged watcher-final-drop test already uses real `/`. Added absent-watcher service assertion using a temporary directory.
- `w6a3/privileged.rs`: existing end-to-end test uses actual LoopXfs and thin-LV mounts; unchanged.
- `w6a1.rs::now_uses_watcher_directories_without_exact_alert_windows`, `w6b.rs` and `w6b/source.rs`: injected Watcher/state fixtures do not start the production sampler/watcher service. Their `/fixture` identities remain synthetic; no changed production expectation or mount hook is needed.
- Core configuration/default-rule tests, fs parser/LVM tests, CLI fake API/render tests and deploy fake-environment tests do not invoke daemon mount sampling. No edits are necessary there. CLI VM deploy fixture already uses `/`.

Added unprivileged coverage:

- `mounts::tests::exact_presence_overmount_and_transitions_once`: initial absence, appearance, disappearance, two entries with one absent, overmount and exact-match behavior, identity change, steady-pass suppression.
- `mounts::tests::logger_emits_one_line_per_transition_and_device_changes_restart`: actual captured tracing lines for initial missing, mounted, device identity changed and missing again; one line each, correct INFO/WARN levels, no repeated-pass lines.
- `mounts::tests::scan_coverage_is_component_wise_and_includes_disabled_watch_entries`: covered missing root refused; present and unrelated roots admitted by the pre-scan guard. This is a guard test, not proof that an unrelated root completed an end-to-end scan.
- `services::sampler::tests::injected_mount_appears_then_disappears_without_sampling_parent`: injected mountinfo bytes and atomic clock; no initial parent sample/persistence, two configured entries with only one present-entry row, no absent-entry persistence after disappearance.
- `services::sampler::tests::descriptor_identity_is_checked_before_sampling_with_injected_clock`: descriptor identity mismatch refuses collection; supplied sample timestamp retained.
- `w6a1::missing_configured_mount_has_no_sample_or_persistence_and_unhealthy_firstboot`.
- `w6a2s::missing_watch_mount_is_degraded_and_has_no_watcher`.

Added privileged watcher regression `w6a2s::watcher_follows_mount_arrival_detach_and_remount_on_vm`: absent start, bind arrival from owned LoopXfs fixture, close-write event observation, lazy detach, handle disappearance, second mount/event cycle. VM environment/root assertions precede privileged operations; RAII stops the worker and detaches the owned target. **Compile only here; pending VM.** Lazy detach exercises disappearance while the watcher holds its mount descriptor.

## Validation

All Cargo commands use pinned Rust 1.98.0, host cargo cache and offline mode. Full transcripts are in the attempt output's library; final command results are listed below. No privileged test was executed here.

## Proposed plan text (not applied)

For 07 §1, 07 §3 and Wave 6 daemon behaviors: A configured sampler/watch entry is present only when the normalized path exactly equals a mount point in the daemon namespace. Its identity is the topmost mount's ID and device major/minor. Absent sampler entries produce no measurements or persistence; presence/identity changes clear only their in-memory rate series. Absent watcher entries start no collector; each presence pass shares one mountinfo read, checks at least every ten seconds, and releases the old collector before replacement. Verify the mark's identity when the collector exposes a cheap descriptor check. Publish current missing-mount degradation and clear it on recovery. Log each service's transition once, missing/changed at WARN and present at INFO. Refuse scans under absent configured entries before run_scan, retaining existing scheduled-failure backoff. Firstboot is unhealthy while any configured entry is absent. Startup still reports ready promptly; no new configuration/wire/schema fields.

## Guest verification commands — pending VM only

Use the disposable Rocky guest and existing loop filesystem from the install gate. Confirm its device and configured mount path before running the following. These commands were not run on the workstation. State must be outside the test mount. Clear/stage a fresh firstboot deployment record through the existing guest install/deploy workflow when checking firstboot.

```sh
# Guest only, with /mnt/tidemark-install configured in both mount lists and roots.
# Use the gate's already-created /dev/loop0; never guess a host device.
systemctl stop tidemarkd
umount /mnt/tidemark-install
systemctl start tidemarkd
tidemark status
tidemark --json now
# Expect missing watcher + degraded sampler, mount not present in now after
# the coordinator state follow-up, no parent sample and healthy:false report.
cat /var/lib/tidemark/deploy/firstboot.json
mount -t xfs /dev/loop0 /mnt/tidemark-install
sleep 10
tidemark status
dd if=/dev/zero of=/mnt/tidemark-install/mount-presence-event bs=1M count=30
sleep 5
tidemark hot --window 5m
tidemark --json now
# Verify the fanotify mnt_id against the decimal mountinfo ID; fdinfo mark IDs
# are hexadecimal. Verify samples restart and rates are unavailable until
# three new samples, after the coordinator series-reset follow-up.
pid=$(pidof tidemarkd)
cat /proc/$pid/mountinfo
for fd in /proc/$pid/fdinfo/*; do grep 'fanotify mnt_id:' "$fd" || true; done
# Lazy detach permits disappearance while an opened mount descriptor is held.
umount -l /mnt/tidemark-install
sleep 10
tidemark status
tidemark --json now
mount -t xfs /dev/loop0 /mnt/tidemark-install
sleep 10
tidemark status
tidemark --json now
journalctl -u tidemarkd --since '-5 minutes' --no-pager
# Also run the new compile-only regression in the disposable guest:
TIDEMARK_DEPLOY_TEST_VM=1 cargo test --offline -p tidemarkd --features privileged --test w6a2s watcher_follows_mount_arrival_detach_and_remount_on_vm
```

## Assumptions and limitations

- Per-service transition logging is intentional: a mount configured for both sampler and watcher has one line per service on each transition.
- Failed canonicalization keeps an absent configured path available for degradation, rather than aborting startup; no filesystem tree traversal is introduced.
- An unobserved detach/remount with exactly the same ID/device tuple cannot be distinguished; identity is the coordinator's specified tuple.
- Firstboot uses existing sampler health as its missing-entry gate, including watch-only entries; no new report fields or firstboot schema changes.
- Persisted history is never deleted. Current rate-ring reset and current now visibility are outstanding, not silently approximated.
- Watcher settings changes retain the existing restart-all behavior; mount transitions only replace affected collectors.

### Test counts before and after

The repository has 619 source-declared `#[test]` attributes at base `67cf683` and 627 at this candidate: seven added unprivileged tests and one added VM-only test. These are source counts, not a claim that every feature-gated or scale test executed. Daemon library unit counts changed from 22 to 27, w6a1 from 19 to 20, and unprivileged w6a2s from 3 to 4. Base runtime counts were not separately measured; unchanged baseline counts are inferred from added test declarations. The completed workspace run reported 567 passed, 5 failed and 13 ignored (all recorded ignored tests are existing scale gates). Final repeat results are recorded in the command table below.

### Focused evidence

Daemon library: 27 passed, 0 failed. w6a1: 15 passed, 5 readiness-deadline failures; the new missing-mount and existing queue-full sampler tests passed. w6a2s: 4 passed, 0 failed. Failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start` and `startup_reads_last_complete_from_catalog`. They match the task's documented socket-restricted sandbox category; confirmation in coordinator CI remains necessary. Real-binary operator tests return early when Unix sockets/root access are unavailable, so locally reported success does not prove their operator scenario executed.

### Acceptance status

| Decision | Evidence | Candidate status |
|---|---|---|
| Exact configured mount and topmost identity | Injected parser/reconciliation tests | Implemented and tested |
| Missing sampler entry never samples parent/persists | Injected sampler lifecycle; missing real-directory test | Implemented and tested |
| Clear rates across identity changes | No supported per-mount LiveState reset | **Blocked on coordinator state seam** |
| Watcher absence, stop/start/restart decisions | Shared pure reconciliation; production loop; missing service test | Decision logic tested; actual fanotify arrival/detach/remount pending VM |
| Verify actual fanotify mark ID | Descriptor private in library | Needs library-owned cheap check or guest confirmation |
| Refuse covered scans with existing failure handling | Guard test; shared worker run_scan precheck | Implemented; end-to-end scheduled/requested refusal not separately exercised here |
| One transition log per service transition | Captured tracing count including device change | Implemented and tested |
| Missing entry visible in now and clears on recovery | Append-only degraded publication cannot satisfy recovery | **Blocked on coordinator state seam** |
| Firstboot healthy=false while absent | Missing-mount firstboot test; aggregate sampler health | Implemented and tested for missing sampler; watch-only path included by code |
| Correct invalid reload and retain prior rule | Updated real-binary operator test | Compiled; operator runtime pending root/socket-capable host |
| Audit old directory-as-mount test fixtures | Per-fixture inventory above | Completed source audit; CI still required for skipped binary tests |

### Required command results

Environment: `PATH=/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, `CARGO_NET_OFFLINE=true`. Every long command used `timeout 1500`.

| Command | Exit | Evidence |
|---|---:|---|
| `cargo fmt --check` | 0 | library/fmt.log; final test strengthening also checked in fmt-final.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | library/clippy.log and clippy-final.log |
| `cargo test --offline --workspace --no-fail-fast` | 101 | library/workspace-tests.log: 567 passed, 5 failed, 13 existing scale ignores; only w6a1 target failed |
| `cargo build --offline --workspace --features privileged --tests` | 0 | library/privileged-build.log; no privileged tests executed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 | library/docs.log |
| `tools/ci.sh` with offline environment | 1 | library/ci.log: stopped at the same five w6a1 readiness deadlines; deny step not reached |

After the broad checks compiled their test binaries, the existing injected sampler test was strengthened to include a second always-absent entry. Production code did not change. The final daemon library run passed 27/27 tests, workspace Clippy passed again, and formatting passed again (daemon-unit-final.log, clippy-final.log, fmt-final.log). The feature-enabled build preceded only this test-body strengthening; the new privileged watcher regression itself compiled successfully. Whole-workspace/CI test counts include early-return socket/root scenarios, not proof of those scenarios' runtime behavior.

The source policy scanner ran through tools/ci.sh without reported ownership/unsafe/forbidden-symbol errors. No privileged or destructive host commands were executed. Guest-only commands above are instructions, not execution evidence.

## Remaining uncertainty

Coordinator CI must confirm the five lifecycle tests and the root-only real-binary reload/operator scenarios in a socket-capable environment. The new fanotify transition regression needs the disposable guest. Neither passing compilation nor this submission establishes complete task success: LiveState series reset and reversible now degradation remain outstanding.
