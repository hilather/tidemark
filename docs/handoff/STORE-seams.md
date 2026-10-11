# Handoff — STORE-seams: internal snapshot queries and cancellable publication

- Attempt: `attempt-5a4ad39341fa5f5c881310f02480c937ccce53dc4d875c05fc1787a7b0bfa7c3`
- Branch: `hp-5a4ad39341fa5f5c881310f02480c937ccce53dc4d875c05fc1787a7b0bfa7c3`
- Date: 2026-10-11
- Status: implemented evidence candidate; independent verification pending, scan execution blocked by other task's callers.

## What was built

Snapshot retains the file path passed to open and adds these internal signatures:

```rust
pub(crate) fn conn(&self) -> &rusqlite::Connection;
pub(crate) fn file(&self) -> &std::path::Path;
```

The existing immutable/read-only connection flags are unchanged. The documented caller obligations require bound parameters, read-only queries, and detachment of attached databases. Narrow dead-code allowances keep Clippy clean until sibling query owners adopt these seams; no diff/link/export implementation changed.

The writer adds:

```rust
pub fn finish_with(self, final_meta: &SnapshotMeta, cancelled: &dyn Fn() -> bool) -> Result<PathBuf, StoreError>;
```

`finish` delegates with an always-false closure. `StoreError::Cancelled` reports cancellation. The ten checks are before indexes, after each of seven indexes, before final metadata, and after close/file sync immediately before the non-replacing rename. Drop closes/removes unpublished temporary files. No cancellation callback runs after rename; directory sync and the existing publication error behavior remain.

The orchestrator passes an Acquire load of the scan cancellation flag to publication. Store cancellation becomes `ScanError::Cancelled`, and lifecycle completion records a cancelled catalog row with finish time/duration and no published file. Publication cancellation discards both complete and partial candidates, including keep-partial candidates. The common lifecycle finalizer now also records other typed cancellation outcomes as cancelled rather than failed; other errors remain failed.

## Acceptance evidence and tests

- `reader::tests::connection_streams_bound_query_rows`: streams a bound SELECT cursor and rejects writes.
- `reader::tests::attach_snapshot_file_read_only_immutable_and_detach`: binds a percent-encoded URI derived from the second reader's file path (including URI-sensitive filename characters), joins both metadata tables, rejects writes to the attachment, detaches, and reopens the old file.
- `finish_cancels_at_every_publication_checkpoint`: cancels at each of ten checks, asserts no candidate `.db` or `.db.tmp`, and compares an existing snapshot byte-for-byte. An eleventh-check cancellation never fires: successful finish has exactly ten callbacks and readable complete metadata.
- `cancellation_during_publication_is_recorded_and_never_kept`: written in orchestrator's existing in-crate test module. Sets the scan flag after the first index, checks the mapped error and actual catalog row, complete/partial candidate removal, and existing-file preservation. Execution pending caller fixes.
- Existing collision regression calls the new helper signature. Existing lifecycle test now expects cancelled completion for typed cancellation.

## Validation

Used cached Rust 1.98.0 on PATH and a temporary copy of `/home/brewerm/.cargo/registry` at `/tmp/tidemark-store-seams-cargo`, with `CARGO_NET_OFFLINE=true` and Cargo `--offline`. No host cache changes or downloads. Logs and exit codes are in this attempt's worker-output `library/`.

| Command | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline -p tidemark-store --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline -p tidemark-store` | exit 0: 6 unit, 25 storage, 4 W3-A, 4 W3-B, 2 W3-C, 4 doc tests passed; 4 existing scale tests ignored |
| `cargo test --offline -p tidemark-store --features privileged --no-run` | exit 0; VM runtime pending |
| `cargo test --offline -p tidemark-scan --lib` | exit 101: missing `max_record_bytes` in bulk constructor |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 101: unrelated walker callers/patterns |
| `cargo test --offline --workspace` | exit 101: unrelated walker callers/patterns |
| `tools/ci.sh` with offline environment | exit 1: source policy and formatting passed; workspace Clippy blocked |

## Scope, deviations and follow-up

Only owned reader/writer/error/storage/orchestrator files and this handoff changed. No dependencies, shared manifests/module declarations, contracts, schema, plan, memory projections, branches, merges or pushes changed. No new privileged operation is introduced; existing feature-gated store tests compile, runtime remains pending VM.

The baseline still lacks `max_record_bytes` in `tidemark-scan/src/bulk/mod.rs:98`, `tidemark-fs/benches/walk.rs:15`, and `tidemark-fs/tests/w1b/main.rs:15`. The latter test file also has obsolete literal `WalkError::Limit` patterns at lines 599 and 726. These are outside scope and left for their owners. Rerun orchestrator and workspace checks after those changes land. No claim of passing scan tests, workspace CI, VM gate, or independently verified task success is made.
