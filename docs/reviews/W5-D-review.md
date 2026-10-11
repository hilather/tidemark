**CHANGES REQUESTED** — 3 BLOCKER, 4 SHOULD-FIX, 1 NIT.

Reviewed the exact candidate `164446d8833f9ded3c79a738201bf1efd9f9c354` against base `6712375f74903cacd88da70040acbf1d9738ffb0`, limited to its W5-D changes. All source line references below refer to that candidate. The attempt checkout starts at the later `7e73cee16a8d68f45d8ca0c20bd9cd68242148ea`; builds and probes used an unmodified archive of the candidate under ignored `target/review-candidate`, not that later tree. This report is review evidence, not an integration or acceptance decision.

Read AGENTS.md, the wave header/W5-D contract, 07 §§2/6/7, 08 alert and metric contracts, 13 contract decisions, W5-D handoff/SCR-1, and the candidate diff. The exact candidate predates ADR-019 and contains the historical PostgreSQL surface; the later checkout removes it. Per the retained owner instruction, this review evaluates the nine remaining kinds and does not review PostgreSQL behavior or propose restoring it. Consequently this is not a certification that the historical candidate itself satisfies the later removal decision.

## Findings

### BLOCKER — Missing thin-pool measurements resolve an active alert

`finding:w5d-unavailable-pool` — `crates/tidemark-live/src/alerts/rules.rs:97`, `:183`; `crates/tidemark-live/src/alerts/mod.rs:142`.

**Contract:** 07 §6.2 requires continuously false observations for `resolve_for`; unavailable input is not a healthy measurement.

`available()` returns true for every thin-pool rule whenever any LVM row exists. `visit()` emits nothing for a pool with missing data or metadata percentages. The engine then treats that omitted subject as absent and feeds `false` into its state machine. A public-API probe fires `vg/pool` at 90% with thresholds 80%, `for=0`, `resolve_for=10s`, then retains the same row with both percentages `None`. Evaluations at t=1 and t=11 emit `Resolved` at t=11, value 0, and remove the active alert. No recovery was measured.

**Change:** distinguish subject-level unknown measurements from confirmed disappearance. Missing/nonfinite percentages must break clearing continuity and preserve an existing firing alert. Only measured false or confirmed disappearance may resolve it. Add regressions for one missing percentage, both missing, and unrelated healthy LVM rows.

### BLOCKER — Healthy observations exhaust the subject cap and hide a later breach

`finding:w5d-subject-cap` — `crates/tidemark-live/src/alerts/mod.rs:106`, `:110`, `:173`.

**Contract:** 07 §§6.1–6.2, W5-D directory-growth evaluation, AGENTS rules 10 and 12.

Every observation allocates a state before its condition is checked; inactive states are removed only after all rules have run. A sorted input of 10,000 healthy directories followed by one directory above its threshold produces zero events and zero active alerts, even with `for=0`. The cap therefore blocks the only subject that needs state. Repeating the same input repeats the loss; the existing cap test only supplies firing observations and misses this case.

**Change:** do not retain or allocate an unseen false observation in the state map. Reserve the bounded state budget for pending/firing subjects, prune inactive states before admission decisions, and make genuine capacity drops observable. Add a regression with 10,000 below-threshold subjects before one above-threshold subject, including multiple evaluations.

### BLOCKER — Enabled stale defaults and accepted windows silently do nothing

`finding:w5d-unsupported-inputs` — `crates/tidemark-live/src/alerts/rules.rs:65`, `:139`, `:253`, `:264`.

**Contract:** 07 §§2/6.1/6.4, 08 §3, W5-D acceptance; SCR-W5-D/SCR-1 remains pending.

The exact unevaluated configurations are:

- Every `scan_stale` rule with `older_than="auto"`, including every generated stale default.
- Every `time_to_full` window other than exactly 900 or 3,600 seconds, including the documented 5-minute rate window.
- Every `watch_lossy` window other than exactly 900 seconds.

All are accepted by construction and remain visible as enabled rules. They generate no observation, event, warning, or runtime explanation from this implementation. The public-API probe accepts an automatic stale rule and emits nothing for a root with no complete scan at t=1,000,000,000. It also accepts a 5-minute TTF rule but never evaluates it. Only the handoff/SCR explains this to a reader of development documents.

