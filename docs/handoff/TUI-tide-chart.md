# Handoff — TUI-tide-chart

- **Branch:** `muse/tui-tide-chart`; PR `hilather/tidemark#2`; unchanged HEAD `d44343c`.
- **Date:** 2026-10-16. **Status:** Mira’s six design follow-ups implemented on top of the prior code-gate fixes. Validation and new visual evidence are recorded below.
- **Scope:** `crates/tidemark-cli/src/tui/`, `crates/tidemark-cli/tests/w7a/`, `docs/screenshots/tui-tide-chart/` (PNGs only), `tools/tui-shots/`, this handoff. No commits, pushes, stashes, resets, branch changes, PR creation, or Git writes were performed.

## What changed

`history.rs` implements exact byte-path history using immutable snapshot readers on one background thread. Catalog totals remain picker/header data only. The chart uses the highlighted Main row, preserved while other panes own the list cursor. Info now follows that same directory, including its path, disk total, and title. Enter selects row zero inside the child; Backspace selects row zero in the parent under the active sort, with no selection stack.

`./` and a filter with no rows resolve to the containing directory's full `t_disk`, explicitly labelled **whole directory** in the chart. Strata retains its whole-directory label. The table and Soundings retain `own_charged`, labelled **files here**. Info's former own-files note is deliberately removed.

`draw.rs` now paints the chart directly, including navy backgrounds in gaps and labels. Heights start at zero and use `min(8H, floor(u128(v) * 8H / M))`, with `M` including the live endpoint. Every occupied top cell uses one shallow, symmetric crest across the 16-column bar: the centre reaches `f`, the shoulders are one eighth lower, and the two edge columns are at most two eighths lower. Positive caps never fall below one eighth; `f == 1` stays flat. Historical bars, including caps, are teal; only the latest endpoint is foam (opened snapshot, or live after refresh). Basic uses cyan and bold for the latest bar; Mono uses reset colors and bold. Indexed retains teal/foam roles. ASCII uses `#`, `~`, and `.` without a repeating sawtooth. No values are printed over bars.

The nine-row chart has five plot rows, two label rows, and two borders. Its box starts at column 1, bars at column 4, and bar origins are 19 columns apart. Width is always 16, gap 3. Labels are UTC `MM-DD HHh`, then existing IEC `fmt_bytes` output. The opened marker is `●` (`*`); live is `◆ live` (`+ live`). Delta has a separate title budget; the subject title includes its current disk value. A dim side tick immediately left of the latest bar marks the known comparison level on the identical scale. It occupies plot row `bottom - saturating_sub(eighths(previous), 1) / 8`; known zero uses the bottom row. The tick uses the same displayed predecessor as the delta (opened snapshot for live), and disappears for missing, unavailable or loading predecessors. A small `0` remains at the foot of the plot; the bottom-border `0 B` is removed. `Role::Wave` restores the full-width top wave and lower rule. The catalog maximum is visibly separated as **| root high-water**; when space is tight, **hw** preserves the header path before the path is dropped.

## Mira follow-up: NEWEST root cause and regression evidence

The captured `projects/` and `logs/` rows inherited the generic test helper’s `newest_mtime: 100` (100 Unix seconds after the epoch), because `series()` never replaced it. The exporter copied those rows unchanged into the visual snapshot databases. The renderer therefore correctly interpreted the bad fixture value as 1970; this was not a seconds/milliseconds/nanoseconds conversion bug or a swapped field.

The minimal root fix is in `crates/tidemark-cli/tests/w7a/tide_chart.rs::series`: set deterministic `newest_mtime` near each snapshot’s start time and a distinct, earlier directory `mtime`. The generic test helper remains unchanged for tests that deliberately use small timestamps. In `tui/draw.rs::fmt_when`, zero and the empty-maximum sentinel `i64::MIN` now render as `-`. No production fix outside `tui/` was needed.

Trace checked end to end:

