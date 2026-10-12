# Handoff — TEST-prune: Remove redundant tests and compile privileged tests in CI

- **Branch:** hp-c03073b3ead93d30795f2e133f9826206e0a09a1831acc65bb2a0c9250952d8b
- **Agent/session:** Codex; attempt-c03073b3ead93d30795f2e133f9826206e0a09a1831acc65bb2a0c9250952d8b
- **Date:** 2026-10-12
- **Status:** implemented; evidence candidate awaiting independent verification

## What changed

Removed the 13 specified tests and the duplicate deployment harness. No new tests, dependencies, contracts or production behavior were introduced. CI now compiles workspace tests with the privileged feature immediately after running the unprivileged workspace tests, using the same Cargo invocation and environment conventions as neighboring steps. No xtask test or source check enumerates the CI steps, so none needed updating.

## Removal inventory

| Item | Disposition and retained coverage |
|---|---|
| CLI w1a `cli_paths_resolve_before_normalizing` | Removed whole `paths.rs` and probe inclusion; executable realpath coverage remains in w1a/main.rs. |
| CLI `exit_codes` | Removed; context-preserving exit-code mapping test remains. |
| CLI size `formatting` | Removed empty test module/import; core formatting tests remain. |
| xtask `example_config_validates_with_built_daemon` | Removed nested Cargo build test; normative configuration and binary surface checks remain. `Command` import remains used by script tests. |
| FS `captured_el8_inputs_pending_vm` | Removed skip-only test and its README paragraph; capture directory is absent. Synthetic mount parser tests remain. Genuine Rocky captures remain pending VM access as stated in the README. |
| sys `rocky8_measured_field_widths` | Removed; pinned layouts and ioctl tests remain. |
| sys `uapi_layout` | Removed with four unused repr(C) structs; decoder now has a short comment documenting metadata and FID byte offsets. Decoder tests remain. |
| live w5d `filling` | Removed test and filling snapshot; degraded snapshot remains. |
| core `three_display_entry_points_agree` | Removed empty shared-display test module/import; path/name and escaping tests remain. |
| core `example_matches_interface_contract_byte_for_byte` | Removed; config example roundtrip/equality test remains. Example fixture remains used. |
| scan w2a `walker_golden_measured_du` | Removed; `scan_disk` now has `cfg(feature = "privileged")` for its remaining loop-XFS caller. Mandatory w2c oracle coverage remains. |
| store `attach_snapshot_file_read_only_immutable_and_detach` | Removed direct test-owned ATTACH; shared fixture remains used by connection streaming test. Production diff attach tests remain. |
| store `metadata_fixture_remains_constructible` | Removed helper-only assertion. |
| CLI w4b deployment re-inclusions | Removed deploy/render/stage module re-inclusions and their imports/allow attributes. All 37 included tests were present in `cargo test -p tidemark-cli --lib -- --list` (normalizing stage_adapter/render module prefixes); no private-item inclusion is needed. The five w4b-owned binary tests remain. |

## Shared-file edits

| File | Change | Reason |
|---|---|---|
| crates/tidemark-cli/src/lib.rs | Removed specified test only | Explicit task ownership; no module or contract change |
| xtask/src/main.rs | Added `build --workspace --features privileged --tests` after workspace tests | Compile privileged tests in CI without executing them |

No manifest or lockfile changes.

## Validation and measurements

Formatting, Clippy and the post-change workspace tests passed locally. The privileged-test build also passed without executing privileged tests. The CI wrapper passed, including the new privileged build and source checks (139 owner tags; forbidden-symbol scan passed). Cargo-deny was unavailable and CI reported its existing allowed skip.

| Workspace test totals | Passed | Failed | Ignored | Total |
|---|---:|---:|---:|---:|
| Before | 532 | 0 | 13 | 545 |
| After | 482 | 0 | 13 | 495 |

The reduction of 50 is exactly 13 specified tests plus 37 deployment duplicates. All five remaining w4b-owned tests compiled and passed.

The clean baseline `cargo test --offline --workspace` passed **532 tests**, with **13 ignored**, **0 failed** (545 total). Deployment duplication is 37 tests in this checkout, rather than the task estimate of 30. All listed removals still met their stated rationale; none were kept.

Commands use Rust 1.98.0, host Cargo cache (`CARGO_HOME=/home/brewerm/.cargo`), `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`; explicit Cargo builds/tests also use `--offline`. This environment is needed because the isolated home has no writable Rustup directory. The CI step inherits offline mode exactly as its neighboring steps do.

| Required command | Observed result |
|---|---|
| `cargo fmt --check` | Passed |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --offline --workspace` | Passed; 482 passed, 13 ignored, 0 failed |
| `cargo build --offline --workspace --features privileged --tests` | Passed; no privileged execution |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | Passed; source checks, fmt, Clippy, workspace tests and privileged build; cargo-deny skipped because unavailable |
| `git diff --check` | Passed |

Logs are retained under the attempt output's `library/` directory. Existing owner headers remain intact. Only the explicitly owned paths changed.

## Limitations

No privileged or destructive tests were executed. Privileged runtime validation remains **pending VM**. Independent verification and integration remain pending submission review. No wave section exists for TEST-prune; the explicit canonical task is the removal/build specification.
