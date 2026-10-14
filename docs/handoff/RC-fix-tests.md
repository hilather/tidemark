# Handoff — RC-fix-tests: merged-tree and Rocky guest test repairs

- Branch: precreated hp-9bd255323ae7bbddc3406dcdcf91afa6be7846cb7bd69ddeb8e3d71555246c3e
- Attempt: attempt-9bd255323ae7bbddc3406dcdcf91afa6be7846cb7bd69ddeb8e3d71555246c3e
- Date: 2026-10-13
- Status: implemented; evidence submitted for independent review, guest runtime pending.

## What changed

The retired-watcher regression counts watcher components separately, requires the current mount name, checks the overall error bound and exactly one current watcher error, and allows only the sampler's documented degraded waiting component/error beside it. The requested first commit was made immediately as “RC-fix-tests: count watcher components only”. Its first test run revealed that degraded sampler health also enters last_errors; a separate follow-up commit corrected that additional assertion.

The second-uid ancestry fixture creates the foreign-owned directory while its temporary parent permits creation, then changes that parent to 0755 before invoking the CLI. A second invocation restores 1777 and expects the writable-parent refusal. Both require exit 1, no state directory creation and unchanged sentinel contents; the final sentinel inode/mode/owner/length also match. The root namespace case skips with one notice because mapping root maps the ancestor owners. The require-clean test skips with one notice if Git cannot establish that the source checkout is a work tree; its isolated tracked-change fixture otherwise still executes.

The watcher helper uses stdout/stderr pipes and universal_newlines, supported by Python 3.6. The compaction script prints estimated UTC completion and its helper deadline budget before setup. The obsolete pinned 70-minute text was removed and the corresponding xtask assertion updated.

## Python audit

Reviewed both Python files under tests/ and inline Python in test/fault/install scripts, Rust tests and tools. Found only the capture_output=True and text=True call in xtask/tests/w7c.rs; changed both. No dataclasses, walrus, debug f-string equals, or future annotations were found. Existing ordinary f-strings are supported by Python 3.6. All 21 discovered Python blocks parse with the local interpreter's Python 3.6 grammar; standalone files passed py_compile with bytecode written to /tmp, and inline blocks compiled without execution. This is static compatibility evidence, not a Python 3.6.8 runtime result.

## Compaction timing

One kill cycle retains three hourly phases: kill, recovery and idempotence. The kill helper sleeps 3590 seconds then polls for 120 seconds; each verification helper allows 3700 seconds. One cycle therefore allows 11110 seconds (3 h 5 min 10 s) in helper waits, enclosed by 3 × 3800-second shell deadlines (3 h 10 min). Default two cycles allow 14820 seconds (4 h 7 min), enclosed by 4 × 3800 seconds (4 h 13 min 20 s). Typical estimates remain about 183/244 minutes. Setup, seeds, readiness, integrity checks and cleanup add time. Startup prints an estimate, not a promised wall-clock upper bound.

## Shared files, dependencies and contracts

None. Only explicitly owned paths changed; no product code, dependencies or contracts changed. Used the existing attempt branch, without branching, merging or pushing. No host mounts, LVM, services or privileged fault commands were run.

## Validation

All Cargo validation uses the pinned Rust 1.98.0 binaries on PATH, offline cached dependencies via CARGO_HOME=/tmp/rc-cargo and four build jobs. The writable cache was copied from the host cache, following W0-A's offline fallback. Cargo unpacked some cache entries while the copy was still running, causing harmless copy destination-exists messages; final builds and checks completed against the resulting cache.

| Command | Observed result |
| --- | --- |
| cargo fmt --check | exit 0 |
| cargo clippy --offline --workspace --all-targets -- -D warnings | exit 0, final source |
| cargo test --offline -p tidemarkd --test w6a2s | exit 0; 8 passed |
| cargo test --offline -p tidemark-cli --no-fail-fast | exit 0; full crate suite |
| cargo test --offline -p xtask | exit 0; full crate suite |
| cargo test --offline -p tidemark-cli --features privileged --test e2e --no-run | exit 0; compilation only |

Shell entrypoints and common.bash passed bash -n. Python checks are described above. git diff --check passed.

## Remaining uncertainty

The root namespace skip runs locally; it does not establish the ordinary-user namespace refusal. The second-uid and sticky-parent cases are feature-gated privileged tests and were compiled only, pending Rocky guest execution. Existing socket/namespace guards can return successfully in this sandbox, so passing suites do not establish those capabilities. No fault script was executed, no Python 3.6 interpreter or guest was available, and cargo xtask ci/deny was not run. Coordinator CI at umask 022 and Rocky 8.10 root execution remain separate review gates. No canonical memory updates were pending at the checked receipts.
