# Handoff — FAULT-fix: repair guest fault suite

- Branch: precreated `hp-49fef1ebc265cec940c84bbf104dbe536e172a470c86b62dcc3a4aafaa238d04`.
- Attempt: `attempt-49fef1ebc265cec940c84bbf104dbe536e172a470c86b62dcc3a4aafaa238d04`.
- Date: 2026-10-13. Implemented evidence candidate; privileged validation pending VM.

## Changes by requested item

1. State-space refusal accepts only exit 1 or 5 and requires `NoSpace` in stderr. Its PASS includes exit/stderr; alert check prints JSON, and the admitted-write check prints its exit. Fill helpers print free and allocated bytes.
2. Metrics now use `$work/metrics/metrics.prom`, with a separate parent explicitly created mode 0755. The API socket parent stays private mode 0700. Both overflow check calls use the new path.
3. Every snapshot iteration saves a sorted pre-scan temporary set and observes only new names, including at the delayed kill. Completed scans print the requested SKIP and their publications/exit are checked. At least three delays must hit the writer. Startup cleanup remains asserted; recorded failures do not terminate the iteration loop. Per-delay scan/temporary evidence is retained.
4. Compaction exits zero with the exact requested SKIP before VM checks/resource setup unless `TIDEMARK_FAULT_SLOW=1`. Its real scenario remains intact. README identifies the separate future Rust child-process compaction test as fast coverage. The requested approximate 70-minute message is preserved; the existing multi-phase slow scenario can actually take four hours.
5. Long helpers use `setsid timeout --foreground` in the background, with tracked PID/group and interruptible `wait`. Explicit stdin duplication preserves heredoc input. Daemon, scans and mutator also lead their own process groups. Signal cleanup continues/stops groups, escalates to KILL, checks survivors, then unmounts/detaches. Mount cleanup is armed before launching mount and can rediscover its loop source if interrupted before recording it. Cleanup's bounded foreground timeouts explicitly use `--foreground`.
6. Checks print the measured JSON/counts/metric lines/integrity/exit/RSS/timing evidence and save separate check logs. Failures are counted and determine final exit; observation failures continue when the remaining fixture is usable. Setup failures, unusable fixtures and hard child deadlines still stop that script safely. `TIDEMARK_FAULT_KEEP=1` retains PASS evidence; FAIL always retains it and prints the path.
7. Both script suites run all scripts under the unchanged quoted SSH/sudo/environment transport. They stream output, then print each PASS/FAIL, actual exit status and elapsed seconds. Any failure yields suite exit 1; cargo-suite exit propagation is unchanged. A unit test executes the generated command with fake sudo and two harmless temporary scripts, proving both aggregate failure and all-success behavior.
8. `w7c` asserts every timeout command explicitly contains `--foreground`, checks the slow guard precedes setup, and checks the separate metrics parent/path. Shell syntax and quoting reviewed at default Shellcheck severity; Shellcheck itself is absent locally, so equipped CI remains required.

## Validation

All Cargo commands use the pinned 1.98.0 toolchain, `CARGO_HOME=/tmp/tidemark-fault-cargo` (copy of host cache), `RUSTUP_HOME=/home/brewerm/.rustup`, and offline mode. Initial invocations using the default rustup home failed because it is read-only; subsequent checks use these explicit paths. No dependencies or manifests changed.

- `bash -n` on all six entrypoints and `common.bash`: passed.
- Python AST parse of both helpers: passed.
- Temporary-directory regression of `tmp-set`/`new-tmp`: stale file rejected (exit 1), only the new file returned: passed.
- `cargo fmt --check`: passed.
- `cargo clippy -p xtask --all-targets -- -D warnings` with `CARGO_NET_OFFLINE=true`: passed.
- `cargo test -p xtask --offline`: passed, 34 tests (18 unit + 9 w6c + 6 w7b + 1 w7c).
- Final focused `cargo test -p xtask --offline --test w7c`: passed.
- `tools/ci.sh`: initial exit 1 at existing `tidemarkd::w6b::atomic_publication_and_modes`, `WouldBlock` (630 harness passes, 1 failure, 14 ignored). A focused offline rerun of that test passed. Second full CI exited 0: source policy, format, workspace Clippy, unprivileged workspace tests (669 harness passes, 0 failures, 14 ignored) and privileged-feature test compilation passed. Cargo-deny was unavailable and explicitly skipped. The anticipated five sandbox daemon readiness failures did not occur in this run. Both CI logs and the focused rerun are retained. Final xtask fmt/Clippy/tests also cover the subsequent temporary-fixture-only test hardening.
- Shellcheck: unavailable; not claimed run. `w7c` runs it when installed.
- All privileged fault scripts: **pending VM**. None was executed here. No host sudo, filesystem provisioning, service operation or product edit occurred.

## Exact guest commands (pending execution)

Repeat the suite on both disposable guests; replace `tester@rocky88` with `tester@rocky810` for the second run. Build in the guest checkout, not on this workstation:

```sh
rsync -a --exclude target --exclude .git ./ tester@rocky88:tidemark-vm-test/
ssh tester@rocky88 'cd tidemark-vm-test && cargo build --offline --workspace --features privileged'
cargo xtask vm-test --host tester@rocky88 --remote-dir tidemark-vm-test --suite fault --env TIDEMARK_DEPLOY_TEST_VM=1 --env TIDEMARK_FAULT_KEEP=1
cargo xtask vm-test --host tester@rocky88 --remote-dir tidemark-vm-test --suite fault --env TIDEMARK_DEPLOY_TEST_VM=1 --env TIDEMARK_FAULT_KEEP=1 --env TIDEMARK_FAULT_SCALE=1
```

For the opt-in slow row and signal-cleanup check, **inside that disposable guest checkout only**:

```sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 TIDEMARK_FAULT_SLOW=1 bash tests/fault/04-compaction-crash.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 TIDEMARK_FAULT_SLOW=1 timeout --foreground --signal=TERM --kill-after=60s 900s bash tests/fault/04-compaction-crash.sh
findmnt -rn -o SOURCE,TARGET | rg tidemark-fault
losetup --list --noheadings --output NAME,BACK-FILE | rg tidemark-fault
pgrep -af '[t]ests/fault/compaction.py'
```

The final three inspection commands should have no resources from the terminated attempt. Review their output and the retained evidence; do not infer guest acceptance from local static checks.

## Scope, assumptions and remaining uncertainty

Only assigned paths changed. No shared manifest/module edit, dependency, unsafe block, contract change or SCR. Retained root-only access and no PostgreSQL work. No branch creation, merge or push. No canonical memory update was pending in the initial receipts; projections were not edited.

Snapshot orphan cleanup is an existing product fault owned elsewhere; this task preserves its failing assertion. Guest scheduling still determines writer hits and directory races. External temporary/WAL observation cannot prove an instruction-level interruption; a commit can race SIGKILL. The signal/resource behavior and exporter lock resolution require guest confirmation. The separate Rust fast-compaction test is another task, not claimed delivered here.
