# Handoff — W1-gate-fix-xtask: VM test parameters and visible output

- Branch: precreated hp-caf535f7c8a8b79376bf5c54248546736396567b520e05b3bccaca8fced5c44a
- Agent: Codex; date: 2026-10-11
- Status: implemented; evidence candidate awaiting independent verification

## What was built

Added repeatable `--env NAME=VALUE` to vm-test. Parsing validates the exact
`TIDEMARK_[A-Z0-9_]+` name policy before run can start SSH or rsync; NUL values
are rejected. Each complete assignment is single-quoted with embedded quotes
escaped, placed after sudo env's PATH and before cargo test (or script bash).
Values may be empty or contain equals signs; repeated names retain input order.
Added cargo-only `--nocapture` after the test argument separator and optional
filter. Duplicate --nocapture is rejected consistently with other singleton options.

## Acceptance criteria evidence

| Item | Evidence |
|------|----------|
| G6.1 validated environment transport | `validates_environment_names_before_connection` covers accepted names, empty values, invalid names, missing assignments and NUL; run parses before connecting |
| G6.2 visible passing-test output | `places_quoted_environment_and_nocapture_in_test_command` checks exact command with --nocapture after -- and filter |
| G6.3 construction tests | Same exact-command test includes two assignments, spaces, a quote, semicolon, command substitution and embedded equals; checks PATH/env/cargo ordering and script-suite transport; existing command tests still pass |
| G6.4 VM documentation | xtask/README.md includes mdadm prerequisites, two disposable loop members, RAID1 creation, per-VM invocation and cleanup; lists runtime v5, bs_blocks comparison and leaf scheduler commands, with required recording destination |

## Shared-file edits and dependencies

None. Only xtask/src/vm_test.rs, xtask/README.md and this handoff changed.
No dependency added; no contract changes or deviations requested.

## Validation

All required checks passed locally with pinned Rust 1.98.0 and offline mode:
`cargo fmt --check`, `cargo clippy --offline --workspace --all-targets -- -D warnings`,
`cargo test --offline --workspace`,
`cargo build --offline --workspace --features privileged --tests`, and `tools/ci.sh`.
`CARGO_NET_OFFLINE=true` also applies to nested CI cargo commands.
The xtask suite reports eight passing tests. `git diff --check` passes.
Logs are check-0.log through check-4.log in this attempt's output library.

## Known gaps and follow-ups

Privileged execution and md provisioning are **pending VM**. No host was contacted,
no privileged/destructive command was run, and the runtime v5/bs_blocks/scheduler
findings were not collected. The README commands follow 09 §5 and assume unused
/dev/md127 in each disposable VM. Independent review and integration remain pending.
Cargo-deny availability/status is recorded in the CI transcript.
