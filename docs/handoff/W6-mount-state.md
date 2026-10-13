# Handoff — W6-mount-state: mount state, firstboot recovery and rates

## Needs from coordinator

Run the socket-dependent readiness cases in unrestricted CI and the privileged mount/watcher and installed deployment checks in disposable Rocky guests. Privileged tests are compiled only here; the sandbox readiness returns are not transport acceptance. Independent verification and integration remain pending.

- Attempt: `attempt-84a9f4dd3d37d372db9a30f7260547184de6926dddf708a3772ddeaed484312b`.
- Base: `a56715928df37515749469df013c1a000c8b592f`; supplied attempt branch, no branch creation, merge or push.
- Date: 2026-10-13. Status: implementation and evidence candidate.

## Per-part changes and regression evidence

1. `LiveState::publish_mount_presence` resolves the complete configured sampler/watch union, then under the state lock records identity/presence and clears changed mounts' filesystem histories and availability. Generation checks reject obsolete passes. Configuration replacement removes retired presence entries. The sampler publishes presence before measurements on every successful table pass. `now` derives missing-mount reasons from current presence and filters them with the requested mount. Existing failure epochs/collector availability behavior is retained; persisted history is untouched. `mount_presence_recovery_remount_and_rate_history` asserts initial absence, no mount row, named degradation, recovery, identity replacement without history, disappearance, config retirement and stale-generation rejection. The existing injected sampler test still checks no parent filesystem measurement or persistence.
2. `firstboot::Followup` reads an existing report and reevaluates changed component facts at ten-second intervals until a healthy report for the current hash exists. It retains atomic durable publication and the report fields. A fresh followup resumes an unhealthy report after restart, preserves the boot timestamp, and refuses to rewrite a healthy report for the same hash. Persisted `written_at` also prevents same-hash rewrites less than ten seconds apart across restart. Daemon supervision polls this coordinator once every ten seconds. The scheduler remains responsible for initial readiness/deadline publication; its public `needs_report` behavior and tests stay intact. `unhealthy_followup_throttles_resumes_and_finalizes` fails if followup publication is omitted, and covers no-change suppression, throttling, restart, healthy finalization and changed hash.
3. Rates require a sample-history span of at least 300 seconds, excluding future rows. The plan's additional three-sample slope requirement is preserved by the existing rate fitter. Before that, `now` provides zero rates, no time-to-full and `rate: collecting <path>`; alert/metrics rate sources remain unavailable. Once history is ready, existing unavailable/stale-rate semantics are retained. The mount-state regression checks 299 seconds versus 300 seconds and no rate across replacement. `ring_eviction_and_injected_rates` now expects the short restarted history to be unavailable.
4. Presence logging has one owner: shared state invoked by the sampler for the configured union. Watcher service reconciliation no longer logs separately. Messages include the path in the message itself: `mount not present: <path>`, `mount present: <path> (<major>:<minor>)`, `mount changed: <path>`. Existing captured-log regression checks four transitions, steady-pass suppression, levels, path and device text.
5. Immediately after watcher start, the service compares `Watcher::mount_identity()` with the configured identity. A mismatch drops the newly started watcher before publication, reports configured and marked identities as an error, and retries on the next ordinary pass. Unsupported means cannot verify and is accepted; other errors fail the start. `marked_identity_mismatch_fails_and_unsupported_is_accepted` injects identity results into the exact production verification helper and covers match, mismatch, Unsupported and permission failure. Actual mark behavior remains pending VM.
6. `watcher_follows_mount_arrival_detach_and_remount_on_vm` writes both cycle events using a child shell process, with the target passed as a positional argument. The hosting process no longer generates events that the watcher deliberately filters. Its other writes/operations were inspected; this is the only event-generating regular-file write in that test. Compile only here; pending VM.

The named tests directly assert the behavior missing without each change; mutation-run evidence, if collected, is listed in the validation appendix. No claim of guest execution is made.

## Tests updated and workspace audit

Workspace searches covered `fs_rate`, `rate_15m`, `rate_1h`, `needs_report`, `not mounted` and sampler/watch configurations across all crate tests and daemon sources.

