VERDICT: READY
BLOCKER 0 / SHOULD-FIX 1 / NOTE 0

Reviewed exact candidate `b609bfa96d43f367a7df3b8211e3dfb929e9582b` against base `78de7188e5a98980088a92e3c9b0f3985bb33e03`, and the requested integration head `cc07779f7d14e6118a66fd0e9f4cb37317ac4510`. Both candidate and integration have tree `c035c9ae28c98f3294fab701587722a9688f34de`; all findings and executed checks apply to both. R1 and R2 are addressed. No remaining blocker was established in the reviewed cold recovery and v2-writer state; one reproducible recovery-isolation issue should be fixed. READY is this scoped review assessment, not an acceptance, integration or release decision.

Read `AGENTS.md`, both previous reviews, snapshot-v2 §2 and the wider plan, storage/link contracts, relevant handoffs and the playbook. Inspected the candidate changes, requested `git diff 3efed8f..cc07779` and integration log, plus full cold/catalog/writer code and relevant callers. No implementation files were edited.

## Finding

### S1 — SHOULD-FIX: a quarantine collision prevents unrelated cold recovery indefinitely

Reference: `finding:w9-review4-coldfix2-s1`. Location: `crates/tidemark-store/src/catalog.rs:931`–`:938`; affected later repair at `:942`–`:943`, outer logging boundary at `:679`–`:685`.

The expected `NOREPLACE` failure when a `.bad` destination already exists propagates out of the entire cold cleanup sweep. The colliding pair is preserved, and its own catalog pointer has already been repaired, but every later cold entry is skipped. Because cleanup preserves the collision, subsequent sweeps can stop at the same entry indefinitely. This is distinct from the intended conservative handling of that one pair.

External probe `review_extra_quarantine_collision_starves_later_recovery` creates two genuine snapshot pairs, uses their observed directory iteration order to place a bad-cold/good-hot pair with an existing quarantine before a healthy cold-only snapshot whose catalog still names missing hot, then calls cleanup three times. Each time the later catalog pointer remains missing while direct cold reading succeeds. Removing only the blocking quarantine artifact inside the temporary fixture lets the next cleanup repair that same pointer. The earlier evidence bytes survive all three failing passes.

Handle a quarantine collision at the individual-entry boundary, warn with both paths, and continue the sweep. Add a regression with another repairable snapshot after the collision; the current single-pair collision test cannot detect starvation. This is classified like the previous review's stale-pointer SHOULD-FIX: snapshot data survives, but automatic recovery can leave it unavailable through the catalog. No new deletion of the sole valid copy was demonstrated.

## R1/R2 and earlier-finding recheck

All named current tests below passed in this review. The three original R1/R2 probe scenarios were also rerun through a separate external harness, using the current fixture helpers and corrected assertions. The fail-before assessment comes from inspecting the old implementation against the assertions; no old-source build or source substitution was performed here. The handoff's ten baseline-failure runs remain handoff evidence, not independently repeated runs.

| Finding | Assessment and regression evidence |
|---|---|
| R1: healthy cold, corrupt hot | Fixed. `review_healthy_cold_corrupt_hot_deleted` retains readable cold, quarantines hot and selects cold. `review_rebuild_hides_healthy_cold_behind_corrupt_hot` records cold. The old mismatch branch unlinked cold; the old rebuild suppressed cold based on hot existence alone, so these assertions fail on that logic. |
| R2: equal payload, wrong header root counter | Fixed. `review_matching_payload_wrong_header_survives_cleanup` deliberately recomputes the JSON checksum after changing `root.t_disk`. Transition refuses, cleanup keeps hot and quarantines cold, and rebuild returns 4096. Old comparison checked only payload bytes and accepted this semantic mismatch. `cold_only_rebuild_rejects_header_corruption_and_old_magic` separately checks accidental JSON alteration and rejected `TMCOLD01`. |
| B1: ATTACH compressed file | Remains fixed. `diff_and_explain_all_hot_cold_directions` passes all four combinations. `diff::with_old` uses the retained backing file; durable metrics paths remain separate by source inspection. |
| B2: valid zstd containing invalid SQLite | Remains fixed. `valid_frame_with_wrong_payload_preserves_hot_and_catalog` rejects transition and preserves hot; invalid cold now becomes `.bad` instead of being deleted. |
| B3: truncated cold shadows good hot | Remains fixed. `rebuild_prefers_hot_over_truncated_cold` selects validated hot. |
| B4: external SQLite sort scratch | Remains fixed in the tested workload. `conversion_never_uses_external_sqlite_scratch` passes with 200,000 directories and 500,000 link instances, monitoring both directory entries and deleted-file descriptors. `staging_leaf_and_readback_plans` checks clustered staging and sorter-free conversion plans. |
| Earlier S1: cold-only stale hot pointer | Isolated case remains fixed: `cleanup_repairs_cold_only_stale_hot_pointer` passes. The new finding above identifies an unrelated-entry obstruction. FULL synchronization still precedes hot unlink. |
| S2: abandoned v2 staging | Remains fixed. `cleanup_sweeps_v2_staging_leaf` passes; the writer child-process fixture still pins the real `.db.stage.tmp` spelling. |
| S3: conversion space callback | Remains fixed. Both refusal-after-input and every-conversion-checkpoint regressions pass and remove both temporary leaves. |
| S4: inflate descriptor failure leak | Remains fixed. `descriptor_exhaustion_never_leaks_inflate_leaf` passes with isolated soft descriptor limits 4–23. Ownership begins immediately after creation. |

