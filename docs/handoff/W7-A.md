# Handoff — W7-A: TUI browser

## Needs from coordinator

- Expose the gated `top::live_rows` implementation as shared CLI support, or explicitly authorize moving it. It is private, and this task forbids editing other commands or duplicating the existing live-refresh path. `R` currently displays this limitation; live refresh and its mutation test are not implemented.
- Add a store child paging API supporting indexed order, cursor/offset and raw-name substring filtering. The existing `children` API only has a limit. This browser pages using indexed `dir(id)` lookups and preorder `subtree_end` jumps, holding at most 1,000 child rows. Sort, growth sort, filter, Home and End operate within a page; global sorting/filtering are not implemented.
- Consider a paged link-instance API: the frozen reader returns all instances (bounded by its existing 1,000,000-row cap). This browser displays only the first 1,000 stored paths.
- Decide exact metrics for `[own files]`: the schema exposes `own_charged`, `own_apparent` and counts, but not an own union/reclaimable/mtime record. The row displays charged bytes, their share bar and file count, including directory inode/LCA charges. It does not invent SIZE, RECLAIM, SHARED, NEWEST or delta values for that row.

- **Branch:** precreated `hp-166ce751cb4b6e25f4f613962a590b2506c2c7182ec1dfaa97706d982b3e9620`
- **Agent/session:** Codex; attempt-166ce751cb4b6e25f4f613962a590b2506c2c7182ec1dfaa97706d982b3e9620
- **Date:** 2026-10-13
- **Status:** partial implementation; evidence candidate awaiting independent review. W7-A acceptance is not claimed complete.

## What was built

The existing command delegates to a new private `tui` module. `state.rs` keeps selection, sort cycling, filtering, pane selection and page cursors; `mod.rs` connects immutable store queries to the input loop; `draw.rs` renders through ratatui without I/O or clock calls; `terminal.rs` handles raw mode, alternate screen, cursor restoration and signal cancellation. The state and data adapter are tested internally through owned external test files, preserving private implementation types.

Only snapshot selection performs existing CLI path canonicalisation. Browsing performs no live filesystem lookup, file-content access, mutation or subprocess action. Previous-snapshot deltas use the immediately preceding complete snapshot of the same root and path lookups for the viewport. Growth sorting needs page-wide delta lookups. Names, annotations and errors use the existing core escaping; tables use the existing shortening and size helpers. The UI sets no foreground/background colours, including when NO_COLOR is present.

### Keys and panes

| Key | Implementation and limits |
|-----|---------------------------|
| Up / Down | Bounded selection; panes scroll via ListState |
| PgDn / PgUp | Next/previous 1,000-child page in main list; 15 lines in other panes |
| Home / End | First/last row in the current page/pane |
| Enter / Right | Descend selected child; links drill into stored instance paths; picker switches snapshot while keeping path |
| Backspace / Left | Ascend by stored parent ID; close panes |
| s | Cycle size/reclaim/files/newest/growth/name; raw-name tie breaks; page-local |
| i | Path, directory/file/inode counts, age histogram, shared/reclaim totals, up to 100 stored errors, annotations, link totals when instance storage permits |
| l | Top 100 link sets; Enter shows up to 1,000 stored instance paths; unresolved bulk names stay unresolved |
| f | Top 100 stored notable files; unresolved paths identified explicitly |
| R | Limitation message only; no live scan |
| p | Complete snapshots for the same root; absent path gives an error without switching; preceding snapshot comparison recalculated |
| / | Edit an escaped-display substring filter of the current page; 256-byte input cap |
| ? | Help and current limitations |
| Escape | Close pane, clear filter; during editing finish filter input |
| q | Quit, except when typing a filter |

Main child rows show directory names, charged share bars, disk/reclaim/shared sizes, H and error markers, file counts, newest Unix timestamps and disk deltas. The header includes snapshot ID, mode, injected age, degraded status and escaped path. Resize redraws through the terminal backend. Below 80x24 only the too-small message appears. Nonterminal stdin/stdout fails with usage exit code 2 before opening the state database.

The restore guard is armed before terminal setup, so partial setup failures unwind through cleanup. SIGINT/SIGTERM set an atomic cancellation flag checked every 100 ms. Normal returns, drawing/input errors and unwinding restore raw mode, screen and cursor. Tests cover cleanup idempotence and unwinding without a real terminal. Real signal/SSH terminal behavior remains pending manual verification. No custom panic hook or global panic-handler changes are made.

## Shared-file edits

| File | Change | Justification |
|------|--------|---------------|
| crates/tidemark-cli/Cargo.toml | Add ratatui.workspace and crossterm.workspace | Ratatui rendering/TestBackend and terminal event/raw-mode handling |
| crates/tidemark-cli/src/lib.rs | Add private mod tui declaration | Connect owned implementation to existing command |
| Cargo.lock | Resolve existing workspace ratatui 0.30 and crossterm 0.29 | Offline host cache resolves ratatui 0.30.2 and crossterm 0.29.0; no workspace dependency edits |

## Assumptions and deviations

The task's explicit instruction to report inaccessible private helpers is applied to `top::live_rows`; it is not copied or made public outside this task's scope. Preorder child paging is the fallback permitted when the store lacks a paged query. It fetches 1,000 rows through indexed primary-key queries rather than one indexed SQL page query. The UI labels sorting as page-local and documents filter scope.

