# Handoff — AUDIT-procfs-budget: Bound process inventory bytes

## Needs from coordinator

The baseline daemon pressure classifier (`crates/tidemarkd/src/lib.rs`, `resource_pressure`) recognizes raw ENOMEM but not `io::ErrorKind::OutOfMemory`. The process service therefore publishes these new typed budget failures as `error`, rather than the task's assumed `degraded`; it still retries and never publishes truncated measurements. Please have the owner of the concurrent daemon task recognize `ErrorKind::OutOfMemory`. No daemon files were changed here. This prevents claiming the requested daemon status behavior is complete.

- Branch: the precreated canonical attempt branch; no new branch, merge, or push.
- Agent: Codex, attempt-f3414c00f0b9312ae3f3ee00f9d64ec2b9a61796d8ce710a3313e16a752c4b7a.
- Date: 2026-10-13.
- Status: implementation and evidence candidate; independent verification and guest measurements pending.

## What changed

`tidemark-fs::procfs` now admits records against `MAX_INVENTORY_BYTES = 128 MiB`. Its running total includes target **capacity**, record-vector **capacity** (which already includes the records themselves), and an 8 MiB conservative allowance for both bounded numeric-ID vectors. Record-vector growth uses fallible exact reservation with geometric growth, checking both the old and new allocation during a moving reallocation. Numeric-ID vectors also reserve fallibly. Targets are moved out of the readlink `OsString`, removing the former extra target clone. One newly read target is temporary until admission; rejection returns a typed `InventoryBudgetExceeded` inside the unchanged `io::Result`, with retained count and accounted bytes. Starttime rollback releases path charges and preserves record spare capacity charges.

Public `OpenFile.target` remains `Vec<u8>`. Interprocess or per-process path interning would require changing that public representation, or discarding descriptor identity before the caller counts descriptors. Neither is done. The budget alone protects this stage; existing inode/process holder deduplication remains intact downstream.

`tidemark-live::procscan` independently validates the injected inventory and preflights all relevant copies before changing histories, published totals, timestamps, rotation, or pending samples. Phantom/sample row sets and writer row sets are each admitted against `MAX_PASS_BYTES = 4 MiB`. Temporary attribution/candidate trees and admission scratch have a separate 16 MiB allowance, with conservative 512-byte node/working-space charges. A bounded heap computes the largest 10,000 possible retained row charges across old and new candidates, preserving the existing growth-based rotating writer admission. Existing 10,000-writer/60-sample history and 20,000-descriptor tests remain applicable. Holder and sample vectors reserve their exact admitted lengths before mutation; writer output vectors reserve at most the tracked count.

Process-pass attribution uses a zero-capacity local `AttributionCache` and a byte-admitted per-pass PID map. The constructor still accepts the existing shared-cache argument with the same callable signature, but does not populate it: otherwise admitted labels could also accumulate in an external cache across passes, defeating the stated copy envelope. Attribution retains exact starttime validation and is fetched once per relevant PID per pass. This changes cache reuse/performance, not attribution semantics.

There are no dependencies, manifests, shared module declarations, frozen core types, config keys, CLI flags, daemon edits, or plan edits.

## Worst-case retained allocation formula

Let:

- `I = procfs::MAX_INVENTORY_BYTES = 128 MiB`, including the two ID-vector allowances and moving inventory reallocations.
- `B = procscan::MAX_PASS_BYTES = 4 MiB` for each admitted row set, including raw path and label bytes, displayed mount strings, records, and exact reserved vector capacity.
- `S = procscan::MAX_SCRATCH_BYTES = 16 MiB` for per-pass attribution/candidate trees and admission bookkeeping.
- `W = MAX_WRITERS = 10,000`, `H = HISTORY = 60`, `N = ROW_OVERHEAD = 512`.
- `Q = tidemarkd::services::tswriter::CHANNEL_CAPACITY = 64` (baseline source), and history tuple size `T = size_of::<(i64,u64,u64)>() = 24` on the target.

A conservative allocation envelope for one pass is:

`I + S + (Q + 12) * B + 3 * W * (N + H * T) + 6 MiB`

`= 534,613,504 bytes = 509.84716796875 MiB < 512 MiB`.

