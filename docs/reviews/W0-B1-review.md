**CHANGES REQUESTED — 5 BLOCKER, 7 SHOULD-FIX, 3 NIT findings.**

Reviewed candidate `f854b42557bddc6fc5bcb294a5fc5127c8ce3163` against base `b7fffb4614e9d757b17679c055206ecd12d70f6a`, scoped to the candidate diff. The supplied review checkout at `d527b9943f3a7381e037ae798623deed32c5dfe5` has exactly the candidate's tree (`git diff candidate HEAD` was empty before this report). Source references below are candidate line numbers. No implementation or plan files were changed.

The explicit scanner and live field lists, snapshot columns, metadata keys and configuration example largely match. The remaining issues include implementation defects and conflicting or incomplete plan contracts. Findings that follow the literal plan but expose a document problem say so explicitly. The four SCR recommendations are proposals for the coordinator, not accepted decisions.

## Findings

### B1 — BLOCKER: metrics validation permits non-loopback listeners

Reference: `finding:w0b1-b1`. Location: `crates/tidemark-core/src/config.rs:858`. Plan: `docs/plan/waves/wave-6-daemon-packaging.md`, W6-B, optional HTTP; `12-decisions-and-risks.md`, R12.

W6-B explicitly requires config validation to reject non-loopback addresses. The implementation only parses `SocketAddr`. A compiled probe found no validation issues for `0.0.0.0:9877`, `[::]:9877` or `192.0.2.1:9877`. These settings would reach the later unauthenticated metrics listener despite the planned loopback restriction.

Require a successfully parsed address whose `ip().is_loopback()` is true; keep the empty string as disabled. Test IPv4 and IPv6 loopback, wildcard and external addresses. Affects W0-B2 and W6-A/B.

### B2 — BLOCKER: raw report serialization does not produce the required path encoding

Reference: `finding:w0b1-b2`. Locations: `crates/tidemark-core/src/live.rs:227`, `live.rs:173`, `crates/tidemark-core/src/path.rs:14`. Plan: `03-link-semantics.md` §5, non-UTF-8 names; W0-B `path`/`json`; W6-A CLI JSON reports.

The normative link semantics require `path_b64` for non-UTF-8 JSON paths. `RawPath::to_json` implements that convention, but derived serialization of `RawPath` and raw live path fields emits integer arrays. A compiled whole-envelope probe serialized an `OpenWriter` path `/data/\xff` as `"path":[47,100,97,116,97,47,255]` inside `JsonDoc`, with no `path_b64`. W6-A's raw-report JSON path therefore does not acquire the promised encoding merely by wrapping the core type.

Preserve the explicit internal byte field types from `07` §7a, but define and implement shared report serialization adapters that emit a UTF-8 `path` string or its mutually exclusive `path_b64` sibling. Apply them to enclosing reports, including nested phantom holders, snapshot paths and path-bearing hot entries, and test complete UTF-8/non-UTF-8 outputs. SCR-002 must settle the corresponding request-root encoding. The API's current root array is an undocumented choice, not a violation of a nonexistent root-string example.

### B3 — BLOCKER: the documents disagree about the root of a stale-scan rule

Reference: `finding:w0b1-b3`. Locations: `crates/tidemark-core/src/config.rs:979`, `config.rs:1170`. Plan: `07-live-monitoring.md` §6.1/§6.4; `08-interfaces.md` §3 alert fields; W5-D rule semantics.

`07` lists `root, older_than` for `scan_stale`. `08`'s normative flat-field table permits `root` only for `dir_growth`. The code follows `08`: a valid-looking stale rule with `root="/data"` is rejected as “not used by this rule kind”, and the generated default is one unscoped rule. This is a document conflict, not silent divergence from the chosen table.

Amend `08` to allow and require `root` for `scan_stale`, change validation accordingly, and generate a stable named default per configured scheduled root. Specify that roots with schedule `off` get no generated stale rule; explicitly configured stale rules can still monitor manually scanned roots. Update SCR-003 and W5-D before freezing. This preserves `07`'s ability to set different stale thresholds per root.

### B4 — BLOCKER: numeric hot-directory growth thresholds are rejected

Reference: `finding:w0b1-b4`. Location: `crates/tidemark-core/src/config.rs:1057`. Plan: `07-live-monitoring.md` §6.1; `08-interfaces.md` §3, `above`; W5-D.

