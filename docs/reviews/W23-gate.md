**NOT READY — 1 BLOCKER, 9 SHOULD-FIX, 1 NIT.**

This is an independent review evidence candidate, not acceptance, integration, or permission to release. The counts refer to the eleven outstanding findings below, including the carried W2-B RSS finding; pending human measurements are listed separately.

## Candidate, scope and method

Reviewed the whole tree at **`88fcd81cd3de025b7b6c0f0f3869023fd18e14b0`**, with blind-review base `926a31f3062f6591c035bf3e2fe4ab983d1521d8`. The exact delta changes `tidemark-store/src/links.rs`, its W3-B tests and `W3-B-fix.md`; both original W3-B defects are resolved. The deliverable checkout starts at `6d9973c6fde900c2bd21371d750181d172134b09`, which additionally contains fourteen later packaging-related files/changes. Those additions were excluded from judgment and builds. The exact candidate was exported under ignored `target/review-candidate`; source and tests there were not modified. Only this report is a tracked deliverable.

Read AGENTS, the complete Wave 2/3 plans, their relevant read-first specifications, contract decisions including D1–D5/R2-1–R2-10, ADR-018/019, playbook §5/7, Wave 6 interfaces, original reviews and the named task/fix/finish/integration notes and SCRs. Canonical task names follow the wave file: W2-A aggregation, W2-B bulk; the walker is W1-B. Scan/safety, query and cross-contract inspection ran in parallel. Source locations below are relative to the reviewed candidate, with `crates/` omitted in compact tables.

Evidence artifacts are under `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-0037422f9c2bb43a8e24ac728f718bbfe610cb1373b7d8658858e075ee6b5826/library/`. The supplemental `scan-safety.md`, `queries.md` and `contracts.md` record detailed source/probe provenance. No VM was contacted; no privileged test, mount, LVM modification, service operation, branch creation, merge or push was performed.

## Original review findings

“RESOLVED” means the inspected defect has a corresponding implementation and meaningful regression, exercised by the exact-candidate CI unless an explicit run is named. It does not claim VM or performance acceptance.

| Original review finding | Status | Candidate evidence |
|---|---|---|
| W2-A #1 unbounded bulk candidate retention | RESOLVED under R2-3 | `scan/src/agg/notable.rs:40`, `agg/mod.rs:348`; `bounded_matches_unbounded`, `cutoff_ties_keep_each_directorys_smallest_inodes`. Retained cutoff ties depend on D×M as expressly decided. Distinct-size CPU cost remains S2 below. |
| W2-A #2 sampled errors used as totals | RESOLVED | `agg/mod.rs:144,292`, `rollup.rs:110`; `tests/w2a/main.rs:800` checks 20,140 errors with one detail in both modes. SCR-008 adds checked `errors_total` through all producers. |
| W2-A #3 error-order nondeterminism | RESOLVED | `agg/mod.rs:237`, `rollup.rs:139`; `error_order_below_at_and_above_cap_preserves_record_order` compares reversed arrival and within-record order. |
| W2-A #4 four-million directory ceiling | RESOLVED under R2-3 | `agg/mod.rs:44,133`; sparse boxed slots, 16M ceiling. `directory_limit_and_five_million_discovery_id` checks sparse 5M/15,999,999 and refusal at 16M; layout test documents 232+8 bytes. |
| W2-B #1 phase-three replacement mount | RESOLVED in implementation; VM runtime pending | `bulk/names.rs:79`, `fs/src/walk/getdents.rs:152`; checked component opens, device/mount/final-inode checks. `checked_read_rejects_symlink_replacement_and_outside_root`, `replaced_directory_names_remain_unresolved`; real replacement mounts have a compiled privileged regression. |
| W2-B #2 worker/batch memory reserve | RESOLVED for decided reserve | `bulk/mod.rs:98,402`; 32MiB admission before namespace work, effective workers, bounded message/ref spill. `worker_memory_boundary`, `reference_spill_cap_is_checked_before_allocation`. Not a process-RSS proof. |
| W2-B #3 duplicate directory identities | RESOLVED | `bulk/join.rs:132`; `duplicate_directory_identity_is_a_counted_fatal_race` checks two raced refs and no attribute callback. |
| W2-B #4 excluded alias used as notable name | RESOLVED | `bulk/names.rs:92`; `names_exclude_smallest_alias`, `excluded_alias_namespace_bulk_equals_walk` exercise actual namespace/join/name code. |
| W2-B #5 cancellation around gaps/tail | RESOLVED | `bulk/join.rs:211,229,266,275,286`, `bulk/mod.rs:166,390`; `cancellation_between_gap_and_tail_batches`, `cancellation_during_namespace_setup_is_normalized`. Blocked Gate latency is separate B1. |
| W2-B #6 pre-phase-two fallback classification | RESOLVED | `bulk/mod.rs:177,184,194,201,390`; `namespace_failure_is_redoable_and_cancellation_is_not`. Only Unsupported/BeforePhase2 auto-fallback uses a fresh aggregator. |
| W2-B #7 misleading RSS smoke | PARTLY RESOLVED | `tests/w2b/vm.rs:112` honestly renamed timing smoke and removed false VmHWM evidence. It still has no isolated bulk RSS/120% assertion: S9. |
| W2-C #1 record memory budget | PARTLY RESOLVED | `orchestrator.rs:733` implements R2-1 formula, checked preflight/fallback admission; walker charges semantic collections and rejects instead of truncating. `record_limit_derivation_table`, record-limit regression pass. Final notable merge exceeds the bound: S1. |
| W2-C #2 publication cancellation | RESOLVED under R2-4 | RowSink batch checks, `publication_status`, `finish_snapshot`; store `finish_cancels_at_every_publication_checkpoint` exercises all ten checkpoints. Scan regression cancels after first index for complete/kept-partial candidates, checks failed catalog row, cleanup and existing-file preservation. |
| W2-C #3 failed completion leaves running | RESOLVED | `orchestrator.rs:1019,1059` tracks successful acknowledgment separately. `rejected_completion_is_finalized_as_failed_preserving_original_error` and independent lifecycle probe check failed fallback, preserved published file and original error. |
| W2-C #4 prune leaves broken query references | RESOLVED under R2-5 | Automatic retention at `orchestrator.rs:554` rebuilds; CLI prune rebuilds; optional top ignores unavailable comparison. `automatic_retention_rebuilds_catalog_after_deleting_old_files`, `prune_rebuilds_catalog_for_remaining_files` include subsequent queries. |
| W2-C #5 scoped rebuild mutates other roots | RESOLVED under R2-5 | `cmd/snapshots.rs` refuses scoped rebuild before mutation; `scoped_rebuild_refuses_without_changing_other_root_rows` preserves complete/failed/running rows. Global rebuild remains explicitly global. |
| W2-C #6 growth prefix truncation | RESOLVED | `cmd/top.rs` requests 1,000,001 to trigger the reader cap. Explicitly ran `growth_reader_cap_refuses_instead_of_ranking_disk_prefix`: passed, 43.77s. It refuses excessive candidates rather than computing a false rank. |
| W2-C #7 emergency prune omits expired partial/failed | RESOLVED | `orchestrator.rs:975`; `emergency_prune_includes_expired_partial_and_failed_artifacts` covers both; recent/latest protection regression retained. |
| W2-C #8 failed walk ignores keep-partial | RESOLVED under current 04 §8 | `orchestrator.rs:311,1076` retains valid records after cancellation/I/O/resource failure; panic/disconnection fail closed. `partial_walk_failure_classification`, `failed_walk_keeps_valid_collected_records_only_when_requested`. |
| W2-C #9 bulk lifecycle missing | RESOLVED under D2 | `orchestrator.rs:354` forwards progress/noatime, distinguishes failure phases and normalizes cancellation. Namespace progress/fallback tests pass; bulk partial output is now expressly forbidden. Real auto-XFS execution pending VM. |
| W2-C #10 malformed options wrong exit | RESOLVED | `cmd/scan.rs:49`, `orchestrator.rs:81` validate before lifecycle. `invalid_scan_overrides_exit_two_without_catalog_mutation` checks 0/65 threads and invalid glob, exit 2. |
| W3-A explain window memory (`w3-a-explain-window-memory`) | RESOLVED | `store/src/diff.rs:332` separates streaming count from bounded top-N. Explicit ignored `wide_explain_uses_streaming_count_and_bounded_top_n` passed, 29.41s, 200K children, three metrics and query variants; production SQLite bytecode checked. |
| W3-B crossing sample cap before eviction | RESOLVED | `store/src/links.rs:299` checks retained−displaced+new and removes old payload before replacement allocation; both callers share it. Unit `crossing_replacement_at_sample_cap` and full `crossing_sets_replaces_at_exact_sample_cap` pass, including genuine overflow. |
| W3-B quadratic large-top selection | RESOLVED | `links.rs:243,449,524` bounded worst-first heaps and one final sort. `largest_supported_limit_has_logarithmic_selection_work` checks 2M candidates/top 1M comparison bound; increasing-stream integration checks ordering through top 1M. |

