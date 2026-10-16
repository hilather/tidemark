# Handoff — TUI-tide: Tide TUI redesign (concept 3)

- **Branch:** muse/tui-tide  **Agent/session:** Muse session 01a14463-c752-7491-8a07-af74af37d6ea (Fable fix; implement session 01a14429-e7d2-7b53-b463-cf1900381b9f)  **Date:** 2026-10-16
- **Status:** implemented

## What was built

The snapshot browser now draws the Tide layout from `crates/tidemark-cli/src/tui/theme.rs` and a rewritten `draw.rs`. `Theme::select` picks Rgb, Indexed, Basic, or Mono from the existing `--color` flag, `NO_COLOR`, `COLORTERM`, and `TERM`, and picks ASCII glyphs when `TERM` is `linux` or `ascii_for_lang` says so. `run()` calls that once. The frame is a header, an optional tide chart, the depth table, a legend, optional strata and soundings panels, a footer, and a message line.

The chart, the startup previous snapshot, and the picker share `previous_complete`: nearest strictly older `started_at`, then the smaller scan id. The chart's Δ title is drawn only when that previous snapshot is also one of the bars in the window. Bar values come from the catalog `ScanEntry.t_disk` already loaded at startup. No new store field or query was added. `Browser.main_selected` keeps the depth-table row for strata and soundings while another pane is open. A reload finds that row by path only. A reused directory id does not count. `s` and any other depth reload from a non-Main pane use the same helper as a finished refresh: they put the panels back on that path and leave the open pane's list cursor where it was. When the path is past the first page, settle ranks it in `pages.growth` if that cache is the list just built, otherwise it walks `children_page` at the existing page size and loads the page whose keyset cursor contains the path. The same walk places `./`. The message says `selected directory is gone` only when that search misses, including from `s`. Esc, Left, and Backspace close the pane before they seek, so the reload is a Main seek to offset 0. Esc also clears the filter. Left and Backspace leave it. `poll_refresh` records `depth_target()` before it replaces `dir` or zeroes the page, and passes that target into the reload. Links and `f` Files draw the column title as a fixed line above the list, so the highlight index is the data row. `Refresh.started` is an `Instant`; the event loop stores elapsed milliseconds before each draw so the spinner does not read the clock inside `draw`.

Below 120 columns the APPARENT column is omitted. The full 25-character scan id is never shortened. At 80 columns the header drops the `≋ tidemark` brand before it shortens the path, and drops the path when fewer than 8 columns would remain. LEVEL bars use `dir.t_charged` as the denominator. Visible rows, including `./`, receive only the cells their weight owns, by largest remainder, and the rest of the track stays empty. FILES uses the integer-rounded k/M/G/T/P/E ladder and stays within six columns. Enter-on-`./` still uses `size::timestamp`, including `1970-01-01 00:00:00Z` for mtime 0.

## Shared-file edits

| File | Change | Why |
|------|--------|-----|
| — | none | No `Cargo.toml`, `lib.rs`, or `main.rs` edits. `mod theme` is inside the already-owned `tui/mod.rs`. |

## Deviations from the spec

| Spec reference | What I did instead | Why | SCR? |
|----------------|--------------------|-----|------|
| N, AB (commit, push, PR body) | No commit and no push from this worktree. The coordinator opens the pull request | The implement request forbade git writes | no |
| M, U, KK (tmux capture and PNGs) | Screenshots already live in `docs/screenshots/tui-tide/` (`before/` and `after/`). This change does not touch that directory | Capture was outside this fix round | no |
| Step 12 (no key or behaviour changes) | Left and Backspace from a non-Main pane call `close_pane`, then `seek_boundary(false)`, which reloads page 0. Esc already did that after round 3 and also clears the filter. Left and Backspace do not. On `origin/main`, Left and Backspace only closed the pane and left the current page in place | Round 3 closed first so a stale offset is not written back. The same seek now covers Left and Backspace | no |
| Step 12 (`mod.rs` change list) | `mod.rs` (1,403 lines) also carries `main_selected`, `reload_keeping_selection`, the depth-target capture in `poll_refresh`, and the column-header split. Plan step 12 limited it to the `t_disk` tuple, theme and elapsed fields, `Refresh.started`, humanised pane strings, and `Theme::select` in `run` | Later review rounds added selection settle, live reload, and the Fable header fix inside the file the plan already owned | no |

APPARENT below width 120 is the specified degrade (K, AB), not an extra cut. The tide chart reads catalog `ScanEntry.t_disk` (AB). Wave decoration and strata link counts stay out of scope.