- `tidemark-fs/src/walk/worker.rs` reads `stx_mtime.tv_sec`; `tidemark-scan/src/agg/mod.rs` rolls up the maximum and writes `DirRow.newest_mtime`.
- `tidemark-store/src/writer.rs` writes that field to `dirs.newest_mtime`; `reader.rs::dir_row` reads column 32 back into the same field, without conversion. The storage contract specifies Unix seconds.
- `tidemark-cli/src/live_rows.rs` returns the aggregator’s `VecSink` rows. `tui/mod.rs::start_refresh` retains whole child rows, and `poll_refresh` installs them without changing timestamps. Snapshot paging likewise retains the stored `DirRow`.
- `tui/draw.rs::data_row` passes `row.newest_mtime` directly to `fmt_when`; UTC formatting needs no timezone or unit correction.

Both new `newest_*` regressions were run before the fix and failed: the fixture round-trip returned `100` instead of `1760024600`, and zero rendered `01-01 00:00` instead of `-`. Both pass after the fix. They check saved values, distinguish subtree newest from directory mtime, verify known dates and refreshed timestamps, and test zero/missing rendering in both saved and live rows. The existing real refresh test now independently compares NEWEST with filesystem `MetadataExt::mtime()` maxima before snapshot publication and after refresh, then checks the table output. This covers actual stat/aggregation units as well as fixture wiring.

## History choices and bounds

| Resource / behavior | Implementation |
| --- | --- |
| Window | Opened scan plus six newest strictly earlier complete catalog entries for the opened root; smaller ID wins a timestamp tie during selection; both earlier equal-time entries remain eligible. Window is selected when the opened snapshot changes. |
| Display | Oldest to newest, opened scan last. At most seven bars including live. Narrow widths drop oldest slots, retaining the endpoint; 100 columns displays five bars. |
| Delta | Existing `previous_complete()` identity is unchanged. Both path samples must be known and displayed. A missing/unavailable predecessor is never substituted with zero or an older success. Live uses the opened saved sample and says `vs snapshot`. |
| Reader validation | Each file must match its own expected scan ID, timestamp, complete status, and exact root bytes. Nonempty filesystem UUIDs must match the opened snapshot UUID; empty means unknown. |
| Queries | One indexed `dir_by_path()` per required historical slot, reading `t_disk`. No directory traversal, contents scan, timeseries fallback, or inode tracking. Paths renamed/replaced are path history. |
| Seeding | Only the in-memory immutable row seeds the opened sample. After refresh, saved data comes from the worker/cache. Live data never seeds a saved slot. |
| Readers | At most seven, owned by the worker; readers outside a new active window are evicted, including when the new window is already cached. Existing browser/predecessor readers are separate. |
| Results | `Known(u64)`, `MissingPath`, `Unavailable(&'static str)`; `None` means loading. Failures retain only bounded static reasons. |
| Cache | UI-owned LRU, at most 128 paths and 1 MiB of retained keys/results, conservatively accounting for key and vector allocations. Oversized keys remain queryable but are not cached. Keys include exact root/path, ordered scan IDs/files/timestamps, and opened UUID. Live values are excluded. |
| Negative cache | Missing paths are immutable. Failed slots have a five-second cooldown stamped with `Browser.clock` when the UI applies the result. Retry preserves known/missing slots. |
| Scheduling | Changed key, visibility transition, successful refresh, or expired cooldown can issue a new generation. An unchanged active/warm key does nothing. Polling happens every loop turn, including resize turns. |
| Concurrency | One worker, one active job, one replaceable pending request, one replaceable result. Locks protect only mailbox operations; opens/queries happen outside the lock. Publication and UI application both reject stale generations. |
| Hidden chart | Below 100×34, invalidate pending work and defer lookups. At the size gate, fewer than two saved window entries report insufficient history with the subject’s current disk value, even after live refresh. The live endpoint does not count toward the saved-history gate (amendment H). Null catalog totals do not hide it. |
| Quit | Signal cancellation and notify before terminal restoration; join afterward. Cancellation is checked between database operations. Navigation never joins the worker. |