## Pair-resolution decision table

Here **valid cold** means the complete predicate: trusted leaf, correct filename identity, checked header, exact complete checksummed frame with no trailing bytes, supported SQLite with `quick_check = ok` and a root, and equality of header version/all metadata/root with its decoded snapshot. A readable payload with an inconsistent header therefore belongs to the invalid-cold rows. This distinction makes some nominal Boolean combinations impossible rather than untested branches.

| Hot | Cold | Payload/header possibilities | Cleanup result |
|---|---|---|---|
| Valid | Valid | Equal payload; consistent header | Proves byte and header equivalence, commits catalog → cold with FULL sync, unlinks hot, fsyncs directory. |
| Valid | Valid | Different payload; header consistent with cold; header may equal or differ from hot | Preserves both and catalog. Existing test changes root totals; extra probe changes only annotations so the duplicated header remains equal. |
| Valid | Proven invalid | Equal decoded bytes but inconsistent header, or invalid container framing | Keeps hot and quarantines cold when invalidity is established. A decoder error classified inconclusive instead preserves the pair. R2 covers an equal payload with a checksum-valid semantic mismatch. |
| Valid | Proven invalid | Different payload; either header state | Keeps hot and quarantines cold. B2 covers structurally valid framing around invalid SQLite; extra probes cover semantic header failure with changed payload. |
| Proven invalid | Valid | Different payload; header consistent with cold | Commits catalog → cold, quarantines hot. R1 covers this. Equal complete payload with the same identity cannot have opposite SQLite validity under the immutable-file assumption. |
| Proven invalid | Proven invalid | Equal or different payload; either header state | Preserves both and catalog. Extra probe uses identical zero-filled hot/decoded-cold bytes; existing tests use different invalid files. A header mismatch also makes otherwise readable cold invalid. |
| Any inconclusive result | Any | Any payload/header state | Preserves both and catalog. Space refusal is tested; access/resource failures are classified conservatively. Generic decoder I/O failures may also be inconclusive. |

An inconsistent header cannot coexist with full cold validity. For equal valid SQLite payloads, a consistent cold header also equals the hot metadata/root. These dependencies cover the remaining nominal validity × equality × header combinations. Comparisons are repeated by the protected catalog transition; no failed comparison alone authorizes deletion.

The table describes resolution once an entry is reached. S1 means later entries are not always reached. Preservation also intentionally leaves an existing invalid or missing catalog pointer unchanged when neither member is valid or validation is inconclusive; the contract does not promise a healthy catalog under those conditions.

## Publication, quarantine and trust

Publication ordering inspected: exclusive private cold temporary → file fsync → NOREPLACE publish → directory fsync → FULL-synchronous catalog update → hot unlink → directory fsync. Before publication, hot remains and the temporary can be swept. Between publish and its directory sync, hot remains recoverable. After publication is durable but before the catalog commit, both files remain. After the catalog commit but before/during hot unlink, the catalog names durable cold; a crash may leave the duplicate to be resolved again. No later cancellation checkpoint removes an already published survivor.

Quarantine ordering is FULL-synchronous survivor catalog update → same-directory NOREPLACE rename of the invalid sibling → directory fsync. A crash before the commit leaves the original pair; after the commit the catalog names the surviving valid file; around rename/fsync the invalid sibling may retain its published or quarantine name. The valid survivor is neither renamed nor unlinked. An existing quarantine prevents replacement, but triggers S1. A missing catalog row is not created by these UPDATE-only repair paths; explicit rebuild remains necessary in that case.

Source leaf opens enforce the ordinary no-follow, owner/mode, regular-file and single-link checks. Hot receives an explicit mount check before resolution; cold opens detect mount crossings. A failed cold mount check can classify cold invalid and lead to an attempted rename, so this is not a claim that every quarantine source passed full validation. NOREPLACE never overwrites or dereferences an existing `.bad` destination, including an untrusted one. Startup and rebuild suffix filters do not open quarantine leaves as snapshots, and retention does not select them. Allocation accounting includes their blocks; state purge uses descriptor-relative traversal. No recursive-depth algorithm was added here.

