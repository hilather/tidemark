# Pilot sign-off

Host type/hosts: ______  Change/ticket: ______  Dates: ______
RPM version/checksum: ______  Config revision/hash: ______
Operator: ______  Independent guide reader: ______
Application owner: ______  Storage owner: ______  Rollout approver: ______

## Evidence

Completed [checklist](checklist.md): ______
Before/after captures for every phase: ______
Business-day/batch latency and iostat graphs: ______
Staging guide walkthrough/yesterday-growth answer: ______
Actual VM/reboot/install/upgrade/erase/security/performance/soak gates: ______
Post-reboot verify exit/JSON and staged/firstboot records: ______
First-night snapshots/du cross-checks: ______
Week of now/diff, RSS/CPU/events, overflows, state headroom and alert review: ______
Tuned deployment config and monitoring freshness/routing: ______
Runbook rehearsal results: ______

## Decision

GO / NO-GO / LIMITED PILOT (select one): ______
Scope and host types authorized for next phase: ______
Latency SLO outcome and worst sustained await/utilisation: ______
Idle footprint and active watcher/write-throughput outcome: ______
Accuracy/link/reflink/partial-coverage limitations understood: ______
Open gaps (watcher windows, mount recovery, packaging assets, etc.): ______
Accepted exceptions: risk, named owner, expiry, evidence ______
Blocking issues and required follow-up: ______
Rollback trigger/owner/procedure/evidence-retention destination: ______
Next review date (including 30-day review): ______

Operator signature/date: ______
Application owner signature/date: ______
Storage owner signature/date: ______
Rollout approver signature/date: ______

A GO requires actual evidence, not compile-only or skipped tests. Unresolved required gates remain NO-GO unless the responsible owner records a bounded exception. This sign-off records a rollout decision; it does not authorize data deletion or unreviewed storage changes.
