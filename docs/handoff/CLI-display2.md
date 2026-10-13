# Handoff — CLI-display2: packaged-daemon display and LVM schema

## Needs from coordinator

- Run socket-dependent CLI cases and daemon readiness tests without this sandbox's Unix-socket restriction. Workspace `w6a1` currently has the same five readiness/deadline failures recorded by EDGE-cli; no daemon tests were changed. Privileged runtime remains pending VM.
- Reconcile the proposed plan text below into the plan. This attempt changes no plan document or daemon wire DTO.

Branch: canonical attempt branch `hp-62088795b9d322209210a448bac2595a2f492ceea7ce95fb21f2dcbd4a83082b`. Base: `485c43b2c09860a83707ee97491831b1a95e3f14`. Status: implementation and evidence candidate, awaiting coordinator verification.

## What changed

The shared table helper now accepts explicit path/unit columns for middle shortening. Other cells wrap at word boundaries, with continuation text placed at the cell's column. Word budgets favor prose and borrow space from path/unit cells before wrapping fixed metadata such as timestamps. Overlapping unbreakable cells are emitted on separate lines, retaining their column indentation; oversized words are kept whole. Existing byte-escape-aware shortening is unchanged.

Status component details and active/history alert messages use this helper. Status roots, open writers and daemon LVM rows now use the shared aligned table helper instead of handwritten header/row spacing. Hot numeric cells also use wrapping rather than shortening. Status error messages and now's degraded reasons/alert messages use explicit prefixes: continuations start at the message column, including ten spaces after `degraded: `. Prose wrapping preserves oversized words rather than shortening them. Typed path/unit fields in prose retain explicit middle shortening.

Only the CLI JSON envelope for the reduced daemon LVM report changes its schema to `tidemark.lvm.daemon.v1`; direct LVM retains `tidemark.lvm.v1`. Fields, exit codes, flags, sampling quantities and wire format are unchanged. Phantom's daemon alias is `Vec<PhantomSummary>` and direct `scanner.phantom` returns the same type; writers likewise uses `Vec<OpenWriter>` on both paths. Their shapes and schema names remain unchanged.

## Acceptance evidence

New pure tests in `tests/w6a3.rs`, with no terminal/socket dependency:

- `packaged_status_wraps_details_and_aligns_roots`: exact supplied watcher detail and `/mnt/tidemark-install` root at widths 80, 100 and 160; reconstructs the complete detail, checks each counter, detail continuation indentation, page width and header/root column starts, including `2026-10-13 02:29:03Z`.
- `packaged_now_continuations_belong_to_their_degraded_item`: exact supplied degraded reason followed by the next watcher item at all three widths; reconstructs the full reason and checks ten-space continuations and page width.
- `free_text_preserves_oversized_words_and_alert_columns`: preserves a 180-character word on its own indented line; checks complete active/history message text, active message continuation indentation and page width at all three widths.
- `status_error_continuations_keep_the_message_and_prefix_indent`: complete error text and indentation to its prefix's display width at all three widths.
- `daemon_lvm_json_has_its_own_schema_and_unchanged_fields`: daemon schema and exact serialized DTO fields. Existing `render::lvm::tests::direct_report_fits_width_without_changing_json` and `tests/w4a/lvm.rs` cover the direct schema and its richer shape.

## Golden files and workspace assertion search

No golden/snapshot file pins these outputs, so none was updated. Whole-workspace searches for `tidemark.lvm.v1` and old shortened-text assertions are retained in `library/assertion-search.txt` and `library/workspace-assertion-search.txt`. The only integration assertion on `tidemark.lvm.v1` is the direct-mode `tests/w4a/lvm.rs` assertion, which correctly remains unchanged. The daemon operator tests invoke the CLI but do not assert the changed LVM schema or complete changed table layout.

Updated existing assertions in `tests/w6a3.rs`: `status_schedules_are_human_readable_and_fit_widths` expects root/header alignment and permits a whole oversized detail word; `cells_and_prose_fit_injected_widths_without_splitting_escapes` and `every_live_renderer_fits_long_byte_paths_and_controls` permit the specified whole-word exception instead of requiring prose truncation. The direct renderer's inline test in `src/render/lvm.rs` likewise permits an oversized VG/LV word while still checking explicit path shortening and unchanged direct JSON.

