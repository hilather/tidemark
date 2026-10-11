# Handoff — W2-A: Aggregator and link resolution

## Needs from coordinator

- Decide [SCR-001](scr-W2-A/SCR-001.md): the walker omits per-directory error totals after its shared detail cap. The frozen record contracts cannot carry those missing counts. No contract fields or shared files were changed.
- Run `loop_xfs_walker_golden` in the disposable VM; privileged execution is **pending VM**.
- Run the advisory/license audit on an equipped host: `tools/ci.sh` reports cargo-deny unavailable and skips that check.
- The node layout is 288 bytes, above the plan's ≤200-byte target. A subsequent memory optimization can reduce this; the measured scale run remains below 700 MiB.

- **Branch:** hp-749f6916a2f403fb887271a17a7faa62a67e29fd56709e022d01b2c928e79783
- **Agent/session:** attempt-749f6916a2f403fb887271a17a7faa62a67e29fd56709e022d01b2c928e79783
- **Date:** 2026-10-11
- **Status:** implemented with the limitations below; submitted evidence is not independent verification.

## What was built

The aggregator consumes walk records and all frozen bulk entry points. It groups walk links by inode, preserves duplicate instances, uses maximum observed nlink and last-seen attributes, computes parent-lifting LCAs, stamps cumulative inode unions, and rolls additive totals through reverse preorder. Directory blocks contribute to reclaimable bytes and their own mtime age bucket; visible link mtimes roll up through every containing subtree.

Tree validation and traversal are iterative, support child-before-parent arrival and discovery-id holes, and assign byte-name-sorted preorder IDs with inclusive subtree ends. Notable selection uses final directory IDs, threshold/top-N union, checked zero-based IDs, and smallest representative names. Bulk name resolution retains only selected representative keys and leaves instance names NULL. Labels use component-wise tree lookup; missing paths are counted internally and reported through a tracing debug field.

`Prepared::write` streams directories using a reusable path buffer in batches of at most 10,000 rows, with an additional 4 MiB path/name byte threshold, then streams other row classes in 10,000-row batches. Sink errors stop writing immediately. `VecSink` now collects all seven row types. Aggregation performs no filesystem I/O, starts no threads and uses no unsafe code.

## Shared-file edits

None. No dependency, manifest, core contract, lockfile, or module declaration outside `agg/` changed. Cargo discovers `tests/w2a/main.rs` directly; no extra integration entry file or manifest entry is needed.

## Deviations from the spec

| Spec reference | Implementation / limitation | Reason | SCR? |
|---|---|---|---|
| Wave 2 node target ≤200 bytes | Node and Option<Node> measure 288 bytes, excluding name/child heap storage | Explicit directory metadata, own metrics, union accumulators and subtree totals are retained inline. Correctness was prioritized; measured memory is documented below. | No frozen contract change |
| 03 §4 E, bucket by depth | Reverse preorder rolls children into parents | Equivalent iterative postorder; avoids a separate depth-bucket allocation | No |
| 03 §4 F, final numbering after rollup | Preorder IDs are assigned before resolving links, but selection remains after rollup | Allows representatives and output link rows to use final IDs immediately; does not change metrics | No |
| 03 §3 error counts | Count all input error records before applying the aggregator's detail cap; upstream omitted errors cannot be reconstructed | Frozen record fields provide no separate count | SCR-001 |
| AGENTS bounded state | Fixed ceilings: 4,000,000 discovery directory slots; 32,000,000 link instances, single candidates, skipped records and annotation records; gone records capped at 4,000,000 | No aggregator memory-budget field exists. Breaches fail rather than silently truncate required accounting/selection. Gone-derived skips can add up to 4,000,000 additional rows. | No contract change |
| Scale fixture | 30M single entries represented by per-directory OwnStats, matching scanner output; no per-entry candidate vectors | Single-link accounting is already aggregated at the worker | No |
| Offline builds | Host cached Cargo dependencies, CARGO_NET_OFFLINE=true and explicit --offline where supported | Sandbox crates.io unavailable | No |

