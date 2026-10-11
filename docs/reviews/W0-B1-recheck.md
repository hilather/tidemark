**DO NOT FREEZE — 1 BLOCKER, 0 SHOULD-FIX, 1 NIT open.**

The remaining blocker is an alert-name contract mismatch: a disabled override using the decided name can leave an enabled generated rule active. The original wire, validation and rounding defects are otherwise resolved in the checks below. These are review observations and a freeze recommendation, not an integration or acceptance decision.

Reviewed the change from `73f15ab898c2c845ab840e29b238f9b5bcdf40eb` to exact candidate `2be8934158f69fcca40226135acfd95c9e4ddb5c`. The supplied checkout at `fd40941f265d8d1dbebf31803a0c472fbe9e8668` has identical `crates/tidemark-core/**` contents; its amended plan and downstream workspace were used for the requested contract/build recheck. Source references are checkout line numbers. Only this report is changed in the repository.

## Original findings and decisions

Source paths in this table are relative to `crates/tidemark-core/src/`. Regression names refer to `crates/tidemark-core/tests/review_regressions.rs` unless otherwise stated. All named tests passed. “RESOLVED” for a decision means the core contract and relevant plan corrections are present; it does not assert that later scanner/storage/service implementations have fulfilled their duties. Documentation-only corrections were inspected directly; executable reviewer probes supplement the existing tests.

| Item | Status | Evidence |
|---|---|---|
| B1 | RESOLVED | `config.rs:858` requires a parsed loopback IP or an empty listener. `metrics_requires_loopback_or_disabled` covers IPv4/IPv6 loopback, wildcards, external addresses and invalid input. |
| B2 | RESOLVED | `path.rs:159` supplies sibling-field codecs; `live.rs:177`, `snapshot.rs:94`, `snapshot.rs:219` and `scan.rs:137` apply them to enclosing types. `complete_live_report_path_outputs`, `snapshot_hit_paths_and_errors_roundtrip`, `complete_snapshot_and_now_envelopes` and `hot_labels_retain_keys_and_path_representations_conflict` pass. Additional byte-enumeration probes found no loss; interpretation limits are below. |
| B3 | RESOLVED | `config.rs:998` requires `root` for stale rules, including disabled rules; `config.rs:1190` generates defaults only for scheduled roots. Both `07` §6 and `08` §3 now agree. `stale_roots_numeric_rates_and_resource_caps` and `stable_default_names_and_scheduled_roots` exercise missing roots, manual roots and schedule changes. |
| B4 | RESOLVED | `config.rs:1077` accepts finite nonnegative numeric bytes/s alongside rate strings. `stale_roots_numeric_rates_and_resource_caps` covers zero, fractional rates, negatives, nonfinite values and numeric/string equivalence. |
| B5 | RESOLVED | `units.rs:89` recomputes precision after rounding; `units.rs:116` and `units.rs:157` promote units. `byte_and_rate_rounding_boundaries` and an independent probe confirm `10239 → 10.0 KiB`, `102399 → 100 KiB`, `1048575 → 1.00 MiB`, and signed rate counterparts. See the sub-KiB rate interpretation below. |
| S1 | RESOLVED | `api.rs:164` uses `Duration`; `api.rs:330` rejects zero on input and output. `writer_windows_are_positive_durations_and_schema_is_owned` accepts `2m`/`1h`; additional probes round-trip durations from 1 ns through `Duration::MAX`. Hot queries retain enumerated windows. |
| S2 | RESOLVED | `json.rs:10` is `Cow<'static, str>` without borrowed deserialization. The same regression drops its input buffer before using the result and exercises `DeserializeOwned`; an independent runtime-buffer probe also passes. |
| S3 | PARTLY RESOLVED | `config.rs:1203` removes mount-order dependence and reserves the base name for `/data`; reorder/add/remove/override tests pass. Its escaping and the assertions at `review_regressions.rs:321` contradict the decided `%XX` format. See R1. |
| S4 | RESOLVED | `config.rs:868` and `config.rs:882` enforce `1 <= max_notable <= u32::MAX`; `snapshot.rs:156` documents checked zero-based file IDs. `stale_roots_numeric_rates_and_resource_caps` tests zero, the maximum and maximum-plus-one. `05` §1 and W1-E/W2-A assign checked conversions/ID assignment to consumers. |
| S5 | RESOLVED | `config.rs:934` rejects a `max` profile definition while keeping the root selector. The resource-cap regression and `config_load::custom_throttle_profile_is_validated` cover reserved and custom names. |
| S6 | RESOLVED | `05-storage.md:240` and `:242` now declare both `comm` and `path` NOT NULL. `live.rs:200` and `:250` retain required sample fields. Whole-sample round trips pass; executing the amended DDL in an in-memory SQLite database rejects NULL in each of these four columns. No stored-NULL-to-empty conversion was introduced. |
| S7 | RESOLVED | `snapshot.rs:239` retains the eight single-snapshot variants. `05` §3 and `08` §1.2 explicitly assign `Growth` and the older-snapshot requirement to the CLI. Reviewer enum probes reject snapshot `growth`; the supplied workspace's `top_sort_includes_snapshot_metrics_and_cli_growth` also passes. Runtime W2-C growth ordering remains future work. |
| N1 | RESOLVED | `snapshot.rs:156` describes a zero-based notable-file ID; `snapshot.rs:200` says bulk link-instance names are absent by design and resolved live. Both match `05` §1. This is a directly inspected documentation correction, with warning-free rustdoc rather than a runtime regression test. |
| N2 | RESOLVED | `07-live-monitoring.md:182` now names `above (overflow count)`; `config.rs:999` uses `above`. A reviewer probe validates the `above` form and rejects an `overflows` field. |
| N3 | RESOLVED | `05-storage.md:253` includes `firing|resolved|renotify`. `live.rs:326` retains all three variants; a reviewer probe round-trips their exact snake_case spellings. |
| SCR-001 | RESOLVED for core | `snapshot.rs:17` retains all 30 metadata fields in documented order; the seven row structs match all SQL columns. Optional `FileHit.path`, filter duration types, stored `instance_count`, deterministic phase map and `scan.rs:311` GateStats match `13` SCR-001. FileHit round trips and the schema-comparison probe pass. Checked SQL conversion, bulk-slot bounds, `u32` aggregator depth and downstream re-exports are specified for their future owners. |
| SCR-002 | RESOLVED for core | `api.rs:348` onward implements required/optional validated root DTOs. `root_wire_envelopes_roundtrip_and_validate` and adversarial probes cover both representations, omission, malformed/conflicting inputs and normalization. All named method params/results, shared payloads, aliases, error codes and preset enums remain present. Report paths and String-only status display responsibilities are addressed below. |
| SCR-003 | PARTLY RESOLVED | Required rule fields, numeric rates, stable subjects, reserved profiles, load bounds and effective defaults are implemented and tested. The accepted suffix format is still wrong (R1). The fixture also no longer exactly matches the amended example (R2). Availability/thin-LV evaluation remains a future consumer obligation. |
| SCR-004 | RESOLVED for core | `time.rs:26` returns a fallible ScanId and rejects dates outside years 0000–9999; `scan_id_supported_year_boundaries` covers both endpoints and out-of-range values. Existing path normalization/escaping/serde, age boundaries and checked ByteSize tests pass; rounding and owned schema evidence are above. Writer collision refusal/retry remains downstream work. |

