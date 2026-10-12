# Handoff — PLAN-round3: gate-review and Wave 6 plan contracts

- Attempt: attempt-21715fa4edbf1bafc2685170537c4d5caeac93ab7078d0f7b0a4752bc2325022
- Branch: existing canonical attempt branch; no branch creation, merge or push.
- Date: 2026-10-12
- Status: documentation delivered as an evidence candidate; independent verification pending.

## What changed

Recorded R3-1 through R3-15 in the existing contract-decision format, including the contract signatures, plan locations and affected tasks. Reconciled consumer documents and Wave 6 ownership without splitting or replacing the collective W6-A section bodies; targeted corrections remove contradictions in those bodies. Added an open-risk register separating accepted contracts from unresolved implementation and runtime evidence.

All eighteen requested notes exist. Read the existing decision record, both gate reviews, the named notes and the affected plan sections. Exact checked-reader and ProcScanResult shapes were also cross-checked against current source because W23-fix-gate describes the additive reader without enumerating its full signature. No code, AGENTS.md or memory projection was edited.

## Every file and section changed

| File under docs/plan/ | Sections and changes |
|---|---|
| 02-architecture.md | §5 processes: distinguish nonroot direct execution from uid-0-only API access. |
| 04-scanner.md | §2.2 EPERM/NOATIME: capability and ownership, not uid; §6.2 ScanGate signatures: cancel and stop_sampling; §8 cancellation helper, wait bounds, limitations, run_scan lock/gate ownership and persistence acknowledgements. |
| 05-storage.md | §5.2/§6 boundary: recorder persistence and unresolved series acknowledgement; §7 completion callback order and pre-retention growth; §10 indexed notable paging and diff provenance without schema/wire changes. |
| 07-live-monitoring.md | §1 daemon LVM omissions and CLI store trends; §3.3 exact watcher windows, cumulative boundary totals, top-list availability and independent loss acknowledgements; §4.2 ProcScanResult diagnostics; §6.2 watcher exception to complete inventories. |
| 08-interfaces.md | §1.2 status config label; §2 root-only authentication, live fallback/override contracts, reduced LVM and current-run history; §3 additive byte parser; §5 G6 example and stage-only local-state check with W4-B-fix3 wording. |
| 11-rollout-runbook.md | P0 preconditions: contract carry-forward, local-state eligibility, compensation distinction, live command behavior and required runtime evidence. |
| 12-decisions-and-risks.md | ADR-018 peer policy; risk register: N1 timestamp, duplicate helper, LVM cancellation, current-run history, SQLite leaf confinement, absent privileged execution, counting-gate limit, socket pairs; additional unresolved watcher and writer/path seams. |
| 13-contract-decisions.md | R2-10 inventory sentence qualified by R3-1; new Gate review and Wave 6 decisions part, R3-1–15, with source inventory and signatures. |
| waves/wave-2-scan-engine.md | W2-C interface/sequence: persistence callbacks, run_scan sole lock owner, completion order, retained cancellation handle via stop_sampling/stats. |
| waves/wave-5-live.md | W5-A/W5-B boundary: watcher-window/dirty policy and permission-count exclusion; W5-B interface adds implemented writer diagnostics. |
| waves/wave-6-daemon-packaging.md | Task table, shared CLI ownership, state skeleton and merge dependencies; collective section ownership map; targeted Key behaviours/CLI/Tests corrections for gate ownership, growth order, peer uid, windows, config, diagnostics and direct fallbacks. |
| waves/wave-7-hardening-rollout.md | New gate-review contracts subsection: carry-forward contract checks and open-runtime-risk evidence for W7-A/B/C/D. |

Own output: docs/handoff/PLAN-round3.md (this note). No shared source/manifests/dependencies changed.

## Precision from implementation notes

- W5-A-fix2 specifies the bounded 1,024-root loss acknowledgement FIFO, conservative eviction and rollback, beyond the brief's independent dirty-root rule.
- W6-A3 says CLI LVM trends still use read-only store queries. Only segment type and other/unmounted LV inventory are omitted; no frozen payload expansion.
- W6-A2b reports that exact watcher measurements are currently unavailable: row drains cannot reconstruct empty-minute/loss/boundary data. Its CompletedMinute/drain_minutes and freshness APIs remain proposals, not accepted signatures.
- W6-A2a reports series error-acknowledgement and pinned/logical storage-path seams. Those proposals remain unresolved risks rather than documentation claims of implementation.
- W23-fix-gate limits the cancellation bound to userspace gate waits, with no deadline on in-flight kernel calls, index/sync or callbacks.
- CI-fix-daemon-state supersedes A1's separate exact-mode/gid startup policy with the shared eligibility/valid-marker normalization policy; the existing §5 policy remains normative.
- W4-B-fix3 resolves TEST-e2e-deploy's historical NFS-state gap and adopts its proposed §5 wording; the old test note is retained as historical evidence, not current behavior.

## Validation

- `timeout 1500 python3 /tmp/plan-round3-check.py`: pass after restricting the obsolete-phrase check to operative text (the new record deliberately quotes G6's historical phrase). Checks contiguous R3 numbering, all eighteen notes present, changed-path scope, balanced Markdown fences, old peer/lock/NOATIME/example contradictions and four Wave 6 task rows.
- `git diff --check`: pass.
- Repository-wide `rg` review for complete inventories, arbitrary windows, uid policy, scan locking, NOATIME, permissions and deployment failure wording; fixes are listed above.
- Manually inspected the deployment table and Wave 6 ownership table; corrected a blank line that initially split the pre-flight table.
- No Rust builds or tests were run for this documentation-only change; no build artifacts were created, so cargo clean was unnecessary. Existing privileged runtime, RPM, real listener/credentials, Prometheus/Grafana and soak evidence remains pending its designated environment.

## Remaining uncertainty

This is a plan reconciliation and evidence candidate, not independently verified implementation success or release acceptance. The plan records the decided contract and explicitly identifies current producer/writer/store limitations. Memory receipt lookup returned no pending deliveries; no memory proposal or acknowledgment was needed. No host service, protected configuration, filesystem or LVM operation was performed.
