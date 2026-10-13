# Handoff — CI-shellcheck-fix2: Remove shellcheck notes

- Date: 2026-10-12
- Attempt: attempt-925cfba1130801f693b7a270b9dab96a100f2021a80c03dd2d5711f79bb03ac5
- Status: implemented; evidence candidate awaiting independent verification.

## Changes

- `tools/check-rpm.sh`: replaced the argument-count and regular/executable-file `A && B || C` constructs (SC2015) with POSIX `if ! { A && B; }; then ...; fi`. Both still fail when either test fails, with the same diagnostics.
- `tools/el8-build-env.sh`: added `|| exit 1` to `cd` (SC2164); `set -e` already stopped the script on failure.
- Retained the existing reasoned SC2016 annotation for literal yum variables in the single-quoted sed expression and SC1091 annotation for sourcing the guest-provided `/etc/os-release`.
- Reviewed all 44 original EL8 script lines by hand for default-severity notes: no SC2015 chains, unquoted expansions (SC2086), declare-and-assign (SC2155), read (SC2162), unquoted command substitutions (SC2046), backticks (SC2006), unused assignments (SC2034), or dynamic sourcing (SC1090) remain. Other single-quoted sed text contains no shell variables.
- No shared files, dependencies, contracts, or test files changed.

## Validation

- `command -v shellcheck`: unavailable; review was by hand, not an actual shellcheck run. GitHub shellcheck remains pending.
- `sh -n tools/check-rpm.sh tools/el8-build-env.sh`: passed.
- `git diff --check`: passed.
- `cargo test -p xtask --offline`: initial invocation found no cargo on PATH. Retried with `/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin` on PATH; blocked with exit 101 because the offline cache lacks `base64`, required by `tidemark-core`. No tests executed.
- Neither script was executed. No system changes, RPM extraction, or privileged operations performed. No VM tests required for this syntax-only change.

## Remaining uncertainty

Actual shellcheck and xtask tests require an equipped runner/cache. No claim of green GitHub CI or independently verified task success.