**Change:** obtain the coordinated SCR decision and provide window/cadence observations sufficient to implement the promised rules, including twice the effective cadence with the windowed-schedule minimum. Until implemented, explicitly surface these rules as unevaluable with a reason; do not present W5-D as complete or an enabled default as working protection. The filed SCR is appropriate, but a pending proposal does not waive these acceptance requirements.

### SHOULD-FIX — Different hot-directory windows reuse the same aggregate

`finding:w5d-hot-windows` — `crates/tidemark-live/src/alerts/rules.rs:158`, `:166`.

**Contract:** 07 §6.1 and the W5-D `hot_dirs_window` rule-window input; pending SCR-1.

Unlike the unsupported windows above, mixed hot-directory windows are actively evaluated with incorrect data. A 5-minute rule and a 1-hour rule both receive the same 3,600-byte aggregate and report 12 B/s and 1 B/s. Both fire against 0.5 B/s. If those bytes arrived 30 minutes ago, the actual 5-minute rate is zero. There is no window identity in the input, and no rejection or diagnostic for this incompatible rule set.

**Change:** after the contract decision, associate observations with their actual window/rule. In the interim detect incompatible windows for the same mount and report them as unsupported rather than divide one aggregate by several durations. Test distinct 5-minute/hour histories with growth outside the shorter window.

### SHOULD-FIX — Renotification stops while an alert is still firing but clearing

`finding:w5d-clearing-renotify` — `crates/tidemark-live/src/alerts/mod.rs:270`, `:274`.

**Contract:** 07 §6.2 says re-notify every `renotify` while firing; an alert remains firing until `resolve_for` expires.

The renotify guard exists only in the `condition == true` branch. With `for=0`, `resolve_for=60s`, `renotify=10s`, fire at t=0 and observe false at t=1 and t=10: `active()` still contains the alert, but no renotification is emitted at t=10. A regression assertion expecting `Renotify` fails. Long resolution intervals and repeated clearing flaps can suppress the documented cadence.

**Change:** evaluate renotification for a still-firing state after handling resolution, regardless of the current known boolean condition. A resolution event should take precedence at its boundary. Add timeline cases where renotify falls inside clearing and where it coincides with resolution. Preserve the separately documented unavailable-input behavior.

### SHOULD-FIX — An HTTP redirect is silently counted as successful delivery

`finding:w5d-webhook-redirect` — `crates/tidemark-live/src/alerts/deliver.rs:92`.

**Contract:** 07 §6.3 requires webhook delivery with bounded retries.

The worker disables redirects but treats any successful `send_json` return as delivery success. With the candidate's locked ureq version, HTTP 302 is an `Ok` response. A local receiver returning 302 with a `Location` header receives only the initial POST; the worker neither follows nor retries it and `failed()` remains zero. The destination never receives the alert.

**Change:** accept only a 2xx status as success. Keep redirects disabled, but treat a redirect response as failure under the existing retry/counter policy. Add a local redirect-response test alongside the 503 test.

### SHOULD-FIX — Renderer collapses distinct byte names and escaped paths

`finding:w5d-display-bytes` — `crates/tidemark-live/src/now.rs:298`, `:309`.

**Contract:** AGENTS rule 7; 07 §7a; 13 SCR-007 and the accepted query-path escaping rule.

`bytes_display()` falls back to UTF-8 replacement for relative writer keys, so `b"proc\xff"` and `b"proc\xfe"` both display as `proc�`. The custom string escape helper also preserves literal backslashes while escaping controls: a configured mount containing a newline and one containing literal `\n` produce identical text. Both collisions were reproduced through `build_now`/`render_now_text`.

**Change:** use `tidemark_core::path::display_lossy_bytes` directly on original bytes for raw keys and string fields. Do not normalize names through `RawPath::new` merely to display them. Add assertions that both pairs remain distinguishable.

### NIT — Extra public engine surface lacks a contract decision

`finding:w5d-public-surface` — `crates/tidemark-live/src/alerts/mod.rs:48`, `:229`.

**Contract:** AGENTS rule 2 and the W5-D published engine interface (`new`, `evaluate`, `active`).

