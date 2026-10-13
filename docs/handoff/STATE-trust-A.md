# Handoff — STATE-trust-A: verified state-directory storage boundary

## Needs from coordinator

Independent review and a disposable guest run with a real second user remain pending. Part B must adopt the shared ancestor policy in daemon/deployment/metrics checks; `tidemarkd/src/api.rs` should later use `trusted_directory` instead of its duplicate socket-directory policy. No daemon production source was edited.

Daemon **unit** fixtures outside this task's scope still create default-permission temporary directories and database leaves (`crates/tidemarkd/src/lib.rs`, `src/scheduler.rs`, and `src/services/tswriter.rs`). The observed default-umask failures are `recovery_wait_preserves_busy_database_and_sidecars`, `healthy_catalog_does_not_wait_for_scan_lock`, the subprocess `recovery_scan_lock_child`, scheduler's `callbacks_wait_for_persistence_and_propagate_writer_error`, `growth_precedes_metrics_publication_and_series_coalesces_across_restart`, `catalog_descriptor_pressure_keeps_scheduler_pass_alive`, `firstboot_has_every_roots_next_due_before_first_admission`, `predecessor_survives_growth_and_series_then_retention_removes_it`, `series_shutdown_interrupts_each_queue_and_ack_wait`, and tswriter's `rollover_error_is_published_and_next_batch_recovers` and `supervisor_reports_panic_and_preserves_producer_queue`. They need explicit private fixture creation. Final broad checks use umask 077 to exercise them without weakening product checks. The shared standalone fuzz fixture `fuzz/support/catalog.rs` also needs explicit private directory creation; the store smoke target now uses a local copy with that fixture change, preserving its parser exercise. Consolidate those copies when the fuzz owner can edit the shared source.

- Date: 2026-10-13
- Attempt: attempt-3389ef89644d3a8f948104721c3622b9031c6bad68ce33004a6a79098ebaa2fb
- Status: implementation and validation evidence candidate; independent acceptance pending.

## Interface for part B

Fallible new functions return `std::io::Result`; storage callers preserve the existing `StoreError::Io` mapping, public signatures and CLI runtime/storage exit code 1. No enum variant, dependency or manifest was added. `tidemark-store/src/lib.rs` adds the public `state_dir` module declaration.

| Public interface | Rule |
|---|---|
| `trusted_directory(path: &Path) -> io::Result<OwnedFd>` | Absolute component walk from `/`; refuses parent traversal and symbolic links; O_DIRECTORY, O_NOFOLLOW, O_CLOEXEC and O_NOATIME with EPERM fallback. Every ancestor is root/euid-owned, or on a read-only mount. Group/other writable ancestors must be sticky; their next child must be euid-owned with no group/world bits. Returns the retained fd. |
| `file_parent(path: &Path) -> io::Result<PathBuf>` | Byte-preserving parent extraction; retains the literal `/.` in daemon descriptor paths, which ordinary Path::parent normalizes away. |
| `StateDir::open(path: &Path, create: bool) -> io::Result<StateDir>` | Verifies ancestry and requires an euid-owned final directory with mode & 077 == 0. May create only the absent final component, 0700 relative to the verified parent, then fstat checks inherited permissions/owner. A read-only open creates nothing. |
| `StateDir::snapshot_dir(path: &Path, create: bool) -> io::Result<StateDir>` | Opens enclosing state and each `snapshots` descendant using the private subdirectory rule. Standalone snapshot directories use StateDir::open. Component storage is capped at 4096. |
| `check_leaf(&self, name: &OsStr) -> io::Result<bool>` | Rejects slash, NUL, empty, dot and dot-dot names. Absent returns false; present must be regular, euid-owned, single-link and have no group/world bits. lstat-style statat never follows links. |
| `open_leaf(&self, name: &OsStr, write: bool, create: bool) -> io::Result<File>` | Prechecks and opens relative to the retained fd with NOFOLLOW, NONBLOCK and CLOEXEC; absent creation is exclusive 0600. Rechecks the opened descriptor before returning. |
| `subdir(&self, name: &OsStr, create: bool) -> io::Result<StateDir>` | Relative no-follow directory open; optional 0700 creation, followed by owner/mode checks. |
| `create_subdir(&self, name: &OsStr) -> io::Result<StateDir>` | Exclusive mkdirat then verified subdirectory open, for recovery collision handling. |
| `unlink_leaf(&self, name: &OsStr) -> io::Result<()>` | Checks the leaf, then unlinks relative to the retained directory; absence is success. |
| `fd(&self) -> BorrowedFd<'_>` | Retained descriptor for metadata and descriptor-relative callers. |

