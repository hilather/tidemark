# Handoff — TEST-vm-harness: privileged tests and VM runner

## Needs from coordinator

Privileged workspace Clippy reports an existing `clippy::unwrap_used` error at
`crates/tidemark-fs/tests/w1a/main.rs:117`. This file is outside the assigned scope;
no change was made there. The unprivileged workspace run also reports five
readiness deadline failures in `crates/tidemarkd/tests/w6a1.rs`: binary readiness,
private/missing state, lifecycle reload, corrupt catalog recovery, and last
complete catalog startup. These files are unchanged. A separate Python probe records EPERM on Unix socket creation
(`library/socket-probe.log`); that is a possible cause, not established by these deadline
messages. Rerun those tests in the coordinator environment and the guest
scenarios below on Rocky 8.8 and 8.10.
The watcher CPU budget remains 2%; the supplied guest evidence suggests it may
still fail, now after collecting ten-minute stability evidence.

- **Branch:** precreated attempt branch hp-b2bd16c5899ee21241b7064597dd7763e06ecda5a376fd163b2a1f15b7508f2e
- **Agent/session:** Codex; attempt-b2bd16c5899ee21241b7064597dd7763e06ecda5a376fd163b2a1f15b7508f2e
- **Date:** 2026-10-12
- **Status:** implemented; evidence candidate, pending independent review and VM execution

## What changed

1. `rules.rs` shares a helper that copies the CLI with mode 0755. The namespace
   test runs the copy from its own mode-0755 temporary directory. The metadata
   test retains its root-to-nobody permission handling.
2. The daemon test creates a PID-named binary exclusively under `/usr/local/bin`,
   sets mode 0755, and invokes restorecon (only missing executable is tolerated).
   The binary guard precedes the unit guard, so stopping happens before removal,
   including on panic. Startup failures and socket readiness timeouts include
   systemctl status and the last 30 journal lines. Both polling loops sleep 100 ms.
3. The scale writer has a kill-and-wait guard declared after the watcher and
   filesystem. Watcher Drop already stops and joins the thread, including on
   panic. CPU, event progress, rate, descriptor count, and Failed mode checks
   are gathered for one final parent assertion after ten minutes. First-minute
   writer rate failure is deferred until its writes finish, retaining 1960/s.
   No budget or threshold changed; the existing scale ignore remains.
4. Deployment phase and RPM tests print skip lines when their harness variables
   are absent. Deployment skips before locking. All service mutex acquisitions
   recover poison. Explicit phase/RPM requests retain the hard VM marker check;
   missing config and unknown phase still fail.
5. `--no-fail-fast` is cargo-only, duplicate-rejected, and inserted only into
   Cargo's test arguments before `--`. Two unit tests cover positions with and
   without filters, nocapture/environment options, scripts, and duplicates.
   README documents whole runs, explicit marker, phase skips, reboot fixture
   recreation, PATH-selected SSH wrappers, and SELinux executable placement.

## Shared-file edits and dependencies

None. Only the eight assigned paths changed; no dependencies, product sources,
contracts, manifests, or plan files changed.

## Assumptions

- The supplied task is the authoritative maintenance brief; existing wave
  documents provide background rather than expanding ownership.
- `/usr/local/bin` exists in each disposable guest. Exclusive creation fails
  safely on a stale PID-named file instead of overwriting it.
- Missing means an environment variable is absent, not empty. Empty/non-UTF-8
  phases still fail; an explicitly supplied RPM retains script validation.
- Guard cleanup ignores errors during unwind, consistent with the existing unit
  guard. Kill-and-wait and watcher join happen before loop-XFS teardown.
- The host cargo registry cache was copied into `/tmp/tidemark-vm-cargo`; all
  validation uses Rust 1.98.0 on PATH, that CARGO_HOME, and offline mode.

## Tests and acceptance evidence

