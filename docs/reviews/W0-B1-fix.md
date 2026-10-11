# Handoff — W0-B1-fix: accepted core contract review changes

- **Attempt:** attempt-901408348b679076dd2b5ef112abab40495eca33be7efcfca237d98ea92b0155
- **Branch:** hp-901408348b679076dd2b5ef112abab40495eca33be7efcfca237d98ea92b0155
- **Date:** 2026-10-11
- **Status:** code and regression evidence submitted for independent verification; no claim of integration or verified task success.

Implemented the coordinator's accepted review decisions in `tidemark-core`, preserving the explicit internal field types and the snapshot sort enum. Shared sibling-field codecs provide readable UTF-8 and padded standard-base64 JSON without changing ordinary byte-oriented `RawPath` storage serde. Added eleven integration regressions covering complete envelopes, validation and boundary behavior. No dependencies, shared manifests, `lib.rs` declarations, plan documents, handoff documents or other crates changed.

## Finding and SCR disposition

All source paths below are relative to `crates/tidemark-core/src/`. Named regression tests are in `crates/tidemark-core/tests/review_regressions.rs`, unless stated otherwise. Documentation-only decisions remain the separate plan task's responsibility.

| Item | Status | File and evidence | Interpretation / remaining responsibility |
|---|---|---|---|
| B1 | fixed | `config.rs`; `metrics_requires_loopback_or_disabled` | Numeric IPv4/IPv6 loopback only; empty listener disables metrics. Wildcard, external and hostname listeners are rejected. |
| B2 | fixed | `path.rs`, `live.rs`, `snapshot.rs`, `scan.rs`; `complete_live_report_path_outputs`, `snapshot_hit_paths_and_errors_roundtrip`, `complete_snapshot_and_now_envelopes`, `hot_labels_retain_keys_and_path_representations_conflict` | Raw path bytes keep their Rust types. Full outputs use exclusive `path`/`path_b64`; see wire interpretations below. |
| B3 | fixed | `config.rs`; `stale_roots_numeric_rates_and_resource_caps`, `stable_default_names_and_scheduled_roots` | `scan_stale` requires `root`, including disabled rules. Generate one rule per scheduled root, excluding `off`; explicit rules may monitor manual scans. Plan conflict is owned by the plan task. |
| B4 | fixed | `config.rs`; `stale_roots_numeric_rates_and_resource_caps` | Finite nonnegative numeric thresholds are bytes/s, including fractional rates and zero. Existing rate strings remain valid. |
| B5 | fixed | `units.rs`; `byte_and_rate_rounding_boundaries` | Shared significant-digit rounding recomputes display precision, with binary-unit carry. Exact bytes below 1 KiB; fractional rates below 1 KiB/s retain their exact numeric display. |
| S1 | fixed | `api.rs`; `writer_windows_are_positive_durations_and_schema_is_owned` | `WritersParams.window` is a positive humantime `Duration`; no additional maximum. `HotWindow` remains unchanged. Zero is rejected during both serialization and deserialization. |
| S2 | fixed | `json.rs`; `writer_windows_are_positive_durations_and_schema_is_owned` | `Cow<'static, str>` without borrowed deserialization; runtime input can be dropped after parsing. `DeserializeOwned` exercised by the generic round-trip helper. |
| S3 | fixed | `config.rs`; `stable_default_names_and_scheduled_roots` | `/data` alone gets unsuffixed names. Other subjects use the injective raw-byte escape described below. Tests cover reorder, addition, removal, same-name override, scheduled roots and invalid bytes versus literal escape text. |
| S4 | fixed | `config.rs`, `snapshot.rs`; `stale_roots_numeric_rates_and_resource_caps` | Positive `max_notable <= u32::MAX`; zero-based notable IDs and checked assignment documented. Consumers retain checked overflow/SQL conversion duties. |
| S5 | fixed | `config.rs`; `stale_roots_numeric_rates_and_resource_caps`; existing `config_load::custom_throttle_profile_is_validated` | `throttle.profiles.max` is rejected with an actionable message. Root selector `max` and actual custom profiles remain valid. |
| S6 | not applicable to code | `live.rs` retains `String` and `Vec<u8>`; sample round trips in `complete_live_report_path_outputs` | SQL `NOT NULL` amendment belongs to the plan task. No optional live fields or fabricated unknown values introduced. |
| S7 | not applicable to code | `snapshot.rs::SortKey` unchanged | CLI growth wrapper and cross-snapshot ordering are plan/CLI responsibilities. No single-snapshot `Growth` variant added. |
| N1 | fixed | `snapshot.rs::FileRow.id`, `LinkInstanceRow.name`; rustdoc check | Comments now describe zero-based notable-file identifiers and intentionally absent bulk link-instance names resolved live on demand. |
| N2 | not applicable to code | `config.rs::AlertRule` retains watcher `above` | Plan task corrects obsolete `overflows` wording; no second key added. |
| N3 | not applicable to code | `live.rs::AlertState` unchanged | Plan task updates the SQL comment to include `renotify`; existing enum spelling remains intact. |
| SCR-001 | fixed | `snapshot.rs`, `scan.rs`, `config.rs`; S4 tests and rustdoc | Added checked consumer conversion/overflow duties, 31-bit bulk slots, checked internal depth, zero-based notable IDs, GateStats re-export contract and mutually exclusive boundary selectors. Optional `FileHit.path`, filter age units, phase map and stored-instance count remain as approved. No downstream implementation attempted. |
| SCR-002 | fixed | `api.rs`, `path.rs`, `live.rs`; `root_wire_envelopes_roundtrip_and_validate` and full report regressions | ASCII, Unicode, invalid UTF-8 and absent status root round-trip. Required roots, conflicts, nulls, invalid base64, byte arrays, unknown API fields, relative/NUL roots and lexical normalization tested. Existing shared API types/result aliases and preset enums retained. Live mount/status string display policy is a plan/consumer decision. |
| SCR-003 | fixed | `config.rs`; B3/B4/S3/S5 regressions; `config_load::load_bounds_input_and_reports_missing_file` | Stable defaults, required stale roots, numeric rates and reserved `max` implemented. Explicit 1 MiB rustdoc added. Existing default thresholds, hysteresis, wildcard merging, dynamic profiles and missing-file I/O behavior preserved. Component availability/thin-LV evaluation is documented as a future consumer responsibility. |
| SCR-004 | fixed | `time.rs`, `units.rs`, `json.rs`, `path.rs`; `scan_id_supported_year_boundaries`, `byte_and_rate_rounding_boundaries`, owned-schema test | `ScanId::new` returns `Result`, accepts years 0000–9999 and rejects dates outside that range. Documented non-unique/noncryptographic suffix and writer refusal/retry requirement, exact-byte display, lexical-vs-live canonicalization and checked size parsing/fractional truncation. |

