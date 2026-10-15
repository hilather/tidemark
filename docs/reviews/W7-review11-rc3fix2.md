VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 1 / NOTE 0
F1: fixed — listed representatives, snapshot entry count and snapshot own charged total are reported independently; no unretained-file byte remainder is inferred.
F4: partly fixed — first-page PgUp/Up preserves `.`, but End followed by reverse paging can show it on two overlapping pages.
N1: fixed — snapshot navigation clears the live footer, and a subsequent successful refresh reinstates it.

Reviewed exact candidate `4be3a3ff00e3938175bebe21ed9e2ffda197d373` against `79da2f84954d77fe1d497cbf1ec5c0451423dbfd`. The approved checkout at `eb38a98ff2594215bb612c14bcbc5c58457d3e2e` has the identical tree, `701fbb397905fa6583e1c97b797090c2ca5c5f13`. The pinned diff excluding reviews is byte-for-byte equal to the requested `git diff 365c512..9812a71 -- ':!docs/reviews'`. The requested endpoint additionally has README/LICENSE files absent from the pinned candidate; its relevant source, tests and handoff match. Read review10 and the RC3-fix2 handoff in full, review9's original scenarios, W7-A and its required link/storage/interface contracts. This report is evidence for review, not release or integration approval. The verdict reflects the remaining requested traversal requirement, not a newly established release blocker.

1. **SHOULD-FIX — F4: End-origin reverse paging repeats the own-files row.** Reference: `finding:rc3fix2-end-reverse-own-duplicate`.

   In a refreshed directory with 2,500 children, choose Name sort with 750 names beginning `!` and 1,750 beginning `z`. The own-files row `.` has rank 750 among children. Press End, PgUp, PgUp. The candidate produces this source-derived trace (zero-based ranks):

   | Action | Offset | Child ranks in page | Own-row position |
   | --- | ---: | --- | ---: |
   | End | 1500 | 1500–2499 | Absent |
   | PgUp | 500 | 500–1499 | 250 |
   | PgUp | 0 | 0–999 | 750 |

   `crates/tidemark-cli/src/tui/mod.rs:173` sets End's offset to `count.saturating_sub(PAGE)`. Reverse paging at `:193` subtracts 1,000 with saturation, while live loading at `:90` always takes up to 1,000 rows. The new live-rank condition at `:471`–`:477` admits `.` into both overlapping slices. `draw.rs:82` inserts it on each qualifying page. Thus one traversal toward the beginning presents `.` twice. Walking upward with Up also revisits children 500–999: the boundary handler at `mod.rs:801` selects the last row after loading the overlapping first page.

   The overlapping child windows predate this candidate. The duplicate own-row exposure follows the changed rank condition: the previous reverse-cursor predicate would exclude rank 750 on the second page because its cursor is child rank 500. The new first-offset guard correctly solves review10's disappearing-row case, but does not solve overlap when arriving at offset zero from a non-page-aligned offset.

   Size, Reclaim and Files can produce the same duplicate whenever the own-row rank lies in the overlap. Newest uses the same rank branch, although its synthetic own timestamp is `i64::MIN`; with ordinary known child timestamps it sorts last and avoids this example. Equal sentinel timestamps can place it earlier via name ties. Growth uses its separate last-slice placement and does not duplicate `.` in this trace; its child slices still overlap.

   The added test at `tests/w7a/browser.rs:1354` covers five sorts, but at `:1406`–`:1415` it reaches the last page by forward paging, so its offsets are 0, 1000, 2000 and then 1000, 0. It never starts reverse traversal with End and does not cover own rank 750. Add an End-origin traversal regression and make reverse slices disjoint, or establish consistent page boundaries, while preserving own-row placement. The trace above was checked against source and independently reviewed; it was not injected into the product test suite or reproduced in an interactive terminal.

**F1 accounting evidence.** At `tui/mod.rs:578`, Enter on `.` rereads the directory from the immutable snapshot. `cmd/files.rs:74`–`:81` obtains retained rows, filters by representative `dir_id`, and refuses cap saturation rather than silently truncating direct records. The summary at `tui/mod.rs:615`–`:618` prints `hits.len()`, `snapshot_dir.own_files` and `bytes(snapshot_dir.own_charged)` separately. There is no subtraction and no branch that claims an unretained-file size. Individual representative sizes remain the snapshot's own `FileRow` values, with `[hard links]` where applicable.

| Scenario, with zero directory blocks unless stated | Listed representatives | Snapshot direct entries | Snapshot own charge supplied to formatter |
| --- | ---: | ---: | ---: |
| Review9: retained shared 8 MiB representative at `/r/a`, unretained local 1 MiB; shared LCA `/r` | 1 | 2 | 1 MiB |
| Review10: unretained shared 8 MiB plus retained local 16 MiB at `/r/a` | 1 | 2 | 16 MiB |
| Shared representative retained in `/r/a`; view `/r/b` | 0 | 1 | 0 B |
| Shared charge at `/r`, with no direct files there | 0 | 0 | 8 MiB |
| One retained local 1 MiB file plus 4 KiB directory blocks | 1 | 1 | 1,052,672 B |
| Threshold retains shared representative elsewhere and local 1 MiB at `/r/b` | 1 | 2 | 1 MiB |

