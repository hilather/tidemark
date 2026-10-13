# Handoff — LEASE: writer exclusion, startup error types, refusal parity

## Needs from coordinator

- Run listener-dependent and privileged runtime acceptance on the coordinator runner / disposable Rocky guests. This is an evidence candidate, not an independent verification or acceptance decision.

**Attempt:** attempt-72341c801e5c44229cd290cb4fe689124c015b5bbb825f1d8aaa5b00db2da63e  
**Branch:** hp-72341c801e5c44229cd290cb4fe689124c015b5bbb825f1d8aaa5b00db2da63e  
**Date:** 2026-10-13  
**Status:** implementation and validation evidence candidate; independent review / guest acceptance pending.

## What changed

**C1:** Added `tidemark_store::lease` with shared `WriterLease` and exclusive `RecoveryLease`. `writer.lock` is opened through StateDir's verified no-follow leaf opener and created at 0600. Contention uses nonblocking flock with monotonic bounded retries: shared opens wait at most two seconds; recovery callers choose their bound (startup and offline CLI use thirty seconds). Typed `LeaseBusy` errors remain the source of an I/O refusal. Both Catalog and TimeSeries retain shared ownership until their connection is dropped, so daemon writer, scheduler and recorder callers participate automatically without signature changes.

Recovery's actual renames/recreation require an exclusive guard argument and verified scan ownership. Startup and offline preparation acquire writer ownership before scan ownership, recheck the database after waiting, close the recovery connection, release the guards, and then open normally. Recovery preserves the database, WAL, SHM, journal and v0 backup companions. Initialization/migration and backup creation also require exclusive ownership; initialization connections close before exclusive ownership is released and normal shared connections are opened. Direct scans open their catalog before acquiring scan ownership; prune/rebuild retain writer ownership before their scan lock. Definitive daemon-absence checks remain before offline preparation and under recovery/mutation locks.

**C4:** Startup maps scan I/O and rustix errors into `DaemonError::Io`, store errors through `startup_store_error`, and lease I/O failures into `DaemonError::Io`. Busy refusals remain bounded storage refusals. The same classification function is used for scan-lock acquisition and writable catalog opens. The pressure classifier also recognizes the pinned toolchain's typed descriptor-exhaustion kind (reported as `TooManyOpenFiles`), which StateDir preserves when adding path context even though it drops raw errno. The variant name is unstable, so code compares typed kinds using the stable `Error::from_raw_os_error(EMFILE).kind()` API. No error-text parsing or StateDir edit is needed.

**C8:** A single `no_space(state)` formatter supplies both direct-mode and daemon NoSpace refusals. The configured state directory is included in both paths, with common human output `scan: /: scan state: <state-dir>: NoSpace` for root `/`. Exit 5 and the API error remain unchanged. Binary expectations share one helper; an in-process test checks equal wording and exit 5 without a socket.

## Regression evidence