The twelve extra row-set allowances cover old/new collector phantom and pending rows, report row copies, writer candidates/history row payloads, producer chunk/current-message copies, and a caller's rendered phantom/writer rows. The three history generations cover collector, preceding immutable report, and newly published report; tree/node slack is charged separately from the row payload allowances. The 6 MiB margin covers a bounded proc-text read/lossy conversion and a temporary descriptor/row. Upstream record reallocation's old/new overlap is already charged in `I`. Candidate working structures share `S`; every candidate gets a conservative node/slack charge before insertion. Display escaping can expand one mount byte into four characters; summary allocation slack is admitted at eight times raw mount length.

A queued Phantom or Writers message from this producer is at most `B`, even with `MAX_BATCH_ROWS = 20,000`, because chunk cloning cannot exceed the whole admitted row set. Therefore the existing 64-slot writer queue retains at most `Q * B = 256 MiB` of process-sample payload. No daemon writer admission edit is needed for **this producer**. Other writer message types are outside this task. Reevaluate the formula if queue capacity, producer batching, report ownership, row sizes, or history constants change. `pipeline_envelope_is_below_512_mib` pins the current arithmetic; it is a regression guard, not an RSS measurement.

## Assumptions and allocation limits

- This is a bound on retained/requested allocations for one pass, including one preceding report generation and a normal caller output. Arbitrarily retaining older public reports or caller outputs adds independent prior-pass storage; public getters cannot prevent callers from retaining their returned owned values. Shared immutable `Arc` readers of the same report do not duplicate its payload.
- The formula does not bound unrelated scan/watcher/store/SQLite work, supplied configuration or an independently populated shared attribution cache. It is not a whole-daemon RSS/cgroup guarantee. Allocator bookkeeping, arena fragmentation, and kernel accounting require guest measurements.
- PID/fd enumeration reserves the full 8 MiB allowance rather than allocating that much on small inventories. The running total is exact for actual inventory record/path capacities; conservative ID allowance exceeds actual ID-vector bytes by at most 8 MiB. Pure shape tests compare capacities with zero accounting tolerance. Geometric vector-growth admission additionally conservatively checks the old record buffer that may coexist during reallocation.
- Standard-library readlink returns one bounded Linux procfs target before inventory admission; it is temporary and moved without another allocation. Fallible reservation covers inventory record growth. Existing proc-text helpers retain their fixed limits.
- Synthetic inventory errors and real transient process errors preserve existing behavior: one exiting/inaccessible process does not stop other processes, while a whole-pass resource failure never returns complete-looking truncated totals. No hundreds of thousands of real descriptors were opened.
- Questions were not sent from the isolated worker. Choices above were made within the assigned scope.

## Acceptance evidence and tests

New synthetic tests:

1. `procfs::budget_tests::long_duplicates_fail_short_targets_fit_and_accounting_is_exact`: same descriptor count, long duplicates reject and short targets fit; capacities agree with the total exactly for empty, short, and 4,000-byte targets; rollback preserves spare-capacity accounting.
2. `procfs::budget_tests::scratch_and_spare_capacity_are_charged_before_reservation`: rejection precedes record growth; scratch participates in admission.
3. `procfs::budget_tests::moving_reallocation_is_admitted_with_both_record_buffers`: insufficient old/new allocation headroom rejects before reallocation.
4. `procscan::tests::inventory_budget_failure_preserves_report_and_pending_then_short_pass_recovers`: 34,000 duplicated 4,000-byte targets exceed 128 MiB; typed failure retains prior report/history/pending rows; the same number of short targets succeeds and counts all descriptors with one holder.
5. `procscan::tests::downstream_long_unique_holders_fail_before_history_changes`: a below-inventory-budget pass exceeds downstream copy admission; failure preserves history/timestamp/pending rows, followed by recovery.
6. `procscan::tests::long_duplicate_phantom_below_budget_keeps_counts_holders_and_samples`: 400 long deleted targets preserve inode totals, attribution, raw paths, and one phantom/writer sample.
7. `procscan::tests::pipeline_envelope_is_below_512_mib`: formula regression guard.

