**CHANGES REQUESTED — 1 BLOCKER, 6 SHOULD-FIX, 0 NIT.**

Reviewed candidate `0774063b9627613c0f640b4d221a948bedb2b569` against base `42eb6c35dafd8a88f79622032f06bf326d04ef95`, limited to that candidate's diff. All source line numbers below refer to the candidate, not the later checkout. The six changed paths are the three bulk modules, two W2-B test files, and the handoff. No shared manifest or public core contract was changed.

Read AGENTS.md, the wave header and W2-B section, scanner §3/5/7/8, link semantics §4, contract decisions, and the candidate handoff; there is no candidate `docs/handoff/scr-W2-B/`. The requested W2-B and W2-integ history was inspected. Later integration changes are not credited to this exact candidate. In particular, its aggregator is still a stub and its bulk error enum has no `BeforePhase2` variant.

## Findings

1. **BLOCKER — Phase 3 can read a replacement mount.** `crates/tidemark-scan/src/bulk/names.rs:33` (`finding:w2b-phase3-mount`). **References:** AGENTS rule 4; `04-scanner.md` §3.3 and §4; W2-B phase-3 safety requirement.

   `resolve` passes only a path and gate to `read_dir_entries`. Its implementation in `tidemark-fs/src/walk/getdents.rs:112` opens and enumerates that path without checking device, mount ID, or the phase-1 directory identity. If a different filesystem or same-device bind mount covers a needed directory or ancestor after phase 1, phase 3 reads the replacement mount. Inode-number collisions can also attach a name from the wrong filesystem. Component-wise `O_NOFOLLOW` does not prevent crossing mounts.

   Preserve the scan's root/mount identity and validate the opened directory and traversed components beneath the scan root before enumerating entries, on the descriptors actually used for the read. Reject a changed mount boundary and leave the names unresolved. Coordinate an approved filesystem-helper contract change if necessary; a separate path-based precheck would still race. Add VM regressions for a newly mounted directory and same-device bind mount between phases. This finding is verified by source tracing; no mount was created during review.

2. **SHOULD-FIX — Accepted worker/batch settings exceed the memory estimate's reserve.** `crates/tidemark-scan/src/bulk/mod.rs:135` (`finding:w2b-worker-memory`). **References:** AGENTS rule 10; `04-scanner.md` §7; W2-B memory acceptance.

   Validation accepts 64 workers and a batch of 65,536. With at least 64 AGs, each worker owns a sys iterator whose retained request buffer is `64 + 192 * batch` bytes (`tidemark-sys/src/xfs.rs:295`). Those buffers alone total **805,310,464 bytes**, approximately 768 MiB, before decoded inodes, joined batches, dents, and aggregator storage. `estimate_memory` reserves a fixed 64 MiB for overhead and cannot account for either setting. Thus the orchestrator can authorize a scan within its configured estimate while the accepted W2-B settings require hundreds of MiB more. The dent cap and bounded channel do not address these buffers.

   Bound the effective worker/batch combination using an explicit byte calculation that includes request buffers, decoded batches, joined messages and queue capacity within the reserved overhead. If the intended settings cannot fit that contract, request a budget-aware interface/estimate amendment. Add arithmetic boundary tests and reject combinations that cannot fit before allocation. This is a source-derived allocation lower bound, not a measured RSS claim.

3. **SHOULD-FIX — Duplicate directory identities are dispatched twice without a race.** `crates/tidemark-scan/src/bulk/join.rs:143` (`finding:w2b-duplicate-directory`). **References:** `04-scanner.md` §3.2's single `self_dir`; `03-link-semantics.md` §4 directory-inode accounting.

   Every matching directory self-reference is appended to `self_dirs`, and lines 97–99 dispatch the same attributes to all of them. A directory renamed while phase 1 traverses the tree can be observed under two discovery IDs with the same inode. The independent probe supplied two such namespace records for inode 42 and one bulk directory record with 4,096 disk bytes. Production dispatch emitted `(DirId(1), 4096)` and `(DirId(2), 4096)`, with **zero races**. Applying both per-directory contributions would count the same directory inode twice. The probe verifies the duplicate callbacks; the stub aggregator prevents a final-row demonstration at this candidate.

   Detect multiple directory self-references for one inode before dispatch and fail explicitly with a typed directory-identity race error, rather than applying attributes to both skeleton nodes. A future recovery policy must reconcile the affected subtrees as well. Add this case separately from legitimate duplicate hard-link instances and duplicate bulkstat records. Evidence: `duplicate_dir_probe.rs` and `.log`.