W3-C's original review had no counted finding; its three inherited observations are independently confirmed as S5–S7. W3-A's removed-path behavior also passes its existing byte-path/parent-component regressions: realpath is attempted first; an unresolved `..` is refused instead of normalized into a different snapshot path.

| Inherited gate / finishing issue | Status and evidence |
|---|---|
| W1 G1 v1/v5 bulk contract | RESOLVED: mode selection requires v5; explicit unsupported bulk reports precondition; sys uses exact 32-bit v5 nlink. Truth table/ABI tests pass; actual EL8 ioctl support pending VM. |
| W1 G2 factory loses stats handle | RESOLVED: `fs/src/throttle.rs:407` ScanGate exposes gate/stats/stop and Drop joins; factory/retained-gate tests pass. |
| W1 G3 walker tie memory | PARTLY RESOLVED: per-directory top-N and 4096 threshold refusal have backpressure regressions; R2-1 bound now exists, with S1 still open. Global-threshold seam remains an unanswered proposal. |
| W1 G4 partial mount parse loses boundaries | RESOLVED: `fs/src/mount.rs:52` preserves independently decoded boundaries and fails closed; malformed/nsfs regressions pass. |
| W1 G5 corruption preflight destroys artifacts | RESOLVED: shared `store/src/timeseries.rs:32,97` header preflight precedes both Catalog and TimeSeries opens. Tests exercise both rw modes and preserve DB/WAL/SHM before actual recovery. |
| W1 G6 VM env/nocapture transport | RESOLVED locally: `xtask/src/vm_test.rs:26,103` validates before SSH, quotes assignments and positions test flags; metacharacter tests and documented md commands present. Remote execution pending. |
| W1 G7 floating token precision | RESOLVED: u128 nano-token debt/remainder, no waiter collection, safe overflow admission/rebasing. Tests cover 2^53, u128 limits, fractional credit and elapsed multiplication. |
| W1 G8 duplicated path escaping | RESOLVED: mount/raw-path/raw-name share `display_lossy_bytes`; three-entry-point regression passes. |
| W0 gate recheck R1 substituted-parent cleanup | RESOLVED for tested serial substitution: `test-support/src/fixture.rs:117,161` disables TempDir pathname deletion and uses bounded descriptor-relative cleanup; sentinel regression passes. Concurrent rename atomicity is not claimed. |
| W0 gate recheck R2 renamed mode-000 child leak | RESOLVED: same file `:172,232` matches retained identity handles; internal-rename and moved-outside regressions pass. Privileged XFS/LVM helpers use OwnedStorage; real teardown pending VM. |
| CI-fix-bench/w1b/w2a and STORE-seams missing constructors | RESOLVED: all production bounds propagated; explicit test/benchmark bounds and String Limit guards compile. Workspace CI and privileged compilation pass. |
| W2-integ old dependency stubs/ignored acceptance | RESOLVED: no scan NotYetImplemented or `needs W2-A` ignores remain; end-to-end/oracle/partial tests run. Mixed-special socket fixture has the environment exemption below. |
| W1-A genuine EL8 mountinfo capture input | NOT RESOLVED, NEEDS-HUMAN: supplied captures remain labelled synthetic; current pending-input test is not evidence from EL8. |

## Outstanding findings

### B1 — BLOCKER: a cancelled scan can remain inside Gate acquisition beyond daemon shutdown deadline

Reference `finding:w23-gate-b1`. `crates/tidemark-fs/src/throttle.rs:123`, `crates/tidemark-scan/src/bulk/join.rs:211`, `bulk/mod.rs:230`; Wave 6 shutdown requires exit within ten seconds.

A valid `throttle.min_ops_per_sec=1,max_ops_per_sec=1`, default bulk batch 4096, and 512-byte XFS inode geometry require 129 tokens for the first bulk advancement. Initial allowance is 0.25 token; the acquisition can take approximately 128.75 seconds. Cancellation is checked outside the blocking call. `ScanGate::stop` intentionally leaves waiting acquisitions at their last rate (`throttle.rs:432`). `run_scan` owns this handle and joins its workers.

An independent probe against the exact candidate factory validated this configuration, set cancellation between the pre-acquire check and the acquisition, and called stop. Stop returned in 2.83µs; the acquisition still had not returned after 11.000083091s. This is a userspace delay, without XFS or privileged execution (`shutdown-gate-probe.rs/.log`). Preparation/index work also lacks a ten-second bound; no bounded kernel-I/O deadline is claimed.

**Recommended change:** agree a cancellable Gate wait contract, wake outstanding reservations on shutdown and return cancellation before issuing metadata I/O. Add deterministic reservation/cancellation coverage and a daemon shutdown regression with an acquisition already blocked at a low legal rate. Do not silently change G2's stop semantics or the frozen Gate signature.

