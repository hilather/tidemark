# SNAP-writer-default handoff

## Needs from coordinator
Nothing outside scope must change. `tidemarkd/tests/w6b/fixtures.rs` (not mine) deliberately writes v1 (`schema_version: 1`) and still passes.

## What changed
- `orchestrator.rs`: `create_writer` now builds `SnapshotWriterV2::create_with_options` with `plain_depth = max(series_depth, 2)` (retry loop unchanged); `WriterSink`/`finish_snapshot`/unit tests use `SnapshotWriterV2`; `SnapshotMeta.schema_version` = 2; first-scan estimate `dirs_estimate * 64` (previous-snapshot `5/4` branch kept).
- New `tests/writer_default.rs`: real scan of a 4-deep tree publishes a file with header `user_version` 2 and meta `schema_version` 2; `dir`, `dir_by_path`, `children` (Disk/Apparent) equal those of a v1 snapshot built with `SnapshotWriter` from the same rows.
- `docs/operator-guide.md`: one paragraph.
- `cold_retention.rs`, `crash_recovery.rs`, `state_trust.rs`, `w6b.rs` needed no change; they pass with v2 output (the cold tamper at byte 80 still changes a hot-file byte and the test passes).

## Assumptions / gaps
- `plain_depth` meta value is not asserted: tidemark-scan has no rusqlite dependency and `Cargo.toml` is out of scope. It is passed straight from `max(series_depth,2)`.

## Tests (before: scan tests that fed v1 writer a v2 meta would fail to compile/ run; now pass)
- tidemark-scan: lib 41 pass; cold_retention 4; crash_recovery 2; state_trust 1; writer_default 1; w2a 14; w2b 21; w2c 5.
- tidemark-store lib 58 pass; test targets cold, compaction_crash, crash_recovery, diff_walk, fuzz_smoke, locks, reader_v2, state_trust, storage, w3b pass. tidemarkd w6b 15 pass.
- fmt, clippy `-D warnings`, `--features privileged --tests` build, rustdoc `-D warnings`: clean.
