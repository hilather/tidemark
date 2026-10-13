# Handoff — GUEST-fix2: guest script repairs

Implemented on the supplied attempt branch, 2026-10-13. Evidence candidate;
privileged acceptance is **pending VM**, and independent review is pending.

## Changes

- `tests/install/install.sh` and `tests/fault/common.bash` export `LC_ALL=C`
  with a comment. Every fault entrypoint sources the common harness.
  `tools/check-rpm.sh` already exports `LC_ALL=C`; it needed no edit.
- Shared cleanup now retries unmount for a 180-second total budget, with
  attempts limited to 30 seconds, a one-second hard-kill grace reserved within
  that budget, and two-second pauses. It also recognizes an unmount that
  completed between attempts. Exhaustion reports `findmnt`, `fuser -vm` (or
  `lsof` when available), and the owned daemon PID/process state. All fault
  suite unmounts use this helper.
- Script 03 retains its three normal 100,000-write bursts and its low-fd-limit
  burst, asserting complete event accounting, zero new overflows and no lossy
  minutes. A separate forced pass stops the owned daemon, confirms its stopped
  state, writes twice `/proc/sys/fs/fanotify/max_queued_events`, and resumes it.
  The write cap is 200,000; a queue above 100,000 fails with an explanation
  rather than changing kernel settings or pretending to exercise overflow.
- Within one shared 60-second deadline after resume, the forced pass requires
  a live daemon answering status, an increased overflow counter, fewer events
  than writes, new lossy history, and lossy current-minute hot rows (or CLI
  unavailable exit 4). Empty successful hot output is rejected. CLI errors and
  timeout exit codes other than 4 cannot substitute for loss evidence.
  The forced-pass EXIT trap sends SIGCONT and preserves the original exit
  status before shared cleanup, which also resumes tracked groups.
- After another 120 quiet seconds, the script checks the mount's watcher
  component is `ok` and the overflow counter has not increased. README and
  shared Python assertions now describe/check the two distinct scenarios.

No product code, contracts, shared manifests, dependencies, or xtask tests
changed. `docs/handoff/W7-C2-fault.md` was not edited.

## Local validation

Used the installed 1.98.0 toolchain with:

```sh
export PATH=/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH
export CARGO_HOME=/home/brewerm/.cargo
export RUSTUP_HOME=/home/brewerm/.rustup
cargo fmt --check
cargo clippy -p xtask --all-targets --offline -- -D warnings
cargo test -p xtask --offline
```

All passed: 43 tests (20 unit, 10 W6-C, 6 W7-B, 4 W7-C, 3 W7-D).
`bash -n` passed for install, common harness, all six fault entrypoints and
check-rpm. Python compilation and `git diff --check` passed. A final W7-C
repeat after the cleanup deadline refinement passed. Synthetic helper checks
outside the repository exercised no-loss acceptance, overflow/missing-event
acceptance, rejection of complete/empty hot output, unavailable versus timeout,
watcher ok/degraded, third-attempt unmount success and permanent unmount failure
within the budget. No real unmount or fault/install entrypoint ran locally.

Initial attempts lacked Cargo on PATH, then lacked the offline cache, and the
first Clippy invocation lacked RUSTUP_HOME. Explicit installed paths resolved
those environment errors; no online fetch was used. ShellCheck is absent here;
actual `--severity=warning -x` validation is **pending GitHub** (the xtask
ShellCheck stub/gate regressions passed). Evidence is in the attempt output's
`library/validation.log` and `library/helper-checks.py`.

## Exact guest verification commands

Run only in a disposable root-capable Rocky 8.8/8.10 guest checkout. Build as
its normal SSH user first:

```sh
cargo build --offline --workspace --features privileged
```

Locale/RPM/install: supply the existing 0.1.0 RPM, changed-default 0.1.1 RPM and
persistent disposable loop-XFS mount as absolute paths in these variables.
The commands deliberately supply the previously failing locale:

```sh
: "${TIDEMARK_INSTALL_RPM:?absolute 0.1.0 RPM}"
: "${TIDEMARK_INSTALL_UPGRADE_RPM:?absolute changed-default 0.1.1 RPM}"
: "${TIDEMARK_INSTALL_MOUNT:?persistent disposable loop-XFS mount}"
env LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 sh tools/check-rpm.sh "$TIDEMARK_INSTALL_RPM"
sudo env "PATH=$PATH" LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_INSTALL_RPM="$TIDEMARK_INSTALL_RPM" TIDEMARK_INSTALL_UPGRADE_RPM="$TIDEMARK_INSTALL_UPGRADE_RPM" TIDEMARK_INSTALL_MOUNT="$TIDEMARK_INSTALL_MOUNT" bash tests/install/install.sh
```

Follow the existing install suite's deploy/reboot instructions for its second
phase; restore the disposable mount before daemon startup, then:

```sh
sudo env "PATH=$PATH" LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_INSTALL_PHASE=verify TIDEMARK_INSTALL_MOUNT="$TIDEMARK_INSTALL_MOUNT" bash tests/install/install.sh
```

Forced overflow, retained no-loss coverage and shared unmount cleanup:

```sh
sudo env "PATH=$PATH" LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 bash tests/fault/03-fanotify-overflow.sh
```

Repeat that command six times to compare slow-unmount behavior with the supplied
six-run evidence. Inspect PASS/FAIL lines and the retained directory's
`forced-before.json`, `forced-now.json`, `forced-hot.json`, `forced-hot.rc`,
`forced-status.json`, `forced-check.log`, `forced-recovery.json`,
`forced-loss-counters.json` and `forced-after.json`. Require the full RESULT
line, including cleanup. Failure must resume/terminate the private daemon and
retain evidence; an exhausted unmount must print holder diagnostics. Natural
slow unmount and diagnostic exhaustion still require guest evidence; local
mock tests prove control flow only. Run the other fault entrypoints through
the existing VM suite to exercise their shared cleanup as well.

## Assumptions and remaining uncertainty

- Guest fanotify queue limit is positive and <=100,000. Larger queues cannot
  satisfy both twice-the-limit and 200,000-write constraints, so fail explicitly.
- The public `now` API exposes only a 15-minute lossy count. The forced pass
  starts with zero lossy history; successful `hot --window 1m` rows independently
  demonstrate current-minute loss. Documented unavailable exit 4 is also allowed
  while the separate status request must succeed.
- **Likely product failure exposed, not repaired:**
  `crates/tidemarkd/src/services/watcher.rs:144` publishes `degraded` while
  `stats.lossy_minutes_15m > 0`. Therefore the requested `ok` after two quiet
  minutes likely fails until the product owner resolves that policy. The
  assertion remains strict; the harness does not restart the daemon, extend
  the wait to 15 minutes or mask the failure. Product code is outside scope.
- A one-minute hot request near rollover may need the polling interval to
  observe the lossy current bucket promptly; the 60-second cap remains strict.
- No privileged test compilation was added or needed: these deliverables are
  guest shell/Python scripts. Real kernel queue overflow, disk timing and Rocky
  behavior remain pending VM. No privileged commands ran on this host.
