# Handoff — CI-fetch: fetch all locked crates before CI tests

- **Branch:** hp-d5420c36054c7fee2b9389cd011db837d9d24206e67d44780bed82b23c5eda08
- **Agent/session:** attempt-d5420c36054c7fee2b9389cd011db837d9d24206e67d44780bed82b23c5eda08
- **Date:** 2026-10-13
- **Status:** implemented; independent verification pending

## What was built

Added the requested named `cargo fetch --locked` step in the `ci` job, after the toolchain and cache steps and immediately before `tools/ci.sh`. Without a target restriction, this fetch supplies locked crates for offline workspace resolution across platforms. The `rpm` job already runs `cargo fetch --locked` without `--target`; it needed no change.

## Shared-file edits

None. Only `.github/workflows/ci.yml` and this handoff changed. No dependencies or Rust source changes.

## Deviations from the spec

None. Used the precreated attempt branch as instructed. Cargo was initially absent from PATH; validation used `/home/brewerm/.cargo/bin`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_HOME=/home/brewerm/.cargo` to access the installed pinned toolchain and populated offline cache.

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| Fetch before tests, after cache/toolchain | Exact diff check confirms only the requested two-line insertion; order assertion passed | Observed locally |
| Fetch all locked platforms | New command is exactly `cargo fetch --locked`, with no target argument | Inspected |
| Check RPM fetch | Existing RPM fetch is `cargo fetch --locked`, no `--target`; RPM job unchanged | Inspected |
| Preserve workflow checks | Read `xtask/tests/w6c.rs`; it checks required strings and repository-install order, not an exact step list. Test unchanged; all 9 w6c tests passed | Observed locally |
| Offline xtask tests | `cargo test -p xtask --offline`: exit 0; 17 unit, 9 w6c, 6 w7b, 1 w7c tests passed | Observed locally |

## Tests

- Unprivileged: offline xtask suite passed (33 tests); exact workflow insertion/order/RPM-target assertions passed; `git diff --check` passed.
- Privileged: not applicable to this workflow-only fix; none run or added.
- Full workspace CI, online fetch, and a fresh GitHub runner were not run. crates.io is unavailable in this environment and contacting GitHub is outside this task.

## Known gaps and follow-ups

Fresh-runner execution remains pending external CI. Local validation demonstrates compatibility with existing xtask checks but does not reproduce a cold registry download.

## Definition of Done

- [x] Only owned paths changed
- [x] Required offline xtask tests passed
- [x] Requested workflow behavior and RPM inspection evidenced
- [x] Handoff written; no test files edited
- [x] No Rust source, contracts, dependencies, or privileged behavior changed
- [ ] Independent verification and fresh-runner CI pending