Stored error details are a deterministic path/op/errno sample capped by `max_stored_errors`; row totals still include every error present in the input. Races include every Gone record, matching W1-B, with OtherFs also producing a skipped row. Input overflow detected in infallible mutators is deferred to `prepare` as Capacity; preparation arithmetic returns Overflow.

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| Golden, property and bulk-equivalence tests pass | `golden_table_and_bulk_equivalence`, `random_reference_and_order`, `large_reference_bounds`, `ages_nonutf8_errors_and_own_metrics`; local logs listed below | Local tests passed; VM pending |
| No recursion anywhere in agg | Iterative parent lifting, explicit DFS stack, reversed preorder rollup, component-wise annotation lookup and iterative path reconstruction; source review | Implemented; independent review pending |
| Memory per directory and link set documented with measured numbers | `storage_layout`, both scale tests, Python RUSAGE_CHILDREN measurement logs | Measured below; 200-byte node target not met |

## Tests

Unprivileged unit test: `agg::measurements::storage_layout`.

Unprivileged integration tests in `tests/w2a/main.rs`:

- `golden_table_and_bulk_equivalence`: exact 03 §6 table and walk/bulk rows, ignoring only bulk instance names.
- `random_reference_and_order`: 40 proptest cases; weighted small/large trees up to 2,000 directories, 20,000 inodes, 2,000 link sets and 30% incomplete sets; all normative stored subtree metrics and age buckets match the independent reference; shuffled records and resolved bulk rows match walk output.
- `large_reference_bounds`: explicit 2,000-directory / 20,000-inode / 2,000-link-set reference case.
- `deep_tree_10000`: reverse-arriving 10,001-node chain; prepare and write to counting sink, depth 10,000 without stack overflow.
- `holes_errors_races_and_annotations`: holes, all Gone race counts, OtherFs skip, detail cap versus total errors, labels and unknown paths.
- `duplicate_instances_complete_race_and_last_stat`: same-directory repeated links, found ≥ nlink, conservative maximum nlink, last-seen attributes and smallest representative name.
- `selection_uses_final_ids_and_names_use_minimum`: final preorder tie order, threshold/cap, zero-based file IDs and nullable bulk name resolution.
- `bad_tree_and_overflow_are_typed_errors`: disconnected cyclic parents, ingestion and rollup overflow, and invalid link directory IDs.
- `sink_failure_stops_stream`: fail on the directory batch and ensure no later sink calls.
- `walker_golden_measured_du`: real W1-B traversal on the fixture, every directory compared with measured GNU du disk usage; root file/directory counts compared with find.
- `ages_nonutf8_errors_and_own_metrics`: directory age boundaries, future/negative mtimes, special inode kind, invalid UTF-8 directory name, errors, direct metrics and bulk/order equivalence.
- `bulk_projid_and_disabled_instances`: directory project ID, symlink counts, directory-inclusive apparent size, filesystem-root annotation and disabled link-instance output.

Scale tests are `#[ignore = "scale"]`, both explicitly executed locally:

- `scale_two_million_directories_thirty_million_entries`.
- `scale_half_million_bulk_link_sets`.

Privileged: `loop_xfs_walker_golden` is gated only by `feature = "privileged"`, never ignored. It creates an owned loop XFS fixture, writes the worked example, checks the exact table after subtracting measured directory blocks, and compares every directory with du. **Pending VM**; compilation checked, no privileged commands executed here.

No tests are ignored pending another implementation task. There are no outside-scope stub-behavior tests in the scan crate at this baseline.

## Measurements

Pinned Rust 1.98.0, x86_64 debug test binaries, shared Linux development host. Counts and inode sizes in synthetic tests are filesystem-independent. Python `resource.getrusage(RUSAGE_CHILDREN).ru_maxrss` reports peak KiB; direct-binary measurements avoid compiler RSS. These are whole-process high-water measurements, not isolated allocator measurements.

