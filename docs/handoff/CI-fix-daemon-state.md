# Handoff — CI-fix-daemon-state

## Needs from coordinator

`firstboot::files` is private and has no crate-visible re-export. Reusing its inclusion requires changing `firstboot.rs`, which this task explicitly excludes. Both path inclusions remain, with the existing duplicate-module lint allowance; both compile the same CLI-owned policy source. Consider exposing that helper in a separately scoped task. Cargo-deny is not installed, so its dependency audit remains pending on an equipped host. Independent review and any privileged VM execution remain pending.

## What changed and why

Daemon startup now calls `state_handles(state_dir, false)` and `validate_marker_roots_at(state_dir, state.as_ref())`, then transfers the checked descriptors to `complete_handles`. It retains the resulting state descriptor for artifact inspection and storage binding instead of checking one descriptor and reopening the path. Reload uses the same noncreating validation path and discards the returned handles.

- `validate_state` retains unsafe-path refusal: absolute/non-root path, protected-directory and tracked-root containment, no symlink/nondirectory ancestry, and mount-point refusal. Existing deploy, snapshots/tmp and artifact checks remain. These checks run before directory completion or storage/service writes.
- `state_handles(..., false)` opens component-relative no-follow handles and applies the shared descriptor-based effective-uid/no-group-or-world-bits eligibility check to existing unmarked state and deploy directories, without creation or normalization. An unsafe existing directory therefore refuses startup before any artifact appears.
- `validate_marker_roots_at` reads any marker through the retained state descriptor, validates its format/path and staged-config roots, and applies default-root protection when absent. Calling it unconditionally avoids a pathname existence gate that could skip marker validation.
- `complete_handles` retains existing descriptors, creates missing private directories through the shared implementation, and preserves the shared valid-marker normalization authorization used by firstboot. Subsequent SQLite-artifact inspection uses `statat(..., SYMLINK_NOFOLLOW)` on that same state descriptor; storage binding retains it.

Removed the daemon's separate exact-mode/gid policy in favor of the current shared eligibility policy. No check implementation was copied. No CLI files, firstboot source, manifests, lockfile, dependencies or frozen contracts changed. The only module-related edit is the explanatory comment alongside the existing `state_files` inclusion in the owned `lib.rs`.

## Acceptance evidence

`nonprivate_state_directories_refused_without_creation` tests both 0777 and 0750: the shared eligibility error is returned, the state directory remains empty with its original mode, and no API socket or metrics output is created. `private_and_missing_state_directories_start` waits for the published daemon-ready state, then requests shutdown and joins successfully for both existing 0700 and missing state directories; resulting mode is 0700. Both use temporary directories and the existing ten-second state deadline, with no sleep-based assumption or success skip. Existing `unsafe_state_refused_without_creation` and `symlink_state_artifacts_are_refused` continue covering unsafe containment and no-follow artifact refusal.

## Assumptions

The requested parity with firstboot includes its existing valid-marker authorization to normalize a marked state root. Unmarked roots and deploy children must already meet shared eligibility; no new normalization exception is introduced. Privileged tests are compiled only, never executed on this workstation. The canonical attempt branch is retained without creating branches, merging or pushing.

## Validation

All commands used pinned Rust 1.98.0, `CARGO_HOME=/tmp/tidemark-daemon-state-cargo` (a temporary copy of the host registry), and `CARGO_NET_OFFLINE=true`. No privileged/destructive command or host service/configuration write ran. Transcripts are in this attempt's worker-output `library/`.

| Command | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline --workspace` | exit 0; 532 passed, 0 failed, 13 ignored across 51 logged suites |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only, pending VM execution |
| `tools/ci.sh` | exit 0; all available checks passed; cargo-deny absent, audit explicitly skipped |

No additional build errors outside scope were observed. Initial environment setup found cargo absent from PATH, then the default offline registry lacked base64; selecting the pinned toolchain directory and copied host registry resolved both without repository changes. The supplied baseline compiler failure was `error[E0425]: cannot find function validate_marker_roots in module state_files` at `crates/tidemarkd/src/lib.rs:757`; it is resolved by the current helper call.

Before: 16 w6a1 tests defined at the baseline; baseline startup cannot compile because it calls the removed `validate_marker_roots`. This is a source count, not a baseline passing test count. After: 18 w6a1 tests defined and all 18 passed in the focused run.

## Remaining uncertainty

This handoff is local evidence for review, not independently verified task or VM acceptance. Existing store pathname/leaf substitution limitations recorded in W6-A1 are outside scope. No new mount/rename race guarantee is claimed beyond the shared helper's model and retained checked state descriptor.