4. **SHOULD-FIX — Name lookup can select an excluded hard-link alias.** `crates/tidemark-scan/src/bulk/names.rs:40` (`finding:w2b-excluded-name`). **References:** `04-scanner.md` §4 exclusions/equivalence; `03-link-semantics.md` §4 Step G.

   Phase 1 filters individual names through `WalkConfig.exclude`; phase 3 has no exclusion input and selects the first matching inode from all directory entries. On an unchanged temporary directory containing hardlinks `a` and `z`, with `a` excluded, the real walker reported only `z` and recorded `a` as skipped. The production resolver returned `a`. The probe's equality assertion failed with `RawName([97])` versus `RawName([122])`.

   Pass the existing exclusion policy into the private resolver and apply it to each full raw-byte entry path before accepting a name. Preserve other applicable skipped-entry boundaries as well. Add the excluded-alias case to both name-resolution and walk/bulk equivalence coverage. Evidence: `names_review_probe.rs` and `.log` (one expected regression failure, two passing safety/grouping probes).

5. **SHOULD-FIX — Cancellation is missed across output batches and misclassified in phase 1.** `crates/tidemark-scan/src/bulk/join.rs:242`; `crates/tidemark-scan/src/bulk/mod.rs:116` (`finding:w2b-cancellation`). **References:** W2-B cancellation requirement; `04-scanner.md` §8.

   Only the outer source-iterator loop checks cancellation. The unmatched tail and missing-inode gap loops can emit multiple 4,096-item batches without checking it. With an exhausted source and 12,001 distinct namespace groups, cancelling in the first emit still produced all three batches and returned `Ok(())`. With one source inode beyond the same namespace range, another full batch was emitted before cancellation was noticed. The enclosing pipeline eventually checks the flag, but only after workers have processed and drained this unnecessary work. Separately, cancellation triggered during phase-1 gated opening returned `Walk(Cancelled)`, rather than the promised `BulkError::Cancelled`.

   Check cancellation when continuing/flushing each join batch, including the final unmatched tail, and normalize `WalkError::Cancelled` at the phase-1 boundary. Keep the existing channel-draining behavior so workers cannot block during shutdown. Evidence: `join_cancel_probe.rs`/`.log` and `phase1_cancel_probe.rs`/`.log`.

6. **SHOULD-FIX — Pre-phase-2 failures lack the required fallback classification.** `crates/tidemark-scan/src/bulk/mod.rs:118` (`finding:w2b-before-phase2`). **References:** W2-C sequence step 7, automatic fallback before phase 2; the assigned review's `BeforePhase2` requirement.

   Namespace errors propagate as `Walk`/`Limit`, sort-pool errors as `Worker`, and preflight geometry failures as `Limit`; the enum has no `BeforePhase2` variant. The missing-root probe returned `Walk(Io(ENOENT))` before any bulkstat call. Generic `Limit` and `Worker` also occur after phase 2 starts, so callers cannot use the error type to identify the safe fallback boundary reliably.

   Add `BeforePhase2` and wrap eligible namespace, allocation/sort, and preflight failures until the first phase-2 iterator call. Preserve the distinct cancellation and unsupported-mode classifications; keep later failures out of the fallback class. Add tests for both sides of the boundary. Evidence: `phase1_error_probe.rs` and `.log`. Later integration adds related handling, but that is outside this review candidate.

7. **SHOULD-FIX — The performance smoke cannot verify the required bulk RSS bound.** `crates/tidemark-scan/tests/w2b/vm.rs:121` (`finding:w2b-rss-test`). **Reference:** W2-B acceptance, peak bulk RSS within 20% of `estimate_memory`.

   The test runs walk first, retains its `VecSink`, then runs bulk and reads the process lifetime `VmHWM`. This includes walk allocations, retained comparison rows, fixture generation, and any earlier tests. It neither isolates peak bulk memory nor checks the 20% criterion. Running this test successfully on a VM would still not establish the required measurement; the handoff correctly acknowledges that limitation.

   Run each mode in a separate measurement process, obtain bulk-only peak RSS, and compare it against the estimate with the stated 20% tolerance. Keep row-equivalence checking outside the measured bulk process and record the inputs and measured values. No scale or VM run was performed here.

## Checked and found correct

