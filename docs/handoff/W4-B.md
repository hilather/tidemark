# Handoff — W4-B: Staged deployment, first-boot report, API client

## Needs from coordinator

- Resolve [SCR-001](scr-W4-B/SCR-001.md): shared `Context` discards the global config path, and global validation can prevent replacement of an invalid installed config. The stage adapter uses `/etc/tidemark/tidemark.toml`; nondefault global `-c` install destinations are not implemented. Library `Env` accepts explicit destinations.
- Resolve [SCR-002](scr-W4-B/SCR-002.md): runtime compensation cannot give a transaction across config replacement, systemd and durable record publication. Pre-flight failure guarantees are implemented without side effects.
- Resolve [SCR-003](scr-W4-B/SCR-003.md): define the local-filesystem classifier; uncommon local types currently fail closed.
- Execute the three compiled privileged scenarios in disposable VMs. Reboot verification needs W6-A startup/API integration and a harness reboot between phases.

- **Branch:** hp-868c095bcad6094e3d58a2f5339aabe73b0811dfe69bc1c9e0814dbb8d789b28
- **Agent/session:** attempt-868c095bcad6094e3d58a2f5339aabe73b0811dfe69bc1c9e0814dbb8d789b28
- **Date:** 2026-10-11
- **Status:** core implementation complete; CLI destination behavior blocked by SCR-001; evidence candidate, pending independent review and VM gate.

## What was built

Implemented injected deployment checks, atomic private config/backup/record writes, exact-byte SHA-256, idempotent staging, optional immediate restart, polling verification and guarded unstaging. Production adapters use read-only mount/capability/capacity probes and bounded systemctl waits; unprivileged deployment tests use fake service operations and temporary paths. Purge validates a marker and the installed config before stopping, confirms inactivity, then removes state from directory handles with no-follow operations and mount-id checks. Firstboot builds core-component health, reads Linux btime, atomically publishes a private synced report and emits one tracing summary for the daemon's journald subscriber. The persistent API client uses frozen Request/Response types, matching version/id checks, newline framing, typed results, deadlines, bounded frames and failure poisoning.

## Shared-file edits

None. No manifests, lockfile, crate roots, shared render declarations, plan documents, core contracts or other task sources changed. New rendering lives under the owned `cmd/deploy/` directory. No dependencies were added. Integration entry `tests/w4b.rs` is explicitly reserved by W3W4-prep and the submission scope.

## Deviations from the spec