## Acceptance criteria evidence

| Criterion (copied from task) | Evidence (test name / command + output excerpt) | Status |
|------------------------------|-------------------------------------------------|--------|
| Theme selector: Mono when color is off or `NO_COLOR` is set (including empty); Rgb for `truecolor`/`24bit`; Indexed when `TERM` contains `256color`; else Basic; `TERM=linux` or ASCII locale uses ASCII glyphs | `theme_select_follows_color_flags_and_locale` | passed |
| Chart only when width ≥ 100, height ≥ 34, and at least two catalog rows have `t_disk`, filtered before take(7). Delta uses `previous_complete`, including a tie broken by the smaller scan id. The Δ title is omitted unless that previous snapshot is also a bar in the window | `tide_chart_uses_root_totals_and_the_smaller_tied_snapshot`, `previous_complete_picks_the_nearer_start_then_smaller_id`, `tide_chart_omits_delta_when_previous_is_outside_the_window` | passed |
| Header keeps the full scan id, mode, age, and high-water; brand drops first at 80 columns. U+00B7 and ≋ each count as one column, so the 160-column header ends on the high-water size. A path wider than the leftover budget is shortened to that budget when at least 8 columns remain. A leftover under 8 columns drops the path | `header_keeps_the_scan_id_and_high_water`, `header_shortens_the_path_to_the_leftover_budget` | passed |
| SIZE is right-aligned at both widths, including `68.9 MiB` | `size_column_is_right_aligned_at_both_widths` | passed |
| `./` dashes RECLAIM, SHARED, NEWEST, and TIDE | `own_row_dashes_the_four_snapshot_cells` | passed |
| LEVEL denominator is `dir.t_charged`. A 1-byte child in a 1000-byte directory paints no cell (`level_cells(1_000, &[1], 12) == [0]`). Weights that already sum to the directory still fill the bar (`[2, 1, 1]`). Empty when charged is 0. Bar values shift before `u64` overflow | `level_cells_use_largest_remainder_and_fit_bars_shift` | passed |
| FILES ladder pinned at the CC cliffs `99_949`, `99_950`, `999_499`, `999_500`, `9_994_999`, `999_499_999`, through `18.4E`, each width ≤ 6 | `fmt_files_stays_within_six_columns` | passed |
| `fmt_when`: same UTC year → `MM-DD HH:MM`; other values, including 0 and negative, → `YYYY-MM-DD`. `fmt_when(0)` with now in 2026 is `1970-01-01`; a small negative is `1969-12-31` | `fmt_when_uses_the_utc_year_and_keeps_epoch_dates` | passed |
| Enter-on-`./` mtime 0 stays `1970-01-01 00:00:00Z` | `direct_retained_files_filter_before_limit_and_show_utc` (assertion not edited) | passed |
| `main_selected`: Info+Down keeps the depth row; Left/Esc returns cursor 0 and the first child (offset 0), including after a deep page and during a live refresh of more than 1,000 children. `/` from another pane restores that row. A finished refresh resolves that directory by path only, never by id, and does not zero the Info cursor. If the path is gone, the index is 0 and the message says `selected directory is gone`, including when a surviving sibling inherited the deleted id, and including `s` from Info. `s` from Info keeps a child or `./` whose new rank is at least 1,000, in snapshot order and in the growth cache, and keeps the open pane's list cursor | `main_selected_stays_on_the_depth_row_across_panes`, `poll_refresh_keeps_the_info_cursor_and_the_selected_directory`, `refresh_does_not_keep_a_sibling_that_inherited_the_deleted_id`, `refresh_follows_the_path_when_an_earlier_page_reuses_its_id`, `sort_from_info_keeps_the_directory_and_the_info_cursor`, `sort_from_info_keeps_a_child_ranked_past_the_first_snapshot_page`, `sort_from_info_keeps_the_own_row_past_the_first_page`, `sort_into_growth_keeps_a_child_the_growth_cache_ranks_past_1000`, `left_from_info_reloads_the_first_page_after_a_deep_sort`, `esc_from_info_during_live_refresh_returns_to_the_first_child`, `sort_from_info_reports_a_directory_that_is_really_gone` | passed |
| No-match copy differs while editing | `no_match_copy_depends_on_editing` | passed |
| Too small: `terminal too small`, `need 80×24, have 79×23`, `minimum 80x24` | `too_small` | passed |
| Info and Help have no `▶` / `>` marker; long Info at 80×24 scrolls | `info_and_help_do_not_use_the_table_marker` | passed |
| Rgb and Indexed frames paint navy on a trailing legend cell and a trailing footer cell (`Rgb(7,21,38)` / `Indexed(233)`). Block titles are bold teal; borders stay the edge colour. A coral rising mark is still painted | `rgb_frame_paints_navy_and_a_coral_rising_mark` | passed |
| Files pane uses `fmt_when` for mtime; Enter-on-`./` title still uses `timestamp`. SIZE is right-aligned and MTIME is left-aligned, the way the links columns are, so PATH starts in one column | `files_pane_formats_mtime_and_keeps_the_retained_timestamp`, `files_rows_line_up_under_the_header` | passed |
| While refresh runs, the Files message names the snapshot and the live refresh. After it finishes or fails, Files still shows that result next to the snapshot disclaimer, including "Live refresh already running". The short result comes before the disclaimer, so at 80 columns `selected directory is gone` and `Esc cancels` stay visible | `files_footer_names_the_snapshot_while_refresh_runs`, `files_message_shows_a_finished_and_a_failed_refresh`, `files_message_keeps_the_result_ahead_of_the_disclaimer_at_80_columns` | passed |
| 80×24 and 160×48 renders stay stable; `./` vs `.`; `steady` replaces `degraded=false`. The 160-column header is flush. Strata uses one age spelling: `≥365d` with Unicode glyphs, `>=365d` with ASCII | `deterministic_render_at_both_sizes` plus reviewed insta snapshots, `pane_titles_stay_inside_the_width_and_strata_spell_one_age` | passed |
| Lowercase depth header, `walk`, full id, `steady` | `human_header_and_pane_labels_are_lowercase` | passed |
| Files block title at 80×24 still fits the scan id and UTC timestamp | `post_tui_files_pane_fits_80_columns` | passed |
| Hostile names stay escaped on all seven panes | `hostile_names_in_data_panes` | passed |
| Links and `f` Files column titles are a fixed line above the list. Enter on the 2nd highlighted link opens that set. End reaches the last link and the last file | `enter_on_the_second_highlighted_link_opens_that_set`, `files_column_header_is_not_a_selectable_row` | passed |
| Refresh with `./` selected keeps `./` when its own bytes grow past the children. A multi-page refresh keeps the open child on that page | `refresh_keeps_the_own_row_when_its_bytes_grow_past_the_children`, `refresh_keeps_a_child_on_a_later_page` | passed |
| Basic `Text` is Reset. ASCII swaps `·`, `Δ`, and `×` for `-`, `d`, and `x`. The sort marker is teal. Picker dates pad to 11 columns. Panel titles clip to the pane width minus the border | `theme_select_follows_color_flags_and_locale`, `too_small`, `tide_chart_uses_root_totals_and_the_smaller_tied_snapshot`, `rgb_frame_paints_navy_and_a_coral_rising_mark`, `picker_dates_pad_so_the_total_column_lines_up`, `pane_titles_stay_inside_the_width_and_strata_spell_one_age` | passed |
| `cargo fmt --all -- --check` | Fable fix: exit 0 | passed |
| `cargo clippy --workspace --all-targets -j4 -- -D warnings` | Fable fix: exit 0. Finished the dev profile in 6.11s, 318 units, with no warnings | passed |
| `cargo test -j4 -p tidemark-cli` | Fable fix: exit 0. Lib 184 passed, 0 failed, 1 ignored (`million_directory_navigation`). Integration: e2e 3, format_scan 1, fuzz_smoke 11, rules 3, state_trust 1, surface 13, version 4, w1a 2, w2c 14 passed / 1 ignored, w3a 6, w3b 2 / 1 ignored, w3c 3, w4a 4, w4b 5, w5b 2, w6a3 34, w7a 1. Doc-tests 0. Ignored scale tests were not run | passed |
| `CARGO_BUILD_JOBS=4 cargo xtask ci` | Fable fix: exit 0 (448s). Printed `ci: all available checks passed`. `w6a2` linked and passed (25 tests). `cargo-deny` is not installed, so that check was skipped with a warning | passed |

