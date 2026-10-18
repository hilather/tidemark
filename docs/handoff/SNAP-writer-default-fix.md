# SNAP-writer-default-fix

## Needs from coordinator
Nothing. (Cargo needs `PATH=/home/brewerm/.cargo/bin` and `CARGO_HOME=/home/brewerm/.cargo` in this sandbox.)

## Changes
- `crates/tidemark-scan/src/orchestrator.rs`: `ScanError::Store` no longer uses `#[from]`; a manual `From<StoreError>` maps `StoreError::Space(_)` to `ScanError::NoSpace` (message logged via `tracing::warn!`), all else to `Store`. `finish_snapshot` falls back to that conversion. CLI then exits 5 and the daemon sees NoSpace.
- `crates/tidemark-scan/tests/conversion_nospace.rs` (new): review20 scenario; fails before the fix, passes after.
- Fixtures writing v1 from scan-produced meta now set `schema_version = 1`: `crates/tidemark-cli/tests/w3b.rs` (two sites: ~72 and ~232), `w3c.rs` (~134). Other `SnapshotWriter::create` users (w2c, w7a) build their own meta and were left unchanged.

## Tests
fmt clean; `tidemark-scan --no-fail-fast` all pass; `w3b` 2 passed/1 ignored, `w3c` 3 passed; clippy `-D warnings` clean; `--features privileged --tests` builds (not run).

## Assumptions
The ScanError message for NoSpace stays "NoSpace"; the store's reason text is only logged.
