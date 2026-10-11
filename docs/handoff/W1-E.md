# Handoff — W1-E: Snapshot store, catalog, time series, retention, space guard

## Needs from coordinator

- Attach the centrally declared globset workspace dependency to tidemark-store, then assign the remaining glob boundary filter. Snapshot::link_sets_under implements ordinary/incomplete/depth queries; nonempty boundary globs still return StoreError::NotYetImplemented.
- Decide [SCR-1](scr-W1-E/SCR-1.md): the existing API cannot explicitly recover a corrupt shared sample/catalog database. Ordinary opens return the SQLite error; recovery must preserve sample history before creating a replacement.
- Arrange VM execution for the two storage tests below. Cargo-deny auditing remains pending an equipped host.

- **Branch:** hp-fa19a3406b85cff64308e49bb69e0c0447991fe1ce1d429dcdb9832854d406af
- **Agent/session:** attempt-fa19a3406b85cff64308e49bb69e0c0447991fe1ce1d429dcdb9832854d406af
- **Date:** 2026-10-11
- **Status:** implemented core modules; partial deliverable with the two functional gaps above. This is local evidence, not independent verification or integration approval.

## What was built

SnapshotWriter bulk-loads the exact v1 tables in transactions of at most 50,000 rows, checks signed SQLite bounds, applies periodic caller-provided space checks, creates indexes and metadata, closes and syncs, publishes with RENAME_NOREPLACE, and syncs the directory. Temporary files are removed on abort/drop/error; an insert error poisons the writer so an incomplete transaction cannot be published. Snapshot reads through a byte-preserving immutable URI, reconstructs all metadata keys, checks unsigned widths, and implements every query family in §3; boundary globs are the explicit exception.

Catalog and TimeSeries share the exact timeseries.db schema, with WAL, NORMAL synchronization, a five-second busy timeout, and a numbered transactional initial migration. An existing v0 database receives a synced SQLite VACUUM INTO backup before migration. Catalog selection excludes partial snapshots unless explicitly selected; healthy-catalog rebuild atomically replaces records from valid published snapshots and skips corrupt snapshot files. Sample inserts, queries, capped directory history, and last-value gauge downsampling are implemented. Retention is pure per-root UTC calendar/ISO-week selection. Space measurement uses O_NOFOLLOW/O_PATH directory-relative handles, O_NOATIME with EPERM fallback, and fdinfo mount ids to skip bind mounts and other filesystems without reading target file contents.

## Shared-file edits

None. No manifests, lockfile, lib.rs, core contracts, plan documents, or another task's files changed. No dependencies were added.

## Deviations from the spec

| Spec reference | Implementation/assumption | Reason / follow-up |
|----------------|---------------------------|--------------------|
| 05 §3 LinkFilter.boundary | Nonempty globs return NotYetImplemented; conflicting globs/depth return validation error | Matcher dependency is not attached; no shared-file workaround made. Depth-region crossing filtering works. |
| 05 §4 corrupt timeseries fallback | Missing DB is initialized and healthy DB can rebuild; corrupt DB is refused | Explicit recovery API proposed in SCR-1, preserving historical sample data. |
| 05 §6 dir_series depth | Caller supplies rows filtered through series_depth; method keeps largest 20,000 with raw-path ties | Frozen signature carries no policy/depth argument. |
| 05 §6 queries | Range requests raw through 48h, five-minute through 35d, otherwise hourly; unavailable finer tiers fall back to stored history and newer finer samples remain visible | Resolution thresholds otherwise unspecified; mixed-tier history tested. |
| 05 §6.1 expiry | Raw/five-minute expiration rounds down to full destination buckets | Retains at most 299/3599 extra seconds so INSERT OR IGNORE cannot freeze an unfinished bucket. |
| 05 §7 dates and partial age | Days/weeks use UTC; failed/partial age uses finished_at, falling back to started_at | No timezone in frozen signature. ISO-week buckets begin Monday. |
| AGENTS bounded results | Collection stops at 1,000,001 rows and errors above documented 1,000,000-result cap | Frozen list/link/sample methods lack result limits. Never silently truncates. |
| 05 §8 pruning | Guards measure/refuse; caller prunes and installs writer callback | Frozen guard signatures have no pruning callback; writer create has no retention policy. |
| Build environment | Rust 1.98.0; offline host registry copy in /tmp/tidemark-w1e-cargo | Required sandbox arrangement; host cache unchanged. |