| Reference | Implementation / limitation | Reason / SCR |
|---|---|---|
| 08 §5 root pre-flight | Writable paths instead of a uid gate; no group check | ADR-018 supersedes the older root check; fake environments stage unprivileged |
| stage Args global `-c` destination | Production adapter uses the default install path | Shared Context loses that path; SCR-001 pending |
| 08 §5 nonzero exit shorthand | Runtime failures use best-effort compensation; transient restart effects, file metadata or empty new directories can remain | No filesystem/systemd transaction; SCR-002 pending |
| Local filesystem pre-flight | Recognized local filesystem types are allowed; unknown, network and pseudo types fail closed | Target XFS and ordinary local ext/btrfs/tmpfs/etc supported; uncommon local types may need the allowlist extended; SCR-003 pending |
| Purge marker unspecified | `.tidemark-state.json`, format `tidemark-state-v1`, contains byte-preserving RawPath state/config paths | Internal ownership proof; additionally load installed config and require its state_dir to equal the argument |
| Bounded resources | Records/frames 1 MiB; proc stat 16 MiB; fdinfo 4 KiB; purge depth 256; max 16 outstanding connection workers | Documented conservative caps, errors fail closed |
| Client connect timeout | Bounded connection threads because std has no nonblocking Unix connect constructor and rustix net feature is not attached | Caller deadline holds; stalled kernel connects retain at most 16 threads until they finish or the process exits |
| Listener runtime coverage | Listener test explicitly reports unavailable on EPERM; actual listener/connect/refused/missing integration branch did not execute here | Sandbox prohibits AF_UNIX socket creation. Socketpair tests exercise framing, transport deadlines and response validation; exit-4 mapping tested with typed I/O errors |
| Boot timestamp read failure | Logs a warning and uses booted_at 0 if proc stat cannot be read/parsed | Frozen build signature cannot return an error; supported-host btime verification is pending VM |
| Firstboot scheduling / journald sink | Report library emits a tracing event; daemon decides when to call it and installs its journald subscriber | W6-A owns startup timing and logging; runtime delivery pending VM |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| All unit tests pass | Offline workspace test log and focused deployment tests; listener coverage exception above | Local evidence only; see final checks |
| Privileged pre-flight and unstage on both VMs | `deploy_stage_preflight_blocks`, `deploy_unstage` compile with privileged feature | pending VM |
| 08 §5 checks and exit codes | Eight pre-flight rows plus previous-state info; fake pass/failure/warning fixtures; CLI invalid-source and confirmation tests exit 5; unavailable mapping exit 4; verify returns unhealthy evidence and command maps to 6 | Default destination implemented; SCR-001/002/003 unresolved |
| Fail → no side effects | `blocking_failures_change_nothing`, `invalid_config_changes_nothing`: config bytes unchanged, no service mutation calls, no state directory / staged record | Exercised locally |
| Backup, permissions, idempotence | `private_install_backup_hash_and_idempotence`, `raw_config_filename_backup_preserves_bytes`, `new_config_restage_requires_new_report` | Exercised locally; production root ownership pending VM |
| Verify health and timeout | `verify_missing_stale_unhealthy_healthy_and_api_failure`, `verify_timeout_is_bounded`; firstboot core health tests | Exercised locally |
| Safe purge | Marker/config/symlink guards, stop refusal and replacement-during-stop tests listed below | Exercised locally; mounted-state guard runtime pending VM |
| Firstboot atomic reporting | `report_lifecycle_and_atomic_permissions`, `failed_write_preserves_report_and_symlink_directory_is_rejected` | Exercised locally; journald/start timing pending W6 VM |
| Reboot verification | Two-phase `deploy_stage_reboot_verify` | Written and compiled; pending W6 VM |

## Tests added

Unprivileged deployment unit tests (`deploy::tests`):

- `preflight_all_environment_checks_and_config_validation`
- `capacity_byte_and_percentage_floors`
- `blocking_failures_change_nothing`
- `invalid_config_changes_nothing`
- `private_install_backup_hash_and_idempotence`
- `warning_checks_allow_staging_and_start_now`
- `new_config_restage_requires_new_report`
- `failed_enable_and_record_publication_restore_previous_files`
- `verify_missing_stale_unhealthy_healthy_and_api_failure`
- `verify_timeout_is_bounded`
- `unstage_retains_data_and_purge_removes_only_state`
- `purge_missing_marker_config_mismatch_and_replaced_symlink_leave_state_untouched`
- `purge_waits_for_stop`
- `skipped_warnings_do_not_invoke_probes`
- `marker_symlink_and_corrupt_marker_are_rejected_before_stop`
- `replacing_state_during_stop_does_not_delete_either_directory`
- `raw_config_filename_backup_preserves_bytes`
- `subprocess_wait_is_bounded_without_systemctl` (runs `/bin/sh` with `exec sleep`, kills/reaps it; no systemctl)

Client unit tests (`client::tests`):

- `framing_and_repeated_calls`
- `absent_and_refused_errors_map_to_exit_four`
- `unix_listener_connect_and_daemon_absence` — listener branch unavailable here (EPERM), prints the reason; pending unrestricted host execution
- `deadline_and_poisoned_connection`
- `mismatched_id_version_and_invalid_envelope_rejected`
- `remote_error_does_not_poison_connection`
- `truncated_and_malformed_frames_rejected`
- `oversized_frames_are_bounded`
- `maximum_frame_includes_its_newline`

Daemon unit tests (`firstboot::tests`):

- `core_health_requires_all_and_only_ok`
- `report_lifecycle_and_atomic_permissions`
- `failed_write_preserves_report_and_symlink_directory_is_rejected`
- `maximum_report_is_readable_after_atomic_write`

CLI integration tests (`tests/w4b/`):