**Files for that design:** an SCR/decision and `docs/plan/{04-scanner,13-contract-decisions}.md`, `waves/wave-6-daemon-packaging.md`; `crates/tidemark-core/src/scan.rs` (Gate/NoopGate), `tidemark-fs/src/throttle.rs` (FixedGate/AdaptiveGate/ScanGate), `walk/{worker,getdents}.rs`, `tidemark-scan/src/{orchestrator.rs,bulk/mod.rs,bulk/join.rs}`, daemon scheduler/shutdown callers and tests. If altering the required existing method, also adapt `tidemark-fs/src/symlinks.rs`, all Gate implementations/callers in `fs/tests/{w1b/main.rs,w1d/main.rs,w3b.rs}`, `scan/tests/w2b/main.rs` and bulk's CountingGate. A compatible additional method still requires callers to honor cancellation.

### S1 — SHOULD-FIX: notable-vector merge exceeds R2-1 record byte bound

Reference `finding:w23-gate-s1`. `crates/tidemark-fs/src/walk/worker.rs:673,752`.

Charging reserves twice the NotableFile layout, names/padding and twice the directory-heap tuple. The final `r.notable.extend(top.into_iter().filter(...))` keeps the original top allocation while growing a separate output vector, with the directory heap still live. For 65,537 eligible seven-byte-name single-link candidates, threshold u64::MAX and top_files 65,537 (supported), the default budget with four workers derives a 16MiB record bound. Charges are 13,041,863 bytes plus small baseline. Simultaneously live vector/heap/name capacities total **18,284,551 bytes**, excluding allocator overhead/baseline.

`record_merge_probe.rs/.log` uses actual candidate NotableFile/RawName and record_limits with the exact push/filter/extend operations under pinned Rust. This is an executable capacity reconstruction and source-lifetime proof, not a full filesystem or RSS measurement. Namespace/link/skip charges and queue arithmetic otherwise exist; aggregate memory remains estimated under the accepted cgroup policy.

**Change/files:** in `tidemark-fs/src/walk/worker.rs`, reuse or transfer storage where possible and charge all coexisting merge allocations before reserve; drop unneeded heaps before merging. Add power-of-two-plus-one top/threshold cases in its unit tests or `tidemark-fs/tests/w1b/main.rs`. No core constructor/interface change is needed. Keep W2-C #1 partly resolved until this regression enforces the peak, not only payload count.

### S2 — SHOULD-FIX: notable ingestion repeatedly scans every retained size bucket

Reference `finding:w23-gate-s2`. `crates/tidemark-scan/src/agg/notable.rs:74`.

After each candidate insertion, the algorithm walks retained buckets to recompute the cutoff. With distinct sizes this costs O(inputs×top_files), or O(inputs×max_notable) for threshold candidates, including discarded candidates. Default top_files is 10,000. An exact-library probe ingesting 30,000 increasing distinct-size single inodes measured 639ms at top 100, 4,248ms at top 1,000 and 24,190ms at default top 10,000 (`candidate_insert_probe.rs/.log`). These debug/shared-host times are illustrative; the repeated full map traversal establishes the complexity independently.

**Change/files:** maintain candidate totals/cutoff incrementally and remove only losing buckets in `tidemark-scan/src/agg/notable.rs`. Preserve R2-3 ties/final-id ordering. Add a large distinct-size operation-count regression or benchmark there or in `tests/w2a/main.rs`; small-cap/equal-size existing tests do not exercise this cost. No shared caller changes.

### S3 — SHOULD-FIX: orchestrator root preflight protects only the final path component

Reference `finding:w23-gate-s3`. `crates/tidemark-scan/src/orchestrator.rs:121,684`.

`open_directory` issues an absolute multi-component `open(...O_NOFOLLOW)`; that flag protects the leaf only. Between CLI canonicalization and this call, renaming an ancestor and replacing it with a symlink to another directory permits following the ancestor. If the replacement remains on the same device, the subsequent st_dev check does not reject it before fstatvfs/XFS preflight probes. The normal walker already uses component-relative no-follow opening and can subsequently refuse; that does not make the earlier metadata access conform to rule 4. This finding is source verified; no adversarial live race was executed.

**Change/files:** use a component-relative no-follow directory-opening helper for root preflight, preserving descriptor identity and NOATIME behavior. Touch `tidemark-scan/src/orchestrator.rs` and its tests; if sharing the existing filesystem helper, expose it through `tidemark-fs/src/walk/{getdents,mod}.rs` with a documented contract. Add a deterministic ancestor-substitution regression that rejects before metadata probes. CLI canonicalization remains first; it is not race protection.

### S4 — SHOULD-FIX: retrying metadata calls bypass Gate accounting

Reference `finding:w23-gate-s4`. `crates/tidemark-scan/src/orchestrator.rs:121,684`; `crates/tidemark-fs/src/symlinks.rs:247`.

The orchestrator charges once, but an O_NOATIME EPERM fallback performs a second open without another charge. Separately, one acquire precedes allocating `rustix::fs::readlinkat(Vec::new())`. The exact locked rustix 1.1.5 implementation starts at 256 bytes and retries on a full buffer; a target of at least 256 bytes performs multiple syscalls per token. Dependency source establishes the retries; syscall tracing was not available. Walker and checked-read open retries already charge each attempt correctly.

**Change/files:** charge inside every opening attempt in `orchestrator.rs` (or its shared helper from S3). In `tidemark-fs/src/symlinks.rs`, use a reusable capped buffer with `readlinkat_raw`, acquiring before each retry. Add fallback/long-target attempt-count regressions in the orchestrator tests and `tidemark-fs/tests/w3b.rs`. Preserve raw targets and error classification; no public type changes are required.

### S5 — SHOULD-FIX: ncdu remainder label counts symlinks twice

Reference `finding:w23-gate-s5`. `crates/tidemark-store/src/export_ncdu.rs:106`.

`own_files` already counts all non-directory entries; `own_symlinks` is its subset. Adding both yields a false `[other files: N]` label. A public-exporter probe with own_files=3/own_symlinks=1 emits `[other files: 4]` (`queries/probe.rs/.log`). Existing disk-sum tests do not check this count.

**Change/files:** initialize remainder count from own_files alone, then subtract individually emitted notable rows. Modify `tidemark-store/src/export_ncdu.rs`, `tidemark-store/tests/w3c.rs`, and add a scanned symlink-label assertion in `tidemark-cli/tests/w3c.rs`.

### S6 — SHOULD-FIX: depth-truncated ncdu export switches apparent-size semantics

Reference `finding:w23-gate-s6`. `crates/tidemark-store/src/export_ncdu.rs:182`.

The truncated apparent remainder uses union `t_apparent`, whereas normal export sums additive `own_apparent`. A synthetic snapshot representing a link shared between `/r/a/d` and `/r/b` charges it at `/r`; exporting `/r/a` fully produces asize 32, but max-depth 0 produces 532, including 500 bytes charged outside the exported subtree. Both dsize totals are 32. Exact exporter probe reproduces this (`queries/probe.log`).

