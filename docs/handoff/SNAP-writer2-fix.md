# SNAP-writer2-fix — v2 staging and conversion fixes

## Needs from coordinator

- Independent review, uid 1000 / umask 022 / socket-enabled checks and GitHub ubuntu-24.04 validation remain coordinator gates.
- Integrate SNAP-cold-fix's abandoned-scan cleanup for `<scan>.db.stage.tmp`; this task deliberately does not modify `catalog.rs`.
- The frozen callback is `FnMut() -> SpaceVerdict`, with no estimate argument. Review the preserved callback policy plus independent estimated conversion-headroom check described below. Passing this estimate into a configured-policy callback would require a separate public API contract change.
- Privileged suites are build-only here; execution remains pending VM. An equipped cargo-deny gate is needed if CI skips that tool.

## What changed

Staging directories, files, link instances, link sets and file spans use `WITHOUT ROWID` B-trees. Directory keys are preorder IDs. File keys are `(dir_id,name_present,name,input_sequence)`; instance keys are `(ino,dir_id,name_present,name,input_sequence)`. Presence flags and empty-byte placeholders represent nullable names without violating non-null primary keys, preserving SQL NULL-before-name order. Sequence suffixes retain duplicate rows, including multiple unresolved bulk-mode instances in one directory. No whole-tree map or sort buffer was added; page caches remain 8 MiB per connection and codec buffers remain at most one configured chunk.

Removed staging index builds entirely. All staging read-back queries walk their clustered primary keys or scan directly. Both connections use `temp_store=MEMORY`. The two required final indexes are created on empty tables and maintained during row insertion, avoiding even an output-index bulk sorter. Published table/index definitions, public methods and the codec are unchanged.

The staging filename remains exactly `<scan>.db.stage.tmp` and now has a public doc comment naming its cleanup contract. A child-process death fixture verifies that exact committed leaf. S2 already used the requested spelling in the baseline; this task's S2 change documents and pins compatibility, while the actual cleanup fix belongs to SNAP-cold-fix. No honest fail-before/pass-after writer filename fix exists for that already-correct baseline behavior.

