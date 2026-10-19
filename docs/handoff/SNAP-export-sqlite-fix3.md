# SNAP-export-sqlite-fix3 — handoff

## Needs from coordinator
- Nothing blocking. No privileged tests added; `--features privileged` workspace tests build.
- The OS errno still cannot be read from SQLite (`unsafe` is confined to `tidemark-sys`; not in scope). Classification is therefore: `SQLITE_FULL` → space; `IOERR_WRITE/FSYNC/TRUNCATE` → space only if a 1 MiB write probe in the staging directory fails with `ENOSPC`/`EDQUOT`, **now run while the failed writer's `.db.tmp` still exists**. A real `sqlite3_system_errno` would be stronger; ask for a safe wrapper in `tidemark-sys` if wanted. No real quota run here.
- `state_dir::tests::readable_storage_warns_once` skipped (known sandbox failure).

## Finding → change → test
| Finding | Change | Test |
|---|---|---|
| S1 | `SnapshotWriter` carries `WriterHooks.classify`; `finish_with` runs its steps in `finish_steps` and applies the hook to any error *before* `self` (and `.db.tmp`) is dropped. The copy phase is classified in `export_inner` while the writer is still in scope. The after-cleanup probe is gone. | `fix3_s1_write_failure_is_classified_before_cleanup_frees_space`: injected `IOERR_WRITE` at `copy` and `finish`; the probe reports `EDQUOT` only while a `.db.tmp` exists (quota that admits a small probe once space is freed) → `Space`; a healthy-directory probe keeps `Sql`. Failed before (probe ran after unlink → exit 1). |
| S2 | `SnapshotWriter::create_with(dir, meta, hooks)` classifies initialisation failures (open, PRAGMAs, schema) via the same hook before the temp file is removed. | `fix3_s2_space_failure_during_writer_creation_is_space` (`SQLITE_FULL`, and `IOERR_WRITE`+quota probe, at `create`) → `Space`, no leftovers. |
| S3 | v1 instances page with `WHERE ino=?1 AND rowid>?2 ORDER BY rowid LIMIT ?3` within the current inode, then `WHERE ino>?1 ORDER BY ino,rowid LIMIT ?2` to fill the page from later inodes. Output order/content unchanged (`(ino,rowid)`). | unit `s3_v1_instance_pages_seek_without_temp_btree` (plans use `li_ino (ino=? AND rowid>?)` / `(ino>?)`, no TEMP B-TREE); `s3_single_inode_paging_is_linear` (30,001 rows of one inode: ~260k VM steps at 10,000/page and ~270k at 500/page, i.e. ≈8.7 steps/row; asserted ≤10 steps/row); `s3_pages_keep_ino_rowid_order`. |

Test hooks: `export_sqlite::ExportFaults` / `export_sqlite_with_faults` (`#[doc(hidden)]`); production paths use defaults.

## Assumptions
- Probe size/heuristic unchanged (1 MiB); only its timing moved.
- Cancellation behaviour from fix2 is untouched (`Cancelled` is never reclassified).

## Counts
- Files: `writer.rs`, `export_sqlite.rs`, `tests/export_sqlite.rs`, this note (CLI files unchanged — `export_error` already maps `Space` → exit 5).
- Checks: fmt ok; store lib 63 pass (1 skipped), all 14 integration targets pass (export_sqlite 13); cli `snapshots_export` 6 + `snapshots_cold` 3; clippy `-D warnings` clean; privileged build ok.