**Change/files:** derive truncated apparent bytes from checked, streamed descendant own_apparent totals, consistently with charged export; avoid t_apparent. Touch `tidemark-store/src/export_ncdu.rs` and `tidemark-store/tests/w3c.rs`, with outside-subroot hard-link depth comparisons. No schema/API change is necessary.

### S7 — SHOULD-FIX: notable-file export paging has quadratic query work

Reference `finding:w23-gate-s7`. `crates/tidemark-store/src/export_ncdu.rs:47`; `migrations/snapshot-v1.sql:77`.

Each 256-row page filters/keysets and sorts the same directory's notable rows without an index matching `(dir_id,name,ino,id)`. An independent probe with the exact query/index layout and bundled SQLite counted 26,966,632 VM steps for 20,000 rows and 106,983,232 for 40,000, a 3.97× increase when input doubles (`queries/probe.rs/.log`). Bounded output batches do not bound total work. The existing 4,097-file regression only establishes streaming correctness.

**Change/files:** use a bounded disk-backed sort within Tidemark's state directory or obtain approval for an ordering index plus a query that seeks it. Index design affects the frozen schema: require an SCR and update `docs/plan/05-storage.md`, `13-contract-decisions.md`, `migrations/snapshot-v1.sql`, `tidemark-store/src/export_ncdu.rs`, schema/query tests in `tests/{storage,w3c}.rs`. Adding an eighth index also changes the documented R2-4 checkpoint count and `finish_cancels_at_every_publication_checkpoint`; verify `writer.rs` index iteration and scan publication tests. Do not silently change the schema or seven-index contract.

### S8 — SHOULD-FIX: diff conceals partial/degraded input as ordinary shrink

Reference `finding:w23-gate-s8`. `crates/tidemark-cli/src/cmd/diff.rs:74,151`; `src/render/diff.rs:35`.

Explicit IDs can select partial/degraded snapshots, but the command drops input metadata and prints only deltas. A real SnapshotWriter/catalog/CLI probe compares complete 100 to partial+degraded 40: exit 0, empty stderr, ordinary `−60 B` text, and JSON with notice=null and no input provenance (`queries/partial-diff.rs/.log`). An administrator cannot distinguish incomplete coverage from actual shrink. Normative 03 §7 requires snapshot ID/mode/age/degraded in every report header; unlike top/files/links/wide text, diff has none.

**Change/files:** pass both snapshots' provenance to `cmd/diff.rs`/`render/diff.rs`; display explicit partial/degraded qualification and input IDs/modes/ages, without necessarily prohibiting explicit partial comparisons. Update every direct renderer caller in `tidemark-cli/tests/w3a/{catalog,reports}.rs` and associated goldens. Agree any changed JSON envelope contract before implementing it. Related top/files/wide JSON and ncdu provenance omissions are recorded under contract drift below, not silently assigned a new shape here.

### S9 — SHOULD-FIX: isolated bulk RSS acceptance still has no measuring test

Reference `finding:w23-gate-s9`; carried W2-B #7. `crates/tidemark-scan/tests/w2b/vm.rs:112`.

The replacement million-file test times walk then bulk while retaining comparison rows; it deliberately performs no isolated peak measurement and no 120%-of-estimate assertion. This corrected the misleading evidence but left the stated acceptance criterion untestable by the supplied command. Merely running that test on a VM cannot close the finding.

**Change/files:** add a dedicated bulk-only child invocation after fixture setup, excluding compilation, earlier walk and retained VecSink storage from the measured peak; record exact input counts/estimate and assert the agreed 20% envelope. Touch `tests/w2b/vm.rs` and any dedicated helper it uses, plus `docs/handoff/W2-B-fix.md` with actual VM results. No production memory-contract change is implied.

### N1 — NIT: scan provenance and phase timestamps remain incomplete

Reference `finding:w23-gate-n1`. `crates/tidemark-scan/src/orchestrator.rs:304,314,457,466`.

The measured walk duration is immediately overwritten with setup-inclusive elapsed time; finished_at is captured before row writes/indexing/sync; os_release is always empty. Successful-scan tests validate totals and status rather than these provenance fields. These were disclosed in the original W2-C review and remain source-visible.

**Change/files:** define the intended phase/finalization boundaries, keep actual walk duration, fill bounded OS-release metadata through a metadata-discovery seam, and inject a clock for deterministic timestamp tests. Update `orchestrator.rs` and its tests/`tests/w2c/main.rs`; coordinate any changed timing definition in `04-scanner.md`/`05-storage.md`. No scanned regular-file content read is needed.

## Cancellation and failure lifecycle audit

The common finalizer now records `failed`, never the unsupported `cancelled` catalog status. Kept-walk cancellation is consumed atomically at the publication boundary; a subsequent signal is visible to finish_with and discards even a partial candidate. The acknowledged completion flag prevents series/retention errors from rewriting an already completed scan as failed.

| Point / scenario | Catalog and artifacts | Result / lock / evidence |
|---|---|---|
| Invalid config or resource/space/mode preflight before scan_started | No new lifecycle row or snapshot; space guard may prune eligible old files by policy | CLI configuration 2; unmet precondition/resource/space/busy 5. Lock RAII on every return. Exact option/space/lock tests pass. |
| Cancellation already set, ordinary walk | Started row finalized failed; no `.db`/`.db.tmp` | ScanError::Cancelled, CLI 3; independent probe reacquires lock. |
| Cancellation already set, keep-partial walk | Partial root-only snapshot may be retained; catalog partial; never latest | Independent probe confirms partial metadata, one `.db`, no temp, reacquired lock; CLI partial maps to 3. |
| SIGINT during active walk, no keep | Failed row with no file; no snapshot directory in fixture | Actual subprocess regression synchronizes on positive progress, exits 3, passes. Signal timing is not inferred from a sleep. |
| SIGINT during active walk, keep | Partial row/file; catalog.latest excludes it | Actual subprocess regression exits 3 and verifies snapshot partial. |
| Recoverable fatal walk I/O/Limit | Failed/no candidate without keep; valid collected records may become partial with keep | Classification and real failing-directory collection regression pass. Panic/disconnection remain fail-closed; invalid partial aggregation still fails. |
| During prepare / row output | After prepare, cancellation aborts or selects kept walk partial; WriterSink checks every batch | Ordinary cancellation maps to 3; other aggregation/writer errors map to runtime 1. Preparation itself is not promptly interruptible; B1/readiness limitation remains. |
| During finish indexes, before metadata, after file sync before rename | Failed row; new candidate and temp removed; existing snapshot unchanged | Store test cancels each of ten checkpoints; scan test injects second checkpoint, complete and partial. Cancellation 3. All pass. |
| Creation collision | Existing file preserved; old attempted lifecycle finalized; fresh ID/start attempted, bounded to 1024 | Retry test passes. Failure propagates and common finalizer handles the unfinished attempt. |
| Publication collision | Existing `.db` preserved; candidate temp removed; failed row | Typed PublicationCollision, CLI runtime 1, no retry under D4; regression passes. |
| Rename succeeded, directory fsync fails | A final file can remain; finish returns error before entry.file is set, so failed row can have no file | Source trace `writer.rs:354` through finalizer. Runtime 1, lock drops. Global rebuild can discover the complete immutable file. Directory-fsync failure was not fault-injected. |
| Complete publication, completion recorder rejects | Failed update attempted; published complete file preserved; failed row excluded from latest | Independent probe reproduces rejection, verifies preserved file/no temp/lock release, then rebuild recovers complete metadata. If recorder also rejects cleanup, original error retained/logged; helper regression covers this. |
| Series callback fails after successful completion | Complete row/file remain and can be latest; no temp | Independent probe confirms error return and lock release. No false failed rewrite; CLI maps actual series/storage runtime failure to 1. Probe injects a Resource error rather than a CLI subprocess. |
| Retention/rebuild fails after successful completion | Complete row/file stay acknowledged, though earlier retention deletions may already have occurred | Source trace; error returned and lock dropped. No injected deletion/rebuild failure in this review. |
| Cancellation after publication | Published completion remains successful | No post-rename cancellation callback by R2-4. Independent recorder probe sets cancel after persistence; run_scan returns Complete and releases lock. |

