# Handoff — W4-B-fix3: refuse non-local deployment state

Needs from coordinator: independent review and existing privileged deployment scenarios in a disposable VM. Local checks are evidence candidates; VM acceptance and cargo-deny auditing are not established.

- Attempt: attempt-119ae36e836fb8384e31b3af73eaeafa8cfa1378a81e08812af8819dc3025e19
- Branch: existing canonical attempt branch; no branches created, merge or push.
- Date: 2026-10-12

## What changed

Stage pre-flight now emits a blocking `state_filesystem` result using the same `all_paths` formatting and injected `Env::local_directory` probe as tracked roots. It probes the state path's longest existing ancestor with the existing production ancestor helper and filesystem classifier. Failures name the configured state path and rejected filesystem type, map to exit 5, and occur before deployment/service mutations. The new check name is also accepted when deserializing staged records.

The previously ignored `deployment_scenario_nonlocal_state` is active with its original exit-5, service-call and whole-tree unchanged assertions preserved. Six table rows add existing NFS state, a two-component missing state path on ext4 or NFS, unknown state type, and NFS unstage with retention or purge. Refusal rows also assert the reported path/type and zero injected writes. Fake filesystem injection resolves the same existing ancestor as production, preserving the original missing-state regression fixture.

Only deploy/mod.rs, real.rs, tests.rs and scenarios.rs plus this note changed. No shared files, dependencies, manifests, API method signatures, flags, config keys, plan documents, daemon or firstboot code changed. files.rs is untouched. Unstage and purge do not invoke the new check.

## Mutation evidence

Removed only the `state_filesystem` pre-flight outcome in the working tree and ran `cargo test --offline -p tidemark-cli --lib deployment_scenario_nonlocal_state`. Exit 101: the scenario received an Ok StagedRecord rather than the expected pre-flight failure. The record showed all old checks passing, demonstrating actual staging with the check absent. Restored the original bytes in a finally block; the full CLI library then passed. No mutation is committed. Transcripts: library/mutation.log and mutation.exit; library/restored.log and restored.exit.

## Assumptions

- The coordinator decision authorizes this additive pre-flight result; the exact plan wording below remains a proposal.
- Filesystem eligibility follows the existing mountinfo-based local-directory probe, including its existing mount-race limitations; this task does not establish a new race guarantee.
- Fake-host tests supply filesystem kinds without creating real mounts. NFS unstage/purge rows stage while eligible, then inject NFS before removal to model an existing bad installation.
- The existing no-follow state guard continues to reject unsafe ancestry before stage reaches this probe.

## Proposed wording for 08-interfaces.md §5

Add this blocking table row:

> | The filesystem holding `general.state_dir`, or its longest existing ancestor when the directory does not exist yet, is an allowlisted local filesystem (xfs, ext2, ext3, ext4, btrfs, tmpfs, zfs, f2fs, jfs, reiserfs; unknown types block) | yes |

Add this paragraph:

> A failed state-filesystem eligibility check reports the configured state path and filesystem type, exits with status 5, and makes no host changes. This check applies only to `deploy stage`; `deploy unstage` and `deploy purge` remain available for removing an installation whose state directory is on an ineligible filesystem. It does not add a daemon or first-boot check.

## Validation

Commands use pinned Rust 1.98.0, `CARGO_HOME=/tmp/tidemark-fix3-cargo` copied from the host cache, `RUSTUP_HOME=/home/brewerm/.rustup`, and offline mode. Initial setup attempts failed because cargo was absent from PATH, then because the default cache lacked base64 and rustup's default home was read-only; these were corrected before validation. No privileged/destructive host command was run. Tests use temporary paths and fake service managers.

Workspace final test summaries (including doctests): 493 passed, 0 failed, 13 ignored. W4-B subprocess integration remains 5 passed.

CLI library baseline: 63 registered, 62 passed, 1 ignored. After: 63 registered, 63 passed, 0 ignored. Table coverage: 28 before, 34 after, plus the separate newly active non-local-state scenario. No test declarations were removed.

| Check | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline --workspace` | exit 0 |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | exit 0; all available checks passed; cargo-deny absent, audit skipped |

Logs: library/fmt.log, clippy.log, tests.log, privileged-build.log and ci.log, each with a matching exit file.

Existing privileged deployment and ownership scenarios are pending VM; feature-enabled builds are compilation only. Cargo-deny availability is reported separately by CI. Complete transcripts and exit files are in the canonical attempt output library.