The exact byte form `/proc/self/fd/<decimal>/.` is accepted as the daemon's retained anchor: the helper safely reopens that descriptor reference with O_DIRECTORY/CLOEXEC/NOATIME and checks final directory ownership/private mode, skipping procfs ancestry. It does not borrow arbitrary raw fds using unsafe Rust. This produces a new directory descriptor for the same retained inode (rather than a literal dup of a raw integer). The retained-fd regression covers ancestor rename and descendant creation. Other procfs spellings do not get this exception.

Paths in new trust errors use the core terminal-safe lossy byte display helper. Filesystem error wrapping preserves the underlying io::ErrorKind, including NotFound for optional snapshot directories. Unsafe preexisting directories and leaves are refused; production code never changes their owner or mode to repair them.

## Entry points

| Entry point | Verification |
|---|---|
| `ScanLock::acquire` | State verification before any lock open; prechecks shared DB, WAL, SHM, rollback journal, v0 backup and its companions before lock creation. Lock uses verified nonblocking/no-follow opener and checked descriptor before flock. CLOEXEC remains set. |
| `Catalog::open` | State verification before writable creation or SQLite; read-only missing state returns error without creation. |
| `TimeSeries::open` / shared `open_connection` | Verifies byte-preserved parent, main leaf and all `-wal`, `-shm`, `-journal`, `.bak-v0` and backup sidecars before header read and again immediately before SQLite open. Main creation uses verified exclusive opener. Header read uses the checked nonblocking/no-follow descriptor. |
| `SnapshotWriter::create` | Private snapshot hierarchy via verified relative subdirectory opens; final and temporary leaf checks before creation; exclusive temporary creation includes NOFOLLOW/NONBLOCK/CLOEXEC; rechecks temporary before SQLite. |
| `Snapshot::open` | Verifies parent hierarchy and opens/checks leaf before immutable SQLite open, including catalog-selected snapshots. |
| `recover_catalog` / `recover_catalog_with_lock` | Verifies state and lock leaves before lock open/flock; main/sidecars/backup family checked before recovery directory creation or moves. Recovery directories use exclusive verified mkdirat. Snapshot namespace preflight precedes the compatibility lock creation and artifact moves. Existing scan-ownership/inode checks are retained. |
| `cleanup_abandoned_scans` | Temporary traversal starts at verified state; snapshots and each used root directory get private-owner checks; temporary unlink goes through checked retained root directory. Existing mount checks and bounded streaming remain. Catalog repair reaches Catalog::open independently, retaining best-effort error logging. |
| Catalog rebuild | Verifies the complete used snapshot namespace before SQL transaction; validates each used root directory and snapshot leaf before loading. Ordinary corrupt content is still logged/skipped; unsafe files return a trust error and roll back row changes. |
| Space guard | Root verification and fstatvfs on retained descriptor; usage traversal starts at that descriptor and refuses used directories with unsafe owner/mode. Existing no-follow metadata-only counting and mount/depth caps remain. |
| Scan retention | Replaces canonicalize/remove race with lexical `snapshots/<root>/<file>.db` validation and relative unlink beneath verified state, snapshots and root descriptors. |

No listed part-A storage entry point is intentionally left uncovered. Generic SQLite connections in tests remain fixture construction, not alternate product entry points. Part-B deployment markers, daemon admission and metrics destination policy remain out of scope. Existing path-based SQLite reopen/publication/recovery operations are protected by trusted ancestry/private child policy; this is not an atomic defense against a hostile process with the same effective uid or root authority.

## Regression evidence