Conversion calls the installed callback before staging preparation, before each chunk table, at every chunk boundary (N=1), before each auxiliary table, every 8192 copied plain rows, and after conversion. Callback refusal returns `StoreError::Space` (the API's NoSpace representation); RAII closes both connections and removes staging and output temporary leaves. Cancellation remains checked at conversion checkpoints and before publication.

At the same checkpoints an independent `statvfs` headroom check estimates **four current staging database sizes + 16 MiB** of additional writes, for output chunks, shallow duplicates, indexes, spans and page overhead. Current staging and output allocation are already deducted from filesystem availability; the estimate does not subtract progress, deliberately remaining conservative throughout finish. Saturating arithmetic refuses overflow-sized requirements. This is an estimate, not a concurrent reservation or a proof of every possible SQLite allocation. The zero-argument callback retains the caller's configured retention policy/estimate; the additional reserve cannot be passed into that frozen callback. No default policy silently overrides the caller's policy.

## Shared-file edits and dependencies

None. Only the two owned Rust files and this handoff changed; no manifests, catalog code, core types or codec changes.

## Regression evidence

- `finish_rechecks_space_after_input_and_each_conversion_checkpoint`: failed on the baseline with zero conversion callbacks; passes after the fix. Small chunks exercise repeated checkpoints and refusal at each one, with both temporary leaves removed.
- `space_guard_turns_to_refusal_after_last_input_row`: installed every-row guard accepts all input, then switches to refusal before finish; finish returns Space and leaves the directory empty.
- `staging_leaf_and_readback_plans`: failed on baseline because staging tables were ordinary rowid tables. Afterward asserts WITHOUT ROWID for all clustered input tables and runs EXPLAIN QUERY PLAN for every conversion read-back/lookup query (no aggregation SQL exists), rejecting `USE TEMP B-TREE`.
- `conversion_never_uses_external_sqlite_scratch`: subprocess sets SQLITE_TMPDIR to an empty private fixture, stages 200,000 directories and 500,000 link instances with 240-byte names, then converts. Parent checks both directory entries and live `/proc/<child>/fd` targets (SQLite unlinks spill files immediately), with no time-based assertions.
- `clustered_staging_preserves_null_names_and_duplicate_instances`: decodes output to verify duplicate NULL-name files/instances survive, followed by named instances in correct order.
- Existing raw format, golden-link fixture, 50k streaming, error/drop/abort, publication collision and cancellation-at-every-checkpoint coverage remains in place.

Baseline audit temporarily substituted only the original writer source from `60c210de18f8fd30e85ff19a0ea4c61e21026f50`, ran the current regression tests, and restored the exact changed source in a `finally` block before rerunning the writer suite. Both guard tests, the staging-plan test and the external-scratch probe exited 101 on the baseline. In particular, the probe failed with `SQLite used external scratch during conversion`. Logs, the audit script, and the restored-source SHA-256 are in this attempt's output `library/`. S2 is explicitly a compatibility regression, not a fabricated red test.

## Assumptions and deviations

- Used the supplied canonical attempt branch; no branch creation, merge or push.
- No questions are answered in isolated workers; preserved the frozen zero-argument guard API and used independent headroom accounting rather than changing its signature.
- Staging schema is private writer state, so presence/sequence columns do not change the on-disk v2 format.
- Public directory input remains increasing preorder, and final IDs/chunk contents retain the existing contracts.
- All fixtures use injected fixed metadata timestamps. Measurement clocks record performance only, never determine assertions.
- No new privileged behavior is introduced; existing privileged suites are compiled only.

## Measurements

Single debug-build runs on this host, uid 0, warm filesystem cache, fixed metadata timestamps. Each run streams **200,000 directories / 500,000 link instances**, in batches of 512; instance names are 240 bytes and inode input order is descending. The flat tree deliberately copies all directories into the shallow table. `measure.py` uses `time.monotonic()` and `getrusage(RUSAGE_CHILDREN)` around the already-built test executable; compilation is excluded. Measurements set SQLITE_TMPDIR inside the private state fixture for both versions. The separate regression probe deliberately uses an external private scratch fixture.

| Phase / metric | Baseline `60c210de` | Changed writer |
|---|---:|---:|
| Input staging elapsed seconds | 5.466 | 10.605 |
| Input staging peak RSS, KiB | 14,980 | 15,236 |
| Leaves after committed input, bytes | 160,169,984 | 229,408,768 |
| Whole writer elapsed seconds | 27.210 | 30.074 |
| Whole writer peak RSS, KiB | 70,028 | 65,964 |
| Published output bytes | 32,215,040 | 32,460,800 |

Ordered primary-key insertion replaces append-only staging plus finishing bulk sorts: input staging takes about 94% longer and its end-of-input leaves are 43% larger in this fixture. Total writer time increases about 10.5%, peak total RSS decreases about 5.8%, and output grows 0.76% from maintaining the final indexes incrementally. End-of-input staging size is **not** peak conversion disk usage: the baseline later adds indexes and sort scratch. The output schema/codec remain identical; physical B-tree page packing can differ.

These are single observations, not a latency or 2M/10M scale gate. Earlier overlapping build/probe observations (29.952 s before / 112.718 s after) remain in `scale-before.log` / `scale-after.log` for transparency; they are not used for the comparison above. The quieter paired runs are `scale-before-stage.log`, `scale-after-stage.log`, `scale-before-isolated.log`, and `scale-after-isolated.log`.

## Validation and test counts

All long Cargo commands used `timeout 1500`, Rust 1.98.0, `CARGO_NET_OFFLINE=true`, `CARGO_HOME=/home/brewerm/.cargo`, and the installed host toolchain. No privileged test executable or VM operation was run.

| Requested check | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0; repeated after baseline restoration |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0; repeated after baseline restoration |
| `cargo test --workspace --no-fail-fast` | Exit 0; aggregate harness results 1,070 passed / 0 failed / 14 ignored |
| `cargo build --workspace --features privileged --tests` | Exit 0; build only, pending VM execution |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Exit 0 |
| `tools/ci.sh` | Exit 0; 153 owner tags and forbidden-symbol scan passed; aggregate harness results 1,073 passed / 0 failed / 14 ignored |
| Final writer suite after restoring changed source | 14 passed / 0 failed; includes repeated conversion refusal and external-scratch probe |
| Four baseline regression probes | Each exits 101 on original writer, with expected assertions; `baseline-audit-status.json` records all results |
| `git diff --check` | Exit 0 |

Aggregate counts include nested subprocess harness reports and CI's separate two-test strict-ancestry run; they are not counts of distinct test functions. Cargo-deny is absent: CI explicitly reports `warning: cargo-deny is not installed; skipping cargo deny check`. No dependency was added. Independent uid 1000 / socket-enabled / GitHub / VM and equipped cargo-deny gates remain pending.

Full transcripts, test-count summaries, reproducible measurement/audit helpers, and the restored-source checksum are under this attempt's `library/` output directory. The baseline audit restores byte-identical source before the final writer tests, measurements, fmt and workspace Clippy rechecks. No acceptance, integration or release decision is claimed by these worker observations.
