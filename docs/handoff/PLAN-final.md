# Handoff — PLAN-final

## Contradictions resolved (later notes control)

- The initial brief’s AF_UNIX/AF_NETLINK and 8.10-unconfirmed item is superseded by GATE-fix: AF_UNIX alone, with supplied measurements on both releases. Normative unit/config blocks were preserved.
- Initial fuzz seams marked OPEN are supplied by FUZZ-seams; 16 targets now exist. Workflow execution and cumulative release gate remain pending. FUZZ-timing replaces stable two-second checks with 30-second/scaling guards; nightly execution limits remain two seconds.
- Initial deny nonblocking/F05 pending is superseded by CI-deny-required; BSL-1.0 remains allowed.
- The both-releases-mount-mark sentence in item 24 is superseded by WATCH-fsmark: prefer filesystem marks on supported 8.10, mount fallback on 8.8 or mark failure.
- STATE-trust-A2 supersedes part A’s readable-storage refusal: integrity checks permit readability with one warning. STATE-trust-B keeps private daemon/deploy admission and permits only checked-marker state read/execute normalization, never ownership or write-bit exceptions.
- RECHECK-life’s 30-second wait applies to missing/corrupt shared-database creation/recovery; healthy startup does not wait for a busy scan lock. CRASH-recovery cleanup itself is nonblocking at startup, skips Busy, and logs individual errors while continuing. Recorded that precision rather than promising universal startup blocking or cleanup success.
- RECHECK-edge supersedes W6-watch-windows’ interior-minute requirement for overflow only: exact boundary totals suffice; hot still needs all consecutive valid facts.
- Original W3-B following-stat target resolution is replaced by gated component resolution stopping at selected mount. The “nothing else reads link text” shortcut was avoided: administrator-supplied path canonicalization is explicitly distinguished.
- The actual fault suite is 01 state full, 02 snapshot crash, 03 fanotify overflow, 04 compaction crash, 05 directory races, 06 huge directory. GUEST-fix separates normal/low-limit completeness passes from forced overflow.

## Work completed

Documentation only. R5-1 through R5-28 follow the brief order and were committed in five-section batches before security and plan amendments. Sources identify implementation notes; no independent verification or release acceptance is asserted.

| File | Sections changed |
|---|---|
| docs/plan/13-contract-decisions.md | New Hardening, second guest runs and Wave 7 decisions, R5-1–28; obsolete listener validation/webhook contract; R2-5 retention precision; R3-1/14 and R4-4/7/9/11/12/13/15/16 supersession/status |
| docs/security.md | New audit-derived security model: trust/capabilities, sudo, sensitivity/communications, read-only limits, state/metrics trust, availability/process budget, deploy, SELinux, historical unsafe dependency inventory, required deny and exclusions |
| docs/operator-guide.md | Introduction atime/security pointer; custom state placement; exact watcher alert availability; new watcher visibility/symlink limits/manual guidance |
| docs/plan/01-requirements.md | G7, FR-17/20, NFR-5/8; new read-only/visibility limits |
| docs/plan/02-architecture.md | Introduction, diagram, metrics thread, daemon/security/network/content-read and dependency choices; new lifecycle/trust detail |
| docs/plan/05-storage.md | New verified storage/recovery/browser-query contracts adjacent to full existing schema; schema unchanged |
| docs/plan/07-live-monitoring.md | Rate prerequisites, watcher §3.1/3.2 probing/pseudocode and §3.3 exact consumer closure; new collector/cap/budget/mapping detail |
| docs/plan/08-interfaces.md | Symlinks command description; API authentication/deadlines; new presentation/TUI/trust/collision clarifications. Normative config and unit fences untouched |
| docs/plan/09-testing.md | Benchmark quantities/limitations and CI no-fail-fast; new fault/tooling/16-target table and pending gates |
| docs/plan/11-rollout-runbook.md | New operator security/visibility/remaining acceptance gate |
| docs/plan/12-decisions-and-risks.md | ADR-006/008/013/018; stale first-run open work; R12 and R3-L4/L8; new current open-risk table |
| docs/plan/waves/wave-0-foundation.md | Remove obsolete ureq/TLS rationale |
| docs/plan/waves/wave-3-queries.md | W3-B target resolution/mount boundary/atime wording |
| docs/plan/waves/wave-5-live.md | Exact consumer closure; journal-only deliverable/tests |
| docs/plan/waves/wave-6-daemon-packaging.md | Mount follow-up closure, peer refusal, metrics-only transport, shell lint/config packaging and new hardening contract detail |
| docs/plan/waves/wave-7-hardening-rollout.md | Stale review/open status, security transport/atime requirements and new landed-work/pending-gate detail |
| docs/handoff/PLAN-final.md | This handoff |

No source, dependency, shared module, schema, flag, metric or wire-type edits. No branch creation, merge, push, privileged command or host service/storage operation. No memory projection edits; attempt receipts were empty.

## Validation performed

- `timeout 1500 cargo test -p xtask --offline`: exit 0; 43 passed, zero failed/ignored (20 unit, 10 w6c, 6 w7b, 4 w7c, 3 w7d). Includes exact normative config/unit and generated packaged manuals. Used installed Rust 1.98.0 and existing host cache; no fetch.
- Explicit baseline comparison: TOML and INI fenced blocks in 08 unchanged; 28 R5 headings exactly ordered.
- HTTP/webhook/listen/9753 sweep of docs/plan: remaining hits are removal/history/absence statements and external reference URLs, not v1 endpoint deliverables.
- `git diff --check`: passed. Documentation scope checked against baseline before submission.
- `timeout 1500 cargo clean`: exit 0. Final status checked after handoff commit.
- Logs retained in this attempt’s worker-output/library, not repository scope.

## Not done / remaining uncertainty

No known contradictory plan file intentionally left unamended. This is not an independent code audit or assertion that every historical plan sentence has been reverified. The security dependency table is explicitly the second audit’s historical inventory; no refreshed advisory audit, GitHub fuzz run or privileged guest execution was performed. Source-note implementation evidence does not close acceptance gates.

Open items retained: million-child sort index tradeoff/100 ms performance; 5M/20M staging and 72-hour soak; workflow execution and cumulative 24-hour fuzz gate; scan.cancel id conditioning; HotMinute top_unit and summed minute file counts; non-path HotEntryWire serializer core sign-off; cap-to-component-degraded policy; real second-user trust, restricted-unit filesystem marks, symlink boundary/atime and final Rocky guest gates. Existing unrelated risks (LVM cancellation, current-run alert history, finalization timestamp and series acknowledgement) remain documented; no fix inferred without a supplied note. Wave tags and core sign-off remain owner decisions.
