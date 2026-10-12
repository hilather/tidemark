# Handoff — W4-B-fix2: deployment eligibility, config byte parsing and client EINTR

Needs from coordinator: run the foreign-owner scenarios in a disposable VM with `TIDEMARK_DEPLOY_TEST_VM=1`, and independently review this evidence candidate. Privileged tests were compiled only; no VM acceptance is claimed. Cargo-deny was unavailable and its audit remains unrun. Review the retained out-of-scope throttle timing failure described below.

- Attempt: `attempt-863301e26afa8f9616e21a0945fc60ac5c3f8a50182fa37ca285d46b4d6e0d2c`
- Branch: existing canonical attempt branch; no branch creation, merge or push.
- Date: 2026-10-11
- Status: implementation and local validation evidence candidate.

## Findings

| Finding | Status | Implementation and regression evidence |
|---|---|---|
| G1 | fixed now | Shared `deploy/files.rs::eligible` checks `fstat` owner against effective uid and rejects every group/world permission bit. `state_handles` returns the no-follow state/deploy handles retained by stage, ordinary unstage and `firstboot::write`; subsequent state reads, writes and deletion use those handles. RealEnv uses the same check, removing its distinct root/group comparison. Errors name the directory, owner and octal mode. `deploy/tests.rs::unsafe_existing_directories_refuse_stage_and_unstage`, `unsafe_existing_directories_refuse_unstage_without_marker`, daemon `unsafe_existing_directory_permissions_refused`, and `tests/w4b/mod.rs::unsafe_directory_permissions_exit_five_before_service_access` cover 0777/0750 state roots and deploy children, unchanged records/outside bytes/modes, absent reports, zero service queries/mutations and CLI exit 5. Foreign-owner equivalents are feature-gated and compiled only. `marker_authorizes_state_normalization_only_after_deploy_is_eligible` protects the existing marker exception and ensures refusal precedes normalization. |
| G2 | fixed now | `Config::from_toml_bytes` provides the single bounded UTF-8/TOML/default-merge path used by `load`. Source staging, marker-root validation and purge parse one bounded no-follow regular-file read, without pathname parsing reopens. `config_load.rs::byte_parser_agrees_with_loader` compares the example, oversize and invalid-UTF-8 cases. `source_replacement_does_not_change_parsed_or_installed_bytes` injects replacement immediately after reading. `stage_installs_and_hashes_the_config_parsed_before_replacement` exercises actual staging and checks installed bytes, state selection and SHA-256. `source_fifo_and_symlink_are_refused` and `marker_and_purge_config_fifo_or_symlink_are_refused` exercise nonblocking refusal at all three read sites. |
| G5 | fixed now | `client.rs` retries Interrupted on partial writes and buffered reads, recalculating the remaining total monotonic deadline each attempt. Private generic I/O seams exercise the same call/poisoning logic as Client. `interrupted_io_recovers_and_reuses_connection` injects both read/write interruptions and verifies two successful calls with an unpoisoned connection. `endless_interruptions_obey_total_deadline` advances an injected remaining-time budget deterministically and asserts three actual interrupted read attempts before timeout. `deadline_and_poisoned_connection` now injects transport timeout rather than relying on socket scheduling. |

## Implementation details and scope

Existing unmarked directories are refused rather than chmod/chowned. A valid marker identifying the exact state root retains the existing normalization authorization; a deploy child has no authorization from that root marker and must itself be eligible. Missing directories are created private, and an existing state handle is retained while a missing deploy child is created relative to it. Marker-authorized normalization uses the retained fd. Permission checks are destination eligibility, not a root uid gate or a group access check.

The daemon continues sharing the CLI-owned file helper through its existing path module. Public Client and firstboot signatures/struct fields are unchanged. The only core production change is the explicitly authorized additive parser and load delegation; its bounded UTF-8 lookahead preserves the loader's error precedence and invalid-UTF-8 message. No dependencies, Cargo files, module declarations, CLI flags, config keys, plan files or other daemon sources changed.

Old firstboot and missing-marker fixtures now explicitly create private directories. Their former permissive fixture modes are covered by refusal tests instead.

## Reproduction evidence

The artifact library contains `g1-before.log`, `g1-firstboot-before.log`, `g2-before.log` and `g5-before.log`. The permission tests fail with the original deployment implementation and original firstboot publication behavior. The private parser/I/O seams replay the original pathname-reopen and Interrupted propagation behavior: the config test observes `/different` instead of the captured state's path, and the client test returns `Io(Interrupted)`. Restoring byte parsing/retries makes these regressions pass. Temporary reproduction variants were restored and are not deliverables.

## Validation

All Cargo checks use the pinned 1.98.0 toolchain and offline host cache copied to `/tmp/tidemark-fix2-cargo`; CI uses `CARGO_NET_OFFLINE=true`. No privileged test, systemctl command, mount/LVM operation, host service enablement, or host configuration write was executed. All new executable fixtures use temporary directories and fake services.

| Check | Local result |
|---|---|
| `cargo fmt --check` | Pass |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Pass |
| `cargo test --offline --workspace` | Pass |
| `cargo build --offline --workspace --features privileged --tests` | Pass; compile only |
| `tools/ci.sh` with offline environment | Pass; cargo-deny unavailable, audit skipped |
| Focused CLI library / W4-B integration / daemon library | 70 / 42 / 8 passed, respectively |
| Core config-load regressions | 8 passed in the focused core run; included in workspace validation |

Privileged tests pending VM: `deploy::tests::foreign_owned_directories_refuse_stage_and_unstage` and `firstboot::tests::foreign_owned_directory_refused`, covering both state and deploy ownership. Existing deployment VM scenarios remain pending VM as well.

An intervening CI run failed the out-of-scope timing-sensitive `throttle::tests::bulk_waiter_follows_decrease_and_recovery` at `crates/tidemark-fs/src/throttle.rs:719:9` with `earned 745.948742852`. The isolated test and the final full CI rerun passed. The failure and rerun logs are retained; no throttle source was changed. Cargo-deny is not installed, so CI reports that it skipped the audit rather than establishing audit success.

## Remaining uncertainty

This is local evidence, not independent verification or production/VM acceptance. Concurrent privileged mount changes and directory renames remain subject to the existing deployment model; this fix retains checked descriptors through later state operations but does not claim a new mount-race guarantee. No workspace, crate manifest or shared module file edits were needed. See the canonical worker report/library for full command transcripts and submission evidence.
