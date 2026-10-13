# Handoff — W7-A2-tui: complete the snapshot browser

## Needs from coordinator

The million-child SQL page measurements exceed the W7-A 100 ms responsiveness target. The shared reader explicitly has no index covering its requested sort orders. Please coordinate an indexed store/schema solution and repeat the performance gate; this task does not change the reader or frozen schema. Page queries, Home/End, and filter queries are synchronous, so slow SQL also delays input during those operations. Ordinary within-page selection is fast. Independent review and integration remain pending.

## What changed

The browser now uses `children_page` keysets for size, reclaim, files, newest and raw-name ordering, reverse paging, and whole-directory Home/End. Filtering is a literal UTF-8 byte substring of the child name, passed to the SQL query. It is applied to the entire directory. Each ordinary page holds at most 1,000 children. Growth loads all children once, obtains previous-snapshot path deltas, and sorts a cache capped at 10,000 children. The first attempt above that cap keeps the current sort and displays a message; the next sort press advances to name order.

`R` runs the existing shared gated metadata rescan on one worker, with a capacity-one result channel and an atomic cancellation flag. The input loop polls results without waiting for the scan. Esc requests cancellation, including during filter editing. Navigation requests cancellation and discards the cancelled result without joining in the input handler. The worker is joined after completion or on browser teardown; teardown restores the terminal before joining. The worker extracts direct children, computes snapshot-relative display deltas and previous-snapshot growth keys (at most 10,000), and orders them before sending its result. Growth ordering keeps these keys separate from displayed live deltas. Live direct-child rows are retained with those deltas, marked `live` in the pane title, and paged in memory. Retained live children have an explicit 1,000,000-row ceiling; the shared helper's complete VecSink allocation remains its existing implementation. A live-only directory absent from the snapshot displays an explanatory message when descent is attempted.

Link-instance drill-in now uses `link_instances_page`, including reverse pages and whole-pane Home/End, and preserves unresolved instances. Enter navigates to the stored containing directory. Own-files rows use charged bytes for SIZE and share, own file counts for FILES, and four unavailable-value dashes for RECLAIM, SHARED, NEWEST and delta. Selecting that row before `i` adds the required own-files explanation. Names, paths, filter text and picker identifiers use core escaping.

## What was kept or changed from W7-A

Kept the pure navigation/order state, separate terminal/input adapter and side-effect-free ratatui drawing, terminal restoration guard, stored metadata panes, snapshot picker, injected rendering time, and existing command surface. Replaced preorder ID/history paging and page-local filtering/sorting with the shared store APIs. Removed the private-helper limitation messages and the first-1,000 link-instance truncation. Kept original source ownership headers where existing files already belonged to W7-A. No shared files, dependencies, contracts, reader code or live-helper code changed.

## Assumptions

- Filter input is literal UTF-8, not an escape-sequence parser; the query operates on those bytes. Hostile stored bytes remain escaped for display.
- Own-files remains an additional row on every child page, so End selects it after the final child.
- A refused growth selection does not permanently prevent reaching name order: a subsequent `s` skips the unavailable option.
- A snapshot browser descends only into stored directories; new live-only directories require a new snapshot for full panes.
- Existing shared scanner/mount tests cover the filesystem protections. No privileged syscall or scanner implementation was added here; actual terminal/SSH and VM execution remain pending.

## Acceptance evidence

The focused browser tests cover all 2,500 children across pages for all six sort modes; a second 2,500-child fixture uses different metric permutations, previous-snapshot deltas and a filter matching more than one page. Tests verify Home/End, reverse paging, nested subtree exclusion, growth allowed at 10,000 and refused at 10,001, own-files columns/info, 2,500 link instances forward/backward/end, hostile control/invalid-UTF-8/300-byte names in every pane, and worker refresh after adding a 1 MiB fixture file. Live disk values match the independent `du` oracle; cancellation is asserted through the shared flag. The existing nonterminal command test and terminal restoration tests are retained.

Updated stored TestBackend snapshots:

- `crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_80_24.snap`
- `crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_160_48.snap`

The ignored million-directory test generates exactly 1,000,000 direct children by default and prints load-page, navigation-step and next-page timings. It records measurements rather than claiming the SQL performance gate passed.

## Validation

Final results are recorded below. All Cargo commands use the host Rust 1.98.0 toolchain, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, offline networking, and `timeout 1500`. Transcripts are under this attempt's output `library/`. Initial development found two test expectation errors (literal byte substring matches additional padded numeric names, and sizes use the core formatter) and one needless-borrow Clippy warning; these were corrected before the final checks.

## Remaining uncertainty

The 100 ms performance acceptance is unresolved as described at the top. In-memory sorting after changing the sort of retained live rows runs in the UI adapter; very large live results have not been benchmarked. Initial live ordering and snapshot comparisons run on the worker. No real terminal, signal-over-SSH, or privileged VM runtime verification was available. The shared live helper's internal full-subtree memory and aggregation cancellation limits remain those documented in W7-A2-live. This handoff is an evidence candidate, not independent verification.

## Final command results

| Check | Observed final result |
|------|----------------------|
| `cargo fmt --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --workspace --no-fail-fast` | Passed: summed summaries 710 passed, 0 failed, 14 ignored (including subprocess-child summaries) |
| `cargo build --workspace --features privileged --tests` | Passed; runtime pending VM |
| `tools/ci.sh` | Passed all available checks: summed summaries 710 passed, 0 failed, 14 ignored; cargo-deny missing and explicitly skipped |
| Focused TUI suite | 19 passed, 0 failed, 1 ignored scale test |
| Million-child ignored test | Executed: 1 passed, 56.29 seconds total including fixture generation; performance threshold not asserted |

All five previously retained daemon readiness failures passed in this sandbox in both final workspace and CI runs. No out-of-scope daemon fixes were made. Final transcripts have the `final-` prefix in the output library.

## Final measurements

Default fixture: one root and 1,000,000 direct children. Ten load and forward-page samples, debug build, x86_64 Intel Core i7-8750H at 2.20 GHz, 12 logical CPUs. Load-page latency: 995.134–1946.820 ms. Forward-page latency: 1363.233–2356.950 ms. Within-page state navigation: 0.007–0.016 ms. Navigation timing excludes drawing and previous-snapshot queries; page timing includes the query and adapter load. No cold-cache or very-large-live-result claim is made. This does not meet a 100 ms target for page navigation.

## Definition of Done evidence

- Owned paths only; no shared-file edits or new dependencies.
- Format and workspace Clippy clean; workspace and CI tests passed with counts above.
- Feature-enabled tests compile; privileged and real-terminal runtime pending VM/manual checks.
- Functional tests and stored render snapshots updated; no unsafe code or production unwrap/expect added.
- Performance acceptance remains open under Needs from coordinator.
- Handoff and external evidence report written; submission remains an evidence candidate.