No assigned code item is marked not done. This disposition reports implementation evidence, not verification of future consumers or of the concurrent plan edits.

## Wire interpretations

- Direct path fields in `PhantomHolder`, `PhantomSample`, `OpenWriter`, `WriterSample`, `DirRow`, `ErrorRow`, `ScanErrorRec` and `FileHit` use `path`/`path_b64`. `FileHit.path = None` omits both siblings, honestly retaining unresolved rows. `LinkSetHit.lca_path` uses those same report keys because the requested report convention explicitly names `path`/`path_b64`; its internal field remains `lca_path: RawPath`.
- `SnapshotMeta.root` uses `root`/`root_b64`, consistently with scan request roots. Standalone `RawPath`/`RawName` serde remains byte-oriented for internal consumers.
- `HotEntry` has no scope discriminator. Its absolute directory keys (leading `/`) use path siblings; unit/comm keys retain the existing byte-oriented `key`. `HotMinute` uses its explicit `scope == "dir"` to choose path siblings, retaining `key` for label scopes. This preserves non-path grouping labels and arbitrary label bytes. Conflicting key/path representations are rejected.
- Scan API adapters require non-null strings, reject both siblings even if one is null, and deserialize through `RawPath::new`. Standard padded base64 is used throughout. API DTOs keep unknown-field rejection.
- Subject suffixes retain ASCII bytes `0x20..=0x7e` except backslash; every other byte, including backslash, becomes uppercase `\xNN`. Thus `/srv` gives `:/srv`, invalid `/x\xff` gives `:/x\xFF`, and a literal escape sequence remains distinguishable. UTF-8 multibyte subjects also escape byte by byte. Names do not depend on list order.

## Validation performed

Rust 1.98.0 on PATH, `CARGO_HOME=/tmp/tidemark-b1-fix-cargo`, `CARGO_NET_OFFLINE=true`. The host registry was copied into that temporary directory; the host cache was not edited. Full command transcripts are in the attempt output `library/` directory.

| Command | Local observation | Artifact |
|---|---|---|
| `cargo fmt --check` | Exit 0 | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 | `clippy.log` |
| `cargo test --offline --workspace` | Exit 0; 26 core tests (10 unit, 5 existing integration, 11 new regression) and 3 xtask tests | `test.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 | `doc.log` |
| `sh tools/ci.sh` | Exit 0; all available checks passed | `ci.log` |
| `cargo test --offline --workspace --features privileged --no-run` | Exit 0 | `privileged-compile.log` |
| `git diff --check` | Exit 0 | Scope/whitespace observed before commit |

CI explicitly skipped `cargo-deny` because it is not installed; dependency advisory/license verification remains pending. No privileged runtime tests or Rocky Linux VM/container checks ran. These changes are pure contract/serialization/validation logic, so no new privileged behavior needs a VM test; feature-enabled compilation was checked. Future scanner/storage/service VM behavior remains pending with its owners.

## Shared-file edits and constraints

None. No new dependencies, manifest/lock changes, branches, merges or pushes. No metadata scanning, unsafe code, content reads, mounts, LVM commands or host/service/configuration modifications were introduced. Tests write only their existing temporary config fixtures. Root-only access policy remains intact, including the obsolete `socket_group` rejection test.

## Remaining uncertainty

Independent task verification and concurrent plan updates have not been observed. Future consumers must implement the documented checked conversions, bulk slot/depth bounds, GateStats re-export, collector availability rules, scan ID collision refusal/retry and CLI growth wrapper; this task documents their contracts and does not implement other crates. `cargo-deny` and platform/VM runtime checks are unverified as described above.