Independent `lifecycle_probe.rs/.log` exercises normal, pre-cancel, pre-cancel+keep, rejected complete, rejected series and post-publication cancellation using production APIs. Every case checks zero temporary snapshots and successful lock reacquisition. The initial probe incorrectly requested a rebuild through a read-only Catalog; the harness was corrected to open writable storage and all six cases then passed. This was a probe error, not a candidate failure.

The recorder protocol cannot force an unavailable storage sink to persist failure. `scan_started` errors return before the common work finalizer; a third-party recorder that commits and then returns an error can leave ambiguous start state. Start callbacks must acknowledge persistence consistently, and startup recovery must handle crash/unacknowledged running rows. Process termination/power loss, arbitrary I/O failure at every syscall, and post-rename fsync failure were not exercised. R2-4 explicitly provides checkpoints rather than atomic cancellation with rename.

## Cross-task safety and contract consistency

- Normative 03 Steps A–F were spot-checked against link resolution/LCA/union stamps/charged/reclaimable rollups and the literal worked example. Golden, random reference, shuffled-order and synthetic walk/bulk equivalence tests pass. No new byte-unit or link-accounting discrepancy was found. Bulk disk_bytes is already bytes and is not multiplied twice.
- Aggregator traversal and output use iterative structures, sparse numeric child IDs and checked caps. Its 240-byte node-plus-slot layout and D×M cutoff tie allowance are accepted R2-3 limitations, not a hard process memory guarantee. Channels and namespace/join buffers have explicit bounds. R2-1 resource refusal happens before scan lifecycle start, including fallback admission, but S1 invalidates the claimed final candidate merge bound.
- Both phase-three name resolution and `links --show-paths` use `read_dir_entries_checked`, final expected inode, component-relative no-follow opens and device/available mount-ID checks. Missing/replaced directories become unresolved/gone; other errors are handled per the agreed contract. Without kernel mount IDs, same-device binds preserving the expected inode remain the explicit R2-2 limitation, not a silently claimed guarantee. Real mount races remain pending VM.
- No scanned regular-file contents are read by the reviewed scan/query paths; proc capability/OS metadata and Tidemark's own SQLite state are distinct. No new unsafe code outside tidemark-sys or mutable filesystem/LVM/mount operation was found in production scan/query source. Source-policy CI passed. Root setup and retry charging have the specific exceptions S3/S4.
- Paths remain bytes; raw-name ties, final IDs and output sorting are deterministic in reviewed algorithms. Diff attaches old snapshots using bound immutable/read-only URIs; guards detach and restore mapping limits on failure/unwind. `Snapshot::conn/file` are `pub(crate)`; external crates cannot reach the SQLite connection through those seams. No external call circumventing them was found.
- CODE-drop-postgres leaves no collector/crate/CLI/config/metric/schema feature in production surfaces checked. Searches found only intentional negative config tests for `[postgres]` and `socket_group`; historical/deferred documents are not active behavior. Generic annotation test uses `app.label`. Generic filesystem/LVM coverage remains; the discarded forced-v1 test conflicts with the decided exact-v5 contract. This review did not reconstruct every historical deletion.
- All inspected privileged tests are feature-gated; only genuine scale tests additionally use `ignore = "scale"`. No dependency-placeholder ignores remain in the scan/query tests. Synthetic accounting literals and 512-byte stat units are valid; measured disk/du assertions avoid assuming this host's allocation size. The main remaining observation-dependent coverage is performance/VM behavior, not a claim that passing host tests establishes it.

Contract drift needing a decision: diff text lacks the required header (S8). Top/files JSON remains arrays of rows, wide JSON lacks input metadata, links JSON has ID/mode/age/degraded but no status, and ncdu metadata has program/version/timestamp/charged-note only. Existing JSON contracts must not be reshaped casually; reconcile “every report header” with these schemas and explicit partial selection. If a common envelope is approved, affected surfaces include CLI `render/{tree,diff,wide,links}.rs`, their `cmd/{top,files,diff,wide,links}.rs` callers, store export, W2-C/W3 renderer tests and goldens. Links text ages from started_at while links JSON ages from finished_at; the chosen age basis should be unified. Same-filesystem indirect symlink classification remains lexical under the specified stat-only target algorithm; strengthening that behavior requires an explicit resolver decision.

## Wave 6 readiness

The necessary scan/store types and functions exist; **Wave 6 cannot claim its specified shutdown behavior on this candidate**. Its daemon service implementations are preparation stubs, so there is no daemon runtime acceptance evidence here.

| Wave 6 need | Shape / integration constraint |
|---|---|
| `run_scan`, ScanRequest/Outcome/Error, ScanRecorder | Exported with the Wave 2 signatures. The three recorder callbacks exist; root/degraded/mode/summary fields line up. |
| Host-wide scan lock | `run_scan` acquires ScanLock itself. Delegate ownership to it; a scheduler that first locks another descriptor and then calls run_scan receives Busy. Clarify “scheduler holds” in the plan; do not double-acquire. |
| Single time-series writer | ScanStarted/ScanFinished stub messages have acknowledgment senders. Callback success must wait for persistence: run_scan immediately reads/rebuilds the catalog after completion. Merely queueing the message races that read. The proposed DirSeries message has no acknowledgment; define queued versus persisted success/error reporting. |
| Directory series | `TimeSeries::insert_dir_series`/query exist. The direct `record_dir_series` helper opens a new writer, so the daemon must implement first-observation-per-second coalescing on its owned writer connection instead. |
| Growth before retention | `diff` and `retention_plan` exist, but run_scan performs retention **before returning**. With recent=1/daily=0/weekly=0 it can delete the predecessor that post-return diff reopens by pathname. Compute growth synchronously in a completion hook before retention, or agree a post-publication seam/order change. This is a real integration mismatch, not a claim that an unimplemented daemon already fails. |
| Complete bounded growth inventory | Diff caps requested output at 1M and returns a limited list. R2-10 complete inventories require the daemon to detect/represent potential truncation as unavailable, or use an approved complete bounded traversal; a returned top-delta prefix alone is not complete. |
| Gate handle/stats | The orchestrator retains and stops ScanGate and persists statistics. Scheduler cannot separately stop its hidden handle; it delegates normal lifetime control. Stop currently cannot cancel outstanding acquisition, B1. |
| Cancel on shutdown, keep no partial | Pass keep_partial=false and set AtomicBool. Publication checkpoints work, but userspace gate waits and uninterruptible prepare/index spans do not establish ten seconds. B1 requires resolution and a real daemon shutdown regression. |
| Retention/recovery | APIs exist; pruning/rebuild is global under R2-5. Startup recovery must stop writers and close connections first; run_scan deliberately uses ordinary non-destructive opens. Do not duplicate a concurrent catalog writer without reconciling this single-writer policy. |

