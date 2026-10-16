# Needs from coordinator

- Run the uid 1000, umask 022, socket-enabled gate and Ubuntu 24.04 CI; this worker provides uid 0 sandbox evidence only.
- Track the existing deploy readiness-file race observed once in the CI repeat (details below); a focused retry and the next CI library run both passed it.
- Privileged runtime suites remain pending VM; this attempt builds them only.
- Review the submitted candidate and local evidence independently; this handoff does not claim release approval.

# POST-tuititle — complete main-pane legend at 80 columns

## What changed

The main pane now places `Name Share SIZE APPARENT RECLAIM SHARED FILES NEWEST Δ` on its bottom border. The top border retains pane, sort, filter and the complete ` | live (Δ snapshot)` suffix. The escaped filter display uses the existing conservative display-width budget and token-preserving middle shortening (`...`). The underlying filter is unchanged. At wider widths it is displayed in full when it fits.

No interior row is consumed: list geometry, row formatting, selection indices, keyboard handlers and page sizes are unchanged. Other panes keep their existing title path, including the files-pane snapshot id and timestamp. No dependencies, manifests, shared files or contracts changed.

## Reproduction and regression evidence

`post_tuititle_full_legend_at_80_columns` uses the existing ratatui TestBackend helper at 80×24. It renders all six sorts (including name), snapshot/live pages and empty/60-byte filters: 24 cases, each requiring exactly one complete legend. Before the drawing change it failed in 22 of 24 render cases, reporting all reclaim/files/newest/growth combinations (and other overflowing states): `0 passed; 1 failed`. See `library/before.log`. After the drawing change the initial targeted run passed (`library/after.log`).

`post_tuititle_filter_budget_preserves_status_and_wide_text` also covers all sorts, live/snapshot pages, long ASCII and Unicode/escaped filters, editing on/off, and 80/160-column terminals (96 render cases). It asserts complete sort/status, visible truncation at 80, full filter text at 160, exactly one legend and an unchanged underlying filter.

The existing `post_tui_disjoint_traversals_all_sorts`, `post_tui_files_pane_fits_80_columns`, and deterministic 80×24/160×48 snapshot tests are retained.

## Before/after output

Before (80-column reclaim with empty filter): the top border ends `FILES NEWES┐`, losing the delta legend. After, the top border contains `main | reclaim | filter ` and the bottom border always contains the complete legend. Live status occupies the top border alongside a shortened filter when necessary.

Exact committed snapshot diff (only the two border lines change at each size; all row positions and contents remain identical):

```diff
diff --git a/crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_160_48.snap b/crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_160_48.snap
index 6856e06..ae305ed 100644
--- a/crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_160_48.snap
+++ b/crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_160_48.snap
@@ -6,7 +6,7 @@ expression: first
 │20260101T000000Z-00000000 walk age 99s degraded=false                                                                                                         │
 │/fixture                                                                                                                                                      │
 └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
-┌main | size | filter  | Name Share SIZE APPARENT RECLAIM SHARED FILES NEWEST Δ────────────────────────────────────────────────────────────────────────────────┐
+┌main | size | filter ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
 │> 00000001/H                                                                             #     4.00 KiB 4.00 KiB 2.00 KiB 2.00 KiB      1        100 -        │
 │  00000002/H                                                                             #     4.00 KiB 4.00 KiB 2.00 KiB 2.00 KiB      1        100 -        │
 │  00000003/H                                                                             #     4.00 KiB 4.00 KiB 2.00 KiB 2.00 KiB      1        100 -        │
@@ -45,7 +45,7 @@ expression: first
 │                                                                                                                                                              │
 │                                                                                                                                                              │
 │                                                                                                                                                              │
-└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
+└Name Share SIZE APPARENT RECLAIM SHARED FILES NEWEST Δ────────────────────────────────────────────────────────────────────────────────────────────────────────┘
 ┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
 │                                                                                                                                                              │
 │q quit | ? help | PgDn next page | read-only                                                                                                                  │
diff --git a/crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_80_24.snap b/crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_80_24.snap
index ff5337d..eba952d 100644
--- a/crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_80_24.snap
+++ b/crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_80_24.snap
@@ -6,7 +6,7 @@ expression: first
 │20260101T000000Z-00000000 walk age 99s degraded=false                         │
 │/fixture                                                                      │
 └──────────────────────────────────────────────────────────────────────────────┘
-┌main | size | filter  | Name Share SIZE APPARENT RECLAIM SHARED FILES NEWEST Δ┐
+┌main | size | filter ─────────────────────────────────────────────────────────┐
 │> 00000001/H   #     4.00 KiB 4.00 KiB 2.00 KiB 2.00 KiB      1        100 -  │
 │  00000002/H   #     4.00 KiB 4.00 KiB 2.00 KiB 2.00 KiB      1        100 -  │
 │  00000003/H   #     4.00 KiB 4.00 KiB 2.00 KiB 2.00 KiB      1        100 -  │
@@ -21,7 +21,7 @@ expression: first
 │                                                                              │
 │                                                                              │
 │                                                                              │
-└──────────────────────────────────────────────────────────────────────────────┘
+└Name Share SIZE APPARENT RECLAIM SHARED FILES NEWEST Δ────────────────────────┘
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │                                                                              │
 │q quit | ? help | PgDn next page | read-only                                  │
```

