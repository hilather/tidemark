# Handoff — CI-deny2: Pin current cargo-deny and log each category

- Branch: precreated hp-3ea272b0706a09210b7ad7479e8d81070fee801cb87a5e052a69336c634353ec
- Agent/session: Codex, attempt-3ea272b0706a09210b7ad7479e8d81070fee801cb87a5e052a69336c634353ec
- Date: 2026-10-13
- Status: implemented; evidence candidate pending independent review and GitHub execution

## What was built

Pinned cargo-deny 0.20.2 in both the install command and binary cache key. Kept the job non-blocking with job-level `continue-on-error: true`. After version logging, advisories, bans, licenses and sources run in four individually named steps, each with `if: always()` and step-level `continue-on-error: true`. A final step exits 1 when any category's original `outcome` is failure; using `outcome` preserves failures masked by step-level continue-on-error. README documents the pin and category behavior.

## Scope and deviations

Changed only `.github/workflows/ci.yml`, `xtask/README.md` and this note. No shared manifest/module edits or dependencies. `deny.toml` is unchanged: the requested registry search found no cargo-deny source/documentation (the isolated home has no registry directory; the existing host registry also has no cargo-deny source). Configuration compatibility must be assessed from the next GitHub run rather than guessed. Release availability and minimum Rust version are accepted from the coordinator's supplied task evidence; no network or GitHub contact occurred.

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|-----------|----------|--------|
| Pin install and cache to 0.20.2 | YAML checks confirmed exactly two 0.20.2 references and no 0.18.4 in workflow | Observed locally |
| Preserve non-blocking job | Parsed job-level continue-on-error is true | Observed locally |
| Independent category logs and final failure | YAML checks confirmed all four commands, IDs, always conditions, step-level continue-on-error and version ordering; final condition checked across all 256 outcome combinations | Observed locally |
| Keep configuration unless cached documentation justifies changes | No cached cargo-deny documentation found; deny.toml has no diff | Observed locally |
| Update README; preserve w6c assertions | README pin updated; unchanged workflow_and_new_shell_syntax test passes | Observed locally |

## Validation

Used pinned Rust 1.98.0 binaries, existing host Cargo/Rustup caches and offline Cargo resolution. `cargo fmt --check`, `cargo clippy -p xtask --offline --all-targets -- -D warnings`, and `cargo test -p xtask --offline` passed. Test and Clippy transcripts are retained under this attempt's output `library/`. Python/PyYAML parsed the workflow and checked the category settings plus the final condition truth table. `git diff --check` passed. Build outputs removed with `cargo clean` before submission.

No Rust implementation or privileged behavior changed; VM testing is not applicable. Actual cargo-deny installation, advisory parsing and all policy/schema findings remain pending the coordinator's next GitHub run. Full `cargo xtask ci` was not run because the tool is absent and this task specifically requires offline xtask validation.

## Follow-ups

Coordinator: inspect all four category logs on the next GitHub run and brief any schema or policy changes with the exact messages. Keep the job non-blocking until findings are resolved. Independent verification and integration remain pending submission review.
