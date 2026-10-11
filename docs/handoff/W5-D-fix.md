# Handoff — W5-D-fix: Rates, alerts and now review fixes

- Branch: `hp-f784ec34facd4307c2d9c6487176234a3363e5193a8d63e2fdd681b4fb11de33`
- Agent: Codex, canonical attempt `attempt-f784ec34facd4307c2d9c6487176234a3363e5193a8d63e2fdd681b4fb11de33`
- Date: 2026-10-11
- Status: implementation and local evidence candidate; independent verification remains coordinator work.

## Findings checked against the attempt baseline

All eight findings were still present at `74e16714453fd3992269de91d92fae70274c84af`. Rates already satisfied the review's numerical checks and were left unchanged. Tests below are in `crates/tidemark-live/src/alerts/tests.rs`, except where another file is named.

| Finding | Status | Implementation file | Evidence test |
|---|---|---|---|
| Missing thin-pool measurements resolve active alert | fixed now | `src/alerts/rules.rs`, `src/alerts/mod.rs` | `missing_pool_percentages_never_resolve`: each missing percentage, both missing, nonfinite percentage, unrelated healthy row, then confirmed disappearance |
| Healthy observations exhaust subject cap | fixed now | `src/alerts/mod.rs` | `healthy_subjects_do_not_hide_breach`, `capacity_is_reported`, `duplicate_observations_and_healthy_eviction`, `capacity_never_evicts_unavailable_clearing_alert` |
| Enabled stale defaults and accepted windows silently do nothing | fixed now | `src/alerts/rules.rs`, `src/alerts/mod.rs`, `src/now.rs` | `arbitrary_windows_and_auto_cadence`, `all_nine_rules_and_unavailable_reasons`, integration `missing_sections_and_alert_diagnostics_are_visible` |
| Different hot-directory windows reuse one aggregate | fixed now | `src/alerts/rules.rs` | `hot_windows_use_distinct_histories`: growth outside the 5m window remains in the 1h result only |
| Renotification stops during clearing | fixed now | `src/alerts/mod.rs` | `clearing_false`, `resolution_precedes_renotify` |
| Redirect counted as successful webhook delivery | fixed now | `src/alerts/deliver.rs` | `redirects_are_failed_deliveries`: 302 with Location, four POSTs to original URL, failure counter; `retries_are_bounded` covers 503 |
| Renderer collapses byte names and escaped paths | fixed now | `src/now.rs`, `src/alerts/rules.rs` | integration `byte_names_and_escaped_paths_are_distinct`; `raw_subject_bytes_preserve_identity` |
| Unapproved public engine surface | fixed now | `src/alerts/mod.rs` | Cap is private; `render_metrics` removed; source inspection and workspace compilation. Metrics owner must derive the existing gauge from `active()` |

Paths in the implementation column are relative to `crates/tidemark-live/`.

## Coordinator decisions implemented

`AlertInputs` now borrows an `AlertSource` instead of fixed aggregates. Filesystem rates, hot directories and overflows are requested with each rule's exact Duration. `lvm()` returns a complete tracked-LV vector, rather than one row, because thin-pool and VG rules can match several subjects. `dir_growth(root, depth)` has no window argument: the nine-kind configuration contract gives that rule no window; it compares snapshots. The source must provide complete, bounded inventories; `Some(empty)` confirms absence and `None` is unavailable.

`ScanAge` carries last-complete time and effective cadence. Automatic stale thresholds are twice that cadence. The daemon source must calculate effective cadence with the plan's minimum 24h for windowed schedules; non-windowed short cadences remain valid. Unknown cadence produces an explicit diagnostic. A known root with no complete scan breaches staleness. No core/config/schema/plan files changed.

Unknown observations preserve pending, firing and clearing states, values and timers without generating events. A pause flag prevents unknown time from establishing continuous true/false evidence: the next known observation restarts pending/clearing continuity, while preserving firing age. Backward clocks likewise pause continuity while retaining pending state; their renotify origin is reset to the injected clock. Each affected enabled rule has a reason in `AlertEngine::diagnostics()`. This accessor is the alert-listing input; `NowInputs.unevaluable` carries it into the frozen report's `degraded` list and terminal view. Actual CLI/daemon alert listing wiring belongs to Wave 6, which has no consumer yet.

Healthy unseen subjects allocate no state. Healthy pending states are removed immediately. At the 10,000-pair limit, clearing states measured false in the current evaluation are evicted before admitting a new breach; if all retained subjects need protection, the new subject is dropped with an explicit capacity diagnostic. Capacity is reported even if another measurement in that rule is unavailable. An earlier clearing observation does not make an unavailable subject eligible for eviction. Input collections are sorted by raw subject bytes before admission. The state and admission sets are bounded; healthy eviction uses an ordered set rather than scanning the full map on every rejected entry. Eviction itself does not fabricate a recovery event.

Each measured `NowInputs` section uses Option: `None` is not measured and `Some(empty)` is measured empty. Missing sections are encoded in `NowReport.degraded` because the core report schema is frozen. Missing phantom and LVM values render as “not measured”; phantom's required core placeholder is never presented as measured zero. Raw byte keys and string fields use `display_lossy_bytes` directly, without path normalization or UTF-8 replacement.