## Gate checklist tables

PASS below is limited to the stated evidence. NEEDS-HUMAN is unperformed VM/staging/owner acceptance. FAIL denotes an identified defect or missing acceptance mechanism.

| Wave 2 task acceptance | Result | Evidence / remaining requirement |
|---|---|---|
| W2-A golden/property/bulk-model equivalence | PASS | Exact-candidate tests; real ioctl equivalence is separately pending. |
| W2-A no depth recursion | PASS | Iterative traversal/rollup/output reviewed; depth-10,000 test passes. |
| W2-A documented memory layouts | PASS | Layout tests/current R2-3 document 232+8 and temporary scratch; old 200-byte target explicitly not met. |
| W2-B bulk/walk on Rocky 8.8/8.10 | NEEDS-HUMAN | Compiled VM tests only. |
| W2-B RSS within 20% of estimate | FAIL | S9: no isolated measurement/assertion; also needs VM execution after harness fix. |
| W2-C scan→top both modes on VM | NEEDS-HUMAN | Unprivileged walk CLI round trip passes; XFS mode path compiled only. |
| W2-C mandatory fixture oracles | NEEDS-HUMAN | Host available fixtures pass; `mixed_special` explicitly skipped for denied Unix socket creation; privileged fixture/real bulk still pending. |
| W2-C schema tags/committed insta snapshots | PASS | Existing text/JSON renderer and CLI regressions pass; S8 qualifies provenance completeness. |

| Wave 2 gate item | Result | Evidence |
|---|---|---|
| All oracle/property/golden tests; bulk=walk on both VMs | NEEDS-HUMAN | Host CI passes with stated fixture exception; VM equality unrun. |
| Manual large staging/generated tree, both modes, three du subtrees, recorded times | NEEDS-HUMAN | No staging operation or timing claim made. Record in W2-gate handoff. |
| 03 literally implemented, Steps A–F spot-check | PASS | Source and normative/synthetic reference tests. This does not clear safety/bounds findings. |
| Tag wave-2-done | NEEDS-HUMAN | Owner action only, after fixes and VM gate; no tag changed. |

| Wave 3 task acceptance / gate item | Result | Evidence |
|---|---|---|
| W3-A mutated directory identified end to end | PASS for host; NEEDS-HUMAN for XFS | Real walk scan/mutate/scan executable regression passes; VM counterpart compiled. |
| W3-A 1M-directory diff/depth 3 <5s | NEEDS-HUMAN | Ignored performance test not independently rerun; prior handoff timing not adopted. |
| W3-B 1M-link-set command <10s | NEEDS-HUMAN | Correctness/large-top comparison-bound tests pass; timed 1M CLI/library gates not rerun. |
| W3-B Regions/crossing_sets rustdoc examples | PASS | Present and doc tests pass. |
| W3-C ncdu import on VM | NEEDS-HUMAN | Compiled test; optional-package branch must be recorded, not called an import success. |
| W3-C metadata charged-view note | PASS | Present in exporter; ordinary export regressions pass. S5/S6 remain data defects. |
| W3-C 1M-directory streaming RSS delta <64MiB (test requirement) | NEEDS-HUMAN | Ignored scale measurement not independently rerun. |
| Wave 3 all tests green; VM scan→mutate→scan→diff --since 1h --explain | NEEDS-HUMAN | Host tests pass; VM story unrun. Existing end-to-end uses explicit IDs, so duration-selection story also requires the manual check. |
| Tag wave-3-done | NEEDS-HUMAN | Owner action only; no tag changed. |

## Pending VM inventory

All are **compiled, not executed** in this review. Feature-only gating was checked; the one explicitly marked scale additionally uses `ignore = "scale"`. These assertions and any Linux/XFS/LVM/mount/atime claim currently rest on source, compilation and synthetic/unprivileged coverage.

| Target / test | What the VM must establish |
|---|---|
| test-support privileged / `loop_xfs_smoke` | Normative model blocks/links/apparent sizes on explicitly configured 4KiB reflink-disabled XFS; owned image/mount cleanup. |
| test-support privileged / `loop_lvm_thin_smoke` | Test-prefixed VG, both linear/thin mounts, 65,536-byte logical write size and image cleanup. |
| test-support privileged / `special_fixture_smoke` | Actual Unix socket and character-device fixture creation. |
| fs w1a / `probe_loop_xfs_geometry_matches_xfs_info` | XFS and independently parsed block/inode geometry match. |
| fs w1a / `resolve_loop_lvm_returns_pv_disk` | One leaf with major/minor matching lvs PV device output. |
| fs w1a / `resolve_vm_md_array_matches_lsblk_members` | Configured md fixture is RAID; at least two members and exact leaf-device set equality. Requires environment input. |
| fs w1a / `real_bind_mount_is_a_same_device_submount` | Actual bind target present in both submount and same-device mount inventories. |
| fs w1b / `privileged::noatime_no_writes` | Strictatime directory timestamps unchanged; zero fallback on owned root/child. |
| fs w1b / `privileged::real_bind_mount_is_skipped` | Same-device bind excluded and recorded as Mount skip, no duplicate traversal. |
| fs w1b / `privileged::ftype_zero_walk_and_namespace` | DT_UNKNOWN fallback gives oracle metadata and namespace child counts. |
| sys w1c / `geometry_matches_xfs_info` | Geometry/flags/AG fields match independent xfs_info; nonzero UUID. |
| sys w1c / `bulkstat_covers_all_inodes` | Strict inode order and nofollow metadata equality, including negative mtime. |
| sys w1c / `bulkstat_ag_ranges_partition` | Half-open AG ranges partition the full stream without duplicates. |
| sys w1c / `bulkstat_blocks_match_statx` | 10,000 regular/sparse files satisfy disk_bytes=measured statx.blocks×512. |
| sys w1c / `bulkstat_probe_records_kernel_support` | Print kernel and actual v1/v5 support, assert callable v5/nonempty stream. No legacy scan fallback. |
| sys library / `idle_class_as_root` | Dedicated worker idle-priority request and ionice observation succeed. |
| fs w1d / `root_sees_other_user_open_file` | Root sees UID/GID65534 child's exact PID/fd/inode. |
| store storage / `snapshot_xfs_publication_owner_permissions_and_sync` | Published root ownership, mode0600 and readable root row. It does not simulate power loss. |
| store storage / `state_usage_skips_same_device_bind_mount` | Outside bind payload excluded from allocation cap; measured small-filesystem free-space refusal distinguished. |
| scan w2a / `loop_xfs_walker_golden` | Exact normative table with measured directory adjustment; per-directory du. |
| scan w2b / `vm::bulk_equals_walk_mandatory_fixtures` | All fixture row classes equal, except documented projid/instance-name differences; unchanged tree has no races/missing names. |
| scan w2b / `vm::bulk_refuses_ftype_zero` | Real crc=0/ftype=0 geometry yields typed Unsupported. |
| scan w2b / `vm::bulk_equals_walk_million_file_timing_smoke` (scale) | Million-file equivalence and both elapsed times; explicitly **does not** establish RSS. |
| fs library / `checked_read_refuses_mounts_added_after_namespace` | Reject newly added other-device and same-device mounts; additional checks when mount ID available. |
| scan w2c / `auto_xfs_mount_root_and_small_subdir_record_reasons` | Auto bulk on eligible mount root, walk on small subdirectory; persisted reasons. |
| cli w3a / `end_to_end::xfs_scan_mutate_scan_diff_names_changed_directory` | XFS scan/mutate/scan identifies measured changed subtree; removed-target story works. |
| fs w3b / `other_filesystem_target_and_mount_boundary_pending_vm` | OtherFs target classified; mounted subtree's sentinel link not traversed. |
| fs w3b / `same_device_bind_mount_is_skipped_pending_vm` | Single dangling link counted once despite bind alias. |
| cli w3c / `vm_ncdu_import_reexport` | Actual ncdu import/re-export succeeds; package absence returns a diagnostic and leaves acceptance pending. |

