# SNAP-export-sqlite-fix — handoff

## Needs from coordinator
- Nothing blocking. Privileged tests: none added; `--features privileged` workspace tests build.
- `state_dir::tests::readable_storage_warns_once` was not run (known sandbox failure).

## Findings → change → test (all "fails before" confirmed against the base sources)
| Finding | Change | Test |
|---|---|---|
| B1 | Staging dir is `mkdir` (0700) under a random name, retried on `EEXIST` (an existing entry is never touched); dev/ino recorded and cleanup only removes a directory with that identity | store `b1_staging_never_touches_a_preexisting_entry` (failed before) |
| S1 | CLI resolves the target with `std::path::absolute` before publishing and prints that path | cli `s1_json_output_after_publishing_a_relative_destination` (failed before) |
| S2 | New `export_sqlite::export_sqlite_cancellable(snap, out, &cancel)` polls between batches → `Io(Interrupted)`; CLI registers SIGINT/SIGTERM/SIGHUP flags (signal_hook, as `scan` does) before opening the snapshot, so drops unwind and remove staging and the cold-inflate leaf; exit 3 (same as scan's interrupted/partial) | store `s2_cancellation_stops_and_leaves_nothing`; cli `s2_termination_signals_leave_no_temporary_copies` (child process + each signal; failed before) |
| S3 | v2 file rows are flushed inside the per-file callback at `BATCH` | no direct test (needs a >10k-file directory; covered by existing v1/v2 equality test) |
| S4 | **Export them.** All stored instances are streamed (v1: `ORDER BY ino,dir_id,name,rowid`; v2: new `Snapshot::v2_for_each_link_instance`), independent of `link_sets`; matches v1 leftover behaviour | store `s4_v2_instances_without_link_sets_are_exported` (failed before) |
| S5 | Same streaming helper (`pub(crate)`, chunk by chunk, uncapped); public `link_instances` cap unchanged | store `s5_export_is_not_limited_by_the_query_cap` (1,000,001 instances; failed before; asserts public query still capped) |
| S6 | `open_snapshot` errors go through `export_error`; `ENOSPC`/`EDQUOT` and store `Space` → exit 5; SQLite `DiskFull` is classified as `Space` in the store | cli `s6_space_floor_refusal_exits_5` (cold + `min_free_pct=100`; failed before). SQLITE_FULL / EDQUOT mapping is by inspection only (no disk-full injection) |
| S7 | Export validates strictly increasing dir ids (`Invalid`); writer's `debug_assert` is now an `Invalid` error | store `s7_non_increasing_directory_ids_are_an_error_not_a_panic`, `s7_writer_rejects_non_increasing_directory_ids` (failed before) |
| N1 | `tidemark-snapshots-export.1` added to `packaging/rpm/rpm.toml` | — |

## Assumptions
- Interrupt exit code is 3 (`CliError::Partial`), consistent with `scan`.
- A signal during cold inflation is noticed after the inflate completes (no cancel hook in the reader), then the export stops before creating staging.
- Staging cleanup has a tiny dev/ino check→remove window; acceptable since the name is random and 0700.

## Counts
- Files changed: 8 (+ this note). Store export tests 4 → 10, CLI export tests 3 → 6.
- Checks: fmt ok; `tidemark-store` all targets pass; cli `snapshots_export`(6) `surface`(13) `snapshots_cold` pass; clippy `-D warnings` clean; privileged build ok; rustdoc `-D warnings` ok.