- Store `recovery_excludes_live_writer_and_preserves_artifacts`: a subprocess holds a real writable TimeSeries connection, with no scan lock or socket. After deliberate private-fixture corruption, a 60 ms exclusive attempt and compatibility recovery refuse; database/WAL/SHM bytes and inodes remain unchanged. After child termination, recovery succeeds and preserves all three artifacts. This fails without writer lifetime leases.
- Store `every_writer_holds_lease_and_readers_do_not`: both write-capable handle types exclude recovery, read-only Catalog takes no lease, shared contention returns typed recovery refusal within its bound, and normal writable open succeeds after release.
- Store `writer_lock_obeys_verified_leaf_rules`: created mode is 0600, hard-linked, writable-by-others and symlink leaves are refused.
- Daemon `startup_refuses_pre_api_writer`: child Catalog and TimeSeries connections exist before any API socket; bounded startup recovery refuses without artifact changes, then succeeds after child exit.
- CLI `rebuild_refuses_pre_api_writer_without_renaming`: the same pre-API ownership window is exercised by the real CLI subprocess. Rebuild refuses within 35 seconds (configured bound 30), preserves all bytes/inodes, and succeeds after child termination. No socket bind is required.
- Daemon `racing_corrupt_starts_recover_once`: two barrier-released child starts succeed with exactly one recovery namespace. The winner rechecks under exclusive ownership; the other does not rename the replacement again.
- `healthy_catalog_does_not_wait_for_scan_lock` now holds a real writer in another process as well as a busy scan lock, and startup completes in under one second. Existing cleanup, byte/inode preservation and abandoned-scan tests remain.
- `startup_initializes_empty_database_without_self_conflicting_lease`: existing version-0 storage migrates under exclusive ownership without attempting to upgrade a locally retained shared lease; the backup remains and no recovery namespace is created. This fails without the startup initialization amendment.
- `startup_pressure_types_retry_at_lock_and_store_seams`: Raw and StateDir-style contextual EMFILE and ENOMEM injected at scan I/O, scan rustix, store-open and lease classification seams retry once then succeed. Busy is attempted once. This fails with stringification at these seams or omission of the contextual descriptor-pressure kind.
- `startup_retries_real_descriptor_exhaustion`: a child lowers only its own NOFILE soft limit to 64, holds descriptors until real EMFILE, exercises `startup_catalog` through `retry_start`, releases the descriptors after the first failure, and succeeds on attempt two. This requires no sockets, root privilege or host limit changes.
- CLI `no_space_paths_have_identical_text_and_exit`: direct and daemon mappings produce identical state-directory text and exit 5 in-process. Existing binary direct/daemon cases use the common expected line.

## Shared-file edits and compatibility

Only assigned paths changed. `crates/tidemark-store/src/lib.rs` adds the documented `pub mod lease` declaration. No manifest, dependency, core type, schema, API, CLI flag or configuration key changed. Existing public signatures remain; new guard-aware recovery and post-recovery open functions were added after searching workspace callers.

Legacy `recover_catalog_with_lock` accepts an already-held scan descriptor, so its compatibility wrapper takes exclusive writer ownership internally with a two-second bound. New production recovery paths take the writer lease first and pass both guards. `ScanLock::open_catalog` retains its compatibility recovery behavior; when a caller already has live writer connections, exclusive recovery is refused rather than upgrading its own shared flock. flock belongs to an open file description; even local connections must close before exclusive recovery. Compatibility wrappers cannot establish a prior acquisition order for a descriptor their caller already holds, but their waits remain bounded and actual replacements remain excluded from every live compliant writer.

## Assumptions and limitations

- Catalog and samples share `timeseries.db`; this baseline has no separate live catalog database. No migration-restore function exists in the assigned implementation; existing backup creation is guard-aware.
- Leases coordinate compliant Tidemark processes. They do not defend against a hostile same-uid process replacing lock paths or an external SQLite writer that never acquires writer.lock.
- StateDir's frozen-in-this-task leaf policy is reused; no changes were made to its implementation. An initial source-only concern about discarded errno was resolved after probing the pinned toolchain: EMFILE retains `TooManyOpenFiles`, which the classifier now handles. `library/c4-state-dir-loss.rs/.log` records that extracted wrapping probe; the real child-process regression separately exercises actual startup.
- No branches, merges, pushes, host service/storage commands, user namespaces, privileged runtime tests or system-directory writes were performed. Builds use the pinned Rust 1.98.0 binaries directly and a temporary offline copy of the readable registry cache, four build jobs, and two test threads for the full checks. The workspace suite explicitly uses umask 022.
- Existing listener tests may return early on denied socket binds; successful harness counts do not prove those branches ran. New lease/process and mapping regressions do not require listeners.
- A concurrent fork can briefly inherit a lock description until exec, even with CLOEXEC. Compatibility exclusive acquisition uses a bounded two-second retry rather than a zero-wait assumption. An early parallel checked-recovery run observed transient contention; focused rerun and the bounded retry passed. Fork inheritance is a possible explanation, not an established attribution. Final validation is listed below.

## Proposed plan text (not applied)