Additional unprivileged pending-input test `captured_el8_inputs_pending_vm` needs 3–4 real EL8 mountinfo captures with provenance; current synthetic fixtures/early return do not satisfy it. Genuine NOATIME EPERM fallback behavior has no forced integration regression; S4 requests attempt-level coverage.

## Open requests, handoff follow-ups and known gaps

| SCR files under docs/handoff | Status at candidate |
|---|---|
| SCR-001, SCR-002, SCR-003, SCR-004 | Each accepted with modifications in its §13 decision. |
| SCR-005, SCR-006 | Each accepted in §13. |
| scr-W1-A/SCR-001, SCR-002 | Decided MountRoot/boundaries and stable CLI surface testing. |
| scr-W1-B/SCR-001 | **OPEN proposal** for global max_notable/spill seam. Current separate 4096 threshold refusal is expressly authorized interim behavior, not approval of the proposed extension. |
| scr-W1-D/SCR-001 | Decided throttle policy/G2/G7. |
| scr-W1-E/SCR-1 | Decided recovery/preflight. |
| scr-W2-A/SCR-001 | Closed by SCR-008 error totals. |
| scr-W2-C/SCR-001 | Closed by D2 bulk progress/failure/cancellation contract. |
| scr-W2-C/SCR-002 | Closed by R2-1 memory and D4 collision policy. |
| scr-W3-A/SCR-1, SCR-2 | Closed by R2-4 reader seam and R2-6 removed paths respectively. |
| scr-W3-B/SCR-001; scr-W3-C/SCR-001 | Both closed by R2-4. |
| scr-W4-B/SCR-001, SCR-002, SCR-003 | Each closed by R2-7. |
| scr-W5-A/SCR-001, SCR-002 | Both closed by R2-8. |
| scr-W5-B/SCR-001 | Closed by R2-9: identity accepted, proposed procfs Gate rejected. |
| scr-W5-C/SCR-001, SCR-002 | Closed without decision after ADR-019 removal. |
| scr-W5-D/SCR-1 | Closed by R2-10. |

Historical Pending labels in request files are superseded by these decisions, not live unresolved authority. No unexplained TODO/FIXME/XXX was found in Rust/tool production source; owner-labelled future stubs are not hidden completed work.

Unanswered handoff work after checking current code: real VM/staging/scale gates, isolated bulk RSS harness (S9), threshold-candidate seam proposal, missing genuine EL8 captures, cargo-deny audit, and the new findings above. W2-C-fix's constructor/publication-seam requests are applied; STORE-seams' invalid cancellation status was corrected by W2-C-fix2. W2-A errors, W2-B dependency implementation, W2-C fallback/collision/prune questions and W3 accessors have current answers. CI-fix-context's recorded cancellation/series failures no longer reproduce in exact-candidate CI. Generic `app.label` replaces the old tracking label. Coordinator/daemon obligations remain closed-connection recovery, acknowledged recorder writes, same-second series coalescing and growth-before-retention ordering.

The plan's §13 sentence claiming `W3-C-finish.md` is missing is stale: that handoff exists. Remove the stale clause when reconciling the plan. The accepted Node target shortfall, same-inode bind limitation without mount IDs, lexical same-filesystem symlink membership and earlier performance caveats remain documented limitations rather than inferred exemptions for other defects.

## For the human gatekeeper

Fix the software findings and review the exact resulting commit before treating a VM run as the final gate. Infrastructure aliases below are the documented examples, not verified hosts. Provision separate disposable Rocky **8.8** and **8.10** VMs, cached pinned Rust/dependencies, xfsprogs, lvm2, mdadm, GNU du/find and preferably ncdu. Provision the disposable md fixture using `xtask/README.md` **inside each VM**, retaining its loop paths for documented cleanup. Do not run mount/LVM/provisioning commands on the development host.

From the exact checkout being gated:

```sh
cargo xtask vm-test --host rocky88 --suite cargo --env TIDEMARK_W1A_MD_DEVICE=/dev/md127 --nocapture
cargo xtask vm-test --host rocky810 --suite cargo --env TIDEMARK_W1A_MD_DEVICE=/dev/md127 --nocapture
```

The runner builds privileged tests as the SSH user and runs them serially under sudo. It excludes scale tests. Preserve complete output/exit status, exact commit, `/etc/os-release`, kernel (`uname -r`), `xfs_info` geometry/mount options, xfsprogs/ncdu versions, v5 runtime probe, all 10,000 block-unit comparisons and the active bracketed `/sys/block/<leaf>/queue/scheduler` for resolved leaves. Record fixture skips individually. Obtain real EL8 mountinfo samples and rerun their parser test; do not substitute synthetic captures.

Run scale tests separately from the synced checkout **inside each disposable VM** (or the documented dev box for its explicit performance criterion), with no concurrent measurement workloads. Use the pinned toolchain on PATH; preload the dependency cache for these offline commands:

