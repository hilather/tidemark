# Needs from coordinator

Independent review, uid 1000 / umask 022 with sockets, and GitHub Ubuntu 24.04 execution remain pending. Privileged runtime remains pending VM; this attempt only builds privileged tests. This handoff is an evidence candidate, not verification or release approval.

# POST-purgetest — retain live SQLite connections and protect sidecars

## What changed

The purge child now commits an fs sample with a fixed timestamp through TimeSeries before atomically renaming `ready.tmp` to `ready`. It retains both Catalog and TimeSeries until after its existing idle wait; the parent checks the child remains alive before snapshotting and after comparing, then terminates it. Both handles own their normal writer leases, so the redundant standalone WriterLease was removed. Catalog and TimeSeries share `timeseries.db`; there are not two separate databases to protect.

The parent explicitly requires nonempty `timeseries.db-wal` and `timeseries.db-shm` entries in its snapshot. The existing comparison checks every captured directory/file inode and every regular-file byte, including both sidecars and the main database, after the expected Guard refusal. No sidecars are excluded or given weaker comparisons. Atomic readiness and both child-liveness checks remain.

Production code, manifests, dependencies, shared files, contracts, flags and schemas are unchanged. Only the owned test file and this handoff change. No SCR or privileged test addition is needed for this test-only correction.

## SQLite determinism

SQLite's [WAL documentation, sections 2.3 and 3.1](https://sqlite.org/wal.html#automatic_checkpoint) specifies automatic checkpoints on the committing thread at the WAL page threshold and when the last database connection closes. A timer or background checkpoint worker must be provided by the application. The child's store opens and `insert_fs` are synchronous, and these handles do not spawn such a worker. The sample commit (including any automatic checkpoint it triggers) returns before readiness. After readiness the child only sleeps: no commits, queries, explicit checkpoints or connection closes occur during a successful parent's comparison interval. Its handles close only if it reaches the existing 60-second child wait; returning then also exits the child, which the parent's final liveness assertion rejects.

The cached, locked `libsqlite3-sys` 0.36.0 SQLite amalgamation also implements `sqlite3WalDefaultHook` as a synchronous threshold check calling `sqlite3_wal_checkpoint`, and `sqlite3_wal_autocheckpoint` installs that commit hook. The relevant source excerpt is retained in `library/sqlite-checkpoint-source.txt`. Therefore no test-only PRAGMA change or weaker byte comparison is required. Full WAL/SHM byte equality remains deterministic for these idle handles.

## Before/after regression evidence

With only the new parent sidecar assertion and the original child setup, the exact parent test failed (exit 101):

```text
live writer sidecar missing or empty: timeseries.db-wal
test result: FAILED. 0 passed; 1 failed; 0 ignored
```

After retaining connections and committing the sample, the same exact test passed (exit 0):

```text
test deploy::tests::purge_refuses_live_writer_without_changing_bytes_or_inodes ... ok
test result: ok. 1 passed; 0 failed; 0 ignored
```

These are `library/before.log` and `library/after.log`. The original regression passed without the added assertion; the demonstrated failure is the strengthened coverage test against the original child, not a production purge failure.

## Assumptions

- The explicit POST assignment supersedes generic wave-task and branch-creation instructions. The precreated attempt branch is retained; no branch creation, merge or push occurs.
- One actual committed fs sample establishes the shared database's live writer WAL/SHM state; an artificial second catalog record is unnecessary because both handles use the same database.
- Sample timestamp `1_700_000_000` is injected test data, with no system-clock dependency. Existing readiness deadlines and child idle wait remain synchronization safeguards; no elapsed-time correctness assertion is added. No production clock-driven behavior is under test here.
- Validation uses uid 0, umask 022, pinned Rust 1.98.0 and the host Cargo cache offline. No sockets or privileged runtime are required by the affected test.
- The realistic open-connection case subsumes the redundant standalone-lease shape for this regression. Existing store lease tests cover lease ownership separately.

## Validation and repetition counts

All long commands run under `timeout 1500`; detailed logs live in this attempt's output `library/`.

- Live-writer parent: **200/200 passed**, zero failures, four lanes of 50, using the built unprivileged test executable with `--exact deploy::tests::purge_refuses_live_writer_without_changing_bytes_or_inodes`, under `timeout 1500 bash /tmp/purgetest-repeat.sh`. Each run starts the real child with `--exact deploy::tests::purge_writer_child` and the state environment set; that child remains alive through comparisons and is intentionally killed afterward. Counts are independently confirmed from 50 passing summaries per lane, recorded in `library/repeat-counts.json`, `repeat-{1,2,3,4}.count` and corresponding logs.
- Helper standalone: **200/200 passed** with `--exact deploy::tests::purge_writer_child` under `timeout 1500`, `library/helper-repeat.log`. Without its environment this is the intentional no-op path; these runs are not additional live-connection coverage. Live child coverage comes from the 200 parent invocations above.
- One additional before/after exact parent invocation: strengthened test failed against old child; corrected test passed. Workspace and CI invocations are separate from the 200-run count.

| Required check | Observed result | Artifact |
| --- | --- | --- |
| `cargo fmt --check` | Passed | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed | `clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | Passed; 1,031 passed / 0 failed / 14 ignored across 87 summaries, including nested child harnesses | `workspace.log`, `workspace-counts.json` |
| `cargo build --offline --workspace --features privileged --tests` | Passed; build only | `privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Passed | `docs.log` |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | Passed; 1,033 passed / 0 failed / 14 ignored across 88 summaries including child harnesses; source checks (150 owner tags), fmt, Clippy, strict-ancestry checks, workspace tests and privileged-feature compilation passed. cargo-deny absent, dependency-policy check skipped | `ci.log`, `ci-counts.json` |

## Remaining uncertainty and cleanup

No privileged tests were run. VM runtime, uid 1000 with sockets and GitHub Ubuntu 24.04 remain coordinator gates. cargo-deny dependency-policy validation remains pending an environment with that tool. The local workspace command passes in this socket-restricted uid-0 sandbox; this does not establish socket-capable execution of cases that return early under sandbox restrictions.

The changed test code uses existing synchronous store APIs, injected sample time, existing temporary fixtures and existing synchronization bounds. No new unsafe code, dependency, production behavior, real-time correctness assertion or privileged operation was introduced. No shared files or memory projections were edited. Validation artifacts and repeat driver are retained outside the repository in this attempt's output. Final cleanup uses `cargo clean --offline` after committing, followed by a clean-status check; its output is retained in `library/clean.log` and `library/final-status.txt`.
