# Needs from coordinator

Independent review, uid 1000 / umask 022 with sockets, and GitHub Ubuntu 24.04 execution remain pending. Privileged runtime tests remain pending VM; this worker compiled them only. This handoff is an evidence candidate, not release approval.

# POST-deploynits — atomic purge readiness and empty verify reasons

## What changed

The purge writer child initializes Catalog and TimeSeries, acquires an additional shared WriterLease, then closes both SQLite connections before publishing readiness. Closing finishes SQLite checkpoint/sidecar teardown before the parent snapshots the tree; the extra lease continuously retains live writer ownership across connection closure and through purge refusal. The child writes `ready.tmp` completely and renames it to `ready`. The parent checks its exact contents and verifies the child is alive before the snapshot and after all byte/inode comparisons. No file or sidecar is excluded from those comparisons. Purge production code is unchanged.

For a unique non-ok core component, verify now appends parentheses only when the already-escaped, bounded reason is not empty or whitespace-only. The existing fake-clock reason test now includes empty, ASCII-space-only and Unicode-space-only input in each of the four core positions, checking human and JSON details. Existing control escaping, truncation, health and healthy-detail suppression checks remain.

No shared files, manifests, dependencies, contracts, flags, schemas or public APIs changed. No spec deviation or SCR is needed.

## Regression evidence and before/after output

The expanded `deploy::tests::verify_core_reasons_in_human_and_json_detail` failed against the original formatter: 0 passed, 1 failed, exit 101 (`library/before.log`). Its failure displayed `sampler=degraded (); scheduler=ok; api=ok; tswriter=ok`. After the fix it passes within the 61-test deploy suite, with 32 reason cases (eight inputs across four positions).

Before: `sampler=degraded ()` or `sampler=degraded (   )`.

After: `sampler=degraded` for empty or whitespace-only escaped detail. A nonempty reason remains `sampler=degraded (not mounted: /mnt/x)`.

The prior purge failure is recorded in POST-tuititle's readiness paragraph: an empty readiness-file snapshot later differed from `ready`. This attempt did not attempt to reproduce the old scheduling race. After the fix, the exact purge test passed **200/200 invocations**, zero failures. Each invocation snapshots every existing directory inode and every regular-file inode and contents, attempts purge while the live child owns the shared writer lease, requires the guard refusal, and compares all originals before terminating the child.

The repetitions used `timeout 1500 bash /tmp/deploynits-repeat.sh`: four bounded workers, each looping 50 times over the built CLI test executable with `--exact deploy::tests::purge_refuses_live_writer_without_changing_bytes_or_inodes`. Four lanes avoid exceeding 1500 seconds from 200 sequential ten-second lease waits. Each test uses its own temporary fixture. `library/repeat-{1,2,3,4}.count` each contains 50; each corresponding log contains 50 passing one-test summaries.

## Assumptions

- The explicit POST assignment supersedes generic wave-task and branch-creation instructions. The precreated attempt branch is retained.
- A live child's shared WriterLease is the live writer authority purge must refuse; SQLite connections may close before readiness while that same authority stays continuously held. This removes connection-close writes from the snapshot interval without excluding database files or changing purge behavior.
- Whitespace is tested after existing diagnostic escaping and bounding. Control characters that escape to visible tokens still produce a reason. Existing nonempty whitespace around a reason is preserved.
- Existing synchronization deadlines are retained; no elapsed-time correctness assertion or new clock dependency was introduced. Verify coverage uses FakeVerifyClock.
- Validation uses uid 0, umask 022, pinned Rust 1.98.0 and the host Cargo cache offline. No privileged runtime commands, branches, merges, pushes, or out-of-scope repository writes.
- No new privileged scenario is necessary for these local test/formatting fixes. Existing privileged tests compile; execution remains pending VM.

## Validation

All long commands run under `timeout 1500`. Detailed logs are in this attempt's output `library/`.

- `cargo fmt --check`: passed (`fmt-fixed.log`).
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed (`clippy-fixed.log`).
- `cargo test --offline --workspace --no-fail-fast`: passed; 1,024 passed, 0 failed, 14 ignored across 87 summaries including child harnesses (`workspace.log`).
- `cargo build --offline --workspace --features privileged --tests`: passed; not executed (`privileged-build.log`).
- `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps`: passed (`docs.log`).
- `tools/ci.sh` with `CARGO_NET_OFFLINE=true`: passed; 1,026 passed, 0 failed, 14 ignored across 88 summaries, including child and strict-ancestry harnesses (`ci-fixed.log`). Source policy (150 owner tags), fmt, Clippy, strict-ancestry checks, workspace tests and privileged-feature compilation passed. `cargo-deny` is absent, so dependency-policy validation was skipped.
- Focused deploy suite: 61 passed, 0 failed; its child helper also emits one passing harness summary (`deploy.log`).
- Exact purge stability loop: 200 passed, 0 failed, plus the focused/workspace invocations above.
- Initial fmt, Clippy and CI wrapper invocations could not complete validation because Rustup's default home was read-only. Corrected by setting `RUSTUP_HOME=/home/brewerm/.rustup`; original logs are retained. These are environment failures, not source regressions.
- `git diff --check`: passed. Commit, cleanup, clean status and submission receipt are recorded in the attempt report.