## Open findings

### R1 — BLOCKER: generated alert names do not use the decided escaping

Reference: `finding:w0b1-recheck-r1`. Location: `crates/tidemark-core/src/config.rs:1210`. Plan: `13-contract-decisions.md:49` (SCR-003) and `07-live-monitoring.md:213` (§6.4).

The contract preserves `/` and ASCII `[A-Za-z0-9._-]` and percent-encodes every other byte as uppercase `%XX`, explicitly including `%` and `:`. `subject_suffix` instead preserves all printable ASCII except backslash and emits `\xNN` for other bytes. The new test at `crates/tidemark-core/tests/review_regressions.rs:321` asserts this incompatible format.

The compiled probe observed:

| Subject | Actual `fs_full_soon` name | Contract name |
|---|---|---|
| `/srv:blue` | `fs_full_soon:/srv:blue` | `fs_full_soon:/srv%3Ablue` |
| `/x%y` | `fs_full_soon:/x%y` | `fs_full_soon:/x%25y` |
| `/x y` | `fs_full_soon:/x y` | `fs_full_soon:/x%20y` |
| `/données` | `fs_full_soon:/donn\xC3\xA9es` | `fs_full_soon:/donn%C3%A9es` |

This affects behavior: a valid disabled user rule named `fs_full_soon:/srv%3Ablue` does not override the generated rule. Effective config contains that disabled rule **and** enabled `fs_full_soon:/srv:blue`. Validation returns no errors. Thus a user following the contract can fail to disable the default alert.

Change `subject_suffix` and its rustdoc to the agreed byte allowlist and uppercase `%XX` encoding. Replace the conflicting expected names and add colon, percent, space, backslash and Unicode cases, including a disabled same-name override that must leave exactly one disabled rule. Preserve the already-correct order independence and `/data` policy. Evidence: `library/config-units-probe.rs` and `library/config-units-probe.log`.

### R2 — NIT: the verbatim config fixture has drifted from the amended plan

Reference: `finding:w0b1-recheck-r2`. Location: `crates/tidemark-core/tests/example.toml:11`; consuming test `crates/tidemark-core/src/config.rs:1246`. Plan: W0-B's verbatim-example requirement and `08-interfaces.md:135`.

The fixture has 3,637 bytes; the current documented TOML block has 3,663. The sole difference is the throttle comment: the plan appends `or a user-defined profile`. Parsed TOML values are identical, so this is documentation/test drift, not a parsing failure or a behavior regression introduced by the fix.

Refresh the fixture to the amended block and make the verbatim test compare it with the extracted `08` §3 TOML block, so future amendments cannot silently invalidate that claim. Evidence: `library/contracts.log` includes the exact one-line diff and semantic equality check.

## Wire interpretations and regression coverage

