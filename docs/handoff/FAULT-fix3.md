# Handoff — FAULT-fix3: Watcher guest checks

- Branch: precreated `hp-d846edd9f428aeb56dbc34d99f62d4f07a312991368a86b0ac451ed5e1b3f4b3`
- Agent/session: Codex, attempt-d846edd9f428aeb56dbc34d99f62d4f07a312991368a86b0ac451ed5e1b3f4b3
- Date: 2026-10-13
- Status: implementation and evidence candidate; independent verification pending.

## What changed

Read the complete W7-review3-correctness review, especially C7, the watcher mode enum/display and daemon JSON mappings, and DAEMON-polish item 1. Read the W7-C section, architecture security model, fault-injection plan, playbook lifecycle and handoff template. This narrow repair changes only fault scripts/docs, the xtask helper regressions and this note. No product code, dependencies, shared module declarations or frozen contracts changed. No branch creation, merge or push.

Readiness recognizes and prints all public names: fd_mount, fd_filesystem, dfid_name_mount, dfid_name_filesystem and failed. Failed is recognized for diagnostics but cannot certify readiness. The four usable modes are accepted at every daemon restart.

The three normal bursts reuse the same 20,000 paths; the low-fd burst does likewise after restart. Each still performs 100,000 close-writes. Keeping paths identical across trials also bounds identities when multiple trials land in one minute. Each pass requires event delta exactly equal to writes, zero overflow delta, zero lossy minutes and healthy component detail without truncation/loss reasons.

A separate fresh-daemon pass performs 100,000 distinct-file close-writes near a minute boundary. The writer asserts its workload finished in that same minute, failing explicitly on a slower guest. It requires complete event counts, zero overflow delta, positive lossy history, degraded status and the public diagnostic `truncated: distinct-file cap reached in the minute` with `(events complete, per-file counts capped)`, without a transport-loss diagnostic. This diagnostic is the public truncation fact: now JSON has no separate truncated-minute field. The status diagnostic is polled alongside the counters to allow publication lag. No queue limits or clocks are changed.

Another restart isolates the forced SIGSTOP queue-overflow pass from cap history. After SIGCONT, the existing forced-loss assertions still require overflow, missing events and current-minute lossy/unavailable hot data. Already-queued event counters must settle before measuring 100 new writes. Within 120 seconds of SIGCONT those writes must add exactly 100 events and no overflow. Status may be ok or degraded with the queue-overflow reason. Health recovery is separate: only TIDEMARK_FAULT_SLOW=1 polls for ok with no retained loss/truncation diagnostic and no further overflow, bounded by 17 minutes after SIGCONT to cover fifteen buckets, alignment and publication. Fast mode prints an explicit long-check SKIP.

## Guest commands and pass assertions

Only in disposable Rocky 8.8 and 8.10 guests, build the synced checkout first:

```sh
cargo build --offline --workspace --features privileged
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 bash tests/fault/03-fanotify-overflow.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 TIDEMARK_FAULT_SLOW=1 bash tests/fault/03-fanotify-overflow.sh
```

Fast: negotiated mode, three normal exact-count/no-loss bursts, one low-NOFILE exact-count/no-loss burst, isolated complete-event aggregation truncation, forced real queue loss, new-write transport recovery within 120 seconds, explicit history-check skip. Slow: all fast assertions plus actual retained-history expiry and ok status with unchanged overflow count. Guest execution is **pending VM**; none of these fault entrypoints were run on this host.

## Validation

Local commands use pinned Rust 1.98.0 on PATH, CARGO_HOME=/tmp/tidemark-fault-fix3-cargo copied from the readable host registry/git cache, and offline Cargo. The first fmt check identified wrapping in the new regression; rustfmt corrected only the owned Rust file before the passing final check.

- Every tests/fault/*.sh and common.bash: bash -n passed.
- check.py: python3 -m py_compile passed with cache redirected under /tmp.
- git diff --check passed.
- cargo fmt --check: passed (exit 0) after correction.
- cargo clippy -p xtask --all-targets -- -D warnings: passed (exit 0), offline environment.
- cargo test -p xtask --offline: passed (exit 0), 48 tests across five binaries, zero failures/ignored; w7c has five passing tests including the new real-helper regression.
- `watcher_helpers_pin_modes_counts_caps_and_recovery_phases` runs the real Python helper against synthetic JSON/Prometheus fixtures. It checks every public mode, failed/unknown refusal, exact counts, short/excess event refusal, overflow refusal, clean loss refusal, cap diagnostic discrimination, degraded transport recovery and healthy history recovery. Script assertions pin the 20,000 identity workload, 120-second deadline and slow-only watch-ok phase.

Real ShellCheck is absent. The existing xtask warning-severity gate and stub regression remain; they test harness handling, not real ShellCheck diagnostics. No privileged compilation change was needed for guest shell scripts; guest build/runtime acceptance remains pending. Full workspace CI is outside the explicit task validation scope.

## Remaining uncertainty

Kernel queue behavior, writer speed sufficient to exceed the distinct cap in one minute, low-fd delivery and fifteen-minute expiry need real Rocky guest transcripts. The isolated cap pass intentionally fails if its writer crosses a minute boundary or loses events; it does not relax assertions to claim coverage. Quiet counters are observed for three seconds before new-write measurement, within the 120-second recovery budget. No privileged host commands or host configuration changes were made.
