# Handoff — GUEST-fix3: Third guest-pass test repairs

- Branch: precreated hp-d3efaabf7cc2afa6a858132a0a6c5a8d2d31b5451f63cfe799c348cf9f8ad9d5
- Agent/session: Codex, attempt-d3efaabf7cc2afa6a858132a0a6c5a8d2d31b5451f63cfe799c348cf9f8ad9d5
- Date: 2026-10-13
- Status: implementation and evidence candidate; independent verification pending.

## Changes

The three affected CLI tests delegate their complete test bodies to an exact-filtered child running as uid/gid 65534 when a root runner can bypass mode-000 permissions. That child creates, scans, reads and updates its own private state; the parent never opens that state. This preserves all catalog, snapshot-error, mode, growth and metadata-only assertions, including the report test's catalog timestamp edit. The child must report one passing test, preventing an accidentally empty filter from certifying the body. A private executable copy and CLI copy let the child run even when checkout ancestors are root-private. The harness and rules helpers accept that copied CLI path. Ordinary users and uid-0 runners without DAC bypass execute the original bodies directly, using the same mode-000 probe as the existing harness. State ownership is never changed back; trust enforcement and product code are unchanged.

Two human hot-table path greps in tests/install/install.sh were replaced: reboot verify and the host namespace external-writer check. Both use `tidemark hot --window 5m --json` and the shared Python matcher validates `tidemark.hot.v1` and matches the `path` field exactly. Reviewed every grep in tests/install/install.sh and tests/fault/*.sh: no other Tidemark human-table path/number grep remains. Retained config markers, systemctl show key/value assertions, service-parser diagnostics, audit `<no matches>` text and the NoSpace error-name check; these are not human Tidemark numeric/path tables.

Script 03 reads the queue sysctl when it exists, otherwise uses the EL8 fixed default 16384, and prints the source and value. Existing numeric/cap checks remain. All clean reused-file bursts, low-fd checks, isolated distinct-file cap assertions, forced queue-overflow assertions, 120-second transport recovery and optional 17-minute retained-history recovery from FAULT-fix3 are unchanged.

Script 04 accepts TIDEMARK_FAULT_CYCLES=1 or 2 (default 2). One selects the first 0.02-second kill delay; default retains both 0.02/0.2 delays. Both retain actual recovery and idempotence compactions. The message and README now explain two kill cycles plus two verification phases at about 61 minutes each: about 244 minutes by default; one kill cycle plus verification takes about 183 minutes. Each hourly phase retains its 3800-second deadline, plus setup/cleanup. The obsolete 70-minute message survives only in an explicitly historical comment because xtask/tests/w7c.rs lines 51–54 pins that literal; its executable message is corrected and the test was not edited.

## Shared files, contracts and dependencies

Only assigned paths changed. No shared manifests/module declarations, product code, dependencies or frozen contracts changed. The new module declaration is in the owned test entrypoint. No branch creation, merge, push, privileged host operation or host configuration change.

## Validation

Used pinned Rust 1.98.0 on PATH, CARGO_HOME=/tmp/tidemark-guest-fix3-cargo copied from the readable host registry/git cache, and offline Cargo. Initial unconditional uid-0 delegation failed at chown with EINVAL in this sandbox; matching the existing harness's DAC-bypass probe corrected that assumption. Final checks:

- cargo fmt --check: exit 0.
- cargo clippy --offline -p tidemark-cli --all-targets -- -D warnings: exit 0.
- cargo test --offline -p tidemark-cli --no-fail-fast: exit 0; 207 passed, zero failures, three existing scale ignores.
- cargo test --offline -p xtask: exit 0; 48 passed, zero failures/ignores, including unchanged w6c and w7c.
- bash -n tests/install/install.sh, tests/fault/03-fanotify-overflow.sh and tests/fault/04-compaction-crash.sh: passed after final edits.
- git diff --check: passed.
- Synthetic execution of the actual shared JSON matcher accepted exact long paths and refused shortened paths, prefix matches and empty rows.
- Isolated shell-block checks used temporary paths instead of /proc: absent sysctl selected 16384/32768 writes; present 8192 selected 16384 writes; invalid text, zero and over-cap limits failed. Cycle selection preserved one/two delays and refused 0, 3 and 01.

The runner reports uid 0 but lacks DAC bypass and cannot chown to uid 65534 (EINVAL). Final metadata-only and full report test bodies passed through the original direct path. The unrelated-listener test reports its existing socket-bind sandbox skip; both namespace rules report uid_map permission-denied skips. Thus real-root delegation and socket/namespace acceptance remain pending VM, despite Cargo reporting those tests as passed. ShellCheck is absent; syntax and xtask harness gates do not replace real warning-severity linting. No install/fault entrypoint was executed on this host.

## Guest commands / remaining uncertainty

Run only in disposable Rocky 8.8/8.10 guests, with the appropriate RPMs and restored loop-XFS mount for install. Preserve transcripts for coordinator review:

```sh
cargo build --offline --workspace --features privileged
sudo env "PATH=$PATH" cargo test --offline -p tidemark-cli --test e2e --test rules -- --nocapture
shellcheck --severity=warning tests/install/install.sh
shellcheck --severity=warning -x tests/fault/*.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 bash tests/fault/03-fanotify-overflow.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 TIDEMARK_FAULT_SLOW=1 bash tests/fault/03-fanotify-overflow.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 TIDEMARK_FAULT_SLOW=1 TIDEMARK_FAULT_CYCLES=1 bash tests/fault/04-compaction-crash.sh
```

For install use tests/install/install.sh with its existing TIDEMARK_INSTALL_RPM, TIDEMARK_INSTALL_UPGRADE_RPM and TIDEMARK_INSTALL_MOUNT variables, then externally reboot/restore the mount and run TIDEMARK_INSTALL_PHASE=verify. Real daemon event timing, queue overflow, hourly spill detection, systemd install/reboot and ordinary-user CI at umask 022 remain independent acceptance work. No tests were weakened to hide those limitations.
