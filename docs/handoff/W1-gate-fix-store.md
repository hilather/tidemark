# Handoff — W1-gate-fix-store: preserve corrupt database before SQLite opens it

- **Branch:** hp-0268c5b8d9acdf8980d84b632a2dbd693181713a897cb6843e57bcd8fd3513ec
- **Agent/session:** Codex / attempt-0268c5b8d9acdf8980d84b632a2dbd693181713a897cb6843e57bcd8fd3513ec
- **Date:** 2026-10-11
- **Status:** implemented; evidence candidate awaiting independent verification

## What was built

G5: the shared time-series connection helper now reads at most the first 16 bytes of an existing state database before opening SQLite. A nonempty file without the complete `SQLite format 3\0` header returns the existing SQL error representation with `NotADatabase`. Interrupted reads retry; other I/O errors propagate. Both Catalog and TimeSeries, in read-only and writable modes, use this helper. Empty and missing files retain the existing initialization behavior; valid headers continue into SQLite normally.

The recovery regression now creates the database and both sidecars once, checks their original bytes after failed ordinary opens, and calls `recover_catalog` without reinstalling sidecars. It verifies all three preserved artifacts and the rebuilt catalog. No changes to recovery itself, public contracts, dependencies, or shared files were needed.

## Shared-file edits

None.

## Deviations from the spec

None. Used the existing attempt branch and offline host Cargo cache as instructed. Cargo was absent from the default PATH; validation used the pinned 1.98.0 toolchain with `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`.

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| Garbage is rejected before SQLite touches DB/WAL/SHM | `recover_corrupt_catalog_preserves_database_and_sidecars`: both Catalog and TimeSeries, both open modes, return NotADatabase and leave all three files byte-identical | Observed locally |
| Actual failed-open → explicit recovery preserves all original bytes | Same test checks three recovery paths and their original contents, catalog rebuilt from a snapshot, and repeated recovery namespace | Observed locally |
| Missing/empty files work as before | Existing catalog/sample creation tests; `timeseries_header_preflight_handles_empty_short_and_valid_headers` opens an empty file writable then read-only | Observed locally |
| Short nonempty headers are corrupt | New header test covers one byte, 15-byte SQLite prefix, and garbage; file contents remain unchanged | Observed locally |
| Valid header with damaged page follows SQLite's decision | New header test compares direct SQLite schema-read error with TimeSeries open error for a truncated first page; both report DatabaseCorrupt | Observed locally |

## Tests

Final validation commands (offline environment applied throughout):

- `cargo fmt --check`
- `cargo clippy --offline --workspace --all-targets -- -D warnings`
- `cargo test --offline --workspace`
- `cargo build --offline --workspace --features privileged --tests`
- `tools/ci.sh`

All final commands passed locally. CI skips cargo-deny because it is not installed; that audit remains pending an equipped host. Privileged tests were compiled, not run — pending VM. No new privileged tests are required for this state-file preflight.

An initial damaged-page test incorrectly assumed SQLite must inspect the b-tree during `user_version`; SQLite returned zero. The final test uses a schema read for the direct comparison and passes. Final command transcripts are in this attempt's output `library/final-*.log`.

## Known gaps and follow-ups

- Independent verification/integration pending submission review.
- Privileged execution pending VM; cargo-deny audit pending equipped host.
- Valid-header corruption intentionally remains subject to SQLite's normal behavior, including WAL recovery. The preflight does not promise artifact preservation for valid-header corruption.

## Definition of Done

- [x] Only owned source/test paths and this handoff changed
- [x] Formatting, workspace Clippy/tests, privileged-feature test build, and CI passed locally
- [x] No dependencies or shared-file edits
- [x] Existing owner headers retained; helper is private; no unsafe added
- [x] G5 acceptance criteria evidenced
- [ ] Independent verification and integration