`07` permits bytes/s or a rate string for `hot_dir_growth`; `08` allows numeric and textual thresholds. The validator has a text-only arm for this kind. A probe with `above=1024.0`, an absolute mount/path and a valid window returns a threshold-type error.

Accept `AlertThreshold::Number(n)` when finite and nonnegative, explicitly interpreting it as bytes/s, alongside the existing rate-string form. Add positive, negative and nonfinite numeric cases plus string equivalence tests. Clarify the unit in SCR-003 so W5-D evaluates both representations identically.

### B5 — BLOCKER: size formatting loses the three-significant-digit contract at rounding boundaries

Reference: `finding:w0b1-b5`. Location: `crates/tidemark-core/src/units.rs:87`. Plan: W0-B `units` deliverable.

The plan requires binary units with three significant digits. Precision is selected before rounding. Reviewer assertions reproduce `fmt_bytes(10239) == "10.00 KiB"` and `fmt_bytes(102399) == "100.0 KiB"`, both four significant digits. Just below a unit boundary, `fmt_bytes(1048575)` returns `"1024 KiB"`. The two existing formatting assertions cover ordinary values only.

Round to three significant digits, recompute precision when rounding changes the decimal magnitude, and handle unit carry consistently. Add boundary tests expecting `10.0 KiB`, `100 KiB` and, with unit promotion, `1.00 MiB`; apply the shared rounding fix to `fmt_rate`. SCR-004 should also explicitly document the implementation's exact-byte exception below 1 KiB, which W0-B currently does not state.

### S1 — SHOULD-FIX: writer windows inherit an unexplained restriction from hot queries

Reference: `finding:w0b1-s1`. Location: `crates/tidemark-core/src/api.rs:164`. Plan: `08-interfaces.md` §1.2/§2; W5-B `ProcScanner::writers`.

Only `hot` is explicitly limited to `1m/5m/15m/60m`. `writers` has a window parameter, and its consumer accepts `Duration`. The candidate reuses `HotWindow`, so even `"2m"` and the equivalent spelling `"1h"` are rejected. Neither the API table nor SCR-002 explains this narrowing.

Use `Duration` with humantime serde for `WritersParams.window`, and document a positive-duration requirement and any intended maximum in `08`. Retain `HotWindow` for `hot`. This is an unresolved API interpretation rather than a claim that `08` explicitly enumerates additional writer values. Affects W0-B2, W5-B and W6-A.

### S2 — SHOULD-FIX: the prescribed JSON envelope cannot deserialize ordinary runtime input

Reference: `finding:w0b1-s2`. Location: `crates/tidemark-core/src/json.rs:9`. Plan: W0-B `json`; SCR-004.

The code follows the literal `schema: &'static str` requirement. Its derived deserializer consequently implements only the static-lifetime case. Reviewer compile probes demonstrate both a failed `DeserializeOwned` bound and E0597 when parsing from a local `String`. Parsing CLI output in normal tests/readers cannot use this envelope without leaking memory or defining a second type.

Amend the plan and field to `Cow<'static, str>`, deriving `Deserialize` without `#[serde(borrow)]`. A control probe verifies that this supports both owned deserialization and a runtime buffer while allowing borrowed schema constants for output. This does not currently break W4-B's API client, whose results are payloads rather than CLI `JsonDoc` envelopes. Affects W0-B2 and later JSON-output tests/readers.

### S3 — SHOULD-FIX: generated alert names depend on mount ordering

Reference: `finding:w0b1-s3`. Location: `crates/tidemark-core/src/config.rs:1091`. Plan: `07-live-monitoring.md` §6.2; W5-D default merging; SCR-003.

The plan keeps alert state per `(rule, subject)` and overrides defaults by name, but does not prescribe the generated naming algorithm. The candidate chooses `/data`, or otherwise the first sampler mount, for unsuffixed names. Probes show that reordering `['/srv','/backup']` to `['/backup','/srv']` moves `fs_full_soon` to a different subject. A same-name override can then suppress the default for a different mount and leave an additional generated rule for the original mount.