Validation transcripts are in the attempt output's `library/` directory.
All long commands used `timeout 1500`. Final results:

| Check | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | exit 101; out-of-scope w1a warning above |
| Same privileged Clippy for `-p tidemark-cli -p tidemarkd -p tidemark-live -p xtask` | exit 0 |
| `cargo test --offline --workspace` | exit 101; five w6a1 readiness deadlines above |
| `cargo test --offline -p xtask` | exit 0; 10 unit + 4 integration tests passed, including both added runner tests |
| `cargo build --offline --workspace --features privileged --tests` | exit 0 |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | exit 1; source checks/fmt/Clippy passed, then same five w6a1 failures |

The initial default Cargo home lacked base64; the copied host cache resolved
that offline dependency issue. No privileged binary was executed.

Source test counts before → after: rules 2 → 2; daemon privileged 1 → 1;
watcher privileged 7 → 7 (one existing scale ignore); deployment privileged
3 → 3; vm_test unit tests 5 → 7; w6c 5 → 5 (including feature-gated install).
Total across these files: 23 → 25. No test deleted or newly ignored.

## Guest verification — pending VM

Run these commands **inside disposable guests only**, from the checkout, with
Rust on PATH. Build first as the ordinary user. Repeat on both target versions.

1. Root namespace execution (also rerun without sudo for ordinary-user mapping):
   `sudo env "PATH=$PATH" cargo test -p tidemark-cli --test rules namespace_mounts_are_skipped -- --nocapture`
2. SELinux/systemd start and cleanup:
   `sudo env "PATH=$PATH" cargo test -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture`
   Check no tmtest-w6a3-tidemarkd PID file remains; simulate readiness failure
   in a separate disposable run and preserve status/journal diagnostics.
3. Full scale run (expected budget failures must report after ten minutes):
   `sudo env "PATH=$PATH" cargo test --release -p tidemark-live --features privileged --test w5a privileged::watcher_sixty_second_load_and_ten_minute_fd_stability -- --exact --ignored --nocapture --test-threads=1`
   Preserve CPU/rate/descriptors/mode and final combined failures. Check no
   paced_writer_child remains and loop-XFS teardown succeeds after failure.
4. Plain marker-enabled suite, with phase/config/RPM variables unset:
   `sudo env -u TIDEMARK_DEPLOY_VM_PHASE -u TIDEMARK_DEPLOY_VM_CONFIG -u TIDEMARK_INSTALL_RPM "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 cargo test --workspace --features privileged --no-fail-fast -- --nocapture --test-threads=1`
   Confirm both skip lines. Separately supply phase=stage|verify with the harness
   config around the guest reboot, and RPM with the built package. Explicit
   phase/RPM without marker, unknown phase, and missing config must fail.
5. Complete runner run (from coordinator workstation; recreate guest md first):
   `cargo xtask vm-test --host <vm> --no-fail-fast --env TIDEMARK_DEPLOY_TEST_VM=1 --env TIDEMARK_W1A_MD_DEVICE=/dev/md127`
   Confirm later test binaries run after an earlier failure and final exit code
   remains nonzero. Script suites must reject the flag before SSH execution.

## Remaining uncertainty

No privileged test was executed here. Namespace creation may be restricted by
this workstation sandbox; successful skip behavior does not establish mount
boundary behavior. SELinux labels, systemd execution, panic cleanup, reboot
phases, and ten-minute measurements all require guest validation. No release,
integration, or verified-success claim is made.

## Completion checklist

- [x] Only owned paths changed; no shared edits or dependencies.
- [x] Formatting and unprivileged Clippy passed; privileged tests compile.
- [x] Added runner tests passed; privileged scenarios documented as pending VM.
- [x] Handoff and validation transcripts written.
- [ ] Full workspace tests/CI and privileged workspace Clippy green: blocked by
  the unchanged files identified under Needs from coordinator.
- [ ] Independent verification and both guest reruns remain pending.
