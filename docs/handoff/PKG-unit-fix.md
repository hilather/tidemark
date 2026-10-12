# Handoff — PKG-unit-fix: RPM interpreter checks and systemd memory limits

## Needs from coordinator

Rerun workspace tests and `tools/ci.sh` in CI where Unix sockets are allowed: both fail locally only on five existing `tidemarkd/tests/w6a1.rs` readiness deadlines, matching the task's documented sandbox limitation. Recheck the RPM and installed unit on Rocky 8.8/8.10; package/VM execution was not performed on this workstation.

- **Branch:** hp-482a23fa289c70833021e99f55c15a2f4fb1d9bf4e55ffcf947bc40ee43ed319
- **Agent/session:** Codex; attempt-482a23fa289c70833021e99f55c15a2f4fb1d9bf4e55ffcf947bc40ee43ed319
- **Date:** 2026-10-12
- **Status:** implemented; evidence candidate awaiting independent verification

## What changed

`tools/check-rpm.sh` accepts only an empty scriptlet interpreter tag or `/bin/sh` and requires an exact `/bin/sh` Requires entry. All other checks are unchanged.

Moved the memory-limit explanations above their settings in the packaged service and its normative plan block, preserving all words and values. The entire unit has no other trailing comments on values. Added `unit_values_have_no_trailing_comments`, which skips blank/comment/section lines, rejects ` #` and ` ;` after `=`, and requires exact whole lines `MemoryLimit=2G` and `MemoryMax=2G`. The existing exact normative comparison also passes. The packaged TOML is unchanged.

## Shared-file edits and deviations

No dependency, manifest, module-declaration, or other shared-file edits. Only the five owned paths changed. The plan change is confined to its unit block and explicitly authorized by this task; no contract values changed. No spec-change request needed.

## Validation

Used Rust 1.98.0 from `/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin`, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`. Cargo build/test/Clippy commands additionally used `--offline`; every long command ran under `timeout 1500`.

| Check | Observed result |
|---|---|
| `sh -n tools/check-rpm.sh` | Passed |
| Interpreter case checks using the actual shell stanza | Empty and `/bin/sh` accepted; `/bin/bash`, `(none)`, and multiple interpreters rejected |
| Requires check using the actual shell line | Exact `/bin/sh` accepted; missing entry and `/bin/sh-extra` rejected |
| `cargo fmt --check` | Passed after formatting the new test |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --offline -p xtask` | Passed, including all eight W6-C tests |
| `cargo test --offline --workspace --no-fail-fast` | Exit 101; only failing target `tidemarkd --test w6a1`, with 13 passed and 5 readiness-deadline failures; all other targets passed |
| `tools/ci.sh` | Source scan (140 owner tags), fmt and Clippy passed; stopped on the same five w6a1 deadline failures |
| `cargo build --offline -p xtask --features privileged --tests` | Compiles the packaging tests without executing VM operations |
| `git diff --check` | Passed |

The five failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`; all panic at w6a1.rs:27 with `deadline exceeded`. Initial fmt/Clippy/CI invocations encountered a read-only default rustup home; final runs used the explicit host toolchain home above. Logs live under this attempt's output `library/` (`fmt.log`, `clippy.log`, `xtask-final.log`, `workspace.log`, `ci-final.log`, `privileged-compile.log`, `shell-cases.log`).

## Assumptions

- An absent RPM interpreter tag is represented by the empty array query output documented in the task; literal `(none)` remains rejected.
- `/bin/sh` is an unversioned Requires line, as reported by the real package build; exact matching avoids accepting a different path.
- The standalone shell case checks validate the changed predicates; they do not replace a real RPM inspection.
- Existing readiness-deadline failures are consistent with the documented sandbox socket restriction; confirmation belongs to coordinator CI.

## Remaining uncertainty

RPM inspection and Rocky systemd 239 verification remain pending coordinator/VM execution. No privileged or destructive host commands were run. No new privileged test is needed for these static checks; the existing packaging privileged test compiles and remains **pending VM**. CI did not reach its later full-workspace privileged build or cargo-deny stage.