## Proposed plan text (three rules; coordinator only)

1. **Human free text:** Component details, alert messages, degraded reasons and errors wrap at word boundaries. Every continuation is indented to the start of its cell's column, or by the prefix's display width for a prefixed item such as `degraded: `. Preserve all words; a word that cannot fit is kept whole on its own line. Middle shortening is reserved for paths and unit names, preserving complete escaped-byte tokens. Use the shared terminal width selection, with width 100 for redirected output.
2. **Human tables:** Every table uses the shared aligned table helper, including status's root/schedule table. Column starts are shared by the header, rows and cell continuations, with deterministic ordering.
3. **Machine schemas:** Direct LVM's richer report uses `tidemark.lvm.v1`; the reduced daemon LVM report specified by R3-4 uses `tidemark.lvm.daemon.v1` in the CLI JSON envelope. No daemon wire field changes. Other commands need `tidemark.<name>.daemon.v1` only when daemon and direct data shapes differ. Phantom and writers currently share identical shapes and retain their existing schema names.

## Assumptions and scope

- The task's explicit coordinator decisions authorize these display/schema changes despite earlier plan text; no additional SCR or plan edit was introduced.
- Free text is terminal-escaped before wrapping. Whitespace between words may be normalized when wrapping; words, counters and escaped controls remain intact. Single oversized words are allowed to exceed the available column budget with the required continuation indentation. Unicode budgeting continues using the existing conservative two-column estimate for non-ASCII scalars.
- Buffers are over existing bounded reports; no scanner I/O, metadata changes, recursion, concurrency, dependency or manifest changes were added. No shared files were changed.
- Existing socket tests retain their one-line skip/return behavior when Unix sockets are refused. A sandbox pass for those cases is not evidence of unrestricted execution. No new privileged behavior required new VM tests; existing privileged suites are compiled, with runtime pending VM.
- No branches, merges, pushes, service commands, mounts or destructive host operations. All Cargo operations use offline mode and the pinned 1.98.0 toolchain with `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`. Initial default-cache resolution failed for base64 before that documented cache was selected.
- Canonical memory receipts returned no deliveries; memory projections were not edited.

## Validation

Source `#[test]` declaration counts (including feature-gated/ignored tests): workspace 673 → 678 (member crates 653 → 658, xtask 20 unchanged); CLI 140 → 145; `w6a3` 15 → 20. Baseline tests were not separately rerun. Five new tests were added; existing direct-mode schema tests remain in place. Final command outcomes and observed runtime totals are recorded below.

All long commands used `timeout 1500`; CI inherited `CARGO_NET_OFFLINE=true`. Final-source logs are in the attempt output's `library/final/`.

| Required check | Observed result |
| --- | --- |
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline --workspace --no-fail-fast` | exit 101: 617 passed, 5 failed, 13 ignored in 59 reported suites; only failing target `tidemarkd --test w6a1` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only |
| `tools/ci.sh` with offline environment | exit 1: source checks (141 owner tags / forbidden symbols), fmt and Clippy passed; tests stopped at the same five `w6a1` failures; deny stage not reached |

The five failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`, each reporting `deadline exceeded` at `w6a1.rs:27`. They match the prior EDGE-cli record and are consistent with this sandbox's refused Unix sockets. No claim is made about unrestricted daemon or privileged runtime success.

The final focused run (`library/cli-settled-final.log`) passed 67 CLI library tests and all 20 `w6a3` tests. The final workspace run also passed the full CLI integration suites, including direct LVM, existing goldens and all five new regressions. Socket-dependent tests can return early under the existing permission detector; their reported passes require unrestricted CI confirmation. Earlier scratch logs contain cache setup and intermediate rendering failures; the final directory and settled focused log describe the submitted source.

## Definition of Done evidence

- Only owned rendering, CLI test and handoff paths changed; no shared-file or dependency edits.
- Public helpers documented; existing ownership headers retained; no new unsafe code or production unwrap/expect.
- Display and schema acceptance covered by fixed-input regressions; no golden file required an update.
- Required commands executed; CI is not green because of the five reported daemon readiness failures.
- Existing privileged suites compiled; runtime pending VM. Commit/cleanup/submission evidence is in the attempt report.