State allocation counts allocated blocks per entry, including directory/symlink inodes, conservatively counting hard-linked state files more than once. Traversal depth is capped at 4096 retained directory fds plus one entry fd; fdinfo reads are capped at 16 KiB. Newly created state/snapshot directories use 0700; database and backup files use 0600. Root ownership comes from the required root runtime; no chown or host configuration changes occur.

## Acceptance criteria evidence

| Criterion | Evidence | Local status |
|-----------|----------|--------------|
| Both schemas match 05 DDL | schema_matches_document_sqlite_master independently executes document DDL and compares every non-internal sqlite_master object | Passed |
| 100k directory/file/link round trip by id/path/children/subtree | roundtrip_100k_dirs_and_all_query_families | Passed |
| Drop/abort cleanup, failed inserts cannot publish, existing id preserved | drop_abort_collision_and_failed_write_never_publish | Passed |
| Immutable reader ignores another process's lock | immutable_reader_opens_under_another_process_exclusive_lock; same-process special-byte URI test | Passed |
| Byte roots/names/file paths; absent FileHit.path preserved | Round trip, catalog non-UTF8 test, slug injection test | Passed |
| Checked SQLite conversions in both directions | reader_rejects_corrupt_unsigned_values_and_versions; writer and transactional sample overflow tests | Passed |
| Every §3 query and rustdoc examples | Snapshot examples call every query family; plain/incomplete/depth links work | Partial: boundary globs unimplemented |
| Catalog selection, absent DB and healthy rebuild | catalog_selection_rebuild_and_nonutf8_file_paths | Passed; corrupt shared DB recovery pending SCR-1 |
| Sample inserts, queries, migration backup, RO opens | samples_roundtrip_checked_inserts_and_readonly_opens; migrations_backup_existing_v0_and_refuse_future_schema; mixed-resolution test | Passed |
| Idempotent compaction; last gauge per bucket | compaction_last_values_idempotence_and_incomplete_buckets checks fs/lvm/pg, repeat counts, partial buckets and expiry | Passed |
| Largest 20,000 directory history rows | series_keeps_largest_20000_rows_with_byte_path_ties | Passed; caller filters depth |
| DST and ISO-year retention cases | retention_utc_days_dst_and_iso_weeks_per_root | Passed |
| Space decision table and no symlink traversal | space_guard_decision_table_and_live_state_measurement; private allocation/symlink test | Passed |
| 2M directory inserts below 60s | Release scale test: 16.164796107s; including finish: 26.11s | Passed locally |
| VM ownership/publication and bind-mount skipping | Feature-enabled workspace test build | Compiled; pending VM |

## Validation commands

All final commands below exited 0. Logs and exit files are in this attempt's worker-output/library directory. The final rerun includes backup sync, NUL URI rejection, bounded compaction bucket search, preservation of snapshot tool_version during rebuilding, and in-memory SQLite sorting scratch.

| Command | Evidence |
|---------|----------|
| cargo fmt --check | library/fmt.log and fmt.exit |
| cargo clippy --offline --workspace --all-targets -- -D warnings | library/clippy.log and clippy.exit |
| cargo test --offline --workspace | library/test.log and test.exit: 112 passed, 0 failed, 2 scale tests ignored, including doctests |
| cargo build --offline --workspace --features privileged --tests | library/privileged-build.log and privileged-build.exit |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | library/doc.log and doc.exit |
| tools/ci.sh with CARGO_NET_OFFLINE=true | library/ci.log and ci.exit: policy/owner scan, fmt, clippy and tests pass; cargo-deny explicitly skipped because unavailable |
| git diff --check | Passed before commit |
| Scope and clean working tree | Recorded in report/submission evidence after commit |

