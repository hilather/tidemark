# Handoff — RC3-fix2: own-files totals, live paging, footer

## Needs from coordinator

- Run uid-1000, umask-022, socket-enabled and GitHub Ubuntu 24.04 gates. This worker is uid 0, umask 022, and socket binding is denied. Passing harness counts do not establish skipped socket bodies.
- cargo-deny is not installed; the CI wrapper warned and skipped its audit. Run the audit in a prepared gate.
- Privileged execution remains pending VM; this worker builds privileged tests only.
- Independent verification/integration remains with the coordinator. No contract change, dependency, manifest or shared module edit is needed.

## Changes and regression evidence

- **F1:** Removed every remainder subtraction, including the conservative hard-link branch. Enter on `.` now reports retained representative count, snapshot direct file-entry count, and snapshot own charged total separately. Multi-link rows keep `[hard links]`. Updated operator-guide wording and older assertions that required the obsolete remainder.
  - `own_files_snapshot_totals_cover_remainder_counterexamples` failed before the implementation change for all five cases: unretained 8 MiB shared inode alongside retained 16 MiB file; representative filed in the sibling; shared charge at `/r` with no direct files; retained file plus 4 KiB directory blocks; threshold retention of both candidates with the shared representative outside `/r/b`. It now passes, checking both displayed counts and the charged total and rejecting `not individually retained` entirely.
  - Existing shared-representative coverage is retained as `own_files_hardlink_totals_are_separate`. `own_files_after_refresh_use_snapshot_observation` still checks snapshot names, counts, timestamp, title and footer after live refresh.
  - Evidence: `library/f1-before.log`, `library/f1-after.log`; final workspace/CI tests cover strengthened fixtures with link sets/instances and internally consistent charged/file totals.
- **F4:** Reverse paging at offset zero returns without installing a cursor. Live own-row placement uses the sorted, filtered live rank and actual offset slice rather than the snapshot keyset cursor.
  - `live_reverse_boundary_keeps_own_row_for_every_sort` failed before the change for Size, Reclaim, Files, Newest and Name, with `!` names before `.`. The one-child case lost `.` after either boundary action; multi-page cases installed an obsolete cursor. It now passes for all five sorts, checking the one-child case and 2,500 children with own-row ranks 0, 1,000, 1,250 and 2,500, forward and reverse. It invokes the same `page(true)` action used by PgUp and Up at the first selection.
  - Evidence: `library/f4-before.log`, `library/f4-after.log`, final workspace/CI logs.
- **N1:** `navigate` clears the previous message when it discards live pages and loads snapshot rows.
  - `navigation_after_refresh_clears_live_footer` failed before the change after refresh → Enter child. It now checks rendered Main footer after entering the child and returning to its snapshot parent.
  - Evidence: `library/n1-before.log`, final workspace/CI logs.

## Exact new own-files summary

```text
<listed> retained representatives listed; <own_files> file entries here
charged to this directory: <formatted own_charged>
(includes directory blocks and hard-linked files whose links share this directory as common ancestor)
```

No unlisted count or byte remainder is inferred. Counts describe different snapshot quantities, avoiding an unsupported one-to-one correspondence between representatives and entries.

## Assumptions

- Metadata-only hand-built snapshots isolate display/accounting behavior from host filesystem block allocation. Retention selection itself is established by the scanner contract; these tests supply its resulting notable rows (top-N or threshold), rather than retesting scanner selection.
- Completed live results use the existing bounded refresh channel seam; no sockets or real metadata scan is needed for the new regressions.
- Clearing a message on successful snapshot navigation is appropriate because any previous live status belongs to the directory being left.
- Growth keeps its existing end-of-slice placement. The offset-zero no-op guard also applies to its offset paging.
- Only owned paths changed; no dependencies, manifest edits, unsafe code, scanner syscalls or privileged runtime tests were added.

## Validation

| Command | Observed result | Log |
| --- | --- | --- |
| `cargo fmt --check` | Passed | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed | `clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | Exit 0; 995 passed, 0 failed, 14 ignored across 87 harness summaries (including nested child summaries). CLI library: 137 passed, 1 ignored. | `tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | Passed; build only, pending VM execution | `privileged.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Passed | `doc.log` |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | Exit 0; all available checks passed. 997 passed, 0 failed, 14 ignored across 88 harness summaries, including nested children and strict-feature checks. cargo-deny absent: warned/skipped. | `ci.log` |

An initial CLI full run exposed one older assertion still demanding the removed remainder wording; it was corrected before the final workspace run. `cli-overlapping-runs.log` contains output from overlapping early CLI runs and is not final validation evidence. Final validation uses the separate workspace and CI logs.

Evidence logs reside in this attempt's worker-output `library/` directory. All long commands use `timeout 1500`, Cargo host cache and pinned Rust 1.98.0. No privileged test execution, service operation or filesystem administration was performed.

## Completion evidence

The code commits are `dea1373` (F1 first) and `2df687a` (F4/N1 and expanded regression fixtures). All changes are in owned paths. The final handoff/report commit, `cargo clean --offline`, clean repository status and submission receipt are recorded in worker-output evidence. Submission is an evidence candidate for coordinator review; external gates above remain outstanding.