## Tests

- Unprivileged: the fifteen new tests named above, plus the round-2 tests in the acceptance table (`refresh_does_not_keep_a_sibling_that_inherited_the_deleted_id`, `refresh_follows_the_path_when_an_earlier_page_reuses_its_id`, `sort_from_info_keeps_the_directory_and_the_info_cursor`, `files_message_keeps_the_result_ahead_of_the_disclaimer_at_80_columns`, `files_rows_line_up_under_the_header`, `tide_chart_omits_delta_when_previous_is_outside_the_window`). Round-3 adds `sort_from_info_keeps_a_child_ranked_past_the_first_snapshot_page`, `sort_from_info_keeps_the_own_row_past_the_first_page`, `sort_into_growth_keeps_a_child_the_growth_cache_ranks_past_1000`, `left_from_info_reloads_the_first_page_after_a_deep_sort`, `esc_from_info_during_live_refresh_returns_to_the_first_child`, `sort_from_info_reports_a_directory_that_is_really_gone`, `header_shortens_the_path_to_the_leftover_budget`, and `pane_titles_stay_inside_the_width_and_strata_spell_one_age`. The Fable fix adds `enter_on_the_second_highlighted_link_opens_that_set`, `files_column_header_is_not_a_selectable_row`, `refresh_keeps_the_own_row_when_its_bytes_grow_past_the_children`, `refresh_keeps_a_child_on_a_later_page`, and `picker_dates_pad_so_the_total_column_lines_up`. `live_refresh_from_later_page_restores_own_row` now keeps the open child on page 2, then walks from the start and still sees `./` once. `live_reverse_boundary_keeps_own_row_for_every_sort` reloads page 0 after refresh so its reverse-page check starts there. Snapshots were not recaptured. Adapted locks in `crates/tidemark-cli/tests/w7a/browser.rs` stay (`deterministic_render_at_both_sizes`, `too_small`, `human_header_and_pane_labels_are_lowercase`, `post_tui_files_pane_fits_80_columns`, Browser/Refresh literals). Snapshots reviewed and kept: `crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_80_24.snap`, `crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_160_48.snap` (160-column header is flush; the strata bucket says `≥365d`).
- Privileged: none added by this task. `cargo xtask ci` does build `--workspace --features privileged --tests`. That build finished. Privileged runtime was not executed — status: pending VM.

