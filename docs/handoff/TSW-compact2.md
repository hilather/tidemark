# Handoff — TSW-compact2: external compaction witnesses

## Needs from coordinator

**Product fix blocked by scope and frozen preflight contract.** Authorize a follow-up owning `tidemark-sys` and the necessary manifests to implement SQLite-coordinated no-follow header access, or approve a storage-spec change providing another safe preflight design. `05-storage.md` §4 explicitly requires each existing database's first 16 bytes to be read without SQLite before opening it. Merely removing that preflight makes the reproduction pass but violates that requirement. A raw descriptor must not be closed while another SQLite connection in the same process owns POSIX locks on that inode. SQLite's Unix VFS coordinates/defer-closes its own descriptors; calling its VFS requires unsafe code, allowed only in `tidemark-sys`, which this task does not own. This is a concrete spec/scope change request recorded here because a separate SCR path is also outside the assigned write scope.

This candidate supplies diagnostics and established causal evidence, **not a product fix**. Older SQLite client runs still fail. Rerun GitHub CI and guest P4 after the follow-up fix; do not mark this task resolved on the basis of the local default-client checks.

- Attempt: `attempt-c57bf46bc9523e2bf593d2b3dec1bc4cf846f7d3d113f9faac1cc4cbf9f7161e`.
- Branch: precreated attempt branch; no branching, merge or push.
- Date: 2026-10-14.
- Status: partial implementation and evidence candidate; product correction pending scope decision.

## Cause: established and inferred

Established locally: opening a second store connection executes `preflight_header`, which opens and closes an independent regular-file descriptor for the same `timeseries.db` inode. POSIX record locks belong to the process; closing that descriptor releases locks held by its already-open SQLite connection. SQLite's internal inode lock bookkeeping still reports its shared main-file lock held, so subsequent connections do not restore the missing kernel lock. Both Catalog and TimeSeries use this open path. The writer opens Catalog then TimeSeries; the supervisor repeats that sequence on restart.

A direct, separate-process `fcntl.lockf` probe confirms this without clocks, writer queues or older SQLite: after the first TimeSeries open and query, an external exclusive lock on SQLite's shared main-file range (510 bytes at `0x40000002`) fails; after the second TimeSeries open/query, it succeeds. Artifact `library/tscompact_lock_probe.rs` and its passing diagnostic transcript reproduce these observations. The diagnostic asserts the defect and therefore is preserved as an artifact rather than a permanent test that would encode defective behavior.

Established A/B reproduction: compile cached SQLite 3.46.0 and preload it into Python, keeping the daemon's bundled SQLite 3.51.1 unchanged. All three compaction tests fail with the reported 600-raw-row symptom. With only the raw header-preflight invocation temporarily omitted, all three pass under the identical command. The invocation was restored before final checks; no product changes remain in the candidate. No new dependencies or network download were used.

Established on failing CHECK: the compaction INFO reports `inserted=2 deleted=600`, but Python reads 600 raw rows. `/proc/<writer-process>/fd` shows the writer's WAL and SHM descriptors pointing to **deleted** inodes, while the named WAL and SHM have different inode numbers. In the captured example, held WAL/SHM were 1457/1458 and named replacements 1461/1462. The writer committed into an orphaned WAL. Thus the external rows are not persistently invisible through a read snapshot: compaction really processes them, but its result is invisible to subsequent independent clients using the replacement sidecars.

Candidate causes:

1. Statement/read-snapshot retention does not explain this reproduction. Query statements are local and dropped on return, including bounded iterator early termination; insert statements are scoped before commit; transactions commit or roll back on drop. `unchecked_transaction()` being deferred does not keep a transaction open across calls. The lock probe uses only completed queries.
2. The two-Flush barrier orders maintenance: each message is acknowledged during processing, then the loop checks maintenance before receiving another message. The `recv_timeout` and `receiver.is_empty()` paths do not bypass that check. Captured completed compaction counts plus orphaned descriptor identities exclude an early CHECK as this reproduction's cause.
3. Compaction ran and selected the expected rows (`2/600`), excluding selection of nothing for the captured failures.
4. The older external client's sidecar cleanup exposes the lost main-file locks. A WAL-index format incompatibility is unnecessary to explain the observations: removing only the independent raw header close fixes the mixed-version run. Default Python SQLite 3.53.4 masks this defect in our local tests. The precise newer-client cleanup change has not been established.