| Measurement | Result |
|---|---|
| Node / Option<Node> | 288 / 288 bytes |
| Totals | 128 bytes (inline in Node) |
| LinkAcc | 128 bytes, including two inline `(DirId, Option<RawName>)` slots |
| Link instance accumulator slot | 32 bytes; spilled groups additionally allocate slot capacity and names |
| FileRow candidate | 72 bytes, plus any name allocation |
| 2M directories / 30M entries | Initial prepare/write-inclusive fixture run 7.49 s; direct-binary repeat under concurrent workspace checks 31.58 s; peak 678,952 KiB (663.0 MiB), about 347.6 gross bytes/directory |
| 500K bulk link sets / 1M instances / 2 dirs | Isolated direct-binary run 1.78 s; peak 261,524 KiB (255.4 MiB), about 535.6 gross bytes/link set, including candidates/output rows and baseline |

Node sizes exclude byte names and child vector allocations. Walk link sets additionally occupy hash-map buckets and name allocations; no walk-specific gross RSS claim is made. The link test uses bulk groups with NULL names and enabled output instances. Global candidate accumulation remains bounded by the fixed ceiling but can dominate memory on candidate-heavy inputs; no claim is made that every allowed ceiling combination fits an arbitrary configured scan memory budget.

## Validation

All commands used the cached host registry and offline mode. Exact output is in the attempt output `library/` (outside the repository). Final command exit status is recorded at the end of each log:

- `fmt-final.log`: cargo fmt --check.
- `clippy-final.log`: cargo clippy --offline --workspace --all-targets -- -D warnings.
- `test-final.log`: cargo test --offline --workspace.
- `privileged-final.log`: cargo build --offline --workspace --features privileged --tests (compilation only).
- `doc-final.log`: RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps.
- `ci-final.log`: tools/ci.sh with offline environment; cargo-deny unavailable is a remaining audit gap.
- `layout.log`, `scale.log`, `scale-final.log`, `link-memory-isolated.log`: measurement evidence.

## Known gaps and follow-ups

- Omitted upstream error totals: SCR-001 requires a coordinator contract decision.
- VM execution, Rocky 8 validation and advisory/license auditing are not done.
- Node layout exceeds the target; candidate-heavy and walk-link-heavy scale measurements remain follow-up performance work.
- Conflicting inode stats intentionally obey last-seen semantics. Arrival-order invariance applies to unchanged inputs; conflicting-stat race inputs can produce differing size/mtime observations, as the normative last-seen rule requires.
- Bulk pipeline and scan orchestration are owned by other tasks; this task verifies their frozen aggregator input APIs with synthetic models, not end-to-end bulk ioctls.

## Notes for the reviewer

Union contributions are added after additive rollup and are never rolled twice. Directory bytes enter the directory's own age bucket. Link groups retain repeated directory IDs, so entry counts differ from distinct inode counts. The only non-test `expect` is explicitly documented as infallible: an absolute validated root plus validated RawName components is accepted by RawPath::new. There are no unsafe blocks or scan-path syscalls.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits.
- [x] Public declarations documented and source ownership headers preserved.
- [x] Core accounting and frozen input APIs implemented and covered by tests.
- [x] Privileged test written and listed as pending VM.
- [x] Scale runs and layout measurements recorded.
- [ ] Independent verification, VM execution and cargo-deny audit completed.
- [ ] Node ≤200-byte target met.

## Final local check results

| Check | Result | Elapsed |
|---|---|---|
| cargo fmt --check | exit 0 | 0.430 s |
| workspace Clippy -D warnings | exit 0 | 8.480 s |
| cargo test --offline --workspace | exit 0 | 82.218 s |
| privileged workspace test compilation | exit 0 | 19.271 s |
| workspace docs -D warnings | exit 0 | 5.944 s |
| tools/ci.sh | exit 0 | 75.253 s |

Workspace test log: 186 passing tests, 5 ignored scale tests, zero failures. The final CI run also passed on the final source; cargo-deny was explicitly skipped as unavailable. Initial single-match lint failure logs were retained, and the equivalent if statement passed the rerun. Privileged execution remains pending VM.
