# Handoff — STORE-locks

## Needs from coordinator

- Run the uid 1000 / umask 022 gate and GitHub Ubuntu 24.04 gate on this candidate. Local runs use uid 0 without socket setup; the older-client reproduction uses SQLite 3.46.0, not Ubuntu's exact 3.45 build.
- Run cargo-deny on an equipped gate host; the CI wrapper explicitly skipped it because it is not installed.
- Rerun guest P4 and the VM gate. Privileged tests are compiled only, never executed here. No new privileged behavior was introduced.
- No additional production database/sidecar descriptor-close defect was found outside the owned store paths. No unowned source fix is requested.

## What changed

The shared Catalog/TimeSeries open path now registers a trusted read-only header descriptor in a process-wide `(device, inode)` registry. It checks the named leaf with no-follow `statat`, reuses a registered descriptor without opening another, and reads the first 16 bytes with safe positioned `FileExt::read_at` calls. Empty files still initialize; other invalid headers still return `SQLITE_NOTADB` before SQLite opens them. Identity checks before and after the SQLite open reject a detected replacement during opening.

An internal `StoreConnection` wrapper retains a counted header guard. Rust field drop order closes the SQLite connection before releasing that guard. The final guard removes and closes descriptors under the registry mutex, so a concurrent opener cannot race final descriptor closure. Failed opens release their guard too. Replacing a name registers the new inode independently and preserves the old entry while its connections remain. In the exceptional stat/open rename race that selects an already registered inode, the duplicate descriptor is retained with that entry rather than closed.

The registry admits at most 256 concurrent header descriptors (documented constant `HEADER_DESCRIPTOR_CAP`). It fails before opening another descriptor at that limit; repeated temporary databases release their registrations. Normal use retains one descriptor per concurrent database inode; the daemon uses only a handful. Reference counting was chosen over lifetime retention to avoid accumulating descriptors across temporary databases, recovery replacements, or many state directories in one process. No dependency, manifest, unsafe code, frozen public type, schema or CLI flag changed.

`Snapshot::open` now performs a trusted stat-only leaf identity check instead of opening/closing an independent file descriptor before its immutable SQLite connection. Snapshot reads still use `immutable=1`; the trust rules are retained.

## Fault introduction and CLI impact

`git log -S preflight_header` and the introducing diff identify commit `2556f8386812933b68a3de49f093bf156451bc99`, dated 2026-10-11, `W1-gate-fix-store: Preserve corrupt database artifacts before SQLite opens`. It added preflight on every existing timeseries database open. The diff is preserved in `library/fault-introduction.patch`.

This is a process-local POSIX lock problem, not specific to the daemon. Any CLI process retaining one Catalog/TimeSeries connection while opening another to the same inode is affected; writable opening also performs an inspection open. CLI calls use the same corrected store path. An independent CLI or sqlite3 client can expose the daemon's lost locks even though that client's own descriptor closes do not release another process's locks. A lone connection's first preflight does not release a not-yet-acquired lock.

## Descriptor audit

Full workspace search evidence is `library/audit-search.txt`. Production call sites and relevant test exceptions were inspected, including store, daemon, scan orchestrator, CLI status/deploy, recovery, backup and trust helpers.

| Path / operation | Finding and disposition |
|---|---|
| `timeseries.rs`, header preflight | Per-open raw descriptor close was the fault; replaced with retained registry and counted connections. Catalog uses the same wrapper. |
| `reader.rs`, `Snapshot::open` | Independent snapshot descriptor removed; stat-only validation retains trust checks. Immutable connection and diff ATTACH URIs use SQLite-managed descriptors. |
| `timeseries.rs`, creation | Exclusively creates an absent database under recovery ownership, closes the creation descriptor before SQLite opens; no existing connection to that new inode. |
| `timeseries.rs`, `backup_v0` | Exclusively creates a new backup, VACUUM INTO completes SQLite's target connection before backup sync/closure; no exposed store connection to the new backup. Parent sync opens a directory. |
| `writer.rs`, snapshot creation/finalization | New temporary is exclusively created and closed before SQLite opens; final sync happens after explicit SQLite close. Published readers are immutable and take no SQLite POSIX locks. Parent sync opens a directory. |
| `catalog.rs`, recovery | Renames artifacts under recovery ownership, then syncs directories; does not open database/sidecar descriptors. Retention unlinks paths and does not open them. |
| `state_dir.rs` | Leaf trust checks use no-follow `statat`; directory traversal opens directories. Store connection registration is the sole mutable-database consumer retaining a leaf descriptor. `writer.lock` and `scan.lock` are separate inodes with their own lease protocol. |
| `tidemark-scan/src/orchestrator.rs:734` | Opens `scan.lock`, not SQLite main/sidecar files. Other production reads are OS/proc metadata. Database connections go through the store. |
| `tidemark-cli/src/deploy/files.rs:472–489` | Database and sidecar admission uses `check_leaf_at` (stat only). `read_state_at` at line 561 is called for deployment markers/JSON records; other reads are config/deployment files. Purge traverses directories and unlinks leaves without opening database contents. |
| `tidemarkd/src/lib.rs:1912`, `firstboot.rs:110`, `services/sampler.rs:35` | Config, `/proc/stat` and fdinfo reads, respectively; no SQLite descriptors. Scheduler and status/report paths open databases through the store. |
| Live/watch, scanner, sys | Metadata scanning does not independently open state database contents. Watch event descriptors are metadata-only; configured Tidemark state is excluded from monitored roots. No backup/recovery SQLite VFS helper outside store was found. |
| Test-only raw reads | Corruption assertions read unopened/rejected databases, immutable snapshots, or preserved artifacts. CLI purge test (`deploy/tests.rs:1397,1408`) reads bytes in the parent while SQLite is held in a separate child process, so it cannot drop that child's locks. The store companion-stat test holds an SHM descriptor to inspect an already-unlinked inode; it closes after the old SQLite connection has closed and cannot release locks on a newly created sidecar inode. |

