# Handoff — W4-B-fix: deployment confinement and unsafe-state refusal

- Branch: canonical attempt branch `hp-9880941c65872e96c475142999f19fd0ba469de2207ca5dcd5208fcc07c5fc86`
- Date: 2026-10-11
- Status: implementation and local evidence candidate; workspace validation blocked by out-of-scope constructors. Independent review and VM gates remain pending.

## Changes and evidence

| Finding / decision | Change | Evidence |
|---|---|---|
| R1 | State ancestors and directories open component by component with `O_DIRECTORY | O_NOFOLLOW`; final reads require no-follow regular files. Writes use `openat`, exclusive temporary names, `renameat`, and descriptor fsync; removals use `unlinkat`. Stage retains state/deploy handles across service actions and compensation. Ordinary unstage opens its deploy handle before stopping; purge retains its existing descriptor walk. Verify and firstboot reject unsafe artifacts. | `state_symlinks_refused_before_service_for_all_operations`: both ordinary and purge unstage, stage and verify refuse symlinks at deploy, marker, staged.json and firstboot.json; outside bytes survive and service mutation calls remain empty. Daemon `unsafe_state_and_record_symlinks_are_refused` covers publication. Existing replacement-during-stop regressions still run. |
| R2 | State preflight rejects unsafe lexical paths, symlink components, protected/tracked containment, and mount boundaries. CLI unstage additionally checks its loaded configuration's roots. Marker-backed unstage/firstboot recover tracked roots from the marker's config; without a marker, their frozen path-only APIs use default roots. Directory permissions/ownership change through an fd only for newly created directories or a valid marker identifying that exact directory. Production refuses an existing unmarked state directory unless it already has 0700 root:root. | `unsafe_state_values_never_mutate`, `state_cannot_contain_tracked_root`, `existing_unmarked_directory_permissions_are_untouched`, daemon unsafe-state test. No fake file writes or service mutations for the refused stage values; no state/config output appears. Mount-id checks use bounded `/proc/self/fdinfo` reads, including same-device mounts; mounted fixtures remain pending VM. |
| R3 | An identical request is a no-op only if enabled, its requested start outcome is satisfied, and its recorded start option matches. New immediate-start requests restart; disabled services are enabled again. | `identical_restage_honors_start_and_repairs_enablement`; original identical-request idempotence test. |
| R4 / SCR-001 accepted | `Context.config_path` preserves global `-c`; the stage adapter builds its install destination from that path, resolving relative destinations against cwd. Stage dispatch bypasses installed-config loading and validates its source itself, including when the installed file is missing/invalid. Other commands retain installed-config validation. | Adapter `custom_destination_and_replacement_use_source_with_fake_environment` installs source bytes at the selected custom destination with missing/invalid previous configs and fake systemctl/Env. CLI `valid_source_bypasses_invalid_installed_config_and_exits_five_for_unsafe_state` proves source dispatch and exit 5 without service access. |
| R5 | Installed-config inspection/comparison/backup reads use verified parent descriptors and `O_NOFOLLOW`; a destination symlink fails preflight before backup or service mutation. | `installed_symlink_blocks_identical_and_differing_config`: both target variants preserved, no backup, no service calls. |
| R6 | `MARKER` and `MAX_FRAME_BYTES` are private. | No other production caller uses either constant; focused build/lint and regression compilation. |
| SCR-002 accepted | Post-preflight publication/service errors restore previous config, marker and staged-record bytes, remove the new backup and attempt the previous enabled/active state. Compensation service steps are independent so an error does not suppress later attempts. Every returned publication error includes the initiating error, compensation errors and an instruction to re-run deploy stage or deploy unstage. | `every_publication_and_service_failure_compensates_previous_state`: backup, config, marker, staged record, enable and restart failures across four previous service states. `compensation_errors_are_reported_with_original_failure` tests combined error text. `previous_staged_and_marker_bytes_survive_publication_failure` covers restoration of existing records; `service_compensation_failure_is_reported_and_verified` covers failed service recovery and final-state confirmation. Existing enable/record restoration test remains. |
| SCR-003 accepted | Exact local-type allowlist retained, extracted into a classifier for direct testing. Unknown types fail with their type named. | `accepted_local_filesystems_and_unknown_types` accepts all ten specified types and rejects nfs/cifs/overlay/fuse/unknown. |

## Exact state_dir refusal policy

Refused: `/`; every nonabsolute path; any path with a `..` component; any path whose existing ancestry includes a symlink or nondirectory; any directory that is a mount point (a conservative superset of tracked-root mount points); any path equal to or an ancestor containing a configured tracked root, sampler mount or watch mount, including an existing root's canonical location; any path equal to or an ancestor containing `/etc`, `/usr`, `/var`, `/var/lib`, `/run`, `/home`, `/root`, `/boot`, `/dev`, `/proc`, `/sys`, or `/tmp`.