A bounded 1,024-cursor history (4 KiB payload) supports back paging without rescanning earlier pages; after more than 1,024 forward pages the oldest cursor is dropped, so PgUp stops at retained history. Descending/ascending does not preserve parent selection. Errors/annotations/top-file panes expose stored metadata only. The clock is captured once on entry and remains fixed for the browser session; deterministic tests inject their own clock and fixed scan ID.

The initial implementation commit includes main list and panes together; it was followed by deterministic fixture, viewport-delta and bounded-back-paging refinements. The requested separate pane/live/scale implementation sequence was not fully followed because live reuse is blocked.

## How to try a fixture by hand

On a machine with a real terminal, build offline and use an existing fixture snapshot, or create a disposable fixture entirely under /tmp:

```sh
fixture_dir=$(mktemp -d /tmp/tidemark-tui-demo.XXXXXX)
mkdir -p "$fixture_dir/tree/a" "$fixture_dir/tree/b" "$fixture_dir/state"
printf 'fixture\n' > "$fixture_dir/tree/a/file"
cargo run --offline -p tidemark-cli -- --config "$fixture_dir/config.toml" --state-dir "$fixture_dir/state" scan "$fixture_dir/tree" --mode walk
cargo run --offline -p tidemark-cli -- --config "$fixture_dir/config.toml" --state-dir "$fixture_dir/state" tui "$fixture_dir/tree"
```

Use arrows, i, f, l, p, / and ?. Resize below 80x24 and back; quit with q. NO_COLOR can be set for the same monochrome drawing. This recipe has not been exercised through a real terminal in the worker.

## Tests and validation

All commands used Rust 1.98.0 from the host toolchain, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, offline mode, and timeout 1500.

| Check | Observed result |
|-------|-----------------|
| TUI unit/fixture tests | 11 passed; 1 scale test ignored in ordinary runs |
| Nonterminal usage integration | 1 passed; exit code 2 asserted |
| cargo fmt --check | Passed |
| cargo clippy --workspace --all-targets -- -D warnings | Passed, including final-code repeat |
| cargo test --workspace --no-fail-fast | 660 passed, 5 failed, 14 ignored across unit/integration/doc-test targets |
| cargo build --workspace --features privileged --tests | Passed; no privileged tests executed |
| RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps | Passed |
| tools/ci.sh | Failed with exit 1 on the same five known tidemarkd w6a1 Unix-socket readiness tests. Source policy checks passed (145 owner tags); formatting and Clippy passed; cargo-deny was not reached because the test stage failed. |

All five workspace failures are the retained known `tidemarkd/tests/w6a1.rs` readiness tests: lifecycle_reload_and_shutdown, corrupt_catalog_is_preserved_before_recovery, private_and_missing_state_directories_start, startup_reads_last_complete_from_catalog, binary_ready_and_stopping. Logs show API socket `Operation not permitted`; the final test summary identifies only that target as failed. Full transcripts are in this attempt's output `library/`.

Initial Clippy setup lacked an explicit RUSTUP_HOME and failed before compilation because the projected home was read-only. Retrying with the host Rustup path passed. No missing terminal dependency or network fetch blocked this attempt.

## Measurements

The ignored `million_directory_navigation` test writes a configurable synthetic snapshot through SnapshotWriter in 1,000-row batches. Default size: 1,000,000 direct children plus root. `TIDEMARK_TUI_SCALE_DIRS` changes the count. Run with:

```sh
cargo test --offline -p tidemark-cli million_directory_navigation --lib -- --ignored --nocapture
```

First local run: ten page loads 67.189, 68.758, 69.208, 68.301, 73.628, 72.216, 68.896, 72.363, 68.677 and 69.089 ms; whole test including database generation 34.42 seconds, debug build. This measures fetching/sorting pages without drawing or a previous snapshot. Final-code repeat: 69.985, 75.116, 69.757, 69.251, 66.525, 77.575, 74.452, 66.843, 64.737 and 67.483 ms; total 20.78 seconds. Host: x86_64 Intel Core i7-8750H @ 2.20 GHz, 12 logical CPUs. See `library/scale-final.log` in the attempt output. No claim is made for global growth sorting, all panes, cold-cache performance or real terminal responsiveness. Gatekeeper verification remains required.

## Remaining uncertainty

Live refresh, its fixture mutation test, global sorting/filtering, SQL-page reads, complete own-file columns and real-terminal signal handling remain outstanding as described above. No privileged tests were added because the implemented portion only reads immutable snapshots and manipulates a user terminal; feature-enabled compilation is still checked. VM/SSH smoke testing is pending.

## Definition of Done

- [x] Only owned paths changed; shared-file edits listed
- [x] Formatting and workspace Clippy clean
- [ ] Workspace tests fully green — five known sandbox Unix-socket failures
- [x] Feature-enabled workspace tests compile; actual VM/SSH checks pending
- [x] Public command items have rustdoc; source owner headers present; no unsafe code added
- [ ] All W7-A acceptance criteria met — partial implementation and measurement limits above
- [x] No unexplained TODOs or unwrap/expect calls outside tests
- [x] Handoff includes implementation, deviations, evidence, assumptions and follow-ups