- Module tests cover unsafe state/ancestor symlinks, private vs 0755 directories, read-only absence, sticky-parent/private-child acceptance, sticky child group-bit refusal, non-sticky writable ancestry, retained descriptor form, and symlink/FIFO/hard-link/group-writable leaves for database, WAL, lock and snapshot names. Referent bytes/size/mode are checked.
- Store `state_trust`: a matrix over main, WAL, SHM, journal, backup and backup sidecars refuses all four hostile leaf kinds through both Catalog and TimeSeries, creates no additional entries and leaves the external empty referent unchanged. Includes missing read-only state and snapshot reader/hierarchy checks.
- Store `storage::writer_refuses_hostile_snapshot_leaves_and_directories_without_changes`: writer final/temp leaf matrix plus planted snapshots directory link, preserving referents and entries.
- Scan `state_trust`: actual ScanLock refuses all four hostile leaf types without changing external bytes/size/mode or directory entries.
- CLI `state_trust::direct_scan_refuses_planted_database_link_without_changes`: actual binary direct scan exits 1, reports the timeseries.db regular-file refusal, creates no lock/database, and leaves the empty external file empty and 0600.
- SQLite experiment: bundled rusqlite/SQLite with SQLITE_OPEN_NOFOLLOW refuses the retained `/proc/self/fd/<n>/.` database path, while the equivalent ordinary renamed directory path succeeds. Without that flag, Catalog opens the retained descriptor path after ancestor rename, leaving the old pathname absent. This confirms the flag cannot be used for the daemon path; it does not prove every SQLite VFS race property.
- Privileged `state_trust::root_refuses_foreign_owned_ancestry_and_preseeded_regular_database` constructs foreign-uid ancestors and regular leaves entirely in disposable temporary state and asserts root refusal without mutation. Compiled only locally; **pending VM**, not executed.
- Lock reacquisition tests in CLI live_rows and scan orchestrator now retry Busy for up to two seconds, accounting for concurrent fork-to-exec descriptor sharing. Product lock CLOEXEC is retained.

## Assumptions and scope limits

- The supplied coordinator decisions govern this task; no questions, branches, merges, pushes or direct SQLite tracking-store changes were made.
- Trusted effective-user ownership supports unprivileged tests; installed production access remains root only. Modes are tested by absence of group/world bits, not exact 0700/0600 comparisons.
- State creation requires its parent already exist and be verified. Missing snapshot/recovery children are created relative to retained verified parents.
- A descriptor anchor is supplied by a caller already retaining that directory and responsible for its ancestry trust; the helper checks its final owner/mode but does not reconstruct ancestry from procfs. Part B must establish that trust before retention. Safe procfs reopen is used because arbitrary raw-fd borrowing would require unsafe outside tidemark-sys, which is out of scope.
- New tests use ordinary temporary files and FIFOs; no privileged-feature test was executed. Effective uid here is root inside the sandbox, but the hostile fixtures need no root-specific operation. Root-run DAC fixtures chown only their own temporary directories to the deliberately dropped child uid.
- Default-permission tempfile directories are 0755 with this pinned tempfile version and umask 022, contrary to the task's fixture assumption. Explicit private helpers were added to test-support and used in owned fixtures. New private directory/file fixture creation is independent of umask.
- The original default-umask workspace pass overlaps implementation edits and is diagnostic, not final-source acceptance evidence. Final broad commands use umask 077 for the out-of-scope daemon unit fixtures noted above.
- Shell fault/install scripts were inspected; fault common already creates state/root 0700, and install uses packaged /var/lib/tidemark rather than constructing a new unsafe custom state fixture. They were syntax-checked only. No host service, mount, LVM or system-directory operation was performed.

## Proposed plan and security text (not applied)

Plan: All direct and daemon storage entry points establish directory provenance before opening locks, reading database headers, opening SQLite or mutating snapshots. Absolute ancestry is walked without following links and accepted only with root/euid ownership (or read-only mount ownership) and safe write permissions. Sticky writable ancestors require a private effective-user-owned next child. State roots and used storage children must be effective-user-owned with no group/world permissions. Existing storage leaves must be regular, private, effective-user-owned and single-link; unsafe state is refused without normalization. Retained daemon fd paths preserve descriptor identity and use the same leaf/child rules. Pruning unlinks beneath verified retained snapshot directories.