## Assumptions

- A fixed legend on the existing bottom border is an acceptable presentation choice under the task's design freedom. It avoids reducing the selectable viewport or changing paging.
- `...` is the visible shortening marker, consistent with the existing renderer; escaped byte tokens are retained whole.
- Existing fake-clock fixtures and completed-refresh channel seams provide deterministic live-state coverage without starting a real scanner or adding timing assertions.
- Task-specific scope/branch instructions override the generic wave branch convention. Work stays in the supplied attempt branch.

## Validation

Each long command is bounded by `timeout 1500`; Cargo uses the host cache offline with pinned Rust 1.98.0 (`CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`). No privileged suites are executed. The initial PATH/rustup environment could not locate the installed toolchain; using the pinned host toolchain resolved this before reproduction.

| Command | Result | Evidence |
| --- | --- | --- |
| `cargo fmt --check` | exit 0 | `library/fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 0 | `library/tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0, build only | `library/privileged.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `library/docs.log` |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | first exit 1 (existing deploy race); retry exit 0 | `library/ci.log`, `library/ci-retry.log` |

Workspace harness summaries total 1,017 passed, 0 failed and 14 ignored across 87 summaries (includes subprocess output; not a unique-test count). CLI library: 154 passed, 0 failed, 1 scale ignored. The final targeted title run reports 2 passed, 0 failed (`library/title-tests-final.log`). The new tests cover 24 legend-only and 96 status/filter render cases.

The first run of the additional Unicode display assertion failed because the shared TestBackend screen helper emits a blank continuation cell after each double-width character. The expected screen text now includes those cells; the final test and full workspace run pass. No renderer fix was needed for that assertion correction.

No privileged runtime test was added for this drawing-only change. Existing privileged suites compile; runtime validation remains pending VM. The uid 1000/socket-enabled and Ubuntu gates remain for the coordinator. CI retry exited 0 with `ci: all available checks passed`; cargo-deny is not installed, so its audit was skipped and remains for an equipped coordinator host.


## CI repeat observation

The first CI wrapper run repeated the CLI library and hit an existing out-of-scope deploy test race: `deploy::tests::purge_refuses_live_writer_without_changing_bytes_or_inodes`, `deploy/tests.rs:1453`, compared bytes `ready` with an earlier empty readiness-file read. The standalone workspace run passed that test. A focused retry using the identical CI executable also passed (1 passed, 0 failed; `library/deploy-retry.log`). The child creates/writes `ready`, while the parent waits only for existence before reading all files; the observed failure is consistent with reading that file before its write completes. No deploy code was changed. Original CI output is retained in `library/ci.log`; the wrapper retry exited 0 with all available checks passing (`library/ci-retry.log`).


CI retry harness totals: 1019 passed, 0 failed, 14 ignored across 88 summaries (includes subprocess output and the two strict-ancestry tests). Source policy reports 150 owner tags and a passing forbidden-symbol scan. The retry also repeated formatting, workspace Clippy, the complete workspace tests and privileged build.

## Final scope and remaining uncertainty

Only `src/tui/draw.rs`, `tests/w7a/browser.rs`, the two main-pane snapshots, and this handoff changed. No shared-file edits, new dependencies, unsafe code, schema/CLI/config changes or privileged runtime actions. All requested local commands completed, with the first CI race and successful retry preserved. Independent verification, cargo-deny, uid 1000/socket-enabled, Ubuntu and VM runtime gates remain outside this worker evidence.
