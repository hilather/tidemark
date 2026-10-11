# Handoff — W5-D: Rates, alert engine, now model and renderer

## Needs from coordinator

- Decide `scr-W5-D/SCR-1.md`: arbitrary rate/overflow windows, multiple hot-directory windows, automatic stale cadence, collector availability and webhook redaction cannot all be represented by the frozen inputs. Unsupported mount/overflow windows and `older_than = "auto"` remain unevaluated. This includes generated auto-stale defaults.
- W6 must install the journald tracing subscriber, invoke delivery sinks, and expose `render_metrics()` through its metrics endpoint. W5-D provides the library sinks and gauge rendering, not daemon wiring.

- **Branch:** hp-9f253c899eb9f40261ec77305b4f8ad97f5e8f0e97cbf367dab2b6e124bee5a7
- **Agent/session:** Codex; attempt-9f253c899eb9f40261ec77305b4f8ad97f5e8f0e97cbf367dab2b6e124bee5a7
- **Date:** 2026-10-11
- **Status:** core implementation and local evidence candidate; full specification has the gaps above. Independent verification remains coordinator work.

## What was built

Implemented translated online least-squares fitting, inclusive window filtering, and checked finite time-to-full estimates without clock calls or temporary sample buffers. Implemented all twelve configured rule kinds, glob compilation, core configuration validation, and per-rule/subject hysteresis with injected time. State is capped at 10,000 subjects; absent collectors break continuity without resolving existing alerts, while subjects absent from a complete collector result resolve normally. Added an internal space-guard alert, deterministic active summaries and escaped Prometheus gauges, structured journal tracing, and a bounded asynchronous HTTP webhook worker. The now model joins core collector values by mount; the text renderer includes growth, thin pools, phantom holders, writers, Postgres slots, scans, alerts and degradation, with UTC dates and deterministic wrapping.

## Shared-file edits

None. No dependencies or public input field lists changed. AlertEngine, WebhookDeliver and error storage were preparation shapes left to W5-D. Added delivery construction/counters and metric rendering methods within owned paths. Cargo discovers `tests/w5d/main.rs` directly; no manifest or integration shim was needed.

## Deviations from the spec

| Spec reference | Implementation | Reason | SCR |
|---|---|---|---|
| 07 §6 arbitrary windows / auto stale ages | Only 15m and 1h mount rates, 15m overflow counts; explicit stale ages | Frozen inputs omit other windows and schedule facts | SCR-1 |
| 07 §6 hot-directory windows | Caller supplies the rule-window entries; distinct windows in one evaluate call unsupported | No window identity in input | SCR-1 |
| 07 §6 webhook path redaction | Constructor explicitly selects subject/message redaction; path subjects beginning `/` become `[redacted]`, messages become generic | Referenced redaction rule is absent from 07 §6 in this checkout | SCR-1 |
| 07 §6 state_space | Immediate fire, 10m continuous-clear resolution, normal renotify | Internal rule has no configurable AlertKind or specified hysteresis | SCR-1 |
| 07 §7 now example | ASCII separators; UTC rather than machine-local time; no ANSI color; wrap by Unicode scalar count | Deterministic without timezone/environment dependency; color optional | No |
| 07 §7 missing phantom input | Empty zero-valued summary; caller must provide degradation for unavailable procscan | Frozen MountNow requires a non-optional summary | SCR-1 |
| 07 §6 webhook transport | Explicit direct connection; redirects disabled; retries back off 1/2/4s and cannot be accelerated by new queued events | Avoid inherited proxy behavior, cross-origin delivery and retry storms | No |
| Tooling | Host Rustup/Cargo homes used read-only, offline mode enabled | Default projected Rustup home was unavailable; host cache contains dependencies | No |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| 100% branch coverage of alert state machine, equivalent argument | Transition/guard table below and named tests; not a measured llvm-cov percentage | Local test evidence supplied |
| Rate behavior | `series_and_edges`: linear, noisy, sawtooth, constant, insufficient/identical timestamps, extreme integers, future-window exclusion, invalid/overflowing TTF | Passed locally |
| Twelve rule kinds | `all_rule_predicates_and_unavailable_collectors`; numeric/text rate equivalence, lossy suppression, unavailable/vanished PG slot tests | Passed locally; unsupported input contracts listed above |
| Webhook delivery | Actual ephemeral-loopback JSON POST, redaction, four-attempt failure, oldest-drop counter, timeout-bounded shutdown | Passed locally; no ignored loopback tests |
| Renderer | Four checked-in insta snapshots: healthy, filling, degraded, multi_mount; control escaping and narrow wrapping; Gregorian UTC boundary tests | Passed locally |

### State machine transition and guard argument

`step_guards_and_transitions` explicitly executes each branch in the pure `step` function. Other tests cover its engine integration. Each row identifies a test and the guard outcomes it exercises; this is the allowed equivalent argument, not an instrumentation claim.