`Browser.clock` advances each event-loop turn; `Browser.now` remains stable for header age. A successful live refresh stamps its completion time, restores the Main selection by path, and then resolves history/Info. Failed or cancelled refreshes add no live endpoint. Navigation and snapshot switching clear live state. Info rebuilds only on subject/data-revision changes, outside drawing, and resolves saved annotation/error IDs by byte path. Live-only paths skip annotations, errors, and link reports. Info text and its subject/revision cache are committed only after required reads succeed. A failed rebuild becomes a browser message, keeps the frame loop running, and retries on the next frame.

Startup and picker predecessor opens now degrade to an unavailable comparison; the chosen predecessor identity remains in the catalog/window. The picker never claims a comparison exists when that open failed.

The status-line follow-up remembers the message from a failed Info rebuild and clears it on the next successful Info preparation only if that error still occupies the status line. Messages from other actions are preserved. The extended `failed_info_frame_keeps_running_and_retries_until_the_read_succeeds` regression covers recovery clearing the error, an unrelated replacement message surviving recovery, and another pane's preparation leaving the error visible.

## Shared-file edits

Only `tui/mod.rs` adds a module declaration (`mod history;`) within the owned TUI surface. No dependencies, Cargo files, store/core/scanner files, schema, CLI flags, or config keys changed. `state.rs` and terminal restoration implementation are unchanged.

## Baseline regressions: before and after

The three `reported_*` tests were written and run before any production implementation edits. They used seven distinct on-disk snapshot files with valid distinct scan IDs, a 181 GiB root, a 41.7 GiB selected child, and a 3.20 GiB nested child.

| Regression | Baseline at bb89d92 | After |
| --- | --- | --- |
| `reported_chart_total_follows_highlighted_directory` | **FAIL**: chart title/data were root history rather than `/fixture/projects`, and did not follow Down. | **PASS**: selected-path history ends at 41.7 GiB; Down changes it. |
| `reported_info_follows_highlight_and_descended_first_row` | **FAIL**: Info named the containing root. | **PASS**: 41.7 GiB selected child, 3.20 GiB first row after Enter, parent row zero after Backspace. |
| `reported_zero_axis_and_visible_wave_crest` | **FAIL**: positive `axis from` floor and no wave crest. | **PASS**: explicit zero baseline and bounded visible crest. |

The full before/after test output was reviewed locally and is not committed. The old tests asserting root-total chart behavior were replaced deliberately. `own_files_columns_and_info` now asserts full-directory Info without the misleading own-files note. The size-column test now identifies the table row explicitly because the insufficient-history state also displays the subject's size.

## Regression coverage

Added 32 test functions across the original work and this follow-up (31 behavior regressions and one opt-in fixture exporter): 11 worker/history tests and 21 browser/chart tests (one is the opt-in visual fixture exporter). Existing navigation/render coverage remains.

