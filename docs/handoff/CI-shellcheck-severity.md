# Handoff — CI-shellcheck-severity

- **Branch:** hp-9bff74d3b951d0a678861c4bb8e687cfb319b6d6b2236473723ffd05547484d5
- **Date:** 2026-10-13
- **Status:** implemented; evidence candidate awaiting independent review

## What changed

Both shell validation tests now run shellcheck at default severity and print its stdout/stderr with `eprintln!` without asserting its exit status. A separate `--severity=warning` invocation gates warnings and errors, preserving the existing failure diagnostics. W7-C retains `-x` and its repository working directory for both runs. Existing Bash/POSIX syntax checks and VM guards remain intact. W6-C additionally covers `docs/pilot/capture-baseline.sh`, the only current pilot shell script.

## Hand review of scripts

Shellcheck is unavailable here; review was by hand, not a shellcheck execution. Read `tests/install/install.sh`, all three `packaging/rpm/*.sh`, both RPM tools, all six `tests/fault/*.sh`, `tests/fault/common.bash`, and `docs/pilot/capture-baseline.sh` for splitting/globbing expansions, directory-change failures, declaration/assignment masking, unused/unassigned variables, `read -r`, and source resolution.

Added narrowly scoped SC2164 annotations before the top-level `cd` in `tests/install/install.sh` and `tools/check-rpm.sh`: each already runs under `set -e`, so failed directory changes exit. Annotations preserve behavior. No other script edits were needed by hand review. Existing deliberate fixed-pair splitting in `check-rpm.sh` already has SC2086 justification; `/etc/os-release` already has SC1091 justification; the fault sources have explicit source directives and are followed with `-x`. Local assignments in the harness use parameters, arithmetic, or strings rather than command substitutions; reads use `-r`; argument arrays are quoted. Shellcheck on an equipped runner remains the actual diagnostic authority.

## Validation evidence

Used Rust 1.98.0 from its installed toolchain. Initial offline Cargo attempts failed because the default cache lacked `base64`. Retried successfully with `CARGO_HOME=/tmp/tidemark-shellcheck-cargo`, a temporary copy of the host registry, following W0-A's offline-cache fallback. No repository dependency or shared-file changes.

- `cargo fmt --check`: passed.
- `cargo clippy -p xtask --offline --all-targets -- -D warnings`: passed.
- `cargo test -p xtask --offline`: passed, 41 tests (20 unit, 9 W6-C, 6 W7-B, 3 W7-C, 3 W7-D).
- `bash -n`: passed for every reviewed script, including common.bash.
- `sh -n`: passed for RPM scriptlets, both tools, and the pilot script.
- `git diff --check`: passed.
- Temporary shellcheck command stub: for each changed test, default-severity exit 1 with informational stdout/stderr was printed and tolerated; warning-severity exit 1 failed the test and included both diagnostic streams. Four expected outcomes observed. Stub and logs are outside the repository; this does not validate real shellcheck diagnostics.

## Scope, deviations, and remaining uncertainty

Only the two owned Rust test files, the two script annotations, and this handoff changed. No shared files or dependencies changed; no contracts changed. No privileged operations or fault/install workloads were run. The existing unprivileged install-marker refusal check ran as part of W6-C. No new privileged tests are warranted for this static CI gate change. Full workspace CI and real shellcheck execution were outside this attempt's requested validation; real warning/error results remain pending an equipped runner.