Webhook bodies preserve rule, escaped subject, state, value, message, host, ts and text. Engine path subjects use `RawPath::display_lossy()` or `display_lossy_bytes`; delivery forwards that display text. There is no separate redaction rule in v1, and the redaction constructor parameter is removed. Only 2xx responses succeed; redirects remain disabled. Queue cap, timeout and retry policy are unchanged.

## Public surface for contract reconciliation

The task explicitly authorizes reshaping these crate-local inputs. New public integration surface: `AlertSource` and its eight methods, `ScanAge`, `AlertDiagnostic { rule, reason }`, `AlertEngine::diagnostics`, `AlertInputs { source, state_space_refused }`, optional measurement sections plus `NowInputs.unevaluable`, and the two-argument `WebhookDeliver::new(url, host)`. Record these in the coordinated Wave 6 interface documentation. Existing delivery construction/error variants and dropped/failed counters remain; they are implementation interfaces absent from the wave's abbreviated signatures. The extra metric method and public cap are removed rather than requesting their approval.

## Transition table and branch argument

This is named-test evidence, not a measured coverage percentage. Every state/input cell includes not evaluable. Unknown cells also assert unchanged timers and active summaries; subsequent observations prove pending/clearing continuity restarts.

| State | Observation | Transition | Named test |
|---|---|---|---|
| Inactive | false | Remain inactive, allocate nothing | `inactive_false` |
| Inactive | true | Start pending; zero delay fires immediately | `inactive_true`, `disabled_invalid_and_zero_resolve` |
| Inactive | not evaluable | Remain inactive; report reason | `inactive_unknown` |
| Pending | false | Reset pending | `pending_false` |
| Pending | true | Before delay wait; at delay fire | `pending_true` |
| Pending | not evaluable | Preserve pending; pause continuity; no event | `pending_unknown` |
| Firing | false | Start clearing; zero delay resolves | `firing_false`, `disabled_invalid_and_zero_resolve` |
| Firing | true | Preserve firing; renotify at cadence | `firing_true` |
| Firing | not evaluable | Preserve firing; no renotify or recovery | `firing_unknown` |
| Clearing | false | Renotify while waiting; resolve at boundary | `clearing_false`, `resolution_precedes_renotify` |
| Clearing | true | Cancel clearing; preserve firing age | `clearing_true` |
| Clearing | not evaluable | Preserve clearing/firing; pause continuity; no event | `clearing_unknown` |

`clock_regression_zero_durations_and_space_guard` covers clock regression (including pending plus unavailable data), internal guard, negative elapsed time, and fractional duration boundaries. `healthy_subjects_do_not_hide_breach` and `missing_pool_percentages_never_resolve` cover confirmed absence from measured empty results. `all_nine_known_predicates_and_threshold_boundaries` covers known predicates and numeric/text hot-rate boundaries; `all_nine_rules_and_unavailable_reasons` covers unavailable sources for every kind. Existing rates and report snapshots remain.

## Shared files and dependencies

None. Only owned paths changed. No dependencies added. No filesystem scan paths, unsafe, metadata writes, host configuration operations, or PostgreSQL features introduced.

## Validation

Final command results are recorded below. Offline Cargo uses Rust 1.98.0, `/home/brewerm/.cargo` and `/home/brewerm/.rustup`, with `CARGO_NET_OFFLINE=true` also passed to CI's child Cargo processes. Full transcripts are in the canonical attempt output's `library/`.

No privileged runtime was run. W5-D is pure logic plus unprivileged HTTP loopback and has no specified privileged test. Workspace privileged-feature compilation is required; VM kernel, daemon/journald, persistence/reload and metrics integration remain **pending VM**. CI may skip cargo-deny if the host lacks it; record the observed result below. Terminal width still counts Unicode scalar values, as before; performance and instrumentation coverage were not measured.


| Final check (offline) | Result | Transcript |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/fmt.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log` |
| `cargo test --workspace` | exit 0 | `library/test.log` |
| `cargo build --workspace --features privileged --tests` | exit 0; compilation only | `library/privileged-build.log` |
| `tools/ci.sh` | retry exit 0; all available checks passed | `library/ci.log` |
| `cargo test -p tidemark-live` | exit 0; 48 unit tests and 8 report integration tests | `library/live-final.log` |

One final CI run failed in the unowned `tidemark-cli/src/client.rs:328` test `client::tests::deadline_and_poisoned_connection`, with `Io(Interrupted system call)`. The individual CLI test retry passed, the changed crate passed separately, and the full CI retry passed. No CLI file was edited. `library/ci-interrupted.log` and `library/cli-deadline-retry.log` retain that evidence; the timing/signal sensitivity merits an owning-task follow-up. CI explicitly skipped cargo-deny because it is not installed. Earlier intermediate check logs are retained under `library/initial-checks/` and `library/second-checks/`; final checks cover the final source revision.

## Definition of Done — local implementation evidence

- [x] Owned paths only; no shared edits or dependencies.
- [x] Formatting and workspace Clippy clean.
- [x] Unprivileged workspace tests pass; CI retry passes with the documented skip and intermittent unrelated failure.
- [x] Privileged-feature workspace tests compile; W5-D defines no privileged runtime case; VM integration remains pending.
- [x] Public items documented; owner headers present; no production unsafe or unjustified unwrap/expect added.
- [x] All eight findings have implementation and named-test evidence; all twelve state/observation cells are covered.
- [x] Handoff records the accepted decision, changed interfaces and remaining integration uncertainty.

These checks are local evidence, not independent verification or a VM/release gate.
