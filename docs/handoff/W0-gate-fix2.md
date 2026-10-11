# Handoff — W0-gate-fix2: Confined fixture cleanup

- **Attempt:** attempt-60a597302ee8a2bb7dbceef80933845bd8632858fdfd02d7c37ad28a2b41f495
- **Branch:** hp-60a597302ee8a2bb7dbceef80933845bd8632858fdfd02d7c37ad28a2b41f495
- **Date:** 2026-10-11
- **Status:** implemented; evidence candidate, independent verification pending.

## What changed

R1: `TempDir` automatic pathname cleanup is disabled before acquiring anchors. `OwnedStorage` retains the parent directory handle, owner name and owner directory handle. Removal uses `rustix` no-follow opens, directory streams, `statat` and `unlinkat`, ending with removal of the owner entry through its verified parent handle. Each descent compares device/inode from the entry, probe and readable handle; the full retained ancestry is checked before each mutation. Mismatches and I/O failures preserve remaining storage, with one explanatory stderr line from Drop. No unsafe code or new dependency was added.

The explicit traversal stack is capped at 1,024 directory levels and traversal work at 1,000,000 entries. Tracked handles and the identity lookup are bounded by the existing 250,000-entry fixture cap. Device and mount identities prevent crossing filesystems or same-device bind mounts. Older kernels without STATX_MNT_ID use a bounded 4,096-byte read of kernel `/proc/self/fdinfo` metadata; no fixture file contents are read by cleanup. Missing mount metadata causes preservation rather than unsafe descent.

R2: tracked permission-controlled directories are indexed by device/inode, found at their current names during the owner-anchored traversal, and restored through their retained original handles only after finding and verifying them inside the fixture. Parents are restored before enumerating children. Directories moved outside receive no permission changes.

## Acceptance evidence

- `cleanup_survives_parent_symlink_substitution`: private parent renamed and replaced with a symlink to a victim tree containing the same owner basename. Victim sentinel survives; original fixture storage is removed through the retained parent.
- `cleanup_finds_internal_renames_by_identity`: nested mode-000 directories moved from root to the outside sibling are restored and removed. The test temporarily grants write permission for Linux's rename operation, then restores mode 000 before Drop.
- `cleanup_does_not_restore_directory_moved_outside`: externally moved tracked directory remains mode 000.
- `cleanup_preserves_replaced_owner`: substituted owner entry and moved original are both preserved on identity mismatch.
- `proc_mount_identity_matches_statx`: proc metadata reports the same mount ID as the normal lookup.
- Four existing cleanup regressions remain unchanged and pass: final symlink, intermediate symlink, outside symlinks, missing/replaced entries. Existing inaccessible-parent and random-tree symlink regressions also pass.

## Other cleanup paths checked

`[[outside]]` is traversed under the same owner and supports internal renames into it. `random_tree` delegates to `materialize` and has no separate destructor. `LoopXfs` and `LoopVg` previously called pathname-based `TempDir::close`; both now use the shared anchored storage remover after resource teardown. Their automatic TempDir cleanup is disabled, and cleanup verifies the owner pathname component by component without following symlinks before running teardown commands. Failure continues to preserve backing storage. Checked all source files for Drop, remove-directory and permission-change paths; no other production removal destructors exist. Permission changes during materialization remain fixture creation operations, not cleanup operations.

## Shared-file edits

None. No manifests, lockfile, contracts, plan documents or other crates changed.

## Validation

All Cargo commands used Rust 1.98.0, `--offline` where supported, `CARGO_NET_OFFLINE=true`, and a temporary copy of the host Cargo cache. Logs are in this attempt's output `library/`.

- `cargo fmt --check`: passed (`fmt.log`).
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed after fixing octal literals in new tests (`clippy-final.log`).
- `cargo test --offline --workspace`: failed outside scope in `crates/tidemark-cli/tests/w1a/main.rs:26`, `probe_json_has_all_sections_and_unknown_postgres`; expected Postgres reachable value to be null (`test.log`). That file was not edited.
- `cargo test --offline -p test-support`: passed, 12 unit tests and 10 integration tests; one scale test ignored (`targeted-final.log`).
- `cargo clippy --offline -p test-support --features privileged --all-targets -- -D warnings`: passed (`privileged-clippy.log`).
- `cargo build --offline --workspace --features privileged --tests`: passed (`privileged-build.log` and final repeat `privileged-build-final.log`).
- `tools/ci.sh`: source checks (126 owner tags), format and Clippy passed; exited 1 at the same out-of-scope CLI test failure (`ci.log`). The pipeline did not reach cargo-deny.
- `git diff --check`: passed.

## Remaining uncertainty

Privileged XFS/LVM runtime tests and mount-substitution checks remain **pending VM**; only compilation was performed. No mounts, LVM commands, service changes, branch creation, merges or pushes were run. No Rocky 8 VM was available. The proc mount-ID fallback was checked on this host, not an older kernel. Identity verification and mutation are separate syscalls; adversarial concurrent renames between those syscalls were not stress-tested and atomic exclusion of such races is not claimed. Resource acquisition failures before construction finishes may intentionally leave temporary storage because pathname rollback is disabled. Workspace test/CI failure outside scope remains for its owner to resolve.