## Measurements (if the task asks for any)

No new measurements. The ignored `million_directory_navigation` scale test was not run.

## Merge of origin/main (TUI commits since 150ba53)

The branch was mid-merge. Tide's layout and selection helpers stayed. Main's behaviour fixes were ported onto them.

| Commit | Resolution |
|--------|------------|
| 72214e8 own row on its forward page | `Pages.lower` look-behind stays in `load`, `turn_page`, and `own_position`. The snapshot page still uses Tide's `snapshot_sort_key`. The end-of-directory page size is main's `limit`, and a reverse page asks for one extra child so the boundary row is not the first row of the page. |
| ca8bdd5 missing-path advice | Already merged in `cmd/snapshots.rs` (`default_entry`). The browser tests came in with the conflict and were kept. |
| 21b9b40 remedy opens capped at twenty | Already merged (`REMEDY_CANDIDATE_LIMIT`). `post_tui2_remedy_search_is_capped` and `post_tui2_remedy_requires_working_default` were kept unchanged. |
| c3ce697 return movement across pages | The stronger asserts inside `post_tui_disjoint_traversals_all_sorts` were already merged: Up lands on the adjacent logical row, and paging back returns to that same row. |
| 488af41 legend in the main-pane bottom border | Dropped. Tide already draws the legend on its own line under the depth table. `post_tuititle_full_legend_at_80_columns` now requires that narrow legend once, for every sort, live on and off, and a 60-byte filter. `post_tuititle_filter_budget_preserves_status_and_wide_text` still requires the sort label, the live marker only while live, `...` at 80 columns, the full escaped filter at 160, one legend, and an unchanged filter. `depth_title` shortens the filter before the path so a long filter does not eat `sort` or `live`. |

`main_80_24` and `main_160_48` stayed on the Tide renders. `deterministic_render_at_both_sizes` passed without a snapshot rewrite. The TUI module tests were 79 passed, 1 ignored (`million_directory_navigation`).

## Known gaps and follow-ups