- `w6a1::ring_eviction_and_injected_rates`: short restarted history expects None.
- `w6b::measured_state_and_unavailable_omission`: assert rate/ttf metrics are absent for two minutes of history, then rewind/repopulate with a five-minute span to retain the measured-metrics assertions. This old expectation was found by the first workspace run and repaired before final checks.
- `mounts::tests::logger_emits_one_line_per_transition_and_device_changes_restart`: new message/path/device text.
- `w6a2s::watcher_follows_mount_arrival_detach_and_remount_on_vm`: child-generated events for both cycles.
- Five w6a1 readiness cases (`binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, `startup_reads_last_complete_from_catalog`) now probe UnixListener availability and emit one diagnostic/return for PermissionDenied or Unsupported. Other failures remain failures; socket-capable CI executes their full existing assertions.
- Added the mount-state integration regression and the firstboot/watcher helper unit regressions. CLI synthetic rate/render tests, live rate-fitting tests, metrics mock sources and scheduler initial-publication tests retain their existing contracts and assertions.

Before/after targeted source inventories: daemon unit tests 46 → 48; w6a1 unprivileged tests 28 → 29; w6a2s unprivileged tests 4 → 4. No fresh baseline workspace runtime run was performed. Tracked Rust `#[test]` declarations across the workspace: 705 → 708 (includes privileged and ignored tests). Final workspace runtime counts are recorded below; socket-dependent early returns count as harness passes, not executed readiness evidence.

## Shared edits, scope and assumptions

`lib.rs` changes only daemon wiring for the firstboot followup; no module/dependency declarations or manifests changed. All edits are within the task paths. No dependencies, HTTP, frozen DTO/config changes, privileged execution, system-directory writes, service operations or mount/LVM operations were added or run on the workstation.

Assumptions:

- The five-minute decision adds a minimum elapsed history span to the plan's existing minimum of three samples; the plan specifies no minimum elapsed span already.
- `needs_report` remains the scheduler's initial-report predicate. Restart recovery is implemented by the separately wired followup, avoiding changes to the unowned scheduler and its embedded initial-publication test.
- Component name/state/detail changes drive unhealthy retries; volatile sample and scan timestamps alone do not rewrite a report. The refreshed report includes current sample/watch/scan facts when a component changes.
- Presence inventory is bounded by the union of two validated 128-path lists (at most 256 entries). Filesystem series and failure inventories retain their existing 128-mount caps.
- Configured paths use the existing normalization and API display-label conventions. Presence failures to read mountinfo retain prior known presence while existing collector failure health reports the read error.
- The firstboot followup uses the same configured state path and no-follow deployment helpers as initial reporting, including staged marker validation.

## Parts not done

No requested implementation part intentionally omitted. Unrestricted socket readiness, real fanotify/remount execution, and installed `deploy verify` recovery remain unverified here. The mismatch seam tests verification failure; actual fanotify start/drop/retry needs the VM gate.

## Proposed plan text (not applied)

Replace Wave 6's open mount-state limitation with: “Live state records current configured mount presence and mount ID/device identity atomically. Presence/identity transitions clear only that mount's in-memory filesystem history and availability; persisted history remains unchanged. `now` omits absent mounts and derives removable `mount not present: <path>` diagnostics from current presence. One shared transition owner logs path-qualified transitions. After starting a watcher, compare the retained mark identity with the configured entry and release/retry mismatches.”

Add to 07 §2: “Rates require at least three samples and at least five minutes of in-memory history of one mount identity. During collection, now reports zero rates, no time-to-full and `rate: collecting <path>`; rate-dependent alert and metric inputs remain unavailable.”

Add to firstboot behavior: “Initial publication retains the component/deadline rule. An unhealthy report is refreshed when component facts change, at most once every ten seconds, including after restart. A healthy report is final for its configuration hash. A changed hash produces a new report.”

## Exact coordinator verification commands (guest only)

From a built checkout in a disposable Rocky VM:

```sh
TIDEMARK_DEPLOY_TEST_VM=1 cargo test --offline -p tidemarkd --features privileged --test w6a2s watcher_follows_mount_arrival_detach_and_remount_on_vm -- --nocapture
cargo test --offline -p tidemarkd --test w6a1 -- --nocapture
```

For the already staged guest using `/mnt/tidemark-install` (use the fixture device already provisioned there; no workstation execution):

```sh
sudo umount /mnt/tidemark-install
sudo systemctl restart tidemarkd
sudo tidemark now --json
sudo cat /var/lib/tidemark/deploy/firstboot.json
sudo systemctl restart tidemarkd
sudo mount /mnt/tidemark-install
sudo tidemark deploy verify --wait 5m
sudo tidemark now --json
sudo journalctl -u tidemarkd --since '-5 minutes' --no-pager
```

Confirm absent mount has no mount row and has its named degradation; restart resumes the unhealthy report; arrival updates firstboot to healthy and verify exits zero; only one path-qualified presence line per transition appears; early now rates are zero and ttf absent until at least five minutes of samples. After healthy publication, record the report checksum, restart and confirm it stays unchanged for the same hash. Exercise identity replacement using the disposable watcher fixture test above.

## Validation

All Cargo commands use the cached pinned Rust 1.98.0 and `--offline`/`CARGO_NET_OFFLINE=true`, four build jobs and two test threads, each long command under `timeout 1500`. Full logs and exit codes are in the external worker output's `library/`. Final results follow below.

| Final command | Result |
|---|---|
| `cargo fmt --check` | 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0 |
| `cargo test --offline --workspace --no-fail-fast` | 0; 651 passed, 0 failed, 13 ignored |
| `cargo build --offline --workspace --features privileged --tests` | 0; compilation only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | 0; all available checks passed, dependency audit skipped because cargo-deny is absent |

The first workspace run had 650 passed, one failed and 13 ignored: the old two-minute rate expectation in w6b. After updating that owned fixture, the selected metrics regression, CI workspace run and final `--no-fail-fast` workspace run pass. Initial and final logs/counts are retained separately. CI source policy reports 141 owner tags and a clean forbidden-symbol scan. The known five readiness cases now print and return when their Unix socket probe is denied, so these passes do not replace unrestricted readiness verification.

### Regression mutation checks

`library/mutation-checks.py` temporarily changes one owned source at a time, runs the selected regression under `timeout 1500`, and restores the exact original bytes in a finally block. All five mutants compiled and failed with test assertions (exit 101), not compilation failures:

| Removed behavior | Failing regression | Log |
|---|---|---|
| Clear changed mount history | `mount_presence_recovery_remount_and_rate_history` | `mutation-presence-clear.log` |
| Five-minute rate gate | `mount_presence_recovery_remount_and_rate_history` | `mutation-rate-span.log` |
| Refresh unhealthy same-hash report | `unhealthy_followup_throttles_resumes_and_finalizes` | `mutation-firstboot-freeze.log` |
| Reject marked identity mismatch | `marked_identity_mismatch_fails_and_unsupported_is_accepted` | `mutation-mark-mismatch.log` |
| Put path in present message | `logger_emits_one_line_per_transition_and_device_changes_restart` | `mutation-message-path.log` |

Restored-source targeted suites (daemon unit, w6a1, w6a2s, w6b) and format are rerun after the mutations; their results and the cleanup/submission receipt are in the external worker report. The child-write VM correction is compiled, not mutation-executed on this workstation.
