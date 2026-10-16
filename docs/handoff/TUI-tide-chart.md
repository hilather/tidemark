# Handoff — TUI-tide-chart

- **Branch:** `muse/tui-tide-chart`; unchanged HEAD `bb89d9282a090e037ace4bed75c2be8ebc1941a4`.
- **Date:** 2026-10-16. **Status:** implemented; code-gate round 1 passed without blockers, and its seven scoped nits are addressed. Final validation results below.
- **Scope:** `crates/tidemark-cli/src/tui/`, `crates/tidemark-cli/tests/w7a/`, `docs/screenshots/tui-tide-chart/`, this handoff. No commits, pushes, stashes, resets, branch changes, PR creation, or Git writes were performed.

## What changed

`history.rs` implements exact byte-path history using immutable snapshot readers on one background thread. Catalog totals remain picker/header data only. The chart uses the highlighted Main row, preserved while other panes own the list cursor. Info now follows that same directory, including its path, disk total, and title. Enter selects row zero inside the child; Backspace selects row zero in the parent under the active sort, with no selection stack.

`./` and a filter with no rows resolve to the containing directory's full `t_disk`, explicitly labelled **whole directory** in the chart. Strata retains its whole-directory label. The table and Soundings retain `own_charged`, labelled **files here**. Info's former own-files note is deliberately removed.

`draw.rs` now paints the chart directly, including navy backgrounds in gaps and labels. Heights start at zero and use `min(8H, floor(u128(v) * 8H / M))`, with `M` including the live endpoint. Every occupied top cell uses the v4 ripple `[f, max(1,f-1), max(1,f-2), max(1,f-1)]`; its tallest columns equal the measured height. Historical bodies are teal, historical crests foam, and the opened/live bars foam. The current crest is bold. Basic/Mono retain the ripple without its tint; ASCII uses `#`, `~`, and `.`. No values are printed over bars.

The nine-row chart has five plot rows, two label rows, and two borders. Its box starts at column 1, bars at column 4, and bar origins are 19 columns apart. Width is always 16, gap 3. Labels are UTC `MM-DD HHh`, then existing IEC `fmt_bytes` output. The opened marker is `●` (`*`); live is `◆ live` (`+ live`). Delta has a separate title budget. `Role::Wave` restores the full-width top wave and lower rule. The catalog maximum is explicitly **root high-water**; when space is tight, **hw** preserves the header path before the path is dropped.

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

Added 28 test functions (27 behavior regressions and one opt-in fixture exporter): 11 worker/history tests and 17 browser/chart tests (one is the opt-in visual fixture exporter). Existing navigation/render coverage remains.

- Real historical path lookups survive changed directory IDs; exact byte lookup covers a depth-21 path, non-UTF-8 bytes, and controls. Missing/renamed paths, corrupt/pruned files, root/scan/time/status mismatches, and actual filesystem UUID mismatches remain gaps, not zero.
- Navigation covers arrows, Enter/Backspace, Home/End, paging beyond 1,000 children, all six sorts, filter/no-match, own-files rows, and pane/picker cursors. Existing refresh tests cover vanished selections and IDs reused by siblings.
- Window tests cover seven entries, timestamp ties, same-start/future exclusion, older opened scans, null totals, insufficient history, and width reduction. Pruned predecessor picker acceptance remains usable with no false delta.
- Live tests cover preserved saved values, changed live IDs resolving saved annotations correctly, live-only paths with reused IDs, above-history-max scaling, injected completion time, navigation clearing live data, and failed/cancelled refreshes.
- Worker barriers cover unchanged generations across 100 turns, 100 coalesced pending requests, stale publication and stale UI result rejection, delayed open without blocking navigation, cancellation/drop, hidden-chart deferral, reader eviction, LRU/byte bounds, oversized uncached paths, warm reuse, and injected five-second cooldowns. No timing sleeps are used by these tests.
- Scaling covers 0/25/50/100, all-zero/equal values, tiny positives, `u64::MAX`, heights 0/1/5, and clamping. Every eighth-height through 40 is checked for a crest that never exceeds its true height, including all one-cell bars and flat fill=1. Each height from 1 through 8 is also drawn through the renderer and checked cell by cell for height bounds, empty rows above, a visible ripple when possible, and flat fill=1.
- Follow-up regressions cover a real failing Info annotation query across repeated frames and recovery without another subject/revision change, long ASCII/wide-character chart paths alongside deltas at 100/140 columns, compact header budgets using the actual right-hand text, and one saved scan plus live remaining insufficient history. Fixture own-apparent values now match the zero own-charged values.
- Buffer assertions pin bar coordinates, glyph caps, foreground/background colors, current marker/bold, labels below the plot, zero baseline, and live scaling. Seven new insta snapshots cover 140×40 selections/missing/live/ASCII, 100×34, and 80×24; Basic/Indexed/Mono use style assertions or snapshots. The existing 80×24 and 160×48 snapshots remain and were reviewed deliberately.

Snapshot proposals were generated first, inspected as text/diffs, and only then individually installed as expectations. `INSTA_FORCE_PASS=1` was used only to collect proposals in one pass, never for final verification. No `cargo insta accept` command was used.

## Check results

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** after the follow-up fixes. |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** after the follow-up fixes, including the final tests. |
| `cargo test -p tidemark-cli` | **PASS** after the follow-up fixes: 234 library tests passed, one existing library scale test ignored; all integration and doc-test targets passed. |
| Focused worker/browser regression runs | **PASS**, including delayed-reader navigation and visual fixtures. |
| `cargo xtask ci` | **NOT RUN**, explicitly reserved for Matt. |
| Privileged VM tests | Not run; no new privileged behavior introduced. Existing scale tests remain ignored by the ordinary suite. |

