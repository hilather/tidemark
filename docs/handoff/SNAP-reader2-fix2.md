# Handoff — SNAP-reader2-fix2

## Needs from coordinator

Nothing blocking. Review S2/S3/N2 closure; no shared files, dependencies or contracts changed.

## What changed

- `crates/tidemark-store/src/reader_v2.rs`
  - S2: both chunk-coordinate queries (single fetch `locate`, projected `projected_chunks`) now select
    `CASE WHEN typeof(data)='blob' THEN length(data) END, typeof(data)`. `chunk_ref` returns
    `Invalid("... dir_chunks.data is not a blob")` for any non-blob type without ever reading `data`.
    Malformed rows are still returned (not filtered), so gaps/overlaps remain detectable. Only `dir_chunks`
    uses this path in the reader (`file_chunks`/`li_chunks` are not read by `reader_v2.rs`).
  - S3: `Backend::open` maps `InvalidColumnType`/`FromSqlConversionFailure` on the `plain_depth`/`hash_width`
    meta read to `Invalid("meta key <key> is not text")`; other SQLite errors stay `Sql`.
- `crates/tidemark-store/tests/reader_v2.rs`: four new tests (below).
- `crates/tidemark-scan/tests/cold_retention.rs`: N2 test rewritten.

## Tests that failed before and pass now

| Test | Before (old reader) | After |
|---|---|---|
| `s2_text_chunk_is_rejected_without_reading_payload` (+ child `s2_child_open_and_query`) | child `VmHWM 140696 KiB` > 40 MiB limit, FAILED | passes (child asserts `VmHWM` < 40 MiB; 64 MiB TEXT payload) |
| `s3_wrong_typed_v2_meta_is_invalid_naming_key` (plain_depth/hash_width × `x'32'`/NULL) | `Err(Sql(InvalidColumnType(0, "value", Blob)))` | `Invalid` naming the key |
| `automatic_deletion_removes_both_published_forms_in_both_pointer_directions` | fails on pre-fix orchestrator (`1e69b02^`, temporarily swapped, not committed) at the both-forms assertion | passes |
| `s2_non_blob_chunk_types_are_invalid_naming_table` (TEXT/int/real) | passes before and after (regression guard; the pre-fix rejection happened after allocation) | passes |

N2 fixture: after compressing the oldest scan (and, for the cold-pointer direction, pointing the catalog at
it), one reserved byte (offset 80) of the hot SQLite header is changed. Hot and cold both stay valid but
differ, so `cleanup_abandoned_scans` preserves both (asserted); the retention deletion then must remove both.
NULL cannot be stored in `dir_chunks.data` (NOT NULL), so it is not tested there.

## Assumptions

- Child process = the test binary re-executed with `--exact` and `TM_TEXT_CHUNK_FILE`; the child test is a
  no-op when the variable is unset. `VmHWM` is Linux-only (the project targets Linux).
- The S3 NULL case rebuilds `meta` without NOT NULL in the fixture only.
- Writing a header byte in the N2 test requires no new dev-dependency (rusqlite is not a dev-dep of scan).

## Validation

fmt, clippy `-D warnings`, `cargo build --workspace --features privileged --tests` (compile only), and
`RUSTDOCFLAGS='-D warnings' cargo doc` clean. Test counts: see the final section of this note.

## Test counts

`cargo test --workspace --no-fail-fast` in this sandbox (uid 0, no sockets, `/` owned by 65534):
1090 passed, 25 failed, 14 ignored. All 25 failures are environmental and outside the owned crates
(tidemarkd/CLI socket and daemon tests, `tidemark-fs` `foreign_directory_fallback_count`,
CLI e2e `regular_contents_are_never_read` EINVAL, "socket ancestor / has untrusted owner 65534").
Every `tidemark-store` and `tidemark-scan` test passes (`reader_v2` 22, `cold_retention` 4).
I did not run these on the baseline to compare, and `tools/ci.sh` was not run because it repeats the same
test step; the coordinator gate (uid 1000 with sockets) should confirm.