- The public `BulkParams`, `BulkStats`, `estimate_memory`, and `run_bulk` signatures match the candidate plan. New implementation helpers remain private. Errors use `thiserror`; production W2-B adds no unsafe code, file-content reads, filesystem writes, recursive traversal, library printing, or unexplained `unwrap`/`expect`.
- Phase 1 forces namespace mode. Dent references are 12 bytes; 31-bit slot overflow is checked before setting the self bit, with the additional documented one-million-directory and 256 MiB dent limits. Name requests, source batches, worker counts, and both channels have explicit caps. Estimate arithmetic rounds up and saturates. These bounds do not establish total-budget compliance (finding 2).
- The merge join counts namespace-only references, missing directory self-references, and directory/non-directory type mismatches as races. Bulk-only inodes are ignored, as specified. It preserves legitimate repeated non-directory hard-link instances and full `u32` link counts, including counts above 65,535. `disk_bytes` is copied without another multiplication. Stale multiple references to an inode now reporting one link are discarded as races.
- Duplicate/nonascending bulkstat records and records outside an AG range fail explicitly rather than silently double-counting. This deliberate failure policy is distinct from the duplicate namespace-directory defect in finding 3.
- Only v5 supplies bulk metadata. `Unsupported` from the iterator becomes `BulkError::Unsupported`. Iterator construction is lazy; Gate charging precedes every advancement, including terminal/error calls, with an initial geometry allowance. Directory opens, fallback opens, getdents refills and fallback stats are gated through the existing helper.
- Allocation groups are partitioned by inode range and claimed by bounded workers. The collector drains channels before joining workers on success/error. No successful-path lost batch or channel deadlock was found by inspection. A worker error does not stop peer workers claiming remaining AGs; propagation waits for them to finish. Multiworker failure timing was not fault-injected.
- Names are grouped/deduplicated by byte path and inode, and ordinarily resolved in raw-name order with missing entries counted. Independent probes confirmed one directory read for several needed names and rejection of a symlink in an ancestor component. The helper uses `O_NOFOLLOW` on every component and `O_NOATIME` with EPERM fallback.

## Coverage and limits

The join, arithmetic, preflight, and basic name tests are meaningful. Privileged tests cover all eight mandatory fixtures plus `mixed_special_root`, compare the required row types with only the allowed project-ID/link-instance-name normalization, and are feature-gated. Only the scale VM test is marked `ignore = "scale"`.

`synthetic_bulk_equals_walk` is present but ignored as `needs W2-A`. At the exact candidate, both aggregator `prepare` and `write` are stubs, so final-row equivalence cannot be established. Once enabled, this test would catch some join/aggregation divergences, but it constructs namespace records and supplies names directly from the model. It bypasses the production namespace collector, AG workers, and resolver, and would miss finding 4. It does not assert that another task must remain a stub.

Generation-only inode reuse, or changes among regular/symlink/other types, cannot be recognized from this namespace representation: it retains inode and directory/non-directory identity, not generation or the original non-directory kind. Those changes are accepted as fuzzy phase-2 metadata under `04` §3.2, not silently claimed to be detected races.

No privileged tests, mounts, destructive commands, VM connections, real-XFS performance measurements, Rocky 8.8/8.10 equivalence, or bulk-only RSS acceptance checks were run. Filesystem-dependent numbers in the probes were obtained from metadata; the 4,096-byte duplicate-directory input is synthetic. End-to-end equivalence and final accounting remain unverified at this candidate.

## Build and test results

The later checkout was not substituted for the candidate. An unchanged `git archive` of the exact candidate was built under the checkout's ignored `target/w2b-review-candidate/`. Rust 1.98.0 and a temporary copy of the host Cargo cache were used with `CARGO_NET_OFFLINE=true`; no dependency fetch was attempted. Required commands ran against that candidate archive.

| Check | Observed result |
|---|---|
| `tools/ci.sh` | Exit 0: source checks, fmt, Clippy with warnings denied, and workspace tests passed; 192 passed, 0 failed, 4 ignored. cargo-deny unavailable, audit skipped. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compile only. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0. |
| Independent name probes | 2 passed; excluded-alias equality failed as described in finding 4 (exit 101). |
| Independent duplicate-directory, join-cancellation, phase-1 cancellation/error probes | All compiled and exited 0 while asserting/printing the observed faulty behavior described above. |
| Candidate diff whitespace check | `git diff --check` between the pinned base and candidate passed. |

Build logs, probe sources, binaries and outputs are retained in this attempt's worker-output `library/`, under `attempt-f5d52e8403e306deddd4476bda47a93176d05574b47c37e82afac55a7e67ce1f`. Main logs are `candidate-ci.log`, `candidate-privileged-build.log`, and `candidate-rustdoc.log`, each with an exit-status file. This review is an evidence candidate, not integration approval or a declaration of verified task success.