No valid-value byte loss or serialization collision was reproduced in the tested domain. The additional probe round-tripped all 65,536 two-byte `HotEntry` keys and checked that their serialized documents were distinct; all 196,608 two-byte `HotMinute` key/scope combinations (`dir`, `unit`, `comm`); and all 65,536 two-byte `OpenWriter` paths. It also exercised every non-NUL byte in validated scan roots, duplicate/null/conflicting root fields, malformed/noncanonical base64, absent optional FileHit paths, and runtime-owned JsonDoc input. Existing whole-envelope tests cover ASCII, printable Unicode, invalid UTF-8, nested phantom holders, snapshot rows and NowReport. This finite enumeration is not a proof over all byte strings.

The following interpretations remain explicit assumptions, rather than additional findings:

- `HotEntry` has no scope field and chooses path encoding by a leading `/` (`live.rs:429`); `HotMinute` uses `scope == "dir"` (`live.rs:496`). Consequently a comm label `/worker` is emitted under `path` in HotEntry but under byte `key` in HotMinute. Both round-trip exactly. The plan requires path encoding for directory entries but does not define label wire keys; consumers must retain query/grouping context instead of inferring scope from the serialized field name. I found no specified label-key contract this violates.
- `SnapshotMeta.root` uses `root`/`root_b64`, while `LinkSetHit.lca_path` uses `path`/`path_b64`. The former preserves the semantic root name consistently with request DTOs and round-trips. The general report-path paragraph does not explicitly name SnapshotMeta's root key; this report treats that choice as compatible. The on-disk `meta.root` hex representation in `05` remains a storage-writer duty, distinct from report JSON.
- Raw report `Vec<u8>` fields preserve their bytes without lexical normalization. Actual `RawPath` fields validate and normalize on input. Explicit `mount: String` and `ScanStatus.root: String` remain unchanged as `07` §7a requires; future status rendering must apply its non-UTF-8 display policy.
- `fmt_rate` emits exact fractional text below 1 KiB/s (for example `123.456789 B/s`), as its new rustdoc and fix note state. SCR-004 grants the byte-unit exception explicitly to `fmt_bytes` and says to share its rounding with rates; I interpreted that as permitting the corresponding byte-unit rate exception. The required rounding examples and signed counterparts pass. This interpretation is not a finding of a proved formatting defect.

Independent field-list checks matched all nine named scanner record structs in `04` §1 and all 21 named live/report structs in `07` §7/§7a, including field types. Manual inspection also retained scanner aliases/enums/Gate, tuple AgeBuckets and live AlertState. All seven snapshot row structs matched their SQL column lists (34 DirRow columns), and SnapshotMeta matched all 30 metadata keys in order. No config keys or literal defaults were removed by the candidate; its new per-root stale defaults are the decided change. The amended example has identical parsed values to the tested fixture. API method coverage remains complete for the 13 methods in `08` §2, counting alerts.active/history separately. I found no additional contradiction between the inspected amended plan sections; R1 is a code-versus-plan mismatch, and R2 is example drift.

## Build, tests and limits

Used Rust 1.98.0 from `/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin`, `CARGO_HOME=/tmp/tidemark-recheck-cargo` containing a copied host registry cache, and `CARGO_NET_OFFLINE=true`. The host cache was not modified. Commands ran in the supplied checkout; no workspace compile failure required a core-only fallback.

| Check | Observed result |
|---|---|
| `sh tools/ci.sh` | Exit 0. Formatting, workspace all-target Clippy with `-D warnings`, workspace tests, 113 owner tags and forbidden-symbol checks passed. |
| Core tests within CI | 10 unit tests, 5 config-load integration tests and 11 review-regression tests passed: 26 total. |
| Other workspace tests within CI | 14 CLI tests, 2 daemon tests and 3 xtask tests passed; other scaffold targets and doctests had no tests. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0; no rustdoc warnings. |
| Additional compiled wire and enum probes | Exit 0 for round trips, sibling rejection, duration bounds, required request params, eight snapshot sorts, watcher field spelling and all three alert states. |
| Config/units probe | Exit 0 as an observation harness; reproduced R1, including the ineffective disabled override, and confirmed the specified rounding outputs. |
| Python source/DDL/example checks | Exit 0; field comparisons, four SQLite NULL rejections and semantic example equality passed; byte comparison reported R2. |
| `cargo deny` | CI explicitly skipped it because cargo-deny is not installed. License/advisory verification remains unperformed. |

Evidence is retained under `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-25f579d3158e8eda35c5ce036650da7e49f860c83ce07a665b7ff3a745304570/library/`: `ci.log`, `doc.log`, `contracts.py`, `contracts.log`, `config-units-probe.rs`, `config-units-probe.log`, `wire-probe/` (source and manifest), `wire-probe.log`, `api-envelope-probe.log` and `enum-contracts.log`.

No privileged runtime, Rocky Linux/container, filesystem-scale, future storage conversion, scanner aggregation, alert evaluation or service integration checks were performed. This recheck did not compile the privileged feature separately. The pure core changes introduce no privileged behavior; future VM-dependent work remains pending with its owners. No implementation, plan, dependency, shared manifest or other repository file was edited. No branches were created, merged or pushed.
