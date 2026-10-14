# Handoff — DEPLOY-restage: publish staging records before restart and restore refused purges

## Needs from coordinator

Run the guest acceptance commands below on disposable Rocky 8.10; no service commands or privileged tests were executed on this workstation. Local validation results and their limitations are recorded below. Install/run cargo-deny in coordinator CI: this host lacks it, so tools/ci.sh explicitly skipped that check.

- Branch: `hp-ef198a995d208a8637b80f9ce8a524bb2fe06362fb673b2c9a921fd3e70439e5`
- Attempt: `attempt-ef198a995d208a8637b80f9ce8a524bb2fe06362fb673b2c9a921fd3e70439e5`
- Date: 2026-10-14
- Status: implementation supplied for coordinator review; guest acceptance pending.

## Cause confirmed and changes

`deploy/mod.rs::stage` enabled/restarted the unit before writing its marker and `deploy/staged.json`. `tidemarkd/firstboot.rs::needs_report` returns false when staged.json is absent, and `scheduler.rs` latches its `reported` decision after checking that predicate. The order could therefore suppress reports for the entire run after purge. Stage now installs config, enables the service, publishes marker and record, then restarts for `--start-now`. Existing descriptor-relative compensation restores previous config, marker and record bytes, removes newly created backups and restores service state on later failure, including restart failure. The daemon is untouched.

`unstage` previously disabled/stopped before acquiring recovery and scan leases, and returned refusal without restoring service state. Purge now records enablement/activity first. On a post-stop failure before any successful unlink, it independently attempts service restoration, checks both resulting states, and appends either `nothing was removed; service left enabled and running as before` (with accurate disabled/stopped alternatives), or compensation failures and recovery guidance. Busy-lease refusals remain Guard errors (exit 5). `purge.rs` tracks successful unlinks so partial removal cannot trigger a restart or a misleading “nothing was removed” claim. Successful purge and non-purge unstage retain their behavior.

## Shared-file edits and dependencies

None. Only the assigned deploy directory and this handoff changed. No dependency added, no contract/daemon/store change, no branch creation or merge.

## Tests and acceptance evidence

- New `start_now_publishes_new_records_before_restart_and_restores_on_failure`: fault 1. Records publication/restart order; at restart checks marker paths and new staged hash/start option. Exercises fresh and existing state, successful restarts and injected failures; failed restarts restore exact previous config/marker/record bytes or absence.
- New `refused_purge_restores_each_previous_service_state_and_reports_failures`: fault 2. Held writer lease refuses purge across all four previous enabled/active combinations; checks exact service action order, record/marker bytes, Guard error and restoration message. Injected enable compensation failure checks both diagnostic verification and the independent restart attempt.
- Existing `every_publication_and_service_failure_compensates_previous_state`, `previous_staged_and_marker_bytes_survive_publication_failure`: continued rollback coverage.
- Existing `purge_refuses_live_writer_without_changing_bytes_or_inodes`: unchanged byte/inode evidence for every original state entry on writer refusal.
- Existing `purge_refuses_busy_scan_without_removing_state`: independent scan-lock refusal and retained data.
- Updated `purge_waits_for_stop`: asserts enabled/active restoration after the post-stop guard refuses.
- Updated `idle_purge_removes_data_locks_and_directory`: begins active and checks successful removal leaves the service stopped/disabled with only disable_now called.
- Updated `scenarios::deployment_scenario_table`: successful purge expects three service queries (saved enablement/activity plus stop verification), previously one. No existing test pinned the old publication-before/after-restart order.
- Existing replacement, unsafe-directory, and finalization tests continue covering purge races and partial deletion.

Before implementation, adding the two new tests to the original code produced **45 passed, 2 failed** for `cargo test --offline -p tidemark-cli deploy::tests::`; both failures were the new tests (transcript `library/regression-before.log`). Original CLI library inventory was 114 tests, now 116. After changes, the broader `deploy::` filter passed **51 tests**, including four additional deploy/render/real-env tests (`library/deploy-tests.log`); the same `deploy::tests::` inventory is 47 tests after versus 45 before. Child subprocess test results are not additional inventory tests.

## Assumptions and proposed plan text

The explicit task decision supersedes the old order in 08 §5 and W4-B; no plan file was edited. Proposed wording: “Install config, enable the unit, publish the marker and staged record, then restart only with --start-now; compensate any later failure. If purge refuses after stopping and before deleting anything, restore and verify prior enabled/active states and report compensation failures.” Partial purges intentionally leave the service stopped/disabled. Existing lease waits remain ten seconds. Guard refusal keeps exit 5; I/O failures retain their runtime error category. Firstboot artifacts written by a daemon before a restart failure remain outside the existing rollback's byte restoration contract.

All Cargo commands use the host's cached Rust 1.98.0 toolchain, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, offline mode, and `timeout 1500`. No privileged/destructive host commands were run; privileged tests are compile-only, pending VM.

## Guest verification — pending VM

Run as root on the disposable guest with the packaged candidate and valid config:

```sh
tidemark deploy unstage --purge-state --yes
tidemark deploy stage --from /path/to/tidemark.toml --start-now
tidemark deploy verify --wait 60s
```

Require exit 0 and healthy with matching staged/firstboot hashes, without an intervening restart. Repeat the purge/restage cycle to expose the original fresh-state race.

For refused purge, capture `systemctl is-enabled tidemarkd` and `systemctl is-active tidemarkd`, run a direct `tidemark scan` holding the configured state directory in another terminal, then run `tidemark deploy unstage --purge-state --yes`. Require exit 5, unchanged state data/records, and the restoration message. Recheck both service queries against the captured values. Exercise enabled/running, disabled/running, enabled/stopped and disabled/stopped; ensure the lease holder remains alive throughout the refusal. Finally release the holder and purge successfully; require stopped/disabled and absent state directory. These commands are acceptance instructions, not workstation execution evidence.

## Validation results

- `timeout 1500 cargo fmt --check`: passed (`library/fmt.log`).
- `timeout 1500 cargo clippy --offline --workspace --all-targets -- -D warnings`: passed (`library/clippy.log`); CI also rechecks this.
- `timeout 1500 cargo test --offline --workspace --no-fail-fast`: exit 0 (`library/workspace-tests.log`). Raw result-line totals, including child subprocess results: 948 passed, 0 failed, 14 ignored. CLI library: 115 passed, 1 scale test ignored (116 total). No workspace baseline run was performed; the focused before/after evidence and baseline inventory are above.
- `timeout 1500 cargo build --offline --workspace --features privileged --tests`: passed (`library/privileged-build.log`); no privileged test executed.
- `timeout 1500 tools/ci.sh`: exit 0, “ci: all available checks passed” (`library/ci.log`), including source checks, format/clippy, strict-ancestry store tests, workspace tests and privileged compilation. cargo-deny was not installed and was explicitly skipped; dependency deny remains a coordinator check.
- `git diff --check`: passed. Scope review: four deploy source/test files and this note; no shared-file edits.

All tests here are implementation evidence, not guest/release acceptance. No sandbox socket failures occurred in this workspace run; any existing environment-dependent early-return tests still do not establish real listener/guest behavior.