The precise Ubuntu 24.04 system SQLite version was not exercised here (cached 3.46.0 was available). It is inferred that GitHub has the same cause, given the identical three-test failure and older-client reproduction. It is also a strong candidate for guest P4: daemon startup opens multiple connections, and external oracle processes can leave it writing an unlinked WAL. Guest logs/inode evidence were not available, so the historical P4 cause remains unconfirmed. The new INFO lines can still log successful writes to an orphaned WAL; logging by itself does not repair it.

## Change

All three compaction tests now run with an isolated tracing subscriber in a child test process. A panic guard prints captured writer INFO/WARN/ERROR lines on any failure, including failures before the restart test's final log assertions. This avoids cross-test tracing callsite interference. Failed Python CHECKs also print the client SQLite version, journal mode, named database/sidecar inode identities, and parent-process open descriptors, making orphaned WAL/SHM visible in the next CI run.

Start-based cadence, restart deadline retention, and production INFO/WARN behavior are unchanged. The existing barrier remains unchanged because it was not the cause. No success assertion was weakened or converted to a skip. The test still fails on older clients until the product fix is implemented.

## Tests and validation

See the final results below. All required commands use pinned Rust 1.98.0, host Cargo cache, offline mode and `timeout 1500`. Default environment is uid 0 with no socket setup; no privileged test was executed. The diagnostic lock probe and older-client A/B tests require no privileges, mounts, sockets or real scans.

- Default-client focused tests: three pass before and after diagnostic logging.
- SQLite 3.46.0 focused tests: final workspace binary exits 101 with all three failing and full diagnostics (`older-client-final.log`); all three pass only in the temporary preflight-omission experiment. An initial final rerun selected the stale diagnostic binary; it was rerun using the binary path printed by the actual workspace test transcript. The stale passing run is retained and excluded from final validation.
- Direct POSIX-lock diagnostic: passes, establishing the loss of a live SQLite lock on second open.
- Privileged-feature build: compile only; VM execution and P4 rerun pending coordinator.
- uid 1000/umask 022 and actual GitHub Ubuntu runner: not available to this isolated worker; pending gate.

## Assumptions and deviations

- Assigned task scope overrides wave ownership/branch instructions. No shared manifest, CLI, core contract, memory projection or unowned source was edited.
- Older SQLite 3.46.0 from the existing Cargo cache is sufficient to reproduce the older-client failure mechanism; it is not claimed to be Ubuntu's exact version.
- Temporary diagnostic source/product edits were restored or removed before final validation. Artifacts retain the causal evidence.
- Existing Python test dependency and Linux `/proc` access are used only in tests. Sidecar diagnostics tolerate descriptor disappearance while enumerating.
- The product fix cannot be silently implemented by removing the normative header preflight, adding unsafe code to the store, adding an external runtime reader dependency, or retaining an unbounded/process-lifetime collection of raw header descriptors. A coordinated VFS helper is a proposed follow-up design, not an implemented or fully validated API. The literal “without SQLite” wording also needs coordinator clarification/amendment if the preflight uses only SQLite VFS I/O rather than opening a database connection.
- Initial Cargo invocation lacked PATH, and the first formatter invocation lacked writable `RUSTUP_HOME`; corrected checks below use explicit toolchain/cache environment.

## Shared-file edits

None. Final repository edits are only the compaction test module and this handoff. No dependency was added.

## Final validation results

| Command / experiment | Observed result | Evidence artifact |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/fmt.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log` |
| `cargo test --workspace --no-fail-fast` | exit 0 | `library/tests.log` |
| `cargo build --workspace --features privileged --tests` | exit 0; build only | `library/privileged-build.log` |
| `tools/ci.sh` | exit 0; cargo-deny absent/skipped | `library/ci.log` |
| Final workspace binary, SQLite 3.46.0 external client | exit 101; all three compaction tests fail | `library/older-client-final.log` |
| Temporary omission of header preflight, identical older client | three tests pass | `library/tsw-no-preflight.log` |
| Direct separate-process POSIX-lock diagnostic | one diagnostic passes, asserting lock loss | `library/tsw-lock-probe.log` |

Workspace transcript: 86 printed summaries, 955 printed passing results, zero failures, 14 ignored. These include nested child-process output and are not counts of unique tests. CI also observes passing source-policy checks (150 owner tags). Detailed reproducibility commands, the removed diagnostic source, and A/B transcripts are in the attempt output `library/`.

Repository scope and formatting checks are clean. Production behavior remains unfixed; the older-client failure is intentionally preserved, with better evidence. Actual Ubuntu, uid 1000 gate and guest/privileged execution remain pending. No verification, integration or release success is claimed.