Reserve unsuffixed mount-rule names for `/data` consistently; suffix every other mount, irrespective of list order. Specify a byte-preserving escaped suffix format and a corresponding stable policy for root-scoped rules. Test reorder, addition, removal and same-name override behavior. This modifies the proposed SCR-003 policy, not an existing explicit naming contract. Affects W5-D/W6-A alert state and effective-config display.

### S4 — SHOULD-FIX: the proposed notable-file ID bound is not enforced

Reference: `finding:w0b1-s4`. Locations: `crates/tidemark-core/src/snapshot.rs:152`, `crates/tidemark-core/src/config.rs:870`. Plan: `01-requirements.md` §2; W2-A notable selection; SCR-001.

SCR-001 justifies `FileRow.id: u32` by the `max_notable` cap. That cap is `usize` and only checked for positivity. A probe sets it to 4,294,967,297, more than all distinct `u32` IDs even with zero-based indexing, and validation returns no issues. The required tens-of-millions scale fits; no actual oversized scan was run.

Keep `u32` IDs but document zero-based file numbering, cap `max_notable` at `u32::MAX`, and enforce the cap in validation. Require checked ID assignment in W2-A and checked SQL conversions in W1-E. Record the policy in SCR-001; do not imply the configured cap already proves representability.

### S5 — SHOULD-FIX: a user profile named `max` advertises limits the consumer will ignore

Reference: `finding:w0b1-s5`. Location: `crates/tidemark-core/src/config.rs:464`. Plan: W1-D `gate_from_config`; SCR-003.

W1-D reserves `max` for a `NoopGate`. The open profile map nevertheless accepts `[throttle.profiles.max]` with utilization/latency targets, and validation accepts it. The probe confirms the supplied profile remains in the effective config. Its values cannot affect the specified consumer, which bypasses throttling for that name.

Reject profile definitions named `max`, while continuing to accept `roots[].throttle="max"` and genuine custom profiles. Add an actionable validation error and record this reservation in SCR-003. Affects W1-D and W0-B2 effective-config output.

### S6 — SHOULD-FIX: live sample and SQL nullability need a conversion policy

Reference: `finding:w0b1-s6`. Locations: `crates/tidemark-core/src/live.rs:203`, `live.rs:207`, `live.rs:251`, `live.rs:255`. Plan: `05-storage.md` §6, `phantom_samples`/`writer_samples`; `07-live-monitoring.md` §7a; W1-E/W5-B.

The SQL allows `NULL` for both `comm` and `path`; the normative live field lists require `String` and `Vec<u8>`. The candidate correctly follows `07`. These are emission/sample types, not necessarily complete persisted rows, so this is a document seam rather than evidence of a failing reader. The plan does not define how a stored NULL could be represented on later reads.

Prefer amending the SQL to make `comm` and `path` `NOT NULL`, preserving the explicit live types. If unknown values must be stored as NULL, instead approve optional fields and define their report behavior. Record the choice before W1-E/W5-B; do not silently replace unknown values with empty strings or paths.

### S7 — SHOULD-FIX: CLI growth sorting has no specified bridge to the snapshot sort enum

Reference: `finding:w0b1-s7`. Location: `crates/tidemark-core/src/snapshot.rs:233`. Plan: `05-storage.md` §3; `08-interfaces.md` §1.2; W2-C CLI.

The candidate exactly implements the eight `SortKey` variants in `05`. The CLI table additionally advertises `--sort growth`, and W2-C says sorting follows that table. No core mismatch is established: growth needs two snapshots, whereas this enum belongs to one-snapshot queries. The plan leaves that distinction unstated.

Document a CLI sort wrapper with a `Growth` case and have W2-C perform cross-snapshot growth ordering with byte-path tie breaking. Keep `snapshot::SortKey` as specified in `05`, and make W0-B2's argument type accommodate the wrapper. Do not add a misleading single-snapshot `Growth` variant without defining its inputs.

### N1 — NIT: two copied snapshot comments describe the wrong contract

Reference: `finding:w0b1-n1`. Locations: `crates/tidemark-core/src/snapshot.rs:195`, `snapshot.rs:151`. Plan: `05-storage.md` §1; `04-scanner.md` §3.3.

`LinkInstanceRow.name` is documented as absent when bulk name resolution loses a race. The plan leaves all bulk link-instance names NULL by design, resolving them live on demand; the race explanation belongs to notable files. `FileRow.id` is also described as a directory preorder ID.