The system-directory policy is containment **by the proposed state directory**. Descendants such as `/var/lib/tidemark` and a private fixture beneath `/tmp` are eligible. `/var/lib` and `/tmp` themselves are refused. Unsafe/mounted deploy subdirectories, symlink/nonregular marker/record/report files and malformed/path-mismatched markers also block before service mutation. Existing unmarked production state directories with inappropriate mode or ownership are refused rather than changed.

## Ownership check; ADR-018

No uid gate or group is introduced. Production preflight checks writable/searchable existing destination ancestors with `access(WRITE_OK | EXEC_OK)`, no-follow directory ancestry, and the existing unmarked state directory's owner/group/mode. Production publication requests root:root on the temporary file before rename; marked/created state directories receive descriptor-based ownership changes. An inability to set required ownership is an I/O failure with compensation where publication has begun. Fakes retain their own uid and only use temporary paths. The obsolete normative "running as root" entry should be replaced with these destination checks.

## Shared-file edits and implementation notes

Only the accepted SCR-001 changes in `crates/tidemark-cli/src/lib.rs`: new Context field, construction sites populated from global config, and stage-specific independent source dispatch. No dependencies, Cargo files, frozen core types, flags or config keys changed.

The daemon compiles the same owned `deploy/files.rs` helper through a `#[path]` module, avoiding a CLI dependency or duplicate security implementations under the no-Cargo-edit constraint. Firstboot retains its summary tracing event. Directory opens request O_NOATIME with EPERM fallback. Reads and fdinfo are bounded; purge's existing capped iterative walk is unchanged.

The W4-B integration target also compiles the deployment and stage-adapter source modules and their fake unit tests. This lets these tests run despite unrelated command-unit-test Context constructors needing coordinator edits; it does not conceal workspace compilation failures.

## Needs from coordinator

- `crates/tidemark-cli/tests/w5b/commands.rs:10` and `:50` construct Context and need `config_path`. These paths are outside this attempt's scope and were not changed.
- `crates/tidemark-scan/tests/w2a/main.rs:553` constructs WalkConfig without `max_record_bytes`; workspace Clippy/tests/privileged builds/CI cannot compile that test. Outside scope; not changed.
- `crates/tidemark-cli/tests/w2c/main.rs:807` and `:813` format ScanEntry with Debug, which that core type does not implement; broad focused-package tests also cannot compile. Outside scope; not changed.
- Update normative plan/runbook wording for accepted SCR-001/002/003 and ADR-018. Existing plan files still describe the older guarantees and destinations; none were edited.

## Validation

All direct Cargo build/lint/test commands used `--offline`; CI used `CARGO_NET_OFFLINE=true`. Pinned Rust 1.98.0 used the host cache copied to `/tmp/tidemark-w4b-fix-cargo`, because the worker's synthetic Cargo/Rustup homes are not writable/populated. Logs and exit files are in the canonical attempt output `library/`.

| Check | Result |
|---|---|
| cargo fmt --check | Pass |
| cargo clippy --workspace --all-targets -- -D warnings | Blocked by the out-of-scope constructors above |
| cargo test --workspace | Blocked by the out-of-scope constructors above |
| cargo build --workspace --features privileged --tests | Blocked by out-of-scope test compilation |
| tools/ci.sh | Source ownership/read-only checks and format pass; blocked at workspace Clippy |
| cargo test -p tidemark-cli -p tidemarkd | Blocked by the W5-B Context constructors and W2-C ScanEntry Debug uses |
| Focused CLI library/binary/W4-B Clippy, -D warnings | Pass |
| Daemon all-target Clippy, -D warnings | Pass |
| cargo test -p tidemark-cli --test w4b | 34 passed, none ignored; all deployment actions use fakes/temp directories |
| cargo test -p tidemarkd --lib | 7 passed, none ignored |
| cargo build -p tidemark-cli --features privileged --test w4b | Pass; compilation only |

## Remaining uncertainty / not done

No systemctl invocation, host configuration write, mount, LVM mutation, service enablement, VM run or reboot was performed. Privileged scenarios remain pending VM. Workspace success is not claimed until the coordinator updates the listed constructors and reruns checks. Cargo-deny auditing is not established by a CI run that stops at Clippy.

Compensation cannot undo observed downtime, timestamps or newly created empty directories. Concurrent privileged directory replacement remains outside the quiescent deployment model; retained handles prevent symlink replacement from redirecting later state writes/deletion. The frozen firstboot path-only API cannot discover arbitrary unstaged custom roots without a valid marker/config; the deployed lifecycle supplies that marker, and the fallback protects default roots. Mount-boundary runtime coverage needs disposable VM fixtures.
