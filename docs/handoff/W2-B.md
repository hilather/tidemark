# Handoff — W2-B: Bulk pipeline

## Needs from coordinator

- W2-A remains an inert aggregator in this baseline: `prepare()` returns `NotYetImplemented`. Enable and run `synthetic_bulk_equals_walk` after W2-A lands, then execute the VM suite on Rocky 8.8 and 8.10. No end-to-end bulk snapshot success is claimed here.
- No dependency, feature, shared-file, or contract edit is requested.

- **Branch:** hp-f85b848aef050801933e8ac8ab3b4a9c074169af4fa10af733b13a373b15cd4b
- **Agent/session:** canonical attempt-f85b848aef050801933e8ac8ab3b4a9c074169af4fa10af733b13a373b15cd4b
- **Date:** 2026-10-11
- **Status:** implementation submitted as an evidence candidate; runtime acceptance pending W2-A and VM.

## What was built

`run_bulk` now forces the existing gated walker into namespace mode, drains its bounded output on collector errors, forwards skeletons and Gone records, and builds an in-place sorted 12-byte dent array. A bounded pool streams disjoint AG ranges using v5 bulkstat only, charging before every iterator advancement, including terminal/error calls, with an extra initial geometry unit. Joined messages go to the single aggregator owner through a two-message bounded channel; directory attributes, single entries, whole link sets, and per-directory races use the frozen W2-A entry points.

The iterator-based join and a private recording target exercise the production dispatch without XFS or W2-A. The join preserves duplicate hard-link instances, full u32 link counts and byte-valued disk usage, checks ordered/unique bulk inode records and rejects records outside their AG range. Missing and type-changed namespace references count as races; stale multiple references to a now-single-link inode are discarded as races rather than double charged. After prepare, selected names are grouped by raw directory path and resolved by one gated, byte-sorted directory read; the first matching name is the representative.

## Shared-file edits

None. All implementation, test, and handoff edits are within the approved paths. No dependencies were added. Public function signatures and field lists are unchanged; the owning-task error shape was extended with typed unavailable, resource, I/O, walker, aggregator, and worker failures. The old NotYetImplemented variant is retained for compatibility and never returned by W2-B.

## Deviations from the spec

| Spec reference | Implementation choice | Reason | SCR? |
|---|---|---|---|
| 04 §7 / resource bounds | Fixed caps: 256 MiB allocated dent capacity, discovery slots below 1,000,000, at most 1,000,000 requested names, batch 1..65,536, threads 1..64. | Frozen BulkParams has no memory-budget field. These additional documented caps fail explicitly; orchestrator still checks the normative estimate against its configured budget. Very large trees may need a future budget-aware interface. | No frozen contract changed. |
| Duplicate bulk inode invariant | Return a typed failure for duplicate/unordered records; reject cross-AG output by validating each range. Tests assert both failures. | Fail closed instead of panicking the daemon. | No |
| Join batching | Flush after either 4,096 joined items or 4,096 streamed inodes. | Keeps progress messages bounded even when no inodes belong to the root. | No |
| Rayon sort | Local pool bounded by BulkParams.threads. | Avoids an unconstrained global sorting pool. | No |
| Tests | Cargo auto-discovers tests/w2b/main.rs. It imports the owned private join/name source modules by path to test their production bodies with a recording fake. | No manifest or visibility edit needed; join unit tests consequently run twice (library and integration crate). | No |
| Offline builds | Host Cargo cache, offline commands, explicit Rustup/Cargo environment. | crates.io unavailable; cargo was initially absent from PATH. | No |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| Synthetic sorted merge join: gaps, duplicates, missing inputs, AG boundaries | `join_gaps_links_units_and_type_races`, `duplicate_inode_across_batches_is_rejected`, `duplicate_across_allocation_groups_is_rejected`, `messages_and_ag_boundaries` | Passed locally; see test log. |
| Full link counts, correct disk units, phase races | `join_gaps_links_units_and_type_races`, `raced_single_link_aliases_are_not_double_charged` | Passed locally. |
| Gate before I/O; cancellation | `gating_precedes_source_and_unsupported_is_typed`, `cancellation_never_advances_source` | Passed locally. |
| Compact layout, checked slot bound, arithmetic | `compact_layout_and_checked_slots`, `estimate_rounds_up_and_saturates` | Passed locally. |
| Names: smallest bytes, disappeared files/directories, cancellation | `names_smallest_bytes_missing_and_cancelled` | Passed locally. |
| Bulk/walk equivalence on Rocky 8.8 and 8.10 | `vm::bulk_equals_walk_mandatory_fixtures` | Compiled only; pending VM and W2-A. |
| ftype=0 refusal | `preflight_refuses_ftype_without_io`; `vm::bulk_refuses_ftype_zero` | Local synthetic refusal passed; real mount pending VM. |
| 1M-file performance and RSS within 20% of estimate | `vm::bulk_equals_walk_million_file_smoke` | Compiled only; pending VM. Bulk-only RSS isolation remains a follow-up (see below). |

## Tests

Unprivileged tests added:

- `compact_layout_and_checked_slots`
- `join_gaps_links_units_and_type_races`
- `raced_single_link_aliases_are_not_double_charged`
- `duplicate_inode_across_batches_is_rejected`
- `duplicate_across_allocation_groups_is_rejected`
- `gating_precedes_source_and_unsupported_is_typed`
- `cancellation_never_advances_source`
- `messages_and_ag_boundaries`
- `estimate_rounds_up_and_saturates`
- `names_smallest_bytes_missing_and_cancelled`
- `preflight_refuses_ftype_without_io`