Change the comments to “Raw entry name; None for bulk snapshots, resolved live on demand” and “Notable-file row identifier”, respectively. The field types themselves are correct.

### N2 — NIT: the rule-kind table uses an obsolete watcher threshold name

Reference: `finding:w0b1-n2`. Location: `crates/tidemark-core/src/config.rs:980`; conflicting source: `docs/plan/07-live-monitoring.md:182`. Plan: `07` §6.1 versus `08` §3 alert fields.

`07` calls the parameter `overflows`; the explicit config field table and code use `above` for that count. Retain the code and amend `07` to say `mount, above (overflow count), window`. Do not add a second configuration key. Affects W5-D documentation/implementation.

### N3 — NIT: the time-series alert comment omits renotification

Reference: `finding:w0b1-n3`. Location: `crates/tidemark-core/src/live.rs:318`; conflicting source: `docs/plan/05-storage.md:243`. Plan: `05` §6; `07` §7a; W5-D.

The candidate's `AlertState::Renotify` correctly follows `07` and W5-D. The SQL comment lists only `firing|resolved`; it has no CHECK constraint that rejects the third state. Amend that comment to `firing|resolved|renotify`, preserving the enum and its snake_case serialization.

## SCR recommendations

### SCR-001 — MODIFY

Accept the proposed widths at the required scale: 5–50 million inodes, 0.5–5 million directories and 100k-wide directories fit the chosen counts/IDs; allocated sizes and subtree totals correctly use `u64`. All explicit scanner widths remain intact. Add the `max_notable` policy in S4 and checked overflow/conversion requirements. In bulk mode, document and enforce the 31-bit directory-slot limit from `04` §3.1 before assigning the self-reference flag. W2-A's proposed internal `Node.depth: u16` also needs a checked bound or a plan change to `u32`; it must not truncate the core depth silently.

Accept `FileHit.path: Option<RawPath>`: `files.name` is explicitly nullable after a bulk race. Amend W0-B's “row + full path” wording to “row + optional full path”; render unresolved rows honestly. Accept `GateStats` in core with the exact W1-D fields, requiring a re-export from `tidemark_fs::throttle` instead of a second incompatible definition.

Accept typed metadata, deterministic `BTreeMap<String,u64>` phase durations, filter duration units relative to snapshot start, and `LinkSetHit.instance_count` as stored instance rows rather than discovered `found`. Document mutually exclusive boundary-glob/depth selectors and resolve S7's CLI growth bridge. These are additional public contracts the plan left unspecified; explicitly incorporate them into the plan before freeze. Affects W0-B2, W1-B/D/E, W2-A/B/C and W3-A/B/C.

### SCR-002 — MODIFY

Accept shared `core::api::{ComponentState,VgInfo}` with the exact W4 fields and downstream re-exports at the documented W4 paths; core cannot import its dependent crates. Accept the selected version/id/scalar types, error envelope, six snake_case codes, named params types and result aliases that preserve the API's array/optional payloads. All listed methods have typed params/results.

Do not freeze roots as exposed byte arrays. Specify `"root":"/data"` for UTF-8 or `"root_b64":"..."` for raw bytes, exactly one for start/cancel and at most one for status. Deserialize through `RawPath` validation, reject conflicting representations, and use padded standard base64. Internal serde may remain byte-oriented behind explicit DTO adapters. Apply the analogous `path`/`path_b64` convention to report paths (B2); it is readable for CLI/daemon diagnostics and preserves arbitrary path bytes. Add full DTO/envelope round-trip tests for ASCII, Unicode, invalid UTF-8 and omitted optional roots.

Resolve writer durations as S1 recommends. Keep `HotWindow`/`HotBy` for hot-ring queries. Explicitly state that custom throttle names are selectable in root configuration under W1-D, while API/CLI overrides are limited to the three documented presets; the latter need not silently expand merely because the config map is dynamic. Also clarify that `07`'s explicit `mount: String`/`ScanStatus.root: String` fields override its general raw-path sentence, and define their display encoding for non-UTF-8 mounts before consumers invent one.

Affects W0-B2, W4-A/B, W5-B/D and W6-A, plus other report renderers using core paths. S6 requires a related W1-E/W5-B storage decision.

### SCR-003 — MODIFY

