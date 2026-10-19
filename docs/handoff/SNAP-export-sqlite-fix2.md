# SNAP-export-sqlite-fix2 — handoff

## Needs from coordinator
- Nothing blocking. No privileged tests added; `--features privileged` workspace tests build.
- S3 uses a write probe (below) because errno cannot be read from SQLite safely; a real EDQUOT/ENOSPC run (VM or the review's preload injection) would confirm end to end. Not run here.
- `state_dir::tests::readable_storage_warns_once` skipped (known sandbox failure).

## Finding → change → test
| Finding | Change | Test |
|---|---|---|
| S1 | Export calls `writer.finish_with(&meta, cancel)` (writer's checkpoints after each index and before its rename now honour the export's callback), then `check(cancel)` immediately before the outer `renameat2`; `StoreError::Cancelled` is mapped to `Io(Interrupted)` (CLI exit 3). After the rename the export succeeds. `writer.rs` unchanged. | store `s1_cancel_during_finalization_before_publication` (v1 hot, v2 hot, cold; trips (a) once the last index `li_dir` exists in the staged db, (b) once the writer's db is renamed into staging, i.e. only the outer check can stop it; asserts Interrupted and an empty output dir). Failed before (export returned Ok). |
| S2 | v1 instances are read in keyset pages `WHERE (ino,rowid)>(?,?) ORDER BY ino,rowid LIMIT 10000`, served by `li_ino` (index entries are `(ino,rowid)`); cancellation is checked between pages. The v1 schema/writer require no order (only `li_ino`/`li_dir` indexes; rows are plain inserts), so order within an inode is source insertion order instead of `(dir_id,name)`; no sorter exists. v2 (already chunked) unchanged. | unit `s2_v1_instance_page_needs_no_temp_btree` (`EXPLAIN QUERY PLAN` on the exact SQL: no `TEMP B-TREE`, uses `li_ino`); existing 1,000,001-instance `s5` still passes. |
| S3 | `classify_with`: `DiskFull` → Space (as before); `SQLITE_IOERR_WRITE`(778)/`FSYNC`(1034)/`TRUNCATE`(1546) → Space only if the OS error is `ENOSPC`/`EDQUOT`. The store is `forbid(unsafe_code)` so `sqlite3_system_errno` is unavailable; instead, while the staging dir still exists, the failure triggers a 1 MiB write+fsync probe in it and the probe's errno is used. Other IOERR codes, or a probe that succeeds/fails with another errno (e.g. EIO), keep the original error. CLI already maps `Space` → exit 5. | unit `s3_quota_and_enospc_io_errors_are_space`, `s3_unrelated_io_errors_are_not_space`, `s3_probe_reports_no_errno_on_a_healthy_directory` (synthetic extended codes; probe injected). Failed before by construction (IOERR was never Space). |

## Memory bound (S2)
Source side: one page of ≤10,000 rows (BATCH) in the Rust buffer plus SQLite page cache; no sort, independent of link-set size. Writer commits ≤50,000 rows per transaction.

## Assumptions
- Order of exported `link_instances` rows within an inode is not part of the v1 contract (readers sort their own queries; tests compare sorted).
- Probe heuristic: a transient quota condition that clears before the probe would leave exit 1; acceptable.

## Counts
- Files changed: 2 source/test (+ this note); store tests 11 export (was 10) + 4 new unit tests. CLI files untouched.
- Checks: fmt ok; store lib (61) and all 14 integration targets pass individually; cli `snapshots_export` 6 + `snapshots_cold` 3 pass; clippy `-D warnings` clean; privileged build ok.
