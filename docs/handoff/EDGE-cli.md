# Handoff — EDGE-cli: CLI review and live-daemon findings

## Needs from coordinator

- **B8 remains blocked by the frozen schema contract.** 08 §1.1/§2 and R3-4 prescribe `tidemark.lvm.v1` and the reduced daemon view, but name no separate direct schema or discriminant. No schema was invented. Direct data is `{status, sampled_at, tracked, vgs, other_lvs}`; daemon data is `{mounts, vgs, sampled_at}`. Please decide their machine-output distinction and issue the corresponding contract decision/SCR.
- **ID-conditional cancellation needs a contract decision.** `scan.cancel` accepts only a root. Checking an ID immediately before cancellation still permits a replacement scan to start between requests. An interrupted forwarded scan now exits 3 without sending that root-only request; its diagnostic says the daemon scan continues. Explicit `scan --cancel` remains the separately requested root operation. Please decide an atomic ID-conditional API if automatic cancellation on SIGINT/SIGTERM is required.
- **B5 observed text/JSON discrepancy needs same-report evidence from the guest.** This baseline already uses `rate_15m` for the `(15m)` label and `ttf_secs` for the `(1h trend)` label. `tidemarkd/src/state.rs` computes TTF from `rate_1h`, as 07 §2 specifies. Different slopes for the two windows are intentional. No daemon change was made; supplied report quantities render unchanged. Separate CLI calls can sample different reports. The quoted 203 MiB/h versus 2362647.27 B/s is not reproduced from one injected report.
- **B9 belongs to watcher aggregation.** The CLI prints `HotEntry.files` unchanged. A file present in two minute buckets is counted by the producer; no CLI deduplication is possible from the aggregate DTO.
- Socket regressions and privileged runtime need unrestricted coordinator/VM execution. Sandbox listening-socket cases explicitly print one line and return. Required workspace/CI outcomes are recorded below; this is an evidence candidate, not independent verification.

Branch: canonical `hp-82d4492abd8354c3a11d55377669698c76f3971c8b502ff750243ce97c697dca`. Date: 2026-10-12/13 UTC. Base: `f9f01d18cec39c729f863cd1582fd8317ad1062f`.

## Per-finding evidence

| Finding | Change | Coverage |
| --- | --- | --- |
| F05 | Definitive missing endpoint or connect refusal alone permits direct fallback. A no-follow endpoint metadata check permits missing sockets without attempting a socket syscall; endpoint inspection, actual connection and socket timeout setup all run inside Client's existing 11-second deadline, including both direct guards and the guard under the scan lock. Other metadata/transport failures become daemon-unavailable exit 4. Connect error classification preserves nonabsence errors rather than collapsing reset/broken-pipe or failed connection workers into absent endpoints. Permission failures include the root-only access hint. | `scan_full_backlog_expires_without_direct_state_writes`, `scan_invalid_endpoint_never_writes_state`, `scan_permission_error_never_falls_back`; CLI subprocess outer deadline 20s, fake accept deadline 5s and read deadline 3s. |
| F06 | Resolve the started ID's completion before considering running activity. Never poll a different running ID. Once the status slot advances/disappears, query that exact ID read-only in the configured local catalog; reconstruct a completion DTO from the catalog when needed. Never automatically send root cancellation for a started ID. | `scan_resolves_only_its_catalog_outcome` covers A complete/B running, A complete/B complete, completion alone and absent status, each with clean/degraded/partial/missing catalog outcomes. |
| F07 | Clean exit requires the exact root/ID's catalog entry to be finished, complete and nondegraded. Degraded/partial or indeterminate outcome exits 3 with a diagnostic, never assumes success. A known degraded completion retains its human/JSON completion payload before returning 3; an unconfirmed or partial entry does not manufacture a clean completion. | Same catalog-backed matrix; existing fake-success case now creates a clean catalog fixture. |
| F08 | Shared stdout TTY width selection, positive width or default 100; explicitly injected width renderers for tests. New bounded table helper shortens cells in the middle with `...` while preserving complete `\\xHH` tokens. Prose wraps only at whitespace and shortens oversized tokens. Direct phantom/writers reuse the shared renderers; direct LVM uses the same selection/wrapping. Existing unrestricted `aligned` remains unchanged for other commands. JSON paths/data bypass fitting. | `cells_and_prose_fit_injected_widths_without_splitting_escapes`, `every_live_renderer_fits_long_byte_paths_and_controls` (40/80/100/200, 200-character path, Unicode, invalid UTF-8 and controls), `render::table::fitting_tests`, `render::lvm::tests::direct_report_fits_width_without_changing_json`. |

## Per-live-run item evidence