Existing procfs exiting/inaccessible-process coverage and deleted-open-file integration test are retained. Existing live identity-mismatch, report-failure preservation, immutable-reader, writer-rotation and 20,000-descriptor coverage is retained.

Final command outcomes and counts are recorded below. Logs are in the canonical attempt output `library/submission/`; earlier iterations are retained separately and are not final acceptance evidence. All long commands use `timeout 1500`, pinned Rust 1.98.0, offline mode, and a temporary copy of the available registry/git cache. No privileged test is executed; feature-enabled tests are compiled only.

## Guest measurements — pending VM

On a disposable Rocky 8.8 and 8.10 guest, use a helper that creates a roughly 4,000-byte path on a disposable fixture, opens a regular file read-only, and duplicates the descriptor across helper processes within guest fd/process limits. Keep descriptor count controlled and stop well before guest-wide exhaustion. Repeat using short targets at the same count, then distinct long targets, and deleted-but-open variants. Observe daemon RSS/VmHWM and cgroup memory before collection, during collection, after rejection, and after recovery; confirm unchanged last-success data, incomplete/unavailable current scan, retry, and the intended degraded status after the daemon classifier follow-up. Also observe a deliberately slow persistence consumer to fill the existing queue and measure peak RSS against the allocation envelope. Measure process-pass duration after removal of cross-pass attribution caching. Do not run this experiment on the host or production servers.

Existing privileged procfs/live tests remain pending VM. No new privileged test is needed for byte admission: all new budget cases use synthetic records.

## Proposed plan text (not applied)

For `07-live-monitoring.md` §4 and W1-D/W5-B resource limits: “Process inventory is limited to 1,048,576 descriptors and 128 MiB of record/path/ID-vector allocation, with fallible admission including reallocation overlap. Exceeding either limit fails the entire pass; no truncated totals are published. Process-pass attribution is retained only within the admitted pass. Phantom/writer copy admission caps each report/sample row set at 4 MiB and temporary working data at 16 MiB; writer histories remain limited to 10,000 inodes and 60 samples. The one-pass allocation formula includes the persistence queue and report generations and remains below 512 MiB. These allocation limits are not a whole-daemon RSS guarantee. Budget errors are unavailable/degraded and retried, preserving the previous completed report internally.”

## Remaining uncertainty

Independent verification and VM/RSS/performance evidence remain pending. The daemon status classifier follow-up at the top is required for the specified degraded behavior. No release acceptance or worker-capacity change is asserted by this handoff.

## Final validation outcomes

| Command (offline, timeout 1500) | Exit | Evidence |
|---|---:|---|
| `cargo fmt --check` | 0 | `library/submission/fmt.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | `library/submission/clippy.log` |
| `cargo clippy --workspace --all-targets --features privileged -- -D warnings` | 0 | `library/submission/clippy-privileged.log` |
| `cargo test --workspace --no-fail-fast` | 0 | `library/submission/test.log`: 775 passed, zero failed, 14 ignored across reported result lines, including child harnesses |
| `cargo build --workspace --features privileged --tests` | 0 | `library/submission/build-privileged.log`; no privileged test executed |
| `tools/ci.sh` | 0 | `library/submission/ci.log`: 146 owner tags, forbidden-symbol scan passed, all available checks passed |
| `cargo test -p tidemark-fs -p tidemark-live` | 0 | `library/submission/targeted.log`: 168 passed, zero failed, one ignored scale test; all seven new budget tests passed |

Cargo-deny was not installed; CI explicitly warned and skipped its check. No advisory freshness result is claimed. The prompt's anticipated readiness failures were not observed in the final workspace/CI outputs. Privileged execution and guest measurements remain pending VM.

## Definition of Done evidence

- [x] Only owned paths changed; no shared-file edits or dependencies.
- [x] Formatting and both required Clippy modes passed.
- [x] Workspace tests and available CI checks passed; feature-enabled tests compiled only.
- [x] Seven new synthetic regressions passed, including byte accounting and previous-report preservation.
- [x] Public additions documented; source owner headers present; no new unsafe code.
- [x] Handoff includes formula, measurement plan, assumptions and proposed plan text.
- [ ] Daemon degraded-state classifier follow-up, independent verification, and guest RSS/performance evidence remain outstanding.