All Catalog and TimeSeries write-capable handles retain a shared state-directory writer lease for their complete lifetime. Shared opens wait at most two seconds and report a typed recovery refusal. Database/sidecar replacement, initialization and migration backups require exclusive writer ownership with a caller-supplied bounded wait. Recovery takes writer ownership before scan ownership, rechecks corruption after waiting, closes recovery connections and releases both guards before normal shared opens. Startup and offline CLI wait at most thirty seconds for exclusive writer ownership; healthy startup does not wait for scan contention. Legacy guard-taking entry points retain bounded compatibility behavior and must refuse self-conflicting writer ownership.

Startup must preserve resource-pressure causes across verified-directory, lease, scan and store boundaries. Human NoSpace refusals identify the configured state directory consistently for daemon and direct paths, preserving the frozen API refusal and CLI exit 5.

## Proposed security text (not applied)

The state directory contains writer.lock, created with mode 0600 through the verified leaf opener. Every compliant write-capable shared-database handle holds a shared flock lease, and database replacement requires an exclusive lease as well as scan exclusion. All local writer connections must be closed before recovery; leases are per open file description, not a process-wide reentrant mutex. Recovery refuses contention without renaming database artifacts. Lock files must never be deleted or replaced while any process holds them. This is cooperative writer exclusion within the existing trusted-state-directory boundary.

## Validation

Detailed transcripts are in the canonical attempt output `library/`. The `amended-*` transcripts cover the final implementation after the initialization and contextual-pressure amendments; earlier `final-*` transcripts retain the preceding successful pass.

| Command | Observed result | Transcript |
|---|---|---|
| `cargo fmt --check` | Exit 0 | `amended-fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 | `amended-clippy.log` |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | Exit 0 | `amended-clippy-privileged.log` |
| `cargo test --offline --workspace --no-fail-fast` | Exit 0: 871 passed, 0 failed, 14 ignored; 83 summaries | `amended-workspace-test.log` |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compile only | `amended-privileged-build.log` |
| `tools/ci.sh` with offline Cargo | Exit 0: 871 passed / 14 ignored; source checks and privileged build passed; cargo-deny unavailable and skipped | `amended-ci.log` |

Counts are sums of harness summaries, including helper subprocesses and zero-test doc harnesses; conditional listener returns are not proof that listener branches executed. The first pass was 867 passed / 14 ignored; its following CI pass included the initialization amendment (868 passed / 14 ignored). The final pass includes the actual descriptor-exhaustion parent/helper and contextual-kind coverage. No privileged runtime tests were executed.

Focused checks already observed: store crash_recovery 9 passed; storage 33 passed / 1 scale ignored; daemon library 96 passed for C1; C4 pressure regression 1 passed; CLI library 101 passed / 1 scale ignored and w6a3 29 passed; C8 in-process regression 1 passed. Final focused startup seams: 5 passed plus 1 child summary; final daemon library: 100 passed. Child/helper summaries are separate from top-level counts.

## Guest / unrestricted-runner checks

No new regression requires privileged features. To exercise all listener branches on an unrestricted ordinary-user runner:

```sh
umask 022
timeout 1500 cargo test --offline -p tidemark-cli --test w6a3 -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a4 --test w6a1 -- --nocapture
```

Existing privileged runtime/deployment acceptance is pending VM; local privileged targets are compiled only. In disposable Rocky guests:

```sh
timeout 1500 cargo xtask vm-test --host <rocky-8.8-vm>
timeout 1500 cargo xtask vm-test --host <rocky-8.10-vm>
```

These must establish real service startup, API ownership, runtime recovery, crash/restart and existing deployment behavior. No local result establishes the guest gates or performance/soak acceptance.

## Definition of Done evidence

- [x] Only assigned paths changed; the sole shared module declaration is listed above.
- [x] Final-source formatting and both Clippy modes pass with warnings denied.
- [x] Final-source unprivileged workspace suite passes with umask 022.
- [x] Privileged targets compile; no privileged tests were run locally.
- [x] New public items have rustdoc; no unsafe, dependency or production unwrap/expect additions.
- [x] C1, C4 and C8 have discriminating local regression evidence and proposed plan/security text.
- [x] Final tools/ci.sh passes all available checks; cargo-deny unavailability is explicit.
- [ ] Independent review, unrestricted listener coverage, cargo-deny advisory checking and guest runtime acceptance remain pending.
