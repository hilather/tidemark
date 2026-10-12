# Handoff — PKG-rpm-metadata: Accept external RPM metadata

## Needs from coordinator

Rerun workspace tests and CI outside this sandbox: both stopped on the same five readiness-deadline failures in `crates/tidemarkd/tests/w6a1.rs`. Actual RPM generation and privileged execution remain pending an equipped Rocky VM; neither was attempted here.

- **Branch:** hp-ca2b5e502f2c603e32624017bad5310e9418dac9abc7df08fbdde671989e839d
- **Agent/session:** Codex; attempt-ca2b5e502f2c603e32624017bad5310e9418dac9abc7df08fbdde671989e839d
- **Date:** 2026-10-12
- **Status:** implemented; evidence candidate awaiting independent verification

## What changed

Appended the empty `[package.metadata.generate-rpm]` table and its explanatory comment to the CLI manifest. Updated the RPM README to explain the manifest stub and workspace-root path resolution. Extended `metadata_and_scriptlet_contract` to parse the CLI manifest, require the generator table, and check every non-target asset and all three script paths against the workspace root.

For the unrelated privileged lint repair, only `crates/tidemark-fs/tests/w1a/main.rs` changed: added the crate-level `clippy::unwrap_used` and `clippy::expect_used` allowance used by neighboring integration test crates. Both Clippy modes passed; no further test or product-source repair was needed.

## Shared-file edits

| File | Change | Reason |
|------|--------|--------|
| crates/tidemark-cli/Cargo.toml | Empty generator metadata table plus comment only | Explicit task authorization; cargo-generate-rpm 0.20.0 requires its existence |

No dependency added. Cargo.lock, RPM metadata/scripts, xtask product source, and plan documents are unchanged.

## Validation

Pinned Rust 1.98.0 was put on PATH. Cargo commands used `--offline`; the wrapper inherited `CARGO_NET_OFFLINE=true`. The host registry cache was copied to `/tmp/tidemark-pkg-cargo` and used via `CARGO_HOME`, leaving the host cache untouched. Each required command ran under `timeout 1500`.

| Command | Result |
|---------|--------|
| cargo fmt --check | Exit 0 |
| cargo clippy --offline --workspace --all-targets -- -D warnings | Exit 0 |
| cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings | Exit 0 |
| cargo test --offline --workspace | Exit 101: only reported failures were the five w6a1 readiness deadlines listed below |
| cargo build --offline --workspace --features privileged --tests | Exit 0; privileged tests compiled, never executed |
| tools/ci.sh | Exit 1: source checks (140 owner tags and forbidden-symbol scan), fmt and Clippy passed; workspace tests stopped on the same five w6a1 deadlines |
| cargo test --offline -p xtask --test w6c | Exit 0: all four tests passed, including metadata_and_scriptlet_contract |
| git diff --check | Exit 0 |

The five failures in both workspace runs were `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. Each reported `deadline exceeded` at w6a1.rs:27. Their cause is consistent with the task's known sandbox restriction, but socket refusal was not independently diagnosed. The failing suite had 13 passing tests. Later workspace suites were not reached; the owned packaging suite was run separately.

A Python TOML cross-check also passed: the generator table is an empty dictionary, all seven static assets exist, and all three script paths exist. Full command transcripts are in this attempt's output `library/` (fmt.log, clippy.log, clippy-privileged.log, tests.log, build-privileged.log, ci.log, w6c.log).

## Assumptions and deviations

- The explicitly assigned PKG task supersedes the general branch-creation and manifest-edit conventions. Used the precreated attempt branch; no branch creation, merge, or push.
- Trusted the supplied Rocky guest evidence for generator 0.20.0; did not install or invoke cargo-generate-rpm.
- Followed neighboring test crates by allowing unwrap and expect at the test crate level.
- Initial default-cache Cargo attempts could not resolve base64 because the isolated HOME had no populated cache. Final runs used the copied host cache and resolved offline.
- No contracts or product source changed; no spec-change request needed.

## Remaining uncertainty

Actual RPM build/install and privileged runtime checks are **pending VM**. The sandbox workspace/CI failures require coordinator validation; this report does not claim a fully passing workspace or verified task success. Submission is an evidence candidate.
