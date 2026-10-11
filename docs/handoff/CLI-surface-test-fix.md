# Handoff — CLI-surface-test-fix

- Date: 2026-10-11
- Status: implemented; evidence candidate awaiting independent verification
- Branch: existing canonical attempt branch (no branch creation, merge, or push)

## Changes and assertion audit

Removed the bare `tidemark probe` execution and its `.assert().code(1)` assertion from `binaries_and_bash_completions`. It depended on the former stub returning an error and exercised host-dependent command behavior. The existing bash completion success and output assertions remain unchanged.

Reviewed every test in `surface.rs`. No other assertion expected stub behavior or matched `not implemented`. Remaining subprocess executions are bash completions (plain and JSON), the offline daemon build, and `tidemarkd --check-config` against temporary valid/invalid configuration files. The invalid-config exit code 2 and unknown-field diagnostic are contract checks and remain unchanged. Command-tree, flags, defaults, parsing, help and rendering checks remain unchanged.

## Scope and deviations

Only `crates/tidemark-cli/tests/surface.rs` and this handoff changed. No shared files, dependencies, CLI contracts or host operations were added. Used the precreated branch and a temporary copy of the host Cargo registry cache with offline mode, as requested.

## Validation

All required commands passed locally (exit 0), using Rust 1.98.0, `CARGO_HOME=/tmp/tidemark-cli-surface-cargo`, and `CARGO_NET_OFFLINE=true`:

- `cargo fmt --check`
- `cargo clippy --offline --workspace --all-targets -- -D warnings`
- `cargo test --offline --workspace` (all 10 surface tests passed)
- `tools/ci.sh` (`ci: all available checks passed`)

CI reported that cargo-deny is not installed and skipped that optional check. Full logs are in the attempt output `library/{fmt,clippy,test,ci}.log`. `git diff --check` also passed.

Privileged tests are not applicable to this surface-only deletion; no VM tests were added or run. Independent verification remains pending submission review.
