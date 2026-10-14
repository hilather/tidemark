# Handoff — DOCS-fault: mount pinning, strict ancestry and second-compaction witness

- **Branch:** existing canonical attempt branch `hp-e2e3b3f325102547ad629d179eab3417521e2b07042e910a13b17b1908cc4481`
- **Agent/session:** attempt-e2e3b3f325102547ad629d179eab3417521e2b07042e910a13b17b1908cc4481
- **Date:** 2026-10-13
- **Status:** implemented; independent verification and VM execution pending

## What was built

Operator watcher guidance and its troubleshooting index now explain the retained mount descriptor, ordinary unmount's `target is busy`, daemon/foreground exit, removing a configured watch and reloading, and the delayed release after lazy detach. The watcher runbook includes a planned-unmount section, and security documents why the descriptor is retained. There is no separate runbook index in this checkout; the operator guide's troubleshooting table links the new section.

State-layout guidance in the operator guide, security document and R5-24 trust text now requires packaged root/invoking-user ancestry without namespace/read-only exceptions. It explains refused symbolic links (`Not a directory`), real paths, sticky-child trust, and announced development exceptions with the strict runtime switch, following TRUST-strict and the existing strict-ancestry packaging feature.

The second compaction helper checks the first oracle, then transactionally seeds 600 due rows under a disjoint mount key. Success requires exactly two last-value five-minute rows (no raw or extra witness rows), an unchanged byte serialization of all prior row columns, and the original daemon PID/start time still alive. Zombie and replaced processes are refused. Both row sets are read in one SQLite snapshot, and the progress deadline is 3700 seconds within the existing 3800-second outer helper limit. The shell captures daemon start time before recovery and passes both identity values. README documents the evidence contract.

## Shared-file edits

None. No dependencies, manifests, module declarations or product code changed. Only the task-owned R5 trust text changed in the plan document.

## Deviations and assumptions

- The canonical task is the assigned section for this follow-up; its explicitly named reviews and TRUST-strict handoff supply the required reading. No branch was created, merged or pushed.
- Byte-identical historical rows means stable serialization of every selected stored row column, not SQLite database pages (which legitimately change during compaction). The first manifest independently defines all 1000 historical rows.
- The helper assumes the private guest daemon is the only compactor. Regression controls mutate only private temporary SQLite fixtures to exercise the oracle; they do not implement or certify product compaction.
- The requested focused xtask checks replace full workspace/CI checks for this documentation/helper-only task. Cargo was initially absent from PATH; validation uses `/home/brewerm/.cargo/bin`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, and `CARGO_NET_OFFLINE=true`. The first format check found one Rust assertion needing formatting; the final format check passed.

## Acceptance criteria evidence

| Criterion | Evidence | Status |
| --- | --- | --- |
| F4 mount pinning and unmount guidance | Watcher-mode section, troubleshooting table, runbook planned-unmount section, security retained-descriptor explanation | Written; runtime behavior supplied by task, not remeasured here |
| R4-N1 packaged ancestry, links and development distinction | Operator state-layout paragraph, security trust paragraph, R5-24 trust rules; inspected packaged `strict-ancestry` feature wiring | Written; packaged runtime acceptance pending VM |
| F6 discriminating second compaction | `second_compaction_requires_new_exact_rows_and_live_daemon` executes the real helper against unchanged, exact, wrong-value, extra-raw, changed-history, replaced-identity and dead-PID fixtures | Passed |
| Rocky guest tools and shell quality | `fault_scripts_use_available_guest_tools`, `guest_tool_guard_covers_shell_and_python_commands`, `bash -n`; Python standard library only | Passed; real ShellCheck unavailable |

## Validation

- `cargo fmt --check`: final exit 0.
- `cargo clippy -p xtask --all-targets -- -D warnings`: exit 0, offline environment.
- `cargo test -p xtask --offline`: final exit 0, 50 passed, 0 failed across five test binaries; w7c has six passing tests including the new helper regression.
- `bash -n tests/fault/04-compaction-crash.sh`: exit 0.
- `PYTHONPYCACHEPREFIX=/tmp/tidemark-docs-fault-pycache python3 -m py_compile tests/fault/compaction.py`: exit 0; no repository cache artifacts.
- `git diff --check`: exit 0.
- Real `shellcheck --severity=warning -x tests/fault/04-compaction-crash.sh`: pending tool availability. The existing xtask warning gate and its stub regression passed; this is not real lint evidence.

Final Cargo logs are retained in this attempt's worker-output `library/`. No fault script, privileged command, service action, mount operation or product runtime test was executed.

## Known gaps and follow-ups

Run the slow fault gate on disposable Rocky 8.8/8.10 guests and retain the actual witness/PID/row evidence. For one kill cycle, use the existing documented `TIDEMARK_FAULT_SLOW=1 TIDEMARK_FAULT_CYCLES=1` guest invocation with an adequate outer timeout (about three hourly phases). Guest execution remains **pending VM**; the helper regression does not establish actual hourly daemon compaction or mount teardown timing. Real warning-severity ShellCheck remains pending an equipped runner. Independent verification/integration is outside this worker report.

## Definition of Done evidence

- [x] Only owned paths changed; no shared-file edits.
- [x] Final requested fmt, focused Clippy and focused offline tests passed.
- [x] Guest fault scenario updated and documented as pending VM.
- [x] No product contracts/dependencies/unsafe code introduced.
- [x] Acceptance evidence and remaining limits recorded.
- [x] Handoff written; no new unexplained TODOs.
- [ ] VM runtime and real ShellCheck acceptance pending.