Accept the dynamic profile map and root throttle string: W1-D expressly supports user-defined profiles, despite the shorter CLI preset list. Reserve `max` as in S5. Accept the documented config representations, optional per-root window, root-specific built-in defaults, same-name alert overrides, duplicate-name validation and effective VG/pool wildcard defaults. Missing config files should continue returning the I/O error so the CLI applies its documented defaults. The documented 1 MiB input cap is a reasonable bounded implementation choice.

Accept moving pure `default_rules(cfg)` into core and re-exporting/wrapping it from W5-D; a config-to-live dependency would be cyclic. Retain sampler-mount scope for filesystem/phantom defaults and watch-mount scope for watcher defaults, but stabilize names as S3 requires. Implement the stale-root contract in B3 and numeric rate handling in B4, and correct N2's parameter name. Keep the eight documented default kinds and their thresholds/windows/hysteresis. Specify that component-dependent rules remain visible in effective config but do not evaluate as firing when their collector/input is disabled or unavailable; thin-pool rules evaluate only tracked thin LVs as `07` requires.

The code chose “all fields in the kind's usage set are required except VG/pool wildcards”; the documents do not fully define this optionality. Approve that rule explicitly with the stale-root correction, including for disabled rules, rather than letting W5-D infer a different contract. Affects W0-B2, W1-D, W2-C, W4-B, W5-D and W6-A/B.

### SCR-004 — MODIFY

Accept fallible raw-path/name constructors, NUL/dot-entry rejection, lexical normalization without filesystem I/O, component-wise prefixes, byte ordering, signed xxh3 bits, uppercase byte escaping and padded standard base64. Callers must canonicalize live query paths before these lexical helpers when the CLI promises `realpath`; normalization is not filesystem canonicalization. Accept `ByteSize`'s checked `u64` arithmetic, documented fractional truncation and explicit SI suffix support, fallible parsers, duration helpers, and `DevKind::Md.level: String`/device widths. These are reasonable resolutions of omitted signatures rather than new required platform behavior.

Fix B5 and document the exact-byte display exception below 1 KiB. Change the JSON schema field to `Cow<'static,str>` as S2 recommends; retaining a derived but practically unusable runtime deserializer adds an avoidable consumer trap.

The eight-hex suffix format is specified; randomness is not. Accept the process/sequence/time hash only with an explicit non-uniqueness statement and a writer requirement to refuse existing IDs and retry with a fresh ID. It is not a cryptographic identifier. Reject the proposed silent epoch prefix for unrepresentable input dates (`time.rs:38`): it falsifies the supplied timestamp. Specify `ScanId::new(now) -> Result<ScanId,CoreError>` for timestamps outside the supported four-digit UTC-year range and update callers before freeze. Keep injected timestamps and isolated `unix_now`; no filesystem I/O is needed.

Affects W0-B2, W1-A/E, W2-C and all later path/JSON consumers. These recommendations require approved plan changes; the candidate presently follows W0-B's literal static schema field.

## Coverage

“Complete” means every declared item in the named source sections was compared, including relevant later-wave signatures; it does not claim exhaustive testing of all possible inputs.

| Module | Comparison | Evidence and limits |
|---|---|---|
| `path` | Complete | W0-B and `03` §5; constructors, normalization, joins, parent/name, byte ordering, prefixes, display, hash and JSON examined; existing and additional edge probes exercised. Wire/report gap B2. |
| `units` | Complete | W0-B and config threshold consumers; integer/string serde, checked arithmetic, rates and binary formatting examined. Boundary failures B5. |
| `time` | Complete | W0-B, `03` age semantics and W2-C IDs; all five exact age boundaries/future/overflow cases tested; epoch and leap-day ID checks. Suffix/date policy requires SCR decision. |
| `scan` | Complete | Every explicit type/field/variant in `04` §1, including Gate/NoopGate, compared; matches. Read all normative `03` and relevant W1–W6 signatures. No aggregator execution exists in this task. |
| `snapshot` | Complete | `05` §1–3: all 30 metadata keys, all 34 directory columns, all other snapshot row columns/nullability, filters/hits and eight sort keys compared. No required core snapshot type missing in consumers. |
| `live` | Complete | Every field/variant in `07` §7/§7a matches; `05` §6 and W1-E/W4/W5/W6 interfaces cross-checked. Storage seams S6/N3 and JSON encoding B2 recorded. |
| `dev` | Complete | W0-B field/variant list plus W1-A/W1-D consumers; no mismatch found. Underspecified widths/MD level covered in SCR-004. |
| `config` | Complete | Every `08` §3 key/default and table's unknown-key behavior; `07` alerts, W1-D/W5-D/W6-B, ADR-018; exact fixture and validation probes. Findings above. |
| `api` | Complete | Every `08` §2 method, params/result and all six error codes; W4/W5/W6 consumers. All payload types exist; wire/duration decisions remain. |
| `json` | Complete | W0-B literal type and `08` schema-tag usage; runtime/lifetime and whole-envelope serialization probes. |
| `error` | Complete | W0-B config/parse-error requirement and load/parse conversions; no mismatch found. |

