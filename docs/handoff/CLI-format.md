# Handoff — CLI-format: Human output formatting

- **Branch:** precreated attempt branch `hp-711afc4b12cdac7532a4ee9870b58c35e5d4bf8b6f99e3b9e8392ef4613eceab`
- **Agent/session:** Codex / attempt-711afc4b12cdac7532a4ee9870b58c35e5d4bf8b6f99e3b9e8392ef4613eceab
- **Date:** 2026-10-13
- **Status:** implementation and validation evidence candidate; source-scan exception needs coordinator review below. No independent verification or VM acceptance claimed.

## What was built

| Item | Old human text | New human text | Files |
|---|---|---|---|
| Daemon LVM optional numbers | `Some(25.0) None None None` | `25.00% - +1.50 -0.50` when supplied; each missing numeric cell is `-` | `render/now.rs`, `render/lvm.rs` |
| Situation-view LVM percentages | `25.0%` / `unknown` | `25.00%` / `-`, sharing direct LVM formatting | `render/now.rs` |
| Direct LVM optional numbers | `n/a` for individual missing percentages/slopes | `-`; measured percentages retain two decimals and `%`, slopes retain signed two decimals with existing percentage-points/day context | `render/lvm.rs` |
| Symlink class counts and examples | `Dangling: 1`, `(OtherFs)` | `dangling: 1`, `(leaves the mount)`; explicit labels for all six classes | `render/symlinks.rs`, symlinks text snapshot |
| Completed foreground minute | bare `row.ts` epoch | `2026-10-12 22:52:56Z` | `cmd/watch.rs` |
| Individual foreground event | `22:52:56` without date/timezone | `2026-10-12 22:52:56Z` | `cmd/watch.rs` |
| Watcher reasons | `10:42 UTC (timestamp 1791888120)` | `10:42 UTC` | live `watch/rings.rs`, CLI `tests/w6a3.rs` |
| Namespace failure | `namespace check: Err(...)` / `host mount namespace check failed: Ok(false)` | `host mount namespace check failed: <Display error>` / `host mount namespace check failed: process is outside the host mount namespace` | daemon `services/watcher.rs` |
| Alert history state | Debug then lowercase | explicit `firing`, `resolved`, `renotify` | `render/alerts.rs` |

No JSON branch, wire type, integer timestamp, CLI flag, manifest, dependency, scanner policy, or lifecycle behavior was changed. The watch minute renderer was extracted so tests exercise the same function as the running command. Existing UTC civil arithmetic is reused. Namespace formatting only changes component detail, leaving pressure handling and recovery flow intact.

## Shared-file edits

None. No module declarations or dependencies added. `render/mod.rs` is unchanged.

## Acceptance criteria evidence

- `daemon_lvm_numbers_are_percentages_and_unknowns_without_sockets`: present and missing values through daemon LVM at widths 80/100, situation-view LVM percentages, signed trends, unknown cells, forbidden Option text, unchanged JSON payload.
- `every_class_has_an_operator_label`: renders every symlink class in both counts and examples; byte-path/target tests remain.
- `completed_minute_uses_utc_and_preserves_machine_seconds`: exact UTC text, signed delta/event/loss detail and unchanged JSON integer timestamp/full payload without watcher sockets.
- `namespace_failures_name_the_cause_without_debug_results`: false namespace identity, error Display text, successful check helper text.
- Updated live tests `diagnostics_distinguish_caps_from_each_transport_loss` and `diagnostics_retain_only_latest_occurrence_per_cause`: retain causes and latest minute while rejecting epoch suffixes.
- Updated CLI `appended_watcher_cap_detail_wraps_without_losing_reason`: status/now width 80/100, complete reconstructed reason and continuation indentation.
- Updated `crates/tidemark-cli/tests/w3b/snapshots/w3b__symlinks_text.snap`: lowercase dangling/loop labels. JSON snapshot unchanged.
- New `tests/format_scan.rs`: recursively scans renderer/command Rust sources, rejects positional/named/pretty/aligned Debug interpolation; narrow exact-line exceptions documented below. This is a textual guard, not Rust AST/data-flow analysis.

## Needs from coordinator

The task asks for source-scan exceptions only for JSON/tests. One preexisting production exception remains: `crates/tidemark-cli/src/render/mod.rs:68`, `format!("{value:?}").to_ascii_lowercase()`. It normalizes enum labels and serves the private TUI `Pane` and `Sort` types (`src/tui/state.rs:7,17`, call sites `src/tui/draw.rs:37,38`), as well as scan/device labels. Replacing its generic Debug interface cleanly with explicit labels requires making those private TUI types accessible or replacing their call sites, outside this attempt's scope. The new guard explicitly allow-lists only that existing expression/path; it does not permit raw Debug in other output. This exception exceeds the requested JSON/test-only exception policy and requires coordinator acceptance or a follow-up with TUI scope. The task's absolute no-Debug-source-scan criterion is therefore not claimed fully met.

