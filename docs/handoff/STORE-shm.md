# Handoff — STORE-shm: contain SQLite companion unlink races

## Needs from coordinator

Rerun the slow guest fault test and cargo-deny on a host with cargo-deny installed. The controlled unprivileged reproduction establishes the zero-link failure mechanism, but does not establish the exact connection lifetime/timing in the original guest. Current tswriter already retains long-lived catalog and sample connections; why the oracle could be the last WAL connection in that guest remains unproven. The supplied socket restriction did not cause failures in the observed workspace run.

## Attempt

Date: 2026-10-14. Branch: supplied `hp-166510b77a207bde4d923a3319ab35a5ebee6bc1c2274d98c6e7c470b09093d0`. Evidence candidate awaiting coordinator review and the guest rerun.

## Cause and shutdown path

`StateDir::check_leaf` uses `statat(..., SYMLINK_NOFOLLOW)`, not an explicit open/fstat pair (also checked at guest commit `6de8837`). A pathname lookup can hold an inode reference across unlink. The test-only thread-local `STAT_HOOK` exposes that window: it holds the actual SHM inode, asks a separate plain rusqlite reader to close, waits for close, asserts `fstat` reports `st_nlink == 0`, and supplies that inode metadata to the store's check. No hard links are created by the reproduction. Three bounded iterations open through `timeseries::open_connection` while the SQLite reader opens/selects/closes the same WAL database.

Before the fix, the first iteration failed in 0.01 seconds:

```text
store open after SQLite close: Some(Io(Custom { kind: Other, error:
"state directory /tmp/.../timeseries.db-shm: leaf has multiple hard links; owner uid 0, mode 600" }))
1 failed; 16 filtered out
```

This demonstrates actual SQLite unlink and the exact misleading store error under a controlled lookup window, not a naturally observed zero-link `statat` return or a replay of the guest's 36-minute timing. The failing reproduction was committed first as `80d2275`.

The fatal runtime path is `scheduler` → `scheduling_catalog` → `Catalog::open` → `timeseries::open_connection` → companion check. Previously the helper contained:

```rust
Err(error) => Err(DaemonError::Store(error.to_string())),
```

The scheduling loop applies `?` to that helper. Unwinding its normal error return drops `SchedulerExit`, whose `Drop` contains:

```rust
self.0.shutdown.store(true, Ordering::Release);
```

The lifecycle loop in `lib.rs` observes that shared flag, leaves its ready loop, and logs `tracing::info!("daemon stopping")`; scheduler join propagates the storage error. This code path explains the two reported messages without a tswriter error log.

## Changes

- Added a companion-specific check for the seven existing suffixes (`-wal`, `-shm`, `-journal`, `.bak-v0` and its three sidecars). Zero links validates all other trust rules, then rechecks once by name. A second removed inode is absent; replacements are validated normally. Database and ordinary leaf checks still require exactly one link.
- Zero links now says `leaf has no hard links (removed during check)`; only counts above one say `leaf has multiple hard links`.
- Runtime scheduler catalog-open errors publish `scheduler=degraded` with the original error and skip that pass, allowing its existing one-second scheduling loop to retry. Refused files remain unopened. Healthy scheduling status is published only after a successful open, so failed retries do not briefly claim recovery. Startup state validation, storage admission/recovery and writer lease refusals are unchanged.
- No connection-lifetime redesign: tswriter's catalog and sample handles are already long-lived. Scheduler intentionally opens a fresh read connection per scheduling pass; changing this would expand scope and alter runtime trust checks.
- `compaction.py` unchanged. Its failing `complete` phase only selects rows. The separate `idempotent` phase deliberately inserts its second-compaction witness; that is necessary to prove another compaction occurred and does not explain the earlier failure.

## Tests and evidence

New tests:

1. `timeseries::query_plan_tests::reader_close_during_companion_stat`: controlled real SQLite reader close, held SHM inode reaches zero links, three store-open iterations; all channel waits have five-second deadlines.
2. `state_dir::tests::leaf_link_counts_are_distinct`: real two-link and subsequently unlinked inode; checks distinct rejection messages and ordinary leaf refusal.
3. `state_dir::tests::removed_companion_keeps_other_rules_and_rechecks_replacement`: zero-link inode with writable mode still refused; retry seeing a two-link replacement still refused.
4. `scheduler::tests::catalog_open_trust_failure_degrades_and_recovers_after_readiness`: real scheduler thread after published readiness, database mode changed in private tempdir to cause a new-open refusal, observes degraded detail and unset shutdown, restores mode, observes healthy retry, then joins normally. Existing clock/service seams; no binary hook or sockets.

Commands used host Rust 1.98.0 with `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, and `CARGO_NET_OFFLINE=true`; every long command ran under `timeout 1500`. Logs are preserved in the attempt's output `library/`.

| Check | Observed result |
| --- | --- |
| `cargo fmt --check` | passed (including final source) |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | passed (including final source) |
| `cargo test --offline --workspace --no-fail-fast` | exit 0; unfiltered summaries total 946 passed, 0 failed, 14 ignored (scale); four additional filtered subprocess passes excluded from that total |
| `cargo build --offline --workspace --features privileged --tests` | passed; no privileged tests executed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | passed |
| `tools/ci.sh` with offline Cargo environment | exit 0; source checks, fmt, clippy, strict-ancestry tests, final workspace tests and privileged build passed; cargo-deny skipped because not installed |
| focused final store library tests | 18 passed, 1 ignored |
| focused final daemon storage regression | 1 passed, 121 filtered out, 0.49 seconds |

The first workspace command began before the final health-publication ordering adjustment; final format, clippy and the focused daemon regression were rerun after it. CI also validates the final source. The before-fix full-workspace outcome was not measured; subtracting the four added tests gives an inferred prior inventory of 942 unfiltered passing tests plus 14 ignored, not a prior execution result.

Test inventory: store library baseline 16 registered tests, now 19 (three additions); daemon library baseline 121, now 122 (one addition). Baseline inventories inferred from unchanged tests and compiler test counts, not a claim that the entire baseline suite was executed. Before-fix regression: 0 passed / 1 failed. The initial focused post-fix store suite before the third added unit test passed 96 tests with 5 ignored, including the ten-cycle unprivileged compaction-crash regression; the focused daemon regression passed 1/1.

## Guest rerun (pending VM)

Only inside the disposable Rocky guest, from the repository root:

```sh
TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_SLOW=1 TIDEMARK_FAULT_CYCLES=1 bash tests/fault/04-compaction-crash.sh
```

No privileged tests or fault scripts were run on this workstation. Existing privileged tests are build-only here.

## Assumptions and scope

Treat the reported failure as a companion unlink race worth containing, while preserving uncertainty about its original trigger. Runtime trust refusals degrade scheduling until repaired rather than authorizing access. Startup trust and lease failures must remain fatal. No user questions were asked.

Only owned paths changed. No shared manifest/module edits, dependencies, frozen contracts, CLI changes, unsafe code, or memory projection edits. Work stayed on the supplied attempt branch; no branch creation, merge or push.

## Finalization

Product commits: `80d2275` (failing reproduction first), `50015df` (fix and regressions). The handoff is committed separately. Build artifacts are cleaned after validation and before submission; the attempt report records the resulting working-tree check. Independent verification and VM acceptance are left to the coordinator.