- Real historical path lookups survive changed directory IDs; exact byte lookup covers a depth-21 path, non-UTF-8 bytes, and controls. Missing/renamed paths, corrupt/pruned files, root/scan/time/status mismatches, and actual filesystem UUID mismatches remain gaps, not zero.
- Navigation covers arrows, Enter/Backspace, Home/End, paging beyond 1,000 children, all six sorts, filter/no-match, own-files rows, and pane/picker cursors. Existing refresh tests cover vanished selections and IDs reused by siblings.
- Window tests cover seven entries, timestamp ties, same-start/future exclusion, older opened scans, null totals, insufficient history, and width reduction. Pruned predecessor picker acceptance remains usable with no false delta.
- Live tests cover preserved saved values, changed live IDs resolving saved annotations correctly, live-only paths with reused IDs, above-history-max scaling, injected completion time, navigation clearing live data, and failed/cancelled refreshes.
- Worker barriers cover unchanged generations across 100 turns, 100 coalesced pending requests, stale publication and stale UI result rejection, delayed open without blocking navigation, cancellation/drop, hidden-chart deferral, reader eviction, LRU/byte bounds, oversized uncached paths, warm reuse, and injected five-second cooldowns. No timing sleeps are used by these tests.
- Scaling covers 0/25/50/100, all-zero/equal values, tiny positives, `u64::MAX`, heights 0/1/5, and clamping. Every eighth-height through 40 is checked for a crest that never exceeds its true height, including all one-cell bars and flat fill=1. Crests are symmetric, at most two eighths below the true top, and have exactly one local maximum run (plateaus count once), preventing the former sawtooth. Each height from 1 through 8 is also drawn through the renderer and checked cell by cell for height bounds, empty rows above, a single smooth crest when possible, and flat fill=1.
- Follow-up regressions cover a real failing Info annotation query across repeated frames and recovery without another subject/revision change, long ASCII/wide-character chart paths alongside deltas at 100/140 columns, compact header budgets using the actual right-hand text, and one saved scan plus live remaining insufficient history. Fixture own-apparent values now match the zero own-charged values.
- The previous-level tick is pinned at 100/140 columns for zero, fractional, cell-boundary and maximum levels, and for a live endpoint with a different saved baseline. Loading/missing/unavailable comparisons suppress it. Palette tests check latest-only emphasis both before and after live refresh in RGB, Indexed, Basic and Mono.
- Buffer assertions pin bar coordinates, glyph caps, foreground/background colors, current marker/bold, labels below the plot, zero baseline, and live scaling. Seven new insta snapshots cover 140×40 selections/missing/live/ASCII, 100×34, and 80×24; Basic/Indexed/Mono use style assertions or snapshots. The existing 80×24 and 160×48 snapshots remain and were reviewed deliberately.

Snapshot proposals were generated first, inspected as text/diffs, and only then individually installed as expectations. `INSTA_FORCE_PASS=1` was used only to collect proposals in one pass, never for final verification. No `cargo insta accept` command was used.

## Check results

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** on the final code, including the status-line fix. |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** on the final code with `CARGO_BUILD_JOBS=2`, including the status-line fix and extended regression. |
| `cargo test -p tidemark-cli` | **PASS** final rerun with `CARGO_BUILD_JOBS=2`, including the status-line fix: 238 library tests passed, one existing library scale test ignored; all integration and doc-test targets passed (existing integration scale exclusions retained). |
| Focused worker/browser regression runs | **PASS**, including delayed-reader navigation and visual fixtures. |
| `cargo xtask ci` | **PASSED** on the final code with `CARGO_BUILD_JOBS=2` (cargo-deny not installed locally, skipped; CI runs deny). Two earlier runs were SIGKILLed while compiling (likely OOM). |
| Privileged VM tests | Not run; no new privileged behavior introduced. Existing scale tests remain ignored by the ordinary suite. |

Mira follow-up logs: `/tmp/tidemark-mira-focused.log`, `/tmp/tidemark-mira-clippy.log`, and `/tmp/tidemark-mira-tests-final.log`. The first full follow-up run exposed two stale test expectations: zero should now be a dash, and the size-column test must select a table row rather than the new value-bearing chart title. Both were corrected and the complete required suite rerun successfully. The status-line recovery fix described above followed the final capture.

Status-line follow-up logs: `/tmp/tidemark-info-status-clippy.log`, `/tmp/tidemark-info-status-tests.log`, and `/tmp/tidemark-info-status-ci-final.log`. The recovery-clears-message assertion failed before the fix and passed afterward. This follow-up's first CI rerun additionally hit a linker SIGKILL while compiling the privileged `w4b` test target, after all workspace tests passed (`/tmp/tidemark-info-status-ci.log`; likely memory pressure). The privileged-target build passed on retry, followed by a complete successful `CARGO_BUILD_JOBS=2 cargo xtask ci` rerun on the same final code.

During the original implementation, the restricted sandbox caused existing Unix-socket client tests to fail/stall. That run was stopped using its verified process ID, and the required full command was rerun with local socket permissions. The follow-up full suite also runs with those permissions; its initial focused run was stopped by verified PID after expected assertion failures, while the existing large traversal test was still running. All new TUI tests passed inside the sandbox. No unrelated process was killed.

## Measurements