- Code-review sweep 3 stayed BLOCKED on the page-0 settle and the Esc offset write-back. Keystone approved these fixes without a fourth review round. This is that waiver, not a fresh code-gate pass. The Fable blockers (header-as-row and `depth_target` captured after the page changes) are covered by the tests named above. They do not replace another reviewer reading the diff.
- This round's `CARGO_BUILD_JOBS=4 cargo xtask ci` exited 0 on the first attempt (448s) and printed `ci: all available checks passed`, including `w6a2` (25 tests) and `vm_test::tests::script_suite_collects_failures_and_prints_all_summaries` in both the xtask binary and `w8`. An earlier round's first attempt was killed by the linker (`ld terminated with signal 9`) while linking `tidemarkd` test `w6a2`; that was not a Tide failure. `xtask/src/vm_test.rs` was not changed.
- The origin/main merge round chained `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -j4 -- -D warnings`, `cargo test -j4 -p tidemark-cli`, and `CARGO_BUILD_JOBS=4 cargo xtask ci`. The chain exited 0 in 743s. xtask printed `ci: all available checks passed`. `tidemark-cli` tests: lib 202 passed / 1 ignored, plus the integration suites in that command. TUI module: 79 passed / 1 ignored.
- `cargo-deny` is not installed, so `xtask ci` printed `warning: cargo-deny is not installed; skipping cargo deny check` and continued. `--require-deny` was not passed. — install `cargo-deny` when a deny gate is required.
- No commit or push from this worktree. Screenshots live in `docs/screenshots/tui-tide/` (`before/` and `after/`). The coordinator opens the pull request.
- No real terminal, tmux, or VM run in this fix round. TestBackend snapshots and cell assertions are the visual check.

## Notes for the reviewer

- `Theme::plain` is `#[cfg(test)]` so the non-test library does not fail `dead_code`.
- `on_key` updates `main_selected` only when the key starts and ends on Main. `close_pane` sets it to the restored selection. `load` updates it only when the pane is already Main. `/` from another pane copies `main_selected` back into `state.selected`. `reload_keeping_selection` takes a `DepthTarget` captured by the caller. `poll_refresh` captures it as its first statement, before `dir` or `pages` change. `boundary` and `s` capture it immediately before the helper. `page` is only reached for Main and Instances; it does not reload under another pane. `s` records the directory before it changes the sort, because the new order can move `./` and would otherwise read the old index as a different child. Settle matches the saved path only. A directory id that a later sibling inherited does not count. Rank comes from `pages.live`, or from `pages.growth` when that cache is the order `load` just built. Otherwise settle walks `children_page` in pages of 1,000 and loads the page with that keyset cursor. `./` uses the same rank. Page size stays 1,000. If the path is absent, the index is 0 and the message gains `selected directory is gone`. `s` keeps that flag. Esc, Left, and Backspace from another pane close it first, then `seek_boundary(false)` from Main, so a stale offset is not written back. Esc also clears the filter.
- The header counts ≋ (U+224B) as one column, the same correction already applied to U+00B7. `fit_header` shortens the path to the columns left after the brand is dropped, and drops the path when fewer than 8 columns remain. `depth_title`, `list_title`, and `panel_heading` clip to the pane. Panel titles leave one column on each side for the border. The strata bucket and the seabed caption share `≥365d` or `>=365d` according to the glyph set.
- Links and `f` Files still store the column title as the first line of `b.text`. `column_header` peels that line for drawing, and the Files count skips it. The Enter-on-`./` report is not a column header: its first line is the snapshot sentence.
- Basic `Text` is `Color::Reset`. Teal stays Cyan and Coral stays Red. ASCII glyphs keep Unicode borders and swap the footer, no-match, chart, and too-small `·`, `Δ`, and `×` for `-`, `d`, and `x`. The active sort marker is a teal span with `DIM` removed, appended to the dim label with no space. Picker dates use `{:<11}`.
- The Files message puts the short result before `Snapshot files; does not include changes since snapshot`. `clip_cols` then shortens the disclaimer first.
- Files rows pad SIZE to at least 8, right-aligned, and MTIME to the wider of the header and the `fmt_when` values, left-aligned.
- The tide Δ title is omitted when `previous_complete` is not in the drawn window, including when the open snapshot is the oldest of those seven bars.
- Strata and soundings never treat `own_position() == usize::MAX` as the `./` row.
- The Files block title reserves the live suffix, then clips the base, so ` · live` is not cut off at 80 columns.
- Highlight is `▶ ` (two cells). The tide arrow is not passed through `display_width` truncation, so the style test can still see the coral `▲`.
- No `unwrap` or `expect` in `crates/tidemark-cli/src/tui/`. The path-included browser tests allow those lints for the test module only.
- `Gauge` is not used. Strata and soundings draw their own spans. Ratatui `Gauge::percent` panics outside 0..=100.

## Definition of Done

- [x] Only owned paths changed (plus the shared-file edits listed above)
- [x] `cargo xtask ci` green (Fable fix exited 0 in 448s; printed `ci: all available checks passed`)
- [x] Privileged tests written and listed with status
- [x] Public items documented; `//! Owned by:` headers present
- [ ] All acceptance criteria evidenced above (code criteria are evidenced; the coordinator opens the pull request)
- [x] No unexplained TODOs, `unwrap`s or `expect`s