The complete read-first sections and `08` were read, and the interface sections of waves 1–6 were checked; later-task-owned types were not incorrectly demanded in core. Snapshot/schema logic, scanner behavior and later services are still stubs and were not treated as implemented. CLI/daemon W0-B2 deliverables are outside this review.

The config fixture is byte-for-byte identical to the document's TOML block: **3,637 bytes**. Its round-trip test embeds that fixture unchanged. All documented literal defaults, twelve alert kinds and eight default alert-kind thresholds are present. Configuration tables, including internal deserialization tables, use `deny_unknown_fields`; dynamic profile names are intentionally open. ADR-018 is reflected: no `socket_group`, no group-access contract or uid gate was introduced, and a test rejects the obsolete key.

The candidate has the owner headers and public rustdoc, forbids unsafe in the crate root, and confines production file I/O to `Config::load`. Production source contains no `unwrap()`/`expect()` calls; clock/process helpers are isolated. Added dependencies use workspace declarations and are justified in the handoff (`humantime`, `base64`, `chrono`); no OpenSSL dependency was introduced. Unspecified public helper/DTO/config/filter contracts are addressed by the four SCRs and the findings above rather than silently treated as frozen.

## Build and test results

All repository checks ran against the exact candidate tree using the pinned Rust **1.98.0**, with the host registry copied into this review's temporary cargo home. No network dependency resolution or privileged operation was used.

```sh
export PATH=/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH
export CARGO_HOME=/tmp/tidemark-w0-b1-review-cargo
export CARGO_NET_OFFLINE=true
cargo xtask ci
RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps
```

| Check | Observed result |
|---|---|
| `cargo xtask ci` | Exit 0: formatting, workspace all-target Clippy with `-D warnings`, workspace tests, 83 owner tags and forbidden-symbol scan passed. |
| Workspace tests inside CI | 10 core unit tests, 5 core integration tests and 3 xtask tests passed; other scaffold targets had no tests. |
| `cargo deny` | **Not run**: CI explicitly skipped it because cargo-deny is not installed. Dependency advisory/license audit remains unverified. |
| Workspace rustdoc with `-D warnings` | Exit 0, no warnings. |
| Additional helper assertions against candidate rlibs | 2 passed (path/hash/ordering/size/rate/time edge groups), 3 failed on B5 formatting boundaries, exit 101. These temporary review tests were not added to the repository. |
| Config probes against candidate rlibs | Compiled and ran, exit 0; recorded the invalid-listener acceptance, numeric-rate/stale-root rejection, unbounded notable cap, reserved profile acceptance and order-dependent names described above. |
| JSON/API serialization probes | Compiled and ran, exit 0; recorded byte-array paths/roots and rejection of the writer `2m` window. |
| `JsonDoc` consumer compile probes | Two expected exit-1 failures: `DeserializeOwned` and runtime-buffer deserialization. The proposed `Cow` control compiled and ran successfully. |
| Fixture/tree checks | Exact document-fixture equality and candidate/review-baseline tree equality confirmed. |
| VM/Rocky 8/privileged tests | Not run in this review; no VM access used. No claim of platform/runtime verification. |

Transcripts and temporary probe sources are retained under this attempt's output `library/` (`ci.log`, `doc.log`, `contract-checks.log`, `helper-probe.*`, `tidemark-config-review-probe.*`, `owned.*`, `runtime.*`, `serde_paths.*`, `cow_control.*`). These observations are review evidence, not a declaration of verified task success or acceptance of the pending SCRs.