On this development machine (Linux `7.2.5-3-omarchy`, Rust 1.98.0, unoptimized test build, tiny local fixture databases):

| Case | Measured time | History database work |
| --- | ---: | --- |
| Cold worker/readers, opened sample seeded | 18.397 ms | Six opens, six indexed lookups |
| New path with warm readers | 1.500 ms | Zero opens, six indexed lookups |
| Returning to cached path | 18.611 μs | Zero opens/queries |
| Down + history preparation while an open is held at a barrier | 163.775 μs | The open remains blocked throughout input handling |

Measured by `tui::history::tests::seven_reader_bound_eviction_and_cold_warm_measurements` (cold 18.4 ms, warm readers 1.5 ms, cached path 18.6 µs) and `blocked_history_reader_does_not_block_navigation_or_relabel_old_bars` (navigation with a blocked history open: 164 µs). These measure cold **reader handles**, not a deliberately evicted OS page cache or remote/slow storage. Capture polling times include the existing 100 ms event-loop interval and are written to `$TIDEMARK_TIDE_CAPTURE_DIR/capture.json` by the capture script. No general table-navigation latency claim is made.

## Visual evidence and accepted departures

Commit-ready PNG evidence lives in [docs/screenshots/tui-tide-chart](../screenshots/tui-tide-chart/). Reproduction scripts live in [tools/tui-shots](../../tools/tui-shots/). The refreshed isolated state/tree is `/tmp/tidemark-tide-mira-review`; fixture sizes are synthetic metadata, without production-scale file allocation. Actual `tmux capture-pane -e` output and HTML intermediates are retained under that temporary fixture directory and removed from the screenshot directory after rendering.

- [Full-screen side-by-side](../screenshots/tui-tide-chart/comparison-full.png): supplied mock on the left, actual captured projects screen on the right.
- [Chart-detail side-by-side](../screenshots/tui-tide-chart/comparison-chart.png), [native chart detail](../screenshots/tui-tide-chart/after-chart.png).
- [Projects](../screenshots/tui-tide-chart/projects.png), [logs with missing paths](../screenshots/tui-tide-chart/logs-missing.png), [descent](../screenshots/tui-tide-chart/descend.png), [ascent](../screenshots/tui-tide-chart/ascend.png), [whole directory](../screenshots/tui-tide-chart/own.png), [Info](../screenshots/tui-tide-chart/info.png), [older opened snapshot](../screenshots/tui-tide-chart/older.png), [completed live refresh](../screenshots/tui-tide-chart/live.png).
- [Pruned predecessor at startup](../screenshots/tui-tide-chart/pruned-startup.png) confirms the browser stays open and shows a gap without a false delta.
- [Basic](../screenshots/tui-tide-chart/basic.png), [Indexed](../screenshots/tui-tide-chart/indexed.png), [Mono](../screenshots/tui-tide-chart/mono.png), [ASCII](../screenshots/tui-tide-chart/ascii.png), [compact](../screenshots/tui-tide-chart/compact.png).

After Mira’s follow-up, all after captures and both comparison composites were regenerated with `capture.py` and `render.py`. The supplied mock, native chart, both comparisons, live endpoint, Basic/Indexed/Mono and compact captures were visually inspected. The new crest reads as one shallow rounded hump; the historical bars stay teal, the latest endpoint is foam, the prior-level tick is visible, and the border is clear. The compact capture retains `/tmp.../tree` beside `hw 181 GiB`. The accepted differences from the mock remain selected-directory data; a zero-based scale; two label rows with IEC values; five plot rows instead of six; delta in the border title; missing/loading/live states; and explicit opened/live markers. Mira’s single-crest and latest-only tint requirements supersede the plan’s v4 repeating ripple and historical foam caps. Fixture paths, row counts, counts and ages differ from the illustrative mock; unrelated table/panel layout is retained.