- `malformed_stage_config_prints_check_table_and_exits_five`
- `json_preflight_failure_and_purge_confirmation_exit_five`
- `missing_purge_marker_is_precondition_exit_five_without_service_calls`

Privileged tests, feature-gated only, no privilege ignores:

- `deploy_stage_preflight_blocks` — pending VM
- `deploy_unstage` — pending VM
- `deploy_stage_reboot_verify` — pending W6 VM; requires harness stage/reboot/verify phases

No tests are ignored pending other tasks. Existing workspace scale ignores are retained.

## VM invocation requirements

Use only a disposable VM with RPM/unit installed and `TIDEMARK_DEPLOY_TEST_VM=1`. Pre-flight and unstage tests serialize access within their test binary. Unstage expects a stopped daemon and restores its original enabled state. The reboot test requires `TIDEMARK_DEPLOY_VM_CONFIG` to name the source TOML and `TIDEMARK_DEPLOY_VM_PHASE=stage` before the harness reboot, then `verify` afterward. Verify asserts btime is newer than the staged timestamp. Do not execute these tests on the worker host.

## Known gaps and follow-ups

- Nondefault global config destination and invalid installed-config replacement require the shared CLI change requested in SCR-001.
- Runtime compensation is best effort, as requested in SCR-002; it cannot undo downtime, and empty directories / old file metadata may differ after failure.
- LVM collection remains another task's implementation. Its unavailable result is a warning; no external stub tests were changed.
- No VM or reboot was available; production root ownership, unit state transitions, native capability probes, real socket connections and journald delivery are not claimed.
- `verify --wait` bounds polling sleeps. An in-flight service query or API ping can extend wall time by its own timeout (10 seconds each); service mutation operations are capped at 120 seconds.
- The state directory must not contain mounts and must fit the 256-directory depth cap for purge; validation happens before service mutation. Purge confirms root identity again after stop, but concurrent privileged mutation after that check is outside the quiescent-state model.

## Notes for reviewer

Purge obtains absolute path components one at a time with directory/no-follow opens, opens the marker without following links, validates installed config equality, and checks fdinfo mount ids (supported on EL8 without statx MNT_ID). It streams entries, keeps a capped descriptor stack and never descends into symlinks. Before deletion it performs a separate bounded validation walk; directories preserve O_NOATIME through descriptor-backed iteration. Atomic file writes sync temporary bytes before rename and sync the parent afterward. Root production writes explicitly set file ownership to root:root; fakes retain their own uid. No unsafe code was added and no production service operation ran on this host.

## Definition of Done / final validation

See final check results below. This is an evidence candidate, not a verified task-success or release declaration. The unresolved SCRs and pending VM work remain explicit.

## Final local check results

All commands used pinned Rust 1.98.0, `CARGO_NET_OFFLINE=true`, and a private host-cache copy at `/tmp/tidemark-w4b-cargo`. Logs are in the canonical attempt output `library/` directory.

| Check | Exit | Evidence |
|---|---|---|
| `cargo fmt --check` | 0 | `library/fmt.log`, `fmt.exit` |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | `library/clippy.log`, `clippy.exit` |
| `cargo test --workspace` | 0 | `library/test.log`, `test.exit`: 322 reported passed, five existing scale ignores |
| `cargo build --workspace --features privileged --tests` | 0 | `library/privileged.log`, `privileged.exit`; compilation only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | 0 | `library/doc.log`, `doc.exit` |
| `tools/ci.sh` | 0 | `library/ci.log`, `ci.exit`; all available checks passed |

The test total includes the listener availability probe, whose integration branch could not run under the sandbox's EPERM restriction. `library/listener-availability.log` records that limitation with `--nocapture`. No newly added tests are ignored. Cargo-deny is not installed, so CI skipped advisory/license auditing; that audit remains pending an equipped host. The deadline fixture was corrected to hold its socketpair peer open, eliminating a scheduling race with peer closure. Client reads use an 8 KiB buffer and enforce the complete 1 MiB frame including newline; report size limits likewise include their newline. Final source and whitespace checks pass; only owned paths are staged for the deliverable.
