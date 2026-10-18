# SNAP-staging-nospace

## Needs from coordinator
Nothing. (Cargo needs `PATH=/home/brewerm/.cargo/bin` and `CARGO_HOME=/home/brewerm/.cargo`.)

## Change
- `crates/tidemark-scan/src/orchestrator.rs`: `ScanError::Agg` no longer uses `#[from]`; a manual `From<AggError>` unwraps `Agg(Sink(Store(e)))` through the existing `From<StoreError>` (so `Space` -> `NoSpace`, other store errors -> `ScanError::Store`) and wraps everything else in `Agg` unchanged. `agg/mod.rs` untouched.
- Assumption: `Agg(Sink(Store(non-Space)))` now surfaces as `ScanError::Store` rather than `Agg`; the message is identical (`transparent`).

## Test
`crates/tidemark-scan/tests/staging_nospace.rs` (new): the review scenario (small first scan, 16 MiB cap, 32 MiB private leaf in `scan_started`, 100,000 root annotations on the second request). Asserts `NoSpace`, prior snapshot intact, one failed catalog entry with no file, no `.tmp`. Fails without the fix (verified by stashing it), passes with it.

## Counts
fmt clean; `tidemark-scan --no-fail-fast` all pass (no failures); clippy workspace `-D warnings` clean; `--features privileged --tests` builds (not run).