| Transition / guard | Named test |
|---|---|
| Inactive + false remains inactive | `step_guards_and_transitions`, `hysteresis_transition_table` |
| Inactive + true starts pending | `step_guards_and_transitions` |
| Pending + true before `for` stays pending; at `for` fires | `step_guards_and_transitions`, `hysteresis_transition_table` |
| Pending + false resets continuity | `step_guards_and_transitions`, `hysteresis_transition_table` |
| Firing + true before renotify stays firing; at renotify emits | `step_guards_and_transitions`, `hysteresis_transition_table` |
| Firing + false starts clearing | `step_guards_and_transitions` |
| Clearing + false before `resolve_for` waits; at boundary resolves | `step_guards_and_transitions`, `hysteresis_transition_table` |
| Clearing + true cancels clearing, preserves firing | `step_guards_and_transitions`, `hysteresis_transition_table` |
| Zero firing/resolution durations | `zero_durations_disabled_and_invalid` |
| Elapsed guard: negative elapsed, zero duration, fractional-duration below/above boundary | `step_guards_and_transitions` |
| Missing observation resets pending / preserves firing; clock regression resets timers | `subjects_missing_and_backward_clock` |
| Independent rules/subjects | `subjects_missing_and_backward_clock` |
| Disabled rules; invalid construction | `zero_durations_disabled_and_invalid` |
| Subject cap exceeded; duplicate observations ignored | `subject_cap_and_duplicate_observations` |
| Complete collector loses subject vs unavailable collector | `vanished_slot_resolves_but_unreachable_does_not` |
| Internal space guard immediate fire and delayed resolution | `internal_space_guard` |

## Tests

Unprivileged tests added (15 unit + 5 integration):

- rates: `series_and_edges`.
- engine: `hysteresis_transition_table`, `subjects_missing_and_backward_clock`, `zero_durations_disabled_and_invalid`, `subject_cap_and_duplicate_observations`, `step_guards_and_transitions`, `internal_space_guard`.
- rules: `all_rule_predicates_and_unavailable_collectors`, `vanished_slot_resolves_but_unreachable_does_not`, `numeric_and_text_rates_match_and_lossy_is_suppressed`.
- delivery: `webhook_json_and_redaction`, `retries_are_bounded`, `queue_drops_oldest`, `request_timeout_bounds_shutdown`.
- now: `utc_dates`; integration: `healthy`, `filling`, `degraded`, `multi_mount`, `narrow_and_controls`.

No privileged W5-D test is listed by the wave task: logic is pure, webhook tests use unprivileged loopback. The workspace privileged-feature test build compiles; privileged workspace execution and daemon/journald integration are **pending VM**. No tests ignored for another task; no host mounts, LVM, metadata or services were changed.

## Validation

All commands use offline Cargo against the host cache. Full final transcripts are in the attempt output `library/{fmt,clippy,test,privileged-build,doc,ci}.log`, with exit codes in `library/checks.tsv`.

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --workspace --features privileged --tests`
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`
- `tools/ci.sh`

Final revision results: all six commands exited 0 (fmt 0.4s, Clippy 4.9s, tests 29.2s, privileged build 15.7s, rustdoc 1.6s, CI 37.5s). W5-D contributes 15 passing unit tests and 5 passing integration tests. The final command outcomes are also recorded in the output report. Cargo-deny is absent; CI explicitly skips its advisory/license check. No VM was available.

## Measurements

No VM performance measurement requested for W5-D. Local pure logic and ephemeral HTTP tests are functional evidence only.

## Known gaps and follow-ups

- Auto stale defaults and arbitrary/multiple rule windows require SCR-1 resolution and coordinated W6 adapter changes.
- Redaction does not remove paths embedded in configured rule names; coordinator must settle the complete policy before external webhook enablement.
- Renderer wraps by scalar count, not terminal column width for wide Unicode, and does not colorize output.
- Snapshot fixtures do not cover every combination of all optional report fields. Every predicate kind has a firing test, but predicate branch coverage has not been measured; the state-machine table covers the acceptance scope.
- Journald and Prometheus daemon endpoint integration belongs to W6; VM execution and cargo-deny audit remain pending.

## Notes for the reviewer

Library logic does not obtain wall-clock time. Webhook retry timing uses monotonic elapsed time only in the I/O worker. Dropping the worker cancels queued deliveries and retries and joins after any in-flight request's five-second timeout. A full alert map drops new subjects until retained states clear; input ordering should therefore be deterministic. An unavailable collector leaves previously firing alerts active, with no re-notify until observed again.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits.
- [x] Public items documented and owner headers present; no new production unwrap/expect or unsafe.
- [x] Core transition/guard evidence supplied.
- [ ] Complete specification satisfied: SCR-1 gaps remain.
- [ ] VM integration and advisory/license audit completed: pending equipped host.