The diff adds public `MAX_ALERT_SUBJECTS` and `AlertEngine::render_metrics`, neither listed in that interface nor approved by a contract decision. The handoff discloses the metric method, but that is not a contract amendment. Webhook construction/error storage is legitimately left to the owning task; this extra engine interface is separate.

**Change:** keep the cap internal and render the metric from the already-public `active()` summaries in the owning metrics layer, or obtain an explicit contract decision before exposing the additions.

## Independent state-machine comparison

The following table derives from 07 §6.2. “Clearing” remains part of the firing state until the resolution delay completes.

| State/input | Required transition | Candidate |
|---|---|---|
| Inactive, false | Remain inactive | Correct |
| Inactive, true | Start pending; fire immediately if `for=0` | Correct |
| Pending, true before/at `for` | Wait / emit Firing | Correct |
| Pending, false | Reset continuity | Correct |
| Firing, true | Remain firing; renotify at cadence | Correct |
| Firing, false | Start clearing; resolve immediately if `resolve_for=0` | Correct |
| Clearing, false before/at `resolve_for` | Remain firing / emit Resolved | Correct, except missing renotify while waiting |
| Clearing, true | Cancel clearing, preserve firing age | Correct |
| Unknown observation | Do not invent false; break pending/clearing continuity | General engine policy does this; thin-pool availability violates it |
| Subject disappears from a known complete result | Treat as false and honor `resolve_for` | Implemented, but completeness representation is limited |

The handoff's transition table and named tests exercise both guards in the pure `step` branches, but that does not establish all semantic cases. It misses the clearing-renotify case and the state-admission bug. Separate-subject behavior, short flaps, zero durations, time regression and internal space-guard behavior are tested meaningfully.

There is no rule-update or state-restoration method. Replacing an engine with a disabled rule drops the previous active state without producing a resolution; reconstructing it after a restart repeats the pending delay and emits a fresh firing event. Standalone probes confirmed those reconstruction behaviors. The frozen interface provides no way to restore persisted timers. Actual persisted-state restart and live-disable semantics therefore remain a W6/contract integration question, not a verified capability or an invented W5-D API requirement.

An empty `dir_growth` vector is considered unavailable, so an existing directory-growth alert stays active indefinitely; another same-root entry is needed before absence becomes false. A probe confirms this. The input cannot distinguish an empty complete result from an unavailable comparison. SCR-1's completeness/availability decision must cover this ambiguity. Empty nonlossy hot entries for an explicitly present mount do permit disappearance resolution; a missing mount or lossy result breaks continuity instead.

## Nine-rule audit

Threshold validation delegates to core and rejects invalid/unused fields even on disabled rules. Boundary comparisons are strict as specified.

| Kind | Accepted threshold and calculation | Unevaluable versus false |
|---|---|---|
| `fs_used_pct` | Numeric percent; `100*used/size > above` | Missing mount or zero size unknown; equal/below false |
| `time_to_full` | Duration-string `below`; available bytes / selected rate | Missing/nonfinite rate or unsupported window unknown; finite nonpositive rate false |
| `hot_dir_growth` | Finite nonnegative numeric B/s or core rate string; raw-byte glob and delta/window | Lossy entries omitted; missing mount unknown; equal/below/negative false; mixed-window defect above |
| `thin_pool_pct` | Numeric data/meta thresholds; VG/pool globs; either percentage above | Only tracked thin LVs; missing percentages mishandled as described; measured equal/below false |
| `vg_free_low` | Core size-string `below`; free bytes < threshold | Empty LVM unknown; a vanished matching VG is false when LVM is considered complete |
| `dir_growth` | Integer numeric bytes or core size string; matching root, depth <= limit, delta > threshold | No same-root observations unknown; missing subject with other same-root observations false |
| `phantom_bytes` | Integer numeric bytes or core size string; total > threshold | Missing mount unknown; measured zero/equal/below false |
| `scan_stale` | Explicit duration; age since last complete > threshold | Missing root/automatic age unknown; present root with no complete scan true; future finish clamps age to zero |
| `watch_lossy` | Nonnegative integer numeric count; overflow count > threshold | Missing mount/non-15m window unknown; equal/below false |

Core `default_rules` is correctly re-exported. Its suffix generation retains `/` and ASCII alphanumeric/`._-` and percent-encodes other UTF-8 bytes in uppercase. The `/data` special case and same-name merging remain core-owned; this diff does not change their implementation. Disabled rules/input collectors do not create fresh alerts; existing unavailable alerts are retained. Automatically generated stale rules have the gap above.

