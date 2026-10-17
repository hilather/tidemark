# SNAP-cold-daemon-fix handoff

## Needs from coordinator
- Socket/daemon tests (api, lifecycle, e2e `identity.rs:31` EINVAL, operator, runtime) fail in my uid-0 no-socket sandbox; none touch files I changed. Please confirm with the uid 1000 gate.
- Nothing else; no manifest change, no spec-change request.

## What changed
- S4: `remove_snapshot` (CLI) and `delete_snapshot` (orchestrator, used by `guard_and_prune` and retention) now unlink `<id>.db`, `<id>.db.zst`, `<id>.db.bad`, `<id>.db.zst.bad` regardless of which the catalog points at; only the selected scan's leaves.
- S5: `entry_sizes` is infallible. Missing file or unreadable cold header gives unknown sizes (text `-`, JSON `null`) plus a stderr warning; list exits 0. If the file vanished, the catalog row is re-read once (`refreshed`).
- S6: `xtask/tests/w6c.rs` strips the two commented cold-default lines from the current packaged config before deriving the historical fixture; the historical scenario is kept, packaged comments untouched.
- N1: `docs/plan/15-snapshot-v2.md` §2 now says deleted scans' `.bad` leaves go with them, retained scans' are never auto-removed, and fixes the stale list/prune sentences.

## Tests (fail before, pass after)
- `snapshots_cold`: `prune_removes_both_published_forms_in_both_pointer_directions`, `list_survives_missing_hot_file_and_corrupt_cold_header` (both failed on the baseline sources, pass now).
- `cold_retention`: `automatic_deletion_removes_both_published_forms_in_both_pointer_directions` (passes; the pre-fix run stopped at the CLI failure first, so this one was not run against the baseline).
- `w6c upgrade_fixture_options_and_drift` passes (failed at baseline per review).

Before: prune exited 0 listing the id but two rows remained; list exited 1 with empty stdout. After: no file or row remains; list exits 0 with `-`/`null`.

## Assumptions
- Unknown sizes in text use `-` (was `unknown` for sizes); diagnostics go to stderr via `eprintln!` in CLI code.

## Test counts
Workspace run: 85 suites ok; failures only in the socket/daemon tests listed above (environmental). fmt and clippy `-D warnings` clean.

## Gate results
- Privileged build and `RUSTDOCFLAGS='-D warnings' cargo doc` clean.
- `tools/ci.sh` reached the test step and failed on 8 targets, all in this sandbox's environment (tidemark-cli e2e/rules, tidemark-fs rules, tidemarkd lib/w6a1-5). I did not run them on the baseline, so "pre-existing" is not demonstrated; the failing tests are socket/identity based and none of my touched code is on their path.
