# Handoff — CI-deny-required: The dependency check blocks the build

- Branch: precreated hp-93fb0d85a034622970525843a8593bfe79e987761794abd4a8cfa3086f8870e4
- Agent/session: Codex, attempt-93fb0d85a034622970525843a8593bfe79e987761794abd4a8cfa3086f8870e4
- Date: 2026-10-13
- Status: implemented; evidence candidate pending independent verification

## What changed

Removed the deny job's job-level `continue-on-error: true` and rollout comment. Retained the four category steps with `if: always()` and step-level `continue-on-error`, and the final failure step based on each category's original outcome. Installation and version logging retain default fatal failure behavior; the weekly schedule remains unchanged.

README now states the job is required, explains red job diagnostics, and directs advisory remediation to an upgrade or a dated, reasoned `[advisories] ignore` exception in `deny.toml`. Licence decisions remain with the owner. It explicitly describes local warning/skip behavior without cargo-deny and the strict `--require-deny` option.

## Scope and deviations

Only the three owned paths changed. No shared-file edits, new dependencies, Rust implementation changes, or changes to `deny.toml`. No task deviations. No branches created, pushes, GitHub contact or privileged commands. The supplied owner decision and prior GitHub category passes are accepted as task inputs, not independently rechecked.

## Acceptance evidence and validation

- Local Python/PyYAML checks passed: no job-level failure allowance; all four category IDs, commands, always conditions and step-level allowances preserved; install/version steps have no failure allowance; final exit 1 references all four failure outcomes; weekly cron retained.
- Read `xtask/tests/w6c.rs`: its workflow string assertions do not pin the deny job's `continue-on-error`; test file unchanged.
- `cargo test -p xtask --offline`: passed (43 tests: 20 unit, 10 w6c, 6 w7b, 4 w7c, 3 w7d; zero failures). Transcript retained in attempt output `library/xtask-test.log`. Used pinned Rust 1.98.0 and existing host Cargo cache without network access.
- `git diff --check` passed. Build artifacts cleaned before submission.
- Full workspace CI and actual cargo-deny execution were not run; the requested offline xtask check is the validation scope. No privileged behavior changed; VM tests are not applicable.

## Remaining uncertainty

Actual GitHub execution and repository branch protection remain uninspected; this change makes workflow failure fatal but does not configure merge protection. Independent verification and integration remain pending submission review.