`synthetic_bulk_equals_walk` feeds the normative golden model through the real bulk entry points via the join and compares all required rows with the walk-record path (project ids and link-instance names normalized). It is ignored with exactly `needs W2-A`, because the current aggregator is a stub. Both paths use the same injected scan-start time.

Privileged tests are gated only by `feature = "privileged"`:

- `vm::bulk_equals_walk_mandatory_fixtures`: every mandatory shared fixture, plus the root-only device-node fixture; copying root and outside together preserves incomplete link sets — **pending VM**.
- `vm::bulk_refuses_ftype_zero`: disposable crc=0/ftype=0 XFS image — **pending VM**.
- `vm::bulk_equals_walk_million_file_smoke`: 100 directories, 10,000 files each, equality, ungated timings and process high-water RSS; ignored with `scale` — **pending VM**.

No mount, loop, mkfs, LVM, service, or privileged runtime commands were executed on this host.

## Measurements

Persistent W2-B dent storage is exactly 12 bytes per namespace child reference plus 12 bytes per directory self-reference. The check uses the specified 31-bit encoding before setting the self flag; the lower operational directory cap also prevents the aggregator's discovery-id vector from growing without a bound. Directory skeleton storage belongs to W2-A: the estimate reserves 320 bytes per directory, not a claim about its currently inert implementation.

The selection estimate is `ceil(used_inodes * 1.1) * 12 + dirs_estimate * 320 + 64 MiB`, using saturating arithmetic. W2-C owns comparison against memory_budget. W2-B also caps the dent vector's capacity at 256 MiB, grows geometrically within that cap, and keeps no all-inode attribute hash map. Each decoded link instance uses a u32 directory id in SmallVec storage; inode records and groups exist only in bounded streaming batches. Each producer has up to 4,096 joined records and the shared channel holds two batches; sys buffers are bounded by batch and worker count. The 64 MiB estimate allowance is not proven sufficient at every allowed batch/thread setting.

Measured decoded layouts and final validation results are appended below from the local logs. No production-scale runtime time/RSS measurement was performed.

## Known gaps and follow-ups

- The real aggregator, final snapshots, and real XFS runtime paths have not executed successfully here; W2-A is a stub and no VM is available.
- Both Rocky VM acceptance items remain open, including runtime v5 availability and the sys-level bs_blocks/statx unit test owned by W1-C.
- The scale smoke logs the test process's VmHWM after both modes. That is a useful diagnostic but cannot isolate bulk-only RSS, and is not evidence for the 20% criterion. Run a bulk-only process with an external peak-RSS measurement before accepting that criterion.
- The fixed caps intentionally reject trees larger than the documented operational bounds. A future configurable join budget and measurements across thread/batch combinations would improve the API.
- Any cargo-deny skip is reported in the final validation table; no license/advisory acceptance is claimed without that tool.

## Notes for the reviewer

No recursive traversal, unsafe blocks, inode-content reads, new dependencies, or writes to scanned paths were introduced. Actual metadata access is delegated to W1-B and W1-C; name reads preserve the existing no-follow/noatime/gate behavior. W2-B owns no disk-profile selection. All producer channels are drained before joining workers on failure or cancellation. Cross-AG duplicate prevention follows from validated disjoint ranges; within-range ordering is checked across batch boundaries.

## Definition of Done

- [x] Only owned paths changed.
- [x] Public items documented and ownership headers present.
- [x] Privileged tests written; status pending VM.
- [x] No unexplained TODOs, unwraps or expects in production.
- [ ] Runtime acceptance and scale RSS evidence complete.
- [ ] W2-A-dependent equivalence test executed.

## Final local validation

Environment: pinned Rust 1.98.0, `PATH=/home/brewerm/.cargo/bin:$PATH`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, `CARGO_NET_OFFLINE=true`. Logs are in this attempt's worker-output `library/` directory. Settled-source checks supersede the initial run; an initial rustdoc check overlapped a source write and failed to resolve join functions. The final strict rustdoc check passed with no source edits in progress.

| Command | Result | Evidence artifact |
|---|---|---|
| `cargo fmt --check` | exit 0 | `final-fmt.log`, `final-fmt.exit` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `final-clippy.log`, `final-clippy.exit` |
| `cargo test --offline --workspace` | exit 0; 192 passed, 0 failed, 4 ignored | `final-test.log`, `final-test.exit` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0, compile only | `final-privileged.log`, `final-privileged.exit` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `final-doc.log`, `final-doc.exit` |
| `tools/ci.sh` with offline environment | exit 0; cargo-deny unavailable and skipped | `final-ci.log`, `final-ci.exit` |
| `cargo clippy --offline -p tidemark-scan --all-targets --features privileged -- -D warnings` | exit 0 | `final-privileged-clippy.log`, `final-privileged-clippy.exit` |
| `git diff --check` | exit 0 | local whitespace check |

The W2-B tests account for 19 passing invocations (11 distinct test functions; the eight private join tests also execute inside the integration crate), plus one ignored real-aggregator equivalence test. Privileged tests were only compiled.

`compact_layout_and_checked_slots --nocapture` measured `DentRef=12`, `Joined=120`, `InodeAttrs=48` bytes on this x86_64 build (`layout.log`). A typical joined group with up to two non-directory links fits inline; larger groups add heap storage for u32 directory ids (allocator capacity overhead also applies). The aggregator's own directory/link-set layout and production peak RSS are not measured by this stub baseline.

- [x] All available local CI checks passed on the settled implementation.
- [ ] cargo-deny audit performed on an equipped host.

This handoff and report are evidence candidates, not independent verification, integration approval, or a release decision.