During the original implementation, the restricted sandbox caused existing Unix-socket client tests to fail/stall. That run was stopped using its verified process ID, and the required full command was rerun with local socket permissions. The follow-up full suite also runs with those permissions; its initial focused run was stopped by verified PID after expected assertion failures, while the existing large traversal test was still running. All new TUI tests passed inside the sandbox. No unrelated process was killed.

## Measurements

On this development machine (Linux `7.2.5-3-omarchy`, Rust 1.98.0, unoptimized test build, tiny local fixture databases):

| Case | Measured time | History database work |
| --- | ---: | --- |
| Cold worker/readers, opened sample seeded | 18.397 ms | Six opens, six indexed lookups |
| New path with warm readers | 1.500 ms | Zero opens, six indexed lookups |
| Returning to cached path | 18.611 μs | Zero opens/queries |
| Down + history preparation while an open is held at a barrier | 163.775 μs | The open remains blocked throughout input handling |

Measured by `tui::history::tests::seven_reader_bound_eviction_and_cold_warm_measurements` (cold 18.4 ms, warm readers 1.5 ms, cached path 18.6 µs) and `blocked_history_reader_does_not_block_navigation_or_relabel_old_bars` (navigation with a blocked history open: 164 µs). These measure cold **reader handles**, not a deliberately evicted OS page cache or remote/slow storage. Capture polling times include the existing 100 ms event-loop interval and are written to `/tmp/tidemark-tide-visual/capture.json` by the capture script. No general table-navigation latency claim is made.

## Visual evidence and accepted departures

Evidence lives in [docs/screenshots/tui-tide-chart](../screenshots/tui-tide-chart/), including the capture/render scripts and logs. It contains actual `tmux capture-pane -e` output, PNGs, HTML companions, and reproducible capture/render scripts. The refreshed isolated state/tree is `/tmp/tidemark-tide-nits`; fixture sizes are synthetic metadata, without production-scale file allocation.

- [Full-screen side-by-side](../screenshots/tui-tide-chart/comparison-full.png): supplied mock on the left, actual captured projects screen on the right.
- [Chart-detail side-by-side](../screenshots/tui-tide-chart/comparison-chart.png), [native chart detail](../screenshots/tui-tide-chart/after-chart.png).
- [Projects](../screenshots/tui-tide-chart/projects.png), [logs with missing paths](../screenshots/tui-tide-chart/logs-missing.png), [descent](../screenshots/tui-tide-chart/descend.png), [ascent](../screenshots/tui-tide-chart/ascend.png), [whole directory](../screenshots/tui-tide-chart/own.png), [Info](../screenshots/tui-tide-chart/info.png), [older opened snapshot](../screenshots/tui-tide-chart/older.png), [completed live refresh](../screenshots/tui-tide-chart/live.png).
- [Pruned predecessor at startup](../screenshots/tui-tide-chart/pruned-startup.png) confirms the browser stays open and shows a gap without a false delta.
- [Mono](../screenshots/tui-tide-chart/mono.png), [ASCII](../screenshots/tui-tide-chart/ascii.png), [compact](../screenshots/tui-tide-chart/compact.png).

After the code-gate nits, all final after captures and both comparison composites were regenerated. The supplied mock, composite, and native chart were inspected for palette, border, spacing, fill, crest bounds, labels, and current emphasis. Colors and bar geometry match the required roles/coordinates. The explicit v4 foam ripple is clearly visible within the top occupied cell in both refreshed composites, reviewed at native resolution. The compact capture retains `/tmp.../tree` beside `hw 181 GiB`; the root and descended `./` rows show `0 B` apparent beside `0 B` disk. The accepted differences are selected-directory data; a zero-based scale; two label rows with IEC values; five plot rows instead of six; delta in the border title; missing/loading/live states; the opened `●`/`*` marker; and a separate sand `◆ live`/`+ live` label. The v4 crest tint overrides v3's single-color cap. Fixture paths, number of visible rows, counts, and ages differ from the illustrative mock; unrelated table/panel layout is retained.

Captures are truecolor UTF-8 at 140×40, plus Mono/ASCII and 80×24. `NO_COLOR` is explicitly cleared for color captures and set for Mono. The refreshed capture session completed at `2026-10-16T18:21:09Z`. Each final chart capture waits for the value-label row to contain neither loading marker (Unicode `…`, ASCII `...`); the capture script writes waits and dimensions to `/tmp/tidemark-tide-visual/capture.json`. 

Pango/Cairo renders captured cells at 16 px, 9.6 px columns and 19.5 px rows, with JetBrains Mono/DejaVu Sans Mono and no ligature shaping across cells. Fixed cell coordinates prevent text shaping from moving labels between style runs. ImageMagick only assembles/crops screenshots. Chromium could not finish the isolated local rendering run, so it is not needed by reproduction. Raw ANSI remains the authority for terminal attributes; reset colors use an explicit neutral background in the Mono rendering.

Reproduce with a fresh isolated directory (choose a new name if it already exists):

```sh
export TIDEMARK_TIDE_CAPTURE_DIR=/tmp/tidemark-tide-visual-recapture
cargo test -p tidemark-cli --lib export_visual_fixture_when_requested -- --nocapture
cargo build -p tidemark-cli --bin tidemark
python3 tools/tui-shots/capture.py
python3 tools/tui-shots/render.py
# intermediate .ansi/.html files land next to the PNGs; only the PNGs are committed
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