Security: Tidemark accepts custom state paths only after checking their directory ancestry and private ownership. It refuses symbolic links, FIFOs, devices, sockets, hard-linked storage files and group/world-accessible storage leaves before database reads or writes. Do not place privileged state below a directory owned by another user. A sticky shared parent such as /tmp is accepted only with a private owned child. Installed production state is root-only. Deployment marker contents do not substitute for trusted ownership; daemon/deployment/metrics provenance is completed by the separate part-B work. These controls assume a trusted kernel/filesystem and exclude hostile root or same-uid processes. Metadata-only filesystem scanning remains distinct from intentional reads of verified Tidemark SQLite state.

## Validation

The final-source checks below completed. All long commands use timeout 1500; pinned Rust 1.98.0, copied offline Cargo registry in /tmp, four build jobs and two final test threads. No dependency was fetched or added. Listener-dependent tests may return early because socket binds are denied; passing harness counts do not establish those listener checks. Privileged compilation is separate from runtime acceptance, which is pending VM.

## Guest acceptance still needed

In a disposable Rocky guest, run the new root-only test (never on the developer workstation):

```sh
timeout 1500 cargo test --offline -p tidemark-store --features privileged --test state_trust root_refuses_foreign_owned_ancestry_and_preseeded_regular_database
```

Then reproduce the audit with a real second user: below that user's home, have the user create a private tidemark directory and plant timeseries.db pointing to a root-owned empty sentinel in temporary guest storage. `sudo tidemark --state-dir <user-home>/tidemark scan <guest-fixture>` must exit 1 with the foreign ancestor reason; sentinel bytes/size/mode, directory contents and all preexisting leaves must remain unchanged. Repeat with a root-owned private state root under that foreign-owned ancestor, and with a trusted root-owned parent but foreign-owned regular DB/lock/snapshot files. Verify FIFO cases complete promptly, hard-linked/unsafe-mode files are refused, valid default private state works, retained daemon state survives ancestor rename, and same-device bind-mount protections in existing privileged cleanup/space tests still hold. Real daemon/deployment marker and metrics checks belong to part B. Run listener-dependent daemon/CLI suites on an unrestricted runner; local conditional returns do not establish readiness/backlog behavior.

| Check | Observed result and transcript |
|---|---|
| `cargo fmt --check` | Exit 0; `post-fmt.log` (also review-fmt). |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0; `post-clippy.log`. |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | Exit 0; `post-clippy-privileged.log`. |
| `cargo test --offline --workspace --no-fail-fast` | Exit 0; **779 passed, 0 failed, 14 ignored**, 76 harness summaries; `review-workspace.log`; umask 077. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; `review-privileged-build.log`; build only. CI also compiled final targets after the last fixture adjustment. |
| `tools/ci.sh` with offline Cargo | Exit 0; `review-ci.log`; source policy (147 owner tags), format, Clippy, tests and privileged compilation passed. Test totals again **779 / 0 / 14**, 76 summaries. Cargo-deny explicitly skipped because unavailable. Umask 077. |
| Each fault `.sh`/`.bash` and install script `bash -n` | Exit 0 individually; `post-status.log`. No scripts executed. |

Final focused checks under **umask 022**: module **4 passed**; cross-crate trust integration **6 passed**; store recovery/fuzz/storage **42 passed, 1 scale ignored**; CLI e2e/rules **5 passed**; scan library **41 passed, 2 scale ignored**. The benchmark's explicitly private temporary ancestry passed its real tiny benchmark test under umask 022 (**1 passed**, `post-benchmark.log`). These results show the owned fixtures do not depend on the private broad-run umask. Harness totals include subprocess/helper summaries and conditional early returns; they are not counts of privileged or listener scenarios exercised.

The final Rust source is at `a18cc9b6509f70db1019a2590dc7a685b6ebac5d`; subsequent edits only finalize this handoff. `review-*` and `post-*` transcripts supersede diagnostic `initial-*` / `first-final-*` logs. Earlier iterations exposed and corrected ordinary fixture permissions, a proc-fd parent normalization bug, error wrappers losing NotFound, an incorrectly replaced test assertion, and the benchmark's temporary ancestor. An early compaction-child spawn ENOENT coincided with concurrent Cargo rebuilds of test executables; the final serialized workspace and CI passes did not reproduce it. Default-umask daemon unit failures remain an out-of-scope fixture issue described at the top, not a relaxation of product policy.