No privileged test was executed. No mount, LVM, service, or host configuration operation was run.

## Tests added

Unprivileged unit tests:

- reader::tests::metadata_fixture_remains_constructible (retains use of W0 test metadata helper).
- space_guard::tests::state_usage_counts_allocation_without_following_symlinks.

Unprivileged integration tests in tests/storage.rs:

- roundtrip_100k_dirs_and_all_query_families
- drop_abort_collision_and_failed_write_never_publish
- schema_matches_document_sqlite_master
- immutable_reader_ignores_exclusive_sqlite_lock_and_uri_special_bytes
- reader_rejects_corrupt_unsigned_values_and_versions
- root_slugs_are_injective_including_bytes
- catalog_selection_rebuild_and_nonutf8_file_paths
- retention_utc_days_dst_and_iso_weeks_per_root
- space_guard_decision_table_and_live_state_measurement
- samples_roundtrip_checked_inserts_and_readonly_opens
- migrations_backup_existing_v0_and_refuse_future_schema
- compaction_last_values_idempotence_and_incomplete_buckets
- depth_boundary_selects_crossing_sets_and_no_instances_is_honest
- series_keeps_largest_20000_rows_with_byte_path_ties
- mixed_sample_resolutions_keep_old_and_recent_history_for_short_ranges
- exclusive_lock_holder_child (child-process lock fixture)
- immutable_reader_opens_under_another_process_exclusive_lock

Scale: insert_2m_directory_rows_under_60_seconds, ignored with reason "scale"; explicitly executed in release mode and passed.

Privileged integration tests, feature-gated only:

- snapshot_xfs_publication_owner_permissions_and_sync — pending VM.
- state_usage_skips_same_device_bind_mount — pending VM.

Two compile-checked Snapshot rustdoc examples cover opening and the reader query surface.

## Measurements

Command: cargo test --offline -p tidemark-store --release insert_2m_directory_rows_under_60_seconds -- --ignored --nocapture.

2,000,000 generated DirRows in batches of 10,000: insertion including generation took **16.164796107 seconds**; total test including indexes/publication took **26.11 seconds**. Linux 7.2.5-3-omarchy, x86_64, Intel Core i7-8750H @ 2.20 GHz, shared sandbox. This is not a Rocky VM measurement. The measured writer was unchanged by subsequent reader/migration/compaction hardening.

## Known gaps and follow-ups

- Glob boundaries remain NotYetImplemented; provision dependency and finish filter tests.
- Corrupt timeseries.db recovery remains unimplemented; SCR-1 is pending, not approved.
- Only schema version 1 exists in this baseline. Undefined pre-v1 snapshots are refused; no v0 schema is invented. Future previous-version reader work needs a documented schema.
- Caller must install periodic writer space checks, filter dir_series by series_depth, and perform prune/retry before a scan.
- Privileged runtime/platform acceptance and cargo-deny auditing remain pending.

## Notes for the reviewer

Frozen public signatures and ScanEntry fields are retained. No unsafe code, async runtime, system clock calls, target file-content reads, metadata writes to scanned roots, or new dependencies were introduced. Publication uses safe rustix renameat2 with NOREPLACE. Catalog database errors and duplicate scan ids during rebuilding propagate and roll back; corrupt individual snapshots are logged and skipped. Rebuilding retains each snapshot metadata tool_version. Non-UTF8 catalog file paths are stored as SQLite TEXT bytes without lossy conversion. Metadata root is hex of original bytes; absent file-hit paths remain None.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits
- [x] Public items documented; owner headers retained
- [x] Required local checks exited 0 offline
- [x] Scale measurement recorded
- [x] Privileged tests written and compiled
- [ ] Privileged runtime acceptance — pending VM
- [ ] Functional acceptance — glob boundaries and corrupt-DB recovery remain
- [ ] Cargo-deny audit — unavailable
- [ ] Independent verification/integration — coordinator responsibility