| Item | Change/result | Coverage |
| --- | --- | --- |
| B1 | CLI owns a report renderer with the established 07 layout and whitespace wrapping, avoiding the out-of-scope live library's character-by-character line cuts. | Width/prose and report renderer tests above. |
| B2 | Schedule rows show yes/no, `-` for missing times and UTC civil timestamps. | `status_schedules_are_human_readable_and_fit_widths`. |
| B3 | Human files MTIME and wide-directory NEWEST, now generated/last-complete, status error/schedule and alert timestamps use `YYYY-MM-DD HH:MM:SSZ`. Relative scan age remains. JSON integers remain. Gregorian civil arithmetic is copied into a shared CLI display helper without a dependency. | Status/now tests include the given 2026 timestamp and pre-epoch dates; `file_rendering_shared_and_unresolved` and the wide-tree snapshots pin UTC human output; explicit assertions retain JSON mtime/newest_mtime 100. |
| B4 | Keep supplied hot-directory rows beside writers for each mount; missing top unit now displays `?`. Writers use the plan’s per-unit byte summary without repeating the unit as a leader column. Baseline already rendered nonempty supplied hot-directory lists; LIVE-hot provides those lists upstream. | `now_uses_report_rates_hot_directories_units_and_utc`; includes attributed/unattributed byte paths and a unit summary shown once. |
| B5 | Retain exact supplied `rate_15m` converted B/s → B/h, and exact supplied `ttf_secs` labelled one-hour trend. No recomputation from another rate. Existing baseline quantities were already correct for one supplied report. | Same test uses 2362647.27 B/s → 7.92 GiB/h and 189s → 3m 9s despite a deliberately different one-hour slope; JSON retains the original integers/floats. |
| B6 | Keep provided top-unit values in directory/command rows. Unit grouping omits the redundant TOP UNIT column without changing JSON. Baseline already rendered non-null top_unit; LIVE-hot fixes its producer. | `hot_units_are_visible_without_repeating_unit_group_keys`. |
| B7 | Only the verify command's config_hash detail changes: both validated SHA-256 prefixes (12 hex chars) or missing; mismatched firstboot explicitly stale relative to staged. Full raw records remain in JSON. Health evaluation and exit codes unchanged. | `cmd::deploy::verify::tests::verify_hashes_show_both_prefixes_and_stale_side`, including equality, mismatch, either/both missing and invalid hash. |
| B8 | Left unchanged pending coordinator's schema decision, per explicit task instruction. | Direct LVM test pins unchanged schema/shape and full JSON cells; existing daemon/fallback tests pin their schema. |
| B9 | Left aggregate files count unchanged; no inode identity/window membership in HotEntry allows deduplication. | Existing hot renderer tests continue passing supplied counts unchanged; producer follow-up required. |

For B4/B5/B6's already-correct producer-to-renderer seams, tests establish current behavior rather than pretending those aspects were reproducible CLI defects in the baseline. New regressions fail baseline behavior for missing-unit text, duplicate unit columns, UTC output, bounded lines and scan lifecycle/error behavior. B8/B9 intentionally do not have a failing-old-output test because changing that output would violate scope/contracts.

## Golden files and workspace assertion search

Updated three human goldens under `crates/tidemark-cli/tests/w2c/snapshots/`: `w2c__files.snap` (MTIME), `w2c__tree_ascii.snap`, and `w2c__tree_unicode.snap` (NEWEST). Epoch columns become UTC dates, with adjusted alignment. `w2c__files_json.snap` and `w2c__top_json.snap` remain unchanged; explicit JSON mtime/newest_mtime assertions confirm integer preservation.

Searched all workspace tests for MTIME, mtime, epoch-style Some/None examples, UTC, config_hash/staged/firstboot strings, hot-directory/units and CLI binary invocations. The affected human goldens are the three files above. Existing w6a3 fake scan-success fixtures needed catalog evidence. Existing `GROWTH  DISK`, schema, fallback and health substring assertions remain valid. `tidemark-live/tests/w5d/snapshots/{healthy,degraded,multi_mount}.snap` (actual filenames prefixed `w5d__`) pin the library's UTC/layout and remain unchanged because that library is outside scope. The daemon's `tests/w6a3/operator.rs` invokes the CLI and asserts component substrings/schema/success, not the changed epoch or schedule text; its privileged module checks report fields directly. W4-A direct LVM JSON shape tests remain unchanged. E2E scan/report lifecycle and file/snapshot command tests pass in the CLI run after definitive-absence handling was added. Search output is retained in attempt `library/old-assertions.txt`.

## Assumptions and bounds

- The configured local state directory is authoritative for forwarded scan outcome, matching existing CLI store access; unavailable/mismatched catalog outcome is exit 3. No wire/core/store contract changed.
- Staging is the authority when comparing staged and firstboot hashes. The diagnostic does not claim to identify divergence from an independently edited installed file, which VerifyResult does not expose.
- Width fitting preserves ASCII escaped byte tokens, scalar UTF-8 boundaries and a path's beginning/last component whenever that component fits the available cell budget. An individual component larger than the entire cell must itself be visibly shortened. The new width-aware helpers conservatively budget two columns for each non-ASCII scalar (one for ASCII), so wide glyphs cannot exceed the budget; combining/ambiguous glyphs may leave extra room or imperfect alignment depending on the terminal locale. The preexisting unrestricted table helper is unchanged.
- Render buffers are temporary over already bounded report/inventory payloads (daemon frame capped at 1 MiB). No scan tree recursion/maps/channels or metadata syscalls were added to traversal. Endpoint metadata checks are socket guards, not scanned-file traversal.
- Library renderer duplication is intentional scope isolation: only the CLI renderer changes; future layout changes should reconcile this copy with the original library renderer. Color remains plain as in the existing library implementation.
- No branches, merges, pushes, privileged execution, dependency additions or host service/LVM/mount commands. Required privileged tests are compiled only; runtime pending VM. Memory receipt checks returned no deliveries.