Audit line references describe the baseline where outside scope; no unowned files were edited. This is a source audit, not a proof against an administrator concurrently replacing trusted state/config files outside the existing recovery/lease protocol.

## Tests and evidence

- `repeated_open_preserves_main_file_locks`: separate Python process uses only `fcntl.lockf`, not SQLite, to request an exclusive lock on 510 bytes at `0x40000002`. It fails while the first connection is open, after second preflight/query, and after second connection closes; it succeeds after both close. Original code fails at the second-preflight assertion (`library/regression-before.log`). The regression was committed first as `a14a825`. Fixed code passes (`library/locks.log`).
- `preflight_checks_replacement_and_accepts_empty_files`: creates and initializes a trusted empty database, retains the connection, replaces the name with a different inode containing an invalid header, and observes `NotADatabase`. The original connection remains queryable. This demonstrates checking the replacement's own header and retaining the old inode's descriptor.
- `sequential_temporary_databases_release_header_descriptors`: opens two connections for each of 300 temporary databases, exceeding the concurrent registry cap over time; all succeed, demonstrating registration cleanup after the last connection.
- All three existing compaction tests pass unchanged with default Python SQLite 3.53.4 (`library/tscompact-default-workspace.log`) and with SQLite 3.46.0 (`library/tscompact-older-workspace.log`, `older-version.log`). They prove external witnesses see completed compaction and restart/cadence behavior remains intact. No assertions were weakened.

Older-client reproduction uses cached `libsqlite3-sys-0.30.1/sqlite3/sqlite3.c`, compiled with `cc -shared -fPIC -O2 -DSQLITE_ENABLE_COLUMN_METADATA -DSQLITE_ENABLE_FTS5 -DSQLITE_ENABLE_RTREE ... -lpthread -ldl -lm` into the attempt's `library/libsqlite3-3.46.0.so`. Run the workspace's actual non-privileged daemon test binary with `LD_PRELOAD=<library>/libsqlite3-3.46.0.so timeout 1500 <binary> services::tswriter::compact_tests --nocapture`; child test processes and Python inherit the preload. Python explicitly reports 3.46.0. The final focused runs use `target/debug/deps/tidemarkd-72517d774a72df5a`, taken from the completed workspace test transcript, and both exit 0 with three passing tests. Earlier package-only focused runs also pass. The daemon continues to use bundled SQLite. No download or new dependency was needed. An initial `cargo test -p tidemarkd tscompact` selected zero tests because the Rust module is named `compact_tests`; it is excluded from acceptance evidence and replaced by the focused binary runs above.

## Validation

All Cargo commands use Rust 1.98.0, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, offline mode, and `timeout 1500`. Detailed command output is in the attempt library. Observed command results:

| Command | Result | Artifact |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 0 | `library/tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; build only | `library/privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `library/doc.log` |
| `tools/ci.sh` | exit 0; cargo-deny absent/skipped | `library/ci.log` |

## Proposed plan text

Append to 05 §4's open-preflight paragraph: “Header reads use a process-wide registry keyed by device and inode. Registered descriptors must not close while any process-local mutable SQLite connection to that inode remains open, because closing an independent descriptor releases POSIX SQLite locks. Each open validates the named inode without opening another descriptor; replacement inodes are checked independently. The implementation bounds concurrent registrations and releases descriptors only after the last associated SQLite connection closes.” This proposal does not remove or relax the existing without-SQLite header requirement.

## Assumptions and remaining uncertainty

- Assigned task scope takes precedence over wave branch conventions; no branches, merges or pushes, and no memory projection edits.
- All production mutable connections to this shared database are created through the internal store wrapper. Introducing a new raw SQLite connection in this crate must also retain a header guard. Existing public APIs do not expose the underlying connection.
- The existing trusted-state ownership and recovery lease protocol continues to govern mutation/replacement. No attempt is made to coordinate an administrator bypassing it.
- SQLite 3.46.0 is the requested older-client witness, not a claim of having tested Ubuntu 3.45 itself.
- No VM was available or used. Guest P4 causality and recovery of previously lost/orphaned-WAL data are not established by these local tests; this change prevents the reproduced lock-loss mechanism.
- No shared-file edits and no dependencies added. No privileged tests executed. Independent verification/integration remains with the coordinator.

## Final scope and cleanup

The regression, implementation, tests and handoff are committed with STORE-locks messages. Only `crates/tidemark-store/**` and this handoff changed; no shared manifest or module declaration was edited. `cargo clean` and a clean-checkout confirmation are captured in the attempt output before submission. Submission is an evidence candidate, not independent verification or release approval.