Captures are truecolor UTF-8 at 140×40, plus Basic, Indexed, Mono, ASCII and 80×24. `NO_COLOR` is explicitly cleared for color captures and set for Mono; Basic/Indexed clear `COLORTERM` to exercise their actual fallbacks. The refreshed capture session completed at `2026-10-16T18:57:13Z`. Each final chart capture waits for the value-label row to contain neither loading marker (Unicode `…`, ASCII `...`). The capture script writes waits, dimensions and environment to `$TIDEMARK_TIDE_CAPTURE_DIR/capture.json`, now respecting the selected fixture directory rather than a hard-coded old destination.

Pango/Cairo renders captured cells at 16 px, 9.6 px columns and 19.5 px rows, with JetBrains Mono/DejaVu Sans Mono and no ligature shaping across cells. Fixed cell coordinates prevent text shaping from moving labels between style runs. ImageMagick only assembles/crops screenshots. Chromium could not finish the isolated local rendering run, so it is not needed by reproduction. Raw ANSI remains the authority for terminal attributes; reset colors use an explicit neutral background in the Mono rendering.

Reproduce with a fresh isolated directory (choose a new name if it already exists):

```sh
export TIDEMARK_TIDE_CAPTURE_DIR=/tmp/tidemark-tide-visual-recapture
cargo test -p tidemark-cli --lib export_visual_fixture_when_requested -- --nocapture
cargo build -p tidemark-cli --bin tidemark
python3 tools/tui-shots/capture.py
python3 tools/tui-shots/render.py
# Preserve provenance outside the repository, leaving PNGs only:
mkdir -p "$TIDEMARK_TIDE_CAPTURE_DIR/captures"
mv docs/screenshots/tui-tide-chart/*.ansi docs/screenshots/tui-tide-chart/*.html "$TIDEMARK_TIDE_CAPTURE_DIR/captures/"
mv docs/screenshots/tui-tide-chart/mock-chart.png docs/screenshots/tui-tide-chart/restored.png "$TIDEMARK_TIDE_CAPTURE_DIR/captures/"
```

The capture script creates a fresh private socket directory and closes only the tmux server it starts there; it never reuses an existing session. It uses existing `-q` because an existing throttle warning on stderr can overwrite the live TUI on this fixture filesystem. The original affected capture was kept locally, not committed; no logging behavior was changed. The real empty fixture directories measure 0 B during refresh, correctly leaving an empty labelled live endpoint; the larger-live-value case is separately covered by the buffer regression.

## Limits and reviewer notes

- `R` remains live refresh, not catalog discovery. The catalog has its existing session lifetime; newly published scans require reopening the browser.
- Snapshot open/query calls are not interruptible. Input stays independent of the history worker; cancellation takes effect between operations and cleanup can wait after terminal restoration.
- Existing UI-thread `dir_by_path()` calls in growth sorting and `deltas()` are unchanged. Info metadata preparation is also outside drawing but on the UI thread. This work makes no responsiveness claim for those operations or the existing million-directory scale gate.
- Cache success/absence is immutable-snapshot data. Pruning a file does not invalidate an already cached numeric sample. Failure reasons are deliberately short, with retry rather than an unbounded error history.
- The mock comparison is terminal-cell rendering, not a screenshot of a specific user's physical terminal/font rasterizer. ASCII fallback is chart-local; existing borders in other panes remain unchanged.
- No new privileged tests were warranted: scanning, store schemas, and deployment behavior were not changed.

## Requested PR description for later publication

Target `hilather/tidemark:main`; Keystone reviews, Helm merges. No PR was created during this no-Git-write implementation task. The requested model attribution below is supplied by the task instructions.

> The tide chart now follows the highlighted directory by exact path across saved snapshots, starts at zero, and renders bounded wave crests with separate labels. Live values have their own endpoint and snapshot baseline. Info follows the selected row; missing older snapshots degrade gracefully instead of closing the TUI.
>
> Codex (gpt-6-astra, xhigh) was used because Matt explicitly asked for it (12:02 ET Oct 16). The round-3 skeptic fixes and the wave amendment were folded without a fourth plan round, with Keystone's approval; the code gate is the check. The approved departures are selected-row Info and graceful handling of missing older snapshots.
>
> — Muse
