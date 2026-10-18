VERDICT: READY
BLOCKER 0 / SHOULD-FIX 1 / NOTE 0

Reviewed candidate `d8c4b433c14650330ee9b0d49426a457032385e7` against base `61abc4c1ab83190276c2b4dd609507823dbdfe27`. The supplied integration checkout `8147309bebdec633e84a4c4a0294c2a3c4580279` has the identical tree, `d3108cfaacf9d1986b4c5bb6a13615d6b485c6fd`; both requested diffs are identical. Read review20, the fix handoff and the relevant storage/interface/v2 contracts. READY means no demonstrated blocker in this fix; the explicitly requested audit of other space-refusal paths found the preexisting SHOULD-FIX below.

**SHOULD-FIX: staging-row space refusal still bypasses the NoSpace conversion.** Reference: `finding:writerfix-staging-nospace`.

Location: the new conversion at `crates/tidemark-scan/src/orchestrator.rs:1447` handles direct `StoreError` values only; `written?` at line 570 still uses `ScanError::Agg` at line 671. At the 100,000-input-row checkpoint, `writer_v2.rs:206` returns `StoreError::Space`. `WriterSink` converts this to `SinkError::Store`, and `PreparedAggregation::write` converts it to `AggError::Sink` (`agg/mod.rs:575–601,691,699`). The new `From<StoreError>` is never called.

Reproduced through the unchanged public `run_scan_with_clock` API: complete a small first scan, set `max_state_size` to 16 MiB, and have `scan_started` allocate a private 32 MiB state leaf after the second scan's preflight. Add 100,000 root annotations to the second request to exercise the real input-row checkpoint without a large filesystem fixture. The candidate returns:

```text
Agg(Sink(Store(Space("maximum state size would be exceeded"))))
```

The same probe returns the same nested error against the base: this is a remaining gap, not a new regression. Any sufficiently large scan emitting 100,000 rows can take this path when state usage exceeds its cap after preflight. Existing dispatch maps this to CLI Runtime/exit 1 (`tidemark-cli/src/cmd/scan.rs:291–304`, `src/lib.rs:60`) and skips the daemon's `StateSpaceRefused(true)` branch (`tidemarkd/src/scheduler.rs:895–901`). Those downstream effects are established by source inspection, not a live daemon fault injection. Both probes preserve the prior snapshot, record one failed catalog entry with no file, and leave no `.tmp` files. Normalize the nested storage-space error at the scan boundary and add an input-row regression; the new small conversion test does not reach this checkpoint.

**The original conversion-time finding is fixed.** `finish_snapshot` at `orchestrator.rs:1465–1472` now routes both conversion callback refusals and conversion-headroom `StoreError::Space` through the new mapping, preserving its cancellation and publication-collision branches. The candidate's exact `conversion_nospace.rs` test passes; compiled unchanged against base libraries, it fails at its NoSpace assertion on line 82 (exit 101). A diagnostic probe confirms the base result is `Store(Space(...))` and the candidate result is `NoSpace`, with cleanup assertions passing in both. The existing CLI mapping then selects exit 5, and the daemon matches its state-space refusal branch. A conversion-time error occurs after scan-start acknowledgement; this does not imply a synchronous API refusal.

**The CLI fixture changes preserve their intent.** `w3b.rs:71,232` explicitly marks metadata v1 for the existing v1 writer, retaining unnamed link instances and the live/gone, replacement and scale-test assertions. `w3c.rs:133–138` passes matching v1 metadata to both create and finish while preserving the deliberately malformed `dir_disk=1, own_charged=0` row. That row still reaches the export accounting check after header output (`export_ncdu.rs:64,104–106`). Successful scan/export/wide paths continue to use v2. This follows `15-snapshot-v2.md` §9 and the unchanged writer schema checks; it does not relax product validation. No additional defect was demonstrated in the changed code.

Validation used Rust 1.98.0 and unchanged production sources:

- `cargo test --offline -p tidemark-scan --lib --tests`: exit 0; 90 passed, 4 ignored. Includes conversion refusal, publication cancellation/collision, real v2 scans, cold retention and crash recovery.
- `cargo test --offline -p tidemark-cli --test w3b --test w3c`: exit 0; w3b 2 passed / 1 ignored, w3c 3 passed.
- Exact candidate conversion regression linked against an unchanged archive of the base: failed as expected, exit 101. External diagnostic probes against both trees confirmed the conversion difference and the shared staging defect.
- `cargo clean --offline`: exit 0; removed the candidate/base builds and base archive (5.1 GiB).

Supporting artifacts are in this attempt's worker-output `library/`: `public_scan_space_probe.rs`, `probe-method.txt`, `candidate-probes.log`, `base-regression.log`, the cargo logs, `test-summary.json` and `validation-context.json`. Candidate probe log SHA-256: `96cf9acf62f37ff76da6820fa364161e5876fe29dc39e89db665342c6031382f`; base regression/probe log SHA-256: `2059529a32cf17a88017381c2f168a6c4b31571da6c4d57921c438b2a3718d02`.

No privileged commands, VM tests, physical ENOSPC experiment or ignored scale tests were run. Full-workspace CI, clippy and privileged-feature compilation were not repeated. No production or repository test code was edited. This report is review evidence, not release or integration approval.