```sh
cargo test --offline --release -p tidemark-store --test w3a million_directory_depth_three -- --ignored --exact --nocapture
cargo test --offline --release -p tidemark-store --test w3b million_set_report_timing -- --ignored --exact --nocapture
cargo test --offline --release -p tidemark-cli --test w3b million_set_cli_timing -- --ignored --exact --nocapture
cargo test --offline -p tidemark-store --test w3c million_directory_export_rss_below_64_mib -- --ignored --exact --nocapture
cargo test --offline -p tidemark-scan --test w2a scale_two_million_directories_thirty_million_entries -- --ignored --exact --nocapture
sudo env "PATH=$PATH" CARGO_NET_OFFLINE=true cargo test --offline -p tidemark-scan --features privileged --test w2b vm::bulk_equals_walk_million_file_timing_smoke -- --ignored --exact --nocapture --test-threads=1
```

Record debug/release profile, input counts, wall time and isolated process RSS. The last command establishes timing/equality only; run the new S9 isolated-RSS test once implemented and record estimate/actual ratio. Do not infer it from VmHWM after walk.

For the manual both-mode/du gate, set `review_root` to a quiescent generated fixture or staging copy already placed on disposable XFS; `review_state` is a private state directory on a filesystem with sufficient free space. For example, after operator fixture preparation, run the entire following scan/query/diff block from a **root shell inside the disposable VM** (`sudo -s`), retaining the pinned toolchain on PATH. Bulk requires CAP_SYS_ADMIN. Explicit IDs ensure that same-second scan ordering cannot select the wrong mode:

```sh
review_root=/mnt/tidemark-review-fixture
review_state=$(mktemp -d /var/tmp/tidemark-review-state.XXXXXX)
printf '%s\n' '[retention.snapshots]' 'recent = 4' > "$review_state/review.toml"
target/debug/tidemark -c "$review_state/review.toml" --state-dir "$review_state" --json scan "$review_root" --mode walk --throttle max > "$review_state/walk.json"
walk_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["data"]["scan_id"])' "$review_state/walk.json")
target/debug/tidemark -c "$review_state/review.toml" --state-dir "$review_state" --json top "$review_root" --snapshot "$walk_id" --depth 99 --limit 1000000 > "$review_state/top-walk.json"
target/debug/tidemark -c "$review_state/review.toml" --state-dir "$review_state" --json scan "$review_root" --mode bulk --throttle max > "$review_state/bulk.json"
bulk_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["data"]["scan_id"])' "$review_state/bulk.json")
target/debug/tidemark -c "$review_state/review.toml" --state-dir "$review_state" --json top "$review_root" --snapshot "$bulk_id" --depth 99 --limit 1000000 > "$review_state/top-bulk.json"
find "$review_root" -xdev -type d -print0 | shuf -z -n 3 | xargs -0 -r -n 1 du -sxB1 -- > "$review_state/du-three.txt"
```

Record the selected paths and compare each mode's exact t_disk at those paths against the corresponding du result; do not compare additive charged bytes to du. Preserve scan IDs/reasons/times and note whether v5 is unavailable. Do not force v1 bulk if the kernel lacks v5. The end-to-end XFS test covers explicit-ID explain; separately retain an approximately hour-old complete snapshot, mutate only the disposable fixture, scan again, and run:

```sh
target/debug/tidemark -c "$review_state/review.toml" --state-dir "$review_state" diff "$review_root" --since 1h --explain --explain-min 1B
```

Record the selected snapshot age/notice and named changed directory. Default explain-min is 1GiB, so a small fixture needs the explicit threshold. Record ncdu import/re-export output/version or perform and record a manual import if the package-gated test skipped. Run low-rate daemon SIGTERM testing after B1 and Wave 6 implementation: record elapsed shutdown, catalog status, `.db`/`.db.tmp`, lock reacquisition and latest selection. None of these commands was executed against a VM in this review.

Human decisions: approve the cancellation contract, choose the export indexing/schema solution, reconcile provenance/JSON shape, resolve the threshold seam proposal, establish the isolated-RSS measurement and decide how a Rocky kernel lacking v5 affects supported deployment. Owner alone approves VM/staging evidence and creates wave tags; this report does not authorize either tag.

## Build and test results / limits

All builds used the exact archive, Rust 1.98.0 on PATH, `RUSTUP_HOME=/home/brewerm/.rustup`, a private copy of the populated host registry at `CARGO_HOME=/tmp/tidemark-w23-cargo`, `CARGO_NET_OFFLINE=true` and separate `CARGO_TARGET_DIR=<attempt-checkout>/target/review-build`. No dependency downloads or host-cache writes were needed.

| Validation | Observed result / artifact |
|---|---|
| Candidate archive integrity | All 399 tracked blobs still match the candidate Git objects after builds/probes; `candidate-integrity.json`. |
| `tools/ci.sh` in offline environment | **Exit 0**: source checks, fmt, workspace all-target Clippy with warnings denied, workspace tests. 476 passed, 0 failed, 13 scale ignored, including doc-test summaries; `ci.log`, `ci.exit`. Cargo-deny absent and skipped. |
| `cargo build --offline --workspace --features privileged --tests` | **Exit 0**, compilation only, 1m30s; `privileged-build.log/.exit`. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | **Exit 0**, 23.67s; `rustdoc.log/.exit`. |
| Ignored W2-C growth-cap regression | **Passed**, 43.77s; `growth-cap.log/.exit`. |
| Ignored wide-explain memory regression | **Passed**, 29.41s; `queries/wide-explain.log`. |
| Mandatory walk oracles explicitly rerun with nocapture | **Passed**, 19.85s; `mandatory-oracles.log/.exit`. Explicitly reports mixed_special skipped because Unix socket creation is denied. |
| Independent lifecycle probe | **Passed all six scenarios**; `lifecycle_probe.rs/.log/.exit`; harness correction disclosed above. |
| Record merge capacity probe | **Reproduced S1** with actual candidate types/formula; `record_merge_probe.rs/.log`. |
| Candidate ingestion probe | **Reproduced S2 scaling pattern**, source-derived complexity; `candidate_insert_probe.rs/.log`. |
| Low-rate shutdown probe | **Reproduced B1** after validated config/cancel/stop; `shutdown-gate-probe.rs/.log`. |
| Exact exporter/pager/partial-diff probes | **Reproduced S5/S6/S7/S8**; `queries/probe.rs/.log`, `queries/partial-diff.rs/.log`. |

Not independently checked: privileged runtime on either Rocky release; real fanotify/deploy/daemon work outside Waves 2/3; destructive fault/power-cut behavior; adversarial concurrent namespace stress; forced NOATIME fallback runtime; isolated whole-process scan RSS; most ignored million-row performance gates; exhaustive history-by-history test deletion attribution; advisory/license audit; a real ncdu executable import. Tests asserting VmHWM/timing are scale tests, not portable unit guarantees. The eleven ordinary CI scale ignores remaining after the two explicit executions still need their designated environment; earlier handoff measurements are historical evidence, not this review's measurements.

No finding relies on the later packaging-only checkout additions. The results above support the reported fixes and remaining defects; they do not establish verified task success or a release decision.