## Other checks and limits

- Least squares uses translated online arithmetic, bytes/second units and at least three samples. Existing tests cover linear/noisy/sawtooth/constant series and TTF edges. Additional assertions cover empty/single/two samples, identical timestamps, negative slope, the full i64 span and 375 extreme three-sample combinations; none returned a nonfinite slope. Inclusive window filtering excludes future samples. TTF rejects nonfinite/nonpositive rates and unrepresentable durations; zero available bytes with a positive rate returns zero.
- Delivery has a 1,000-event oldest-drop queue, a drop counter, an initial attempt plus three retries, 1/2/4-second backoff and a five-second global request timeout. New events cannot shorten a retry wait. Existing loopback tests verify POST JSON, timeout-bounded shutdown, four attempts on 503, and oldest-drop counting. No daemon or remote service was contacted.
- Journal delivery emits the four specified tracing field names. Actual journald subscriber installation/field-prefix configuration is W6 work and was not exercised. The metric uses exactly `rule,subject`, escapes backslash/quote/newline, and exposes at most 10,000 active user pairs plus the internal alert. The separate configured metrics-series limit is not applied by this helper; W6's exporter remains to be checked.
- **Verified unresolved privacy limit:** `redact_paths=true` still sends a generated name such as `fs_used_pct:/secret/mount` in both `rule` and `text`. The probe captured it. The handoff explicitly acknowledges this and SCR-1 requests a redaction policy; no complete privacy guarantee can be made from the candidate. No additional policy was inferred here.
- `build_now` sorts mounts, scans, alerts and hot entries deterministically and carries watcher degradation. Missing phantom input becomes a zero summary, leaving explicit degradation to the caller as recorded in SCR-1. UTC/date and four requested renderer snapshot cases pass. Terminal-column handling for wide Unicode and actual sub-50-ms performance were not measured.
- The diff contains no shared manifests/core/schema modifications. Library logic uses injected time; monotonic timing is limited to delivery I/O. No added unsafe, filesystem traversal, scanned-file content reads, metadata writes, host configuration operations or recursion over directory depth were found. Symlink/mount/NOATIME/Gate traversal rules have no new scanning path to exercise here. Production errors use `thiserror`; no unjustified production unwrap/expect or library println was introduced.
- W5-D's required unprivileged tests exist and make real assertions; none checks that another task remains a stub. W5-D has no specified privileged test. Workspace privileged tests were compiled, never executed. VM runtime, actual daemon restart/reload/persistence, journal/metrics integration, cargo-deny audit and a measured coverage report remain unchecked. The handoff's pure-branch argument is not proof of the omitted scenarios above.

## Build and probe evidence

All required checks ran offline on the exact candidate, with Rust 1.98.0, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup` and `CARGO_NET_OFFLINE=true`. An initial CI attempt stopped at fmt because the projected Rustup home was read-only; the corrected environment was used for the results below.

| Check | Observed result |
|---|---|
| `tools/ci.sh` | Exit 0: source checks, fmt, Clippy with warnings denied, workspace tests; cargo-deny explicitly skipped because not installed |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0; no rustdoc warnings |
| Candidate W5-D tests within CI | 15 unit and 5 integration tests passed |
| Standalone state probes | 2 expected-behavior assertions failed (cap starvation, clearing renotify); 2 reconstruction-behavior checks passed |
| Standalone rules/rates probes | Exit 0; reproduced thin-pool false resolution, mixed windows, unsupported rules and empty-growth behavior; numeric edge assertions passed |
| Standalone delivery/render probe | Exit 0; reproduced display collisions, path-bearing rule-name leakage and 302 loss |

Supporting sources and full transcripts are under the attempt output directory `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-1a1fbbbf661d482cdb55a10efcaeabc6a836fcbb370d53294555d1868fa42561/library/`: `ci.log`, `privileged-build.log`, `doc.log`, `checks.tsv`, `state-probe/`, `state-probe.log`, `rules-probe/`, `rules-probe.log`, and `delivery_now_probe.rs`/`.log`. Probe dependencies match the candidate lock versions. The archive's source was not edited, and the only repository deliverable is this report.
