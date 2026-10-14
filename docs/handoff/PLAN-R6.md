# Handoff — PLAN-R6: Third and fourth review decisions

- **Branch:** retained canonical `hp-303907e4880b606150a361e052ca6e685ecc8172bfa1388f5704bcfa7eff6b62`
- **Attempt:** `attempt-303907e4880b606150a361e052ca6e685ecc8172bfa1388f5704bcfa7eff6b62`
- **Date:** 2026-10-13
- **Status:** documentation evidence candidate; independent review/integration pending.

## What changed

R6 was committed first as `2438108` (`PLAN-R6: Record third and fourth review decisions first`). Twelve numbered decisions cover ancestry, purge, writer leases, watcher memory/history/lifetime/marks, mount pinning, sampler reload health, CLI/build identity, fault/CI evidence, all four review rounds and undecided release items. Subsequent amendments mark the relevant R5 decisions superseded without deleting their historical evidence.

| File and section | Amendment / contradiction resolved |
|---|---|
| `docs/plan/13-contract-decisions.md`, R6 and R5-2/5/6/8/9/14/18/19/21/24/26/27 | Added decisions, sources, verdict/fix-note table and open gates; marked older guarantees superseded by specific R6 items |
| `docs/plan/02-architecture.md`, release-candidate ownership and health | Added lifetime writer ownership, strict packaging, sampler generations, watcher byte/history/lifetime and attribution boundaries |
| `docs/plan/05-storage.md`, verified storage/recovery | Replaced overflow-uid ancestry allowance and scan-lock-only recovery; added writer.lock lifetime, ordering, identity and purge finalization |
| `docs/plan/07-live-monitoring.md`, watcher/rings and release-candidate corrections | Replaced count-only memory estimate and complete-looking unmapped fallback; clarified independent query retention, memory fallbacks, shutdown, full-pass/generation sampler health and CPU uncertainty |
| `docs/plan/08-interfaces.md`, final prose clarification | Added common NoSpace wording, human formatting/build identity, ancestry and lifecycle behavior; all fenced blocks unchanged |
| `docs/plan/09-testing.md`, landed tooling and release-candidate gate evidence | Replaced healthy-after-120-second claim; separated exact no-loss/cap/overflow/recovery passes, default queue fallback, witness/live-daemon compaction oracle, realistic durations and isolated log capture |
| `docs/plan/11-rollout-runbook.md`, release-candidate operator checks | Added package identity/trust, bounded lease refusal, purge safeguards, incomplete attribution, reload health and unresolved gates |
| `docs/plan/12-decisions-and-risks.md`, release-candidate decisions/open table | Recorded superseded assumptions and explicit core/cap-policy/CPU/scale/soak/slow-guest/re-review questions |
| `docs/security.md`, read-only limits, storage trust, mount limits, availability, deployment | Corrected O_PATH-only handle wording, missing lifetime lease/purge safeguards, count-only watcher bounds and complete-looking unmapped attribution |
| `docs/operator-guide.md`, watcher visibility/mode | Corrected unmapped root fallback to explicit incomplete attribution and recoverable health |

No shared code/manifests, dependencies, schema, CLI flags, metric names or config keys changed. No branches created, merges, pushes, privileged commands, service actions or memory projection edits.

## Sources and assumptions

Read the four requested third/fourth review reports and the named handoffs, including their proposed text; consulted earlier review verdicts/dispositions and R5 for round-one/two trails. Notes take precedence over brief summaries: development exceptions require debug assertions and no strict feature; lease shared-open wait is two seconds; purge waits ten seconds per contention phase; startup/offline exclusive recovery thirty seconds; script 03's 120-second transport recovery differs from slow seventeen-minute history recovery; script 04 takes about 183 minutes for one kill cycle plus verification and 244 minutes for its default two. WATCH-history supersedes WATCH-bounds2's map-removal-on-drain description and changes headroom within the same 64 MiB ceiling.

`docs/handoff/RC-fix-tests.md` and `RC-fix-product.md` do not exist in this checkout. No fifth report exists under `docs/reviews/`. Recorded these absences explicitly. The supplied eight-of-eight Rocky 8.10 attribution and 8.65% CPU at 2,000 events/s (over 10% on a loaded host) are attributed to the task brief, not measurements made here.

LEASE and fourth-review C8 establish the common NoSpace mapper; GATE-fix2 describes the historical differing assertions. CLI-format explicitly retains a normalized-enum Debug helper exception, so the absolute source-guard policy is not claimed closed. PKG-build-id leaves status JSON identity and RPM VCS metadata as follow-ups. SAMPLER-reload/PURGE-lease report a stale component-count test failure, while WATCH-history reports a later passing local suite: both are historical evidence, not a current merged gate. No implementation fix or policy decision was inferred from absent RC notes.

## Validation

The requested `cargo test -p xtask --offline` uses pinned Rust 1.98.0 and the populated host Cargo cache; no network fetching. Final outcome is recorded below before submission. This documentation-only task uses the explicit focused check instead of workspace/privileged runtime checks.

- `git diff --check`: passed after amendments.
- Compared every fenced block in `08-interfaces.md` with the task baseline: all identical, including normative config and unit.
- Compared existing operator state-layout/unmount paragraphs and security ancestry paragraph with baseline: identical. Read the watcher runbook planned-unmount section; it agrees with the retained operator procedure. No runbook change needed.
- Canonical memory attempt brief/input/receipts checked: no pending deliveries; no update acknowledgment or proposal needed.

## Remaining uncertainty / handoff

Fourth-round fixes have not been independently re-reviewed. Core non-path wire encoding sign-off, cap-only health policy, repeated quiet-host watcher CPU, 72-hour soak, 5M/20M owner-hardware runs and completed slow compaction-crash guest execution remain open. Existing paging-index, cumulative fuzz, restricted-unit and real-second-user gates remain. No privileged tests were added for prose-only changes; runtime/guest acceptance is pending its owners. This note is an evidence candidate, not verified task success or release approval.

## Final observed check result

`cargo test -p xtask --offline`: exit 0, 50 passed, 0 failed, 0 ignored (20 unit, 14 w6c, 6 w7b, 6 w7c, 4 w7d). Includes `exact_normative_unit_and_config`, `strict_ancestry_packaging_gates`, second-compaction witness and watcher helper regressions. The w7b tiny benchmark builds the debug CLI and passed; it is not a staging performance run. Cargo build artifacts are removed with cargo clean before submission; final cleanup/status and submission receipt are in the external attempt report.