These conclusions assume immutable published files and the caller-held exclusive scan lock required by `cleanup_abandoned_scans`. Validation and subsequent rename/unlink do not promise immunity to a privileged actor replacing paths or modifying inodes concurrently. Physical power loss and bind-mount refusal were not executed.

## Header format and boundedness

`cold.rs:95`–`:118` reads the fixed 20-byte prefix, rejects zero or greater-than-1-MiB JSON length before allocation, reads the JSON and little-endian u64 checksum, checks `xxh3_64` over exactly those JSON bytes, then parses and validates metadata. The writer emits the same bytes and checksum; the frame begins at `20 + M + 8`. Every source-level header read funnels through this function: public header reads, frame validation, comparison, explicit inflate and guarded reader preparation, including catalog reads and the inflate space-check input. No bypass was found. Old magic is rejected.

This checksum covers JSON, as specified; it does not cover the binary uncompressed-length field and is not authentication. Length still has signed-domain/nonzero checks and full decoding enforces exact output length. The space-refusal regression deliberately changes length without touching JSON. Semantic comparison protects pair resolution even when a changed JSON checksum is deliberately recomputed. Cold-only header trust remains the explicit limitation below.

Payload decoding requires a standard zstd frame with the checksum flag, limits decoder window to 128 MiB, limits output to declared length plus one, and checks trailing input. Comparison uses a 128-KiB buffer; independent cold validation uses the guarded state-local inflate leaf and space check. No full-snapshot Rust allocation was introduced. Concurrent cold opens still have independent capacity checks without shared reservations, as documented previously.

## Coverage limits and integration boundaries

- Cold-only and invalid-hot fallback rebuild intentionally trust the checked header without validating the frame (`catalog.rs:384`–`:395`). Extra probe `review_extra_cold_only_header_boundary_is_not_full_validation` confirms a one-byte-truncated payload is unreadable but still cataloged. That is the explicit §2 policy, not a claim that every catalog target is valid. Hot-only rebuild also retains legacy filenames and metadata/root opening without the stricter pair quick-check predicate.
- Writer implementation/tests are unchanged since reviewed integration `b953fe9`. Source inspection and passing raw-format, duplicate/NULL-name, shallow-row, link-fixture, cancellation and publication-collision tests support preservation of earlier fixes. This run does not establish the 2M-directory/10M-instance memory bound or §8 query latency. The conversion reserve remains an estimate, not a shared allocation reservation; no real disk-full injection was run.
- Production compression and v2 writer wiring remain follow-up work, as in the earlier handoffs. `Snapshot::open` remains v1-only (`reader.rs:323`); raw v2 writer tests do not establish v2 reader readiness. CLI removal (`snapshots.rs:265`) and automatic deletion (`orchestrator.rs:1094`) still accept only `.db`; quarantine listing/prune support is also unfinished. This assessment does not certify those pending features as ready to enable.
- Existing mode/no-follow/hard-link, corruption/length/trailing-data, space refusal, live-inflate-lock, catalog, direct-reader and recovery tests passed. No full workspace CI, Clippy, cargo-deny, privileged-feature build/execution, uid-1000 gate, physical crash test or concurrency-reservation experiment was performed in this review.

## Executed validation and artifacts

1. `cargo test --offline -p tidemark-store -- --nocapture`: exit 0 with installed pinned Rust 1.98.0 and dedicated `/tmp/tidemark-review-243f711-target`. Cold: **24 passed**. Writer-v2: **14 passed**. Aggregate completed harness summaries: **153 passed, 0 failed, 5 ignored**, including a nested child summary and doctests; not a distinct-function count. Transcript: `library/store-tests.log`.
2. External Rust test harness, linked to the exact tested library, ran **8 probes, all passed**: the original three R1/R2 scenarios and five additional recovery/decision-table probes. The collision and header-only-boundary probes assert observed limitations, not desired correctness. Source/build/results: `library/review-probes.rs`, `library/probe-build.log`, `library/probe-results.log`.
3. `git diff --check 3efed8f..cc07779`: exit 0. Requested diff/log retained as `library/reviewed.diff` and `library/reviewed.log`. Candidate and integration tree hashes are identical.
4. `cargo clean --offline` against the dedicated target: exit 0, **2785 files / 1.6 GiB removed**. Standalone probe executable also removed. Transcript: `library/clean.log`.

Artifacts are under this attempt's worker-output directory. Only this requested Markdown report changes in the repository. The report and submitted receipt are evidence proposals; remaining gates and any acceptance decision belong to the coordinator.
