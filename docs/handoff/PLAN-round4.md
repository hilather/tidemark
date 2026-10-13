# Handoff — PLAN-round4: First real runs and Wave 6 review decisions

- **Branch:** canonical hp-5e5bf71e12fda0b66c05616a34277ecf1ce946d742f9baa2652c2c7b5351e54e; no branch creation, merge or push.
- **Attempt:** attempt-5e5bf71e12fda0b66c05616a34277ecf1ce946d742f9baa2652c2c7b5351e54e.
- **Date:** 2026-10-12.
- **Status:** documentation and bounded threshold edit completed; evidence candidate for independent verification.

## What was built

Recorded R4-1–R4-16 in a new “First real runs and Wave 6 review decisions” part of 13 and committed that record first. Amended operator, contract and wave documents, distinguishing implemented library contracts from unresolved daemon publications. All sixteen requested handoff notes and both review files exist. Exact catalog legacy resolution, CompletedMinute/drain cursor and rollback semantics, runtime interval exemptions, transactional reload reservations, install assertions and watcher validation caching follow the notes where more precise than the brief. Platform measurements and owner budget/push decisions are attributed to the task brief; they were not independently remeasured here.

## Every changed file and section

| File | Sections changed |
|---|---|
| docs/plan/13-contract-decisions.md | New R4-1–R4-16 part; Wave 1 “Bulk mode requires v5 bulkstat” runtime evidence; R3-1 producer evidence updated to R4-7 |
| docs/plan/01-requirements.md | §2 SELinux assumptions; §6 NFR-4 idle versus active-watcher CPU |
| docs/plan/04-scanner.md | §5.1 successful bulkstat/no statx mount IDs; §8 daemon mount admission and scan failure/backoff |
| docs/plan/05-storage.md | §4 relative raw-byte catalog paths, legacy reads and rebuild repair |
| docs/plan/07-live-monitoring.md | §1 exact mount presence/open seams; §3.1 guest fanotify facts; §3.3 ranked hot reports, CompletedMinute/drains/identity/exact consumer windows and persistence caps; §4.3 attribution validation cache; §7 human presentation; §8 revised resource table/measurement |
| docs/plan/08-interfaces.md | §1.3 CLI/exit/presentation rules and open schema/cancel gaps; §2 reload result/admission; §3 captured-byte validation, operational bounds, shared shutdown deadline and retained storage (outside normative blocks) |
| docs/plan/09-testing.md | §6 guest harness and discriminating packaging gates; §9 native RPM/scripts/workflow/default-severity shellcheck and push decision |
| docs/plan/11-rollout-runbook.md | Gate guidance before P0; P4 CPU distinction; new first-run operational contracts for package/reboot/harness/platform/mount/reload |
| docs/plan/12-decisions-and-risks.md | First-run platform/resource facts and open risks before risk table; R7 measured kernel facts; R3-L2 retained worker limitation, L5 guest evidence, L8 CompletedMinute consumer gap, L9 catalog portability resolved |
| docs/plan/waves/wave-6-daemon-packaging.md | W6-A key behaviors, firstboot limitation and CLI scan branch; W6-B bounded metrics/cache/staging recovery; W6-C native build/upgrade fixture/CI/install lifecycle assertions and postun restart |
| docs/plan/waves/wave-7-hardening-rollout.md | Gate-review contracts/R4 open work; W7-B CPU gate; W7-C SELinux posture; W7-D generic workload baseline |
| docs/plan/waves/wave-5-live.md | W5-A load CPU gate; adopted CompletedMinute/identity versus open daemon consumer |
| docs/plan/waves/wave-1-primitives.md | W1-C bulkstat runtime evidence paragraph |
| docs/plan/README.md | Layer table L2 cost revised |
| crates/tidemark-live/tests/w5a/privileged.rs | Only scale CPU threshold 2→10, matching diagnostic and R4-8 comment |
| docs/handoff/PLAN-round4.md | This handoff |

## Shared-file edits and dependencies

None. No manifests, module declarations, dependencies, schemas, API signatures or packaged files changed. The single Rust change is the explicitly requested threshold. No privileged commands or guest tests ran.

## Acceptance evidence and validation

All long Cargo commands used timeout 1500, offline, Rust 1.98.0 on PATH and the populated /home/brewerm/.cargo cache.

| Check | Observed result |
|---|---|
| cargo test -p xtask --offline | Exit 0; 10 unit and 9 integration tests passed, including exact_normative_unit_and_config |
| cargo build -p tidemark-live --features privileged --tests --offline | Exit 0; feature-gated tests compiled, not executed |
| cargo fmt --check | Exit 0 |
| Independent Python comparison of 08 TOML/INI fenced blocks with assigned base | Identical bytes for both normative blocks |
| git diff --check | Exit 0 |
| Contradiction search across docs/plan | Updated old watcher CPU, unexecuted bulkstat, absent CompletedMinute, no guest evidence and once-per-config claims; preserved historical replaced budget wording as explicitly superseded |
| cargo clean --offline | Exit 0; removed 3239 files / 1.5 GiB |

No workspace test, full CI, privileged runtime, RPM generation, GitHub run or real-server performance claim is made. Required narrow checks suit a documentation task with a numeric test change. The scale test remains pending VM/real-server measurement. Logs are in this attempt's worker-output library.

## Deferred HTTP/webhook text inventory

ADR-020 and endpoint removal are separately assigned; no endpoint contract from EDGE-metrics was adopted. Existing HTTP/webhook text intentionally remains:

- 08 §3 normative [metrics] listen and [alerts] webhook keys; §3 validation prose; §4 metrics introduction.
- 07 §6.2 WebhookDeliver contract and §6.3 alert-delivery list (unchanged).
- Wave 6 W6-B optional HTTP deliverable and W6-C default-config description.
- 02 architecture overview diagram and §10 dependency list (ureq/tiny_http).
- 01 FR-17 delivery row; 12 R12 exposure risk.
- 13 historical SCR-003 loopback validation and R2-10 webhook contract.
- Wave 5 W5-D webhook tests; Wave 7 W7-C webhook exposure threat-model item.

## Known gaps and follow-ups

R4-16 / 12 list core S1–S5, S7–S8 and edge F02–F04 without landed fix notes. CORE-fix-1 covers B1/B2/S6/S9 and F12 validation; EDGE-cli covers F05–F08; EDGE-metrics F09–F12; INSTALL-assert F13. F01 has a landed fix note, but separately assigned HTTP removal remains open. CompletedMinute library delivery does not establish exact daemon alert windows. Rate-ring clearing and removable now.degraded mount presence, mark identity consumer/race, A2b continuity/removal/cancellable procscan, LVM schema and conditional scan cancellation, intermittent 8.8 cancellation deadline, and firstboot rewrite remain open.

Memory APIs were read for this canonical attempt; receipts returned an empty list, so no required update was pending or acknowledged. No memory projection or SQLite store was edited. No release, gate acceptance, merge or independently verified task success is declared.