## Proposed plan text (coordinator only)

- 08 scan fallback: only a definitively missing/refused endpoint permits direct scanning; every real connection has an absolute deadline, rechecked under the scan lock. Other connection failures exit 4 before direct state mutation.
- 08 scan completion: track the started ID, resolve its persisted outcome read-only, and return 3 for degraded/partial or unknown outcomes. Do not follow replacement scans. Automatic ID-targeted cancellation requires an atomic conditional API; frozen root-only cancellation is available only as an explicit root operation.
- 07/08 human display: share positive stdout TTY width/default 100; middle-shortened cells retain escaped tokens; prose wraps at whitespace; machine report fields are untouched. Human absolute times are UTC civil times with Z, and relative ages remain.
- 07 now: display provided hot dirs and top_unit, `?` for unknown. Rate label uses `rate_15m` converted to hourly display units; TTF uses the provided one-hour `ttf_secs`, with no CLI recomputation.
- Decide distinct LVM machine view schemas/discriminant and clarify whether hot FILES means sum of per-minute distinct counts or distinct files across the requested window.

## Validation

Source test declarations (includes feature-gated/ignored tests): workspace 616 → 628; CLI 128 → 140; w6a3 6 → 15. Baseline tests were not separately rerun. Earlier CLI-only run passed 66 library tests and all its integration/doc suites before the final terminal-selection/permission additions; final-source required-run totals and command exits follow (source commit `1e78681`). Socket-dependent tests returning early are not proof of their unrestricted behavior.

| Required check | Observed result |
| --- | --- |
| `cargo fmt --check` | passed; also repeated after final source changes |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | passed; final-source repeat passed |
| `cargo test --offline --workspace --no-fail-fast` | exit 101: 569 passed, 5 failed, 13 ignored across 59 reported suites; sole failing target `tidemarkd --test w6a1` |
| `cargo build --offline --workspace --features privileged --tests` | passed; compilation only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | passed |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | exit 1 at the same five daemon w6a1 readiness failures; deny stage not reached |

The five `w6a1` failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`, all `deadline exceeded` at w6a1.rs:27. They match the task's known sandbox daemon/socket readiness limitation; no daemon test was changed to suppress them.

Targeted `cargo test --offline -p tidemark-cli --lib --test w2c --test w6a3` after the final layout/completion changes passed 67 library + 10 w2c + 15 w6a3 cases (92 reported passes, one scale test ignored). The earlier `--lib --test w6a3 -- --nocapture` run passed 82 cases and records socket skip notices. Four w6a3 cases explicitly returned after the EPERM socket probe: `explicit_direct_never_connects_to_listening_daemon`, `live_tables_and_shared_byte_path_machine_output`, `scan_resolves_only_its_catalog_outcome`, and `scan_full_backlog_expires_without_direct_state_writes`. Catalog/fake-daemon/full-backlog runtime therefore remains pending unrestricted CI. Pure renderer, missing/invalid endpoint, permission, direct-mode and socketpair coverage executes locally. Privileged runtime is pending VM.

All cargo runs use the existing host cache (`CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, pinned 1.98.0) and offline mode. Each long command is individually bounded by `timeout 1500`. No dependency was fetched/added. Retained `library/settled/checks.txt`, final-source required-command logs, earlier logs and targeted regression logs record exits, test output and skip notices. Initial experimental runs included a Rust closure-lifetime error, wrong JSON fixture field paths and one octal-permissions Clippy diagnostic; each was corrected before final source validation. The only initial runtime failure caused by this patch was refusing absent-socket EPERM in direct E2E; definitive absence checking resolves it while preserving F05's policy.

## Shared-file edits

No manifest, lib.rs/main.rs module declaration, core, daemon or live-library edits. The client edit is limited to its shared connection-deadline helper and moving timeout setup into that helper, preserving constructor behavior while bounding the entire attempt. The shared render table helper's original `aligned` behavior remains intact; audited consumers are tree/files/snapshots, and only designated live commands call the added width-aware variant.

## Definition of Done evidence

Only task-owned paths and this note change. Formatting, lint, test, privileged-build, rustdoc and CI outcomes are reported above, including sandbox gaps. Independent review/integration and VM execution remain coordinator responsibilities.

Final-source checks completed on `1e78681`: fmt 0, clippy 0, workspace tests 101 (only the five w6a1 failures), privileged build 0, rustdoc 0, CI 1 (same w6a1 failures, deny not reached). Logs: attempt `library/settled/`.