## Completion limits

All part-A storage entry points listed above have verification. Root/second-user and bind-mount runtime acceptance is **pending VM**; listener readiness/backlog behavior needs an unrestricted runner; advisory checks need cargo-deny. Default-umask workspace CI needs the out-of-scope daemon unit fixture corrections before independent integration. This report is an evidence candidate, not a verified task/release success declaration.


## Fixtures changed

The following inventory lists each changed fixture file and explicit private helper call sites. Temporary-owner helpers were changed together where they can become state ancestry; this keeps setup independent of umask. E2E/rules root-run fixtures additionally transfer only their own temporary directory/state to the dropped uid and keep permissions private; the Python lock-holder runs as that same uid and creates 0600. The v0 migration SQL fixture precreates its database privately. The shared smoke-test parser copy changes fixture creation only.

| File | Private fixture call sites (lines) |
|---|---|
| `crates/test-support/src/fixture.rs` | tempdir: 372, 575, 597, 604, 617, 637, 666, 724 |
| `crates/test-support/src/lib.rs` | tempdir: 18; dir: 27; write: 36 |
| `crates/test-support/src/lvm.rs` | tempdir: 62 |
| `crates/test-support/src/oracle.rs` | tempdir: 284 |
| `crates/test-support/src/xfs.rs` | tempdir: 58 |
| `crates/tidemark-cli/src/client.rs` | tempdir: 448 |
| `crates/tidemark-cli/src/cmd/diff.rs` | tempdir: 217, 230 |
| `crates/tidemark-cli/src/cmd/export.rs` | tempdir: 121; dir: 123 |
| `crates/tidemark-cli/src/cmd/watch.rs` | tempdir: 299 |
| `crates/tidemark-cli/src/lib.rs` | tempdir: 187 |
| `crates/tidemark-cli/src/live_rows.rs` | tempdir: 147, 173, 206 |
| `crates/tidemark-cli/tests/e2e/scan_lifecycle.rs` | tempdir: 18, 24, 186 |
| `crates/tidemark-cli/tests/e2e/support.rs` | tempdir: 26; dir: 58 |
| `crates/tidemark-cli/tests/rules.rs` | tempdir: 85, 160 |
| `crates/tidemark-cli/tests/state_trust.rs` | tempdir: 9; dir: 11, 13; write: 15 |
| `crates/tidemark-cli/tests/surface.rs` | tempdir: 229 |
| `crates/tidemark-cli/tests/w1a/main.rs` | tempdir: 31 |
| `crates/tidemark-cli/tests/w2c/main.rs` | tempdir: 276, 277, 355, 356, 379, 380, 437, 438, 478, 479, 514, 515, 516, 556, 557 |
| `crates/tidemark-cli/tests/w3a/catalog.rs` | tempdir: 35, 99 |
| `crates/tidemark-cli/tests/w3a/end_to_end.rs` | tempdir: 8, 18, 26 |
| `crates/tidemark-cli/tests/w3b.rs` | tempdir: 48, 126, 209 |
| `crates/tidemark-cli/tests/w3c.rs` | tempdir: 16, 129 |
| `crates/tidemark-cli/tests/w4a.rs` | tempdir: 24, 50, 78, 99, 289 |
| `crates/tidemark-cli/tests/w4a/lvm.rs` | tempdir: 43 |
| `crates/tidemark-cli/tests/w4b/mod.rs` | tempdir: 6, 28, 56, 87, 116; dir: 59, 118 |
| `crates/tidemark-cli/tests/w4b/privileged.rs` | tempdir: 23, 62 |
| `crates/tidemark-cli/tests/w5b/commands.rs` | tempdir: 10, 101 |
| `crates/tidemark-cli/tests/w5b/main.rs` | tempdir: 7, 41 |
| `crates/tidemark-cli/tests/w6a3.rs` | tempdir: 38, 196, 211, 270, 282, 294, 322, 377, 432, 483, 723, 992, 1048; dir: 1056 |
| `crates/tidemark-cli/tests/w7a/browser.rs` | tempdir: 14, 112, 203, 253, 297, 390, 428, 437, 484, 485 |
| `crates/tidemark-scan/src/orchestrator.rs` | tempdir: 1384, 1478, 1493, 1522, 1618, 1646, 1697, 1760, 1772, 1802, 1919, 1921, 1951, 1994, 2034; dir: 1479, 1649, 1775, 1920; write: 1620, 1651, 1702, 1782, 1804, 1923, 2062 |
| `crates/tidemark-scan/tests/crash_recovery.rs` | tempdir: 66, 67 |
| `crates/tidemark-scan/tests/state_trust.rs` | tempdir: 9, 10; write: 12, 26 |
| `crates/tidemark-scan/tests/w2b/main.rs` | tempdir: 59, 233, 269, 336, 398, 423, 524, 561 |
| `crates/tidemark-scan/tests/w2c/main.rs` | tempdir: 82, 167, 239, 304, 313, 359, 421 |
| `crates/tidemark-store/src/diff.rs` | tempdir: 476, 577 |
| `crates/tidemark-store/src/reader.rs` | tempdir: 778 |
| `crates/tidemark-store/src/space_guard.rs` | tempdir: 202, 203; dir: 206 |
| `crates/tidemark-store/src/timeseries.rs` | tempdir: 502 |
| `crates/tidemark-store/tests/compaction_crash.rs` | tempdir: 57 |
| `crates/tidemark-store/tests/crash_recovery.rs` | tempdir: 11, 23, 76, 87, 88, 120, 137; dir: 13, 122; write: 20, 24, 90, 124, 125, 139, 140 |
| `crates/tidemark-store/tests/state_trust.rs` | tempdir: 49, 50, 68, 77, 78, 124, 125, 132, 160; dir: 134, 162, 164, 176; write: 30, 52, 80, 178 |
| `crates/tidemark-store/tests/storage.rs` | tempdir: 104, 262, 308, 385, 552, 579, 679, 718, 843, 882, 906, 1084, 1167, 1217, 1269, 1323, 1363, 1413, 1437, 1509, 1545, 1696, 1773, 1774, 1811, 1812; dir: 418, 968, 969, 971, 1371, 1442; write: 285, 420, 421, 557, 681, 972, 1092, 1271, 1280, 1298, 1776, 1792 |
| `crates/tidemark-store/tests/support/catalog.rs` | tempdir: 9 |
| `crates/tidemark-store/tests/w3a.rs` | tempdir: 14, 128, 155, 218, 270, 315, 349; dir: 274 |
| `crates/tidemark-store/tests/w3b.rs` | tempdir: 98, 240, 267, 382 |
| `crates/tidemark-store/tests/w3c.rs` | tempdir: 16, 77, 167, 218, 240, 263, 318, 344 |
| `crates/tidemarkd/tests/w6a1.rs` | tempdir: 45, 92, 783; dir: 343, 437, 621, 668, 836, 883, 978; write: 843 |
| `crates/tidemarkd/tests/w6a2/privileged.rs` | tempdir: 50 |
| `crates/tidemarkd/tests/w6a2/runtime.rs` | tempdir: 63, 69 |
| `crates/tidemarkd/tests/w6a2s.rs` | tempdir: 27 |
| `crates/tidemarkd/tests/w6a3.rs` | tempdir: 124, 180, 227, 264, 308, 326, 353, 379, 436, 509, 575, 576; dir: 512 |
| `crates/tidemarkd/tests/w6a3/operator.rs` | tempdir: 74, 210 |
| `crates/tidemarkd/tests/w6a3/privileged.rs` | tempdir: 80 |
| `crates/tidemarkd/tests/w6a4.rs` | tempdir: 37, 40; dir: 224, 273; write: 231, 280 |
| `crates/tidemarkd/tests/w6a5.rs` | tempdir: 16 |
| `crates/tidemarkd/tests/w6a6.rs` | tempdir: 27, 81, 101 |
| `crates/tidemarkd/tests/w6b.rs` | tempdir: 108, 269, 317, 429, 586, 685 |
| `xtask/src/bench.rs` | state directory: 473, 540, 566, 595, 717 |
| `xtask/tests/w7b.rs` | tempdir: 164, 276 |
