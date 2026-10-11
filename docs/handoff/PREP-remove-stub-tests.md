# Handoff — PREP-remove-stub-tests: Remove preparation stub tests

- **Branch:** hp-73027c5963163a543120a1ca1c40552b465ff948abbd8ea0822c881aa7430894
- **Agent/session:** Codex, attempt-73027c5963163a543120a1ca1c40552b465ff948abbd8ea0822c881aa7430894
- **Date:** 2026-10-11
- **Status:** implemented; evidence candidate awaiting independent verification

## What was removed

Deleted `crates/tidemark-scan/tests/preparation.rs` (239 lines) and
`crates/tidemark-live/tests/preparation.rs` (130 lines). Both contained
`every_stub_has_an_inert_result`, asserting errors and inert values from preparation
stubs. `crates/tidemark-pg/tests/preparation.rs` did not exist at the baseline.
No production source, dependency, contract, or shared file was changed.

## Remaining stub assertions — coordinator follow-up

Audited all files under `crates/tidemark-scan`, `crates/tidemark-live`,
`crates/tidemark-pg`, and `crates/tidemark-cli/tests`, plus
`crates/tidemark-sys/src/fanotify.rs`, using text searches and inspection of tests.
These tests remain unchanged because their paths belong to implementing tasks:

| File | Test | Stub behavior asserted | Owner to warn |
|------|------|------------------------|---------------|
| `crates/tidemark-pg/src/collector.rs` | `signature_tests::every_stub_has_an_inert_result` | Probe, collector methods, and fake connector return `PgError::NotYetImplemented`; latest and backoff are `None` | W5-C |
| `crates/tidemark-sys/src/fanotify.rs` | `signature_tests::every_stub_has_an_inert_result` | Probe, initialization modes, mark/read/open-by-handle return `Unsupported`; event iterator returns `None` | W5-A |
| `crates/tidemark-cli/tests/w1a/main.rs` | `probe_json_has_all_sections_and_unknown_postgres` | Actual CLI probe returns null PostgreSQL reachability with reason `NotYetImplemented` | W1-A / coordinate with W5-C |

`crates/tidemark-cli/tests/w1a/render.rs::json_snapshot_and_text_unknowns`
also contains `NotYetImplemented` assertions and its `probe_v1` snapshot stores
that string. This is a rendering fixture explicitly constructed by the test,
not an assertion about a stub's runtime behavior; it can remain valid when
implementations land. Listed for completeness. Missing-path tests assert genuine
unavailable metadata, and CLI surface tests assert real contracts rather than
stub behavior. No additional stub assertions remain in scan or live after deletion.

## Shared-file edits and deviations

None. Used the precreated attempt branch without creating branches, merging, or
pushing. Builds use offline mode and the host Cargo cache, as requested. Set
`RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`,
`CARGO_NET_OFFLINE=true`, and put the pinned 1.98.0 toolchain's bin directory on
`PATH`; the initial invocation without host `RUSTUP_HOME` could not create the
sandbox's read-only rustup home and was corrected before validation.

## Acceptance criteria evidence and tests

| Criterion | Evidence | Result |
|-----------|----------|--------|
| Remove owned preparation tests | Git diff deletes scan/live preparation files; PostgreSQL file absent | Observed locally |
| Audit other tests without editing them | Remaining-test table above; no production-source diff | Observed locally |
| Format | `cargo fmt --check` | Exit 0 |
| Lints | `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| Workspace tests | `cargo test --offline --workspace` | Exit 0 |
| CI wrapper | `CARGO_NET_OFFLINE=true tools/ci.sh` | Exit 0; `ci: all available checks passed` |

Full logs are in the attempt output's `library/fmt.log`, `library/clippy.log`,
`library/test.log`, and `library/ci.log`. CI reported cargo-deny unavailable and
skipped its audit. No privileged tests are needed for deleting assertion-only
tests; no runtime behavior was added. No VM tests were run.

## Known gaps and notes for the reviewer

The three remaining runtime stub assertion tests require owner follow-up before
implementations land. Actual cargo-deny auditing remains pending an equipped
host. Independent verification and integration are pending submission review.
The final commit contains only the two deletions and this handoff; the repository
is checked for a clean tree after committing.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits or dependencies
- [x] Formatting, workspace Clippy, workspace tests, and CI wrapper passed offline
- [x] Remaining stub assertions listed with file, test name, and owner
- [x] Acceptance evidence and limitations recorded
- [x] Public documentation/unsafe/privileged-test requirements: no new source code
- [x] No new TODOs, unwraps, or expects
- [ ] Independent task verification and integration pending submission review