The other exact-line exception is `src/cmd/deploy/stage.rs:125`: an existing test-only path-to-TOML fixture, with a comment explaining it. No JSON exceptions are needed in the current sources.

Whole-workspace searches for `(timestamp ` and `Dangling` were recorded in external `library/old-strings.txt`. Outside owned paths:

- `docs/reviews/W7-review3-correctness.md:112,114`: historical finding and probe text.
- `docs/reviews/W1-gate.md:30`: unrelated prose `latest (timestamp then ...` matches the broad substring search.
- `docs/handoff/DAEMON-polish.md:32` and `docs/handoff/CLI-polish.md:18`: historical old watcher text; left untouched.
- `docs/plan/waves/wave-3-queries.md:90,110`: frozen enum/source semantics, not human labels.
- `crates/tidemark-fs/src/symlinks.rs:37,47,286,351`: enum declarations and classification logic; unchanged by design.
- `crates/tidemark-fs/tests/w3b.rs:57,114,195,279,296,302,355,358,404`: enum-based assertions/fixtures, unaffected.
- `crates/tidemark-cli/src/cmd/symlinks.rs:69`: enum comparison, unaffected.

`tests/fault/check.py` and `tests/install/install.sh` were explicitly searched and contain neither old substring. No out-of-scope executable consumer of the removed human text was found. The exact enum name remains available in code; JSON serialization is unchanged. Historical documentation is listed rather than edited.

## Validation and limitations

Commands use the pinned Rust 1.98.0 directory on PATH, a temporary copied Cargo cache (`/tmp/tidemark-cli-format-cargo`), `CARGO_NET_OFFLINE=true`, and `timeout 1500` per long command. No network dependency fetch, privileged command, socket-policy change, host configuration change, branch creation, merge or push was performed.

Initial focused CLI library/format-scan/w3b/w6a3 run passed: 105 library tests, 1 guard, 2 w3b, 30 w6a3 (two ignored scale tests across those harnesses). Socket-sensitive tests can return early; those counts do not claim listener coverage. Initial Clippy found helper `unwrap()` calls in the new source-scan test; those were replaced with propagated I/O errors before final validation. Initial and final evidence logs remain distinguishable.

Final command outcomes are appended after completion. Privileged runtime, real fanotify/XFS/LVM, ordinary-user Unix listener and namespace scenarios remain **pending VM / coordinator CI**. No new privileged test is needed for these pure formatting changes; existing privileged suites are compiled only.

## Final observed validation

| Command | Exit | Evidence |
|---|---:|---|
| `timeout 1500 cargo fmt --check` | 0 | `library/final-fmt.log` |
| `timeout 1500 cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `library/final-clippy.log`; initial failed run retained in `library/clippy.log` |
| `timeout 1500 cargo test --offline --workspace --no-fail-fast` | 0 | `library/tests.log` |
| `timeout 1500 cargo build --offline --workspace --features privileged --tests` | 0 | `library/privileged.log`; compilation only |
| `timeout 1500 tools/ci.sh` | 0 | `library/ci.log`; source checks (150 owner tags), fmt, Clippy, tests, privileged compilation passed |

CI explicitly skipped cargo-deny because it is not installed; dependency advisory/license freshness is not established. UID 0 and umask 022 were observed. The sandbox restricts Unix binds/user namespaces, so successful harness totals include guarded early returns and do not establish those scenario assertions. Privileged runtime remains pending VM. No five-daemon-readiness failure occurred in these runs.

- `tests.log`: 896 passed, 0 failed, 14 ignored across 84 harness summaries (including nested helpers).
- `ci.log`: 896 passed, 0 failed, 14 ignored across 84 harness summaries (including nested helpers).

Final Clippy and CI covered the final source, including the source-scan helper I/O correction, tightened negative assertions and shared LVM helper rustdoc. No source edits followed the passing CI run. `git diff --check` passed. Cleanup, commit and submission evidence is recorded in the external report/artifacts after those operations.

## Definition of Done evidence

- [x] Only owned paths changed; no shared manifest/module declarations or dependencies.
- [x] Final fmt and workspace Clippy pass; workspace tests and available CI checks pass locally.
- [x] Existing privileged test suites compile; runtime remains pending VM.
- [x] New source header and shared helper rustdoc present; no new unsafe or production unwraps.
- [x] Five reported output paths, updated assertions and snapshot are evidenced by tests.
- [ ] Strict source-scan exception policy: existing normalized enum helper remains a documented production exception requiring coordinator review, as detailed above.
- [x] Handoff includes old/new text, search results, validation and limitations.