These are exact field values before the existing human-readable byte formatter rounds them; no new numerical precision claim is made. The five review10 cases execute in `own_files_snapshot_totals_cover_remainder_counterexamples`; the original representative case executes in `own_files_hardlink_totals_are_separate`. Their snapshots are constructed fixtures, not fresh scanner retention runs.

Nearby cases follow the same independent-field calculation. Two local links count as two entries but at most one retained representative and one inode charge. A directory that is the LCA of descendant-only links may have zero direct entries and representatives but nonzero own charge. With an ancestor LCA, a local representative can display its full disk size without that size being charged locally. A link set entirely in another subtree contributes no charge to the viewed directory. An empty directory prints zero representatives and entries alongside its recorded directory blocks; a directory with no retained files still prints its full recorded entry and charged totals. Top-N versus threshold affects retained selection, not this calculation. Incomplete sets likewise do not turn charged bytes into a reclaimability claim.

Producer evidence agrees with `03-link-semantics.md` §§2–4 and `05-storage.md` §1: representative selection at `tidemark-scan/src/agg/linkset.rs:48`, LCA charges at `:57`, entry-instance counts at `:74`, directory blocks at `agg/mod.rs:163`, and own totals at `:543`–`:555`. Retention selection is at `agg/notable.rs:21`. The new summary and `docs/operator-guide.md:165` separate these quantities. No supported residual F1 defect was found.

**Remaining F4 and N1 checks.** For forward-aligned pages, the five non-Growth sorts now place `.` by the filtered rank and actual live slice, so it appears once at ranks zero, a page boundary, within a page, or at the end. Growth appends it only on the final slice. First-page PgUp and Up now return before installing an excluding cursor (`mod.rs:188`–`:190`). Home resets the offset/cursor. At the final page, a short page is a no-op; an attempted page after an exactly full final page restores the previous rows, offset, cursor and selection when the result is empty (`:219`–`:224`). Empty directories still expose `.` when the filter matches it. A filter that excludes `.` intentionally hides it. Refresh from any prior page resets offset, cursor and reverse state before loading, for all six sorts (`:408`–`:414`). These paths are source-consistent; End-origin reverse traversal is the exception above.

After successful refresh, `poll_refresh()` installs the live footer at `mod.rs:415`–`:419`. Entering an existing snapshot child resolves its path and calls `navigate()` (`:624`–`:627`); ascending uses the same method (`:836`). Navigation now clears both live pages and the message (`:516`–`:518`), so `draw.rs:141` cannot reuse the previous live footer. A second successful refresh takes the same installation path and restores it. The new rendered regression executes the first refresh, child navigation and return to parent; the second-refresh conclusion is source tracing, not additional test execution. A live-only child still refuses navigation with its existing explanatory message.

**Validation and limits.** Commands ran in the approved checkout with pinned Rust 1.98.0, uid 0, umask 022, using the existing Cargo cache offline. Only the review document was added to the repository; no source, tests, manifests or dependencies were changed.

| Command or check | Observed result | Evidence |
| --- | --- | --- |
| `cargo test --offline -p tidemark-cli --lib -- --nocapture` under a 900-second timeout | Exit 0; 137 passed, 0 failed, 1 scale ignored; 50.10-second test run | `library/cli-lib.log` |
| `cargo fmt --check` | Exit 0 | `library/fmt.log` |
| Pinned/requested diff equality and End arithmetic trace | Matched diff; trace shown above | `library/source-trace.txt` |
| `cargo clean --offline` | Exit 0; removed build artifacts | `library/clean.log` |

Evidence files are under this attempt's worker-output directory. The two schedule-socket tests explicitly skipped their listener bodies because bind returned EPERM; passing harness counts do not establish those bodies. The new RC3-fix2 regressions use fixed snapshots and an already-completed bounded refresh-channel seam, without wall-clock or timezone input. Their traversal loops terminate for the supplied counts of 1 and 2,500; no new normal-execution hang was found. The preexisting no-client socket test retains its monotonic deadline and five-second elapsed assertion, which severe runner starvation could fail; it was not exercised here. UTC formatting remains integer arithmetic, and fixture state directories explicitly use private permissions.

No new exit-code change, scan metadata operation, unsafe block, inspected-file content read or metadata mutation was introduced by this diff. No additional supported wrong-data or safety regression was established. Full-workspace CI, Clippy, cargo-deny, uid-1000 and GitHub Ubuntu 24.04 gates, socket-enabled execution, real-terminal reproduction and privileged/VM tests were not run. Those limits and the source-derived status of the remaining paging finding are not release acceptance evidence.
