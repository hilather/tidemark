VERDICT: BLOCKED
BLOCKER 1 / SHOULD-FIX 1 / NOTE 1

F1: partly fixed — a retained direct multi-link representative now makes the remainder unknown, but absent representatives still permit understated bytes, and directory/LCA charges are still presented as unretained-file bytes.
F2: fixed — own-file names, counts and bytes now come from one snapshot and are labelled accordingly, including after refresh and navigation; the preexisting Main-footer qualification is N1 below.
F3: fixed — the second active probe catches activating → active between the original probes; stable failed/inactive and probe errors cannot produce a healthy result.
F4: partly fixed — refresh clears stale snapshot pagination for every sort, but reverse paging at the first live page can still hide the own-files row.
F5: fixed — changed-config recipes use --start-now and explain immediate restart versus deferred application.
F6: fixed — missing-path wording identifies only the selected snapshot and its actual complete/partial status.
F7: fixed — upgrade guidance distinguishes omitted/explicit destinations and describes stale files and freshness checks.
F8: fixed — acceptance is nonblocking with a deadline, followed by bounded reads/writes; execution of the socket bodies remains unestablished in this sandbox.

Reviewed exact candidate `98eb364b25e6f11f2ad4be6dedd3ef7d73f95da1` against `787c55efc217dc32de57823b3671e5b841a3b790`, and the requested `5e18fd4..365c512` diff excluding `docs/reviews`. The approved checkout at `591babe15d5d7c5a7035b3f5261e32fc7444f1cd` has the exact candidate tree (`fe46dd8b302d5a6a9ea7156755fb090d92dda158`). The requested `365c512` tree additionally contains root README/LICENSE files absent from the pinned candidate; the fixes, tests and operator guide match. Read the previous review and RC3-fix handoff in full, together with the normative link accounting/storage contracts and TUI task/interface text. This report is review evidence, not release or integration approval.

1. **BLOCKER — F1: the numeric branch still does not describe unretained file sizes.** Reference: `finding:rc3fix-remainder`.

   `crates/tidemark-cli/src/tui/mod.rs:574` detects hard links only among retained representatives returned for this directory. `cmd/files.rs:80` filters by representative `dir_id`, not by link-instance location. The numeric branch at `tui/mod.rs:612` then prints `own_charged - retained disk` beside `own_files - retained rows`. Those quantities remain incompatible as a statement about the unlisted files.

   A concrete understatement with an actually unretained inode: scan `/r`, containing the same 8 MiB inode at `/r/a/shared` and `/r/b/shared`, plus a single-link 16 MiB `/r/a/large`. With `top_files=1`, `big_file_threshold=1GiB` and zero directory blocks, only `large` is retained. At `/r/a`, `own_files=2`, `own_charged=16 MiB`, and the sole retained row has `disk=16 MiB`, `nlink=1`. Enter on `.` therefore says **`1 other file, 0 B not individually retained`**, although the unretained shared inode occupies 8 MiB. No snapshot/live race is needed.

   Nearby cases expose both directions of the mismatch:

   | Case | What the numeric branch says | Unsupported implication |
   | --- | --- | --- |
   | The existing F1 fixture, viewed at `/r/b` instead of `/r/a` | `1 other file, 0 B not individually retained` | The local 8 MiB link instance has no direct representative; its representative is in `/r/a`. |
   | The same fixture, viewed at `/r` | `0 other files`, 8 MiB not individually retained | The 8 MiB is charged at this LCA and is already retained under `/r/a`; there are no direct files at `/r`. |
   | One single-link file, retained, in a directory with 4 KiB of directory blocks | `0 other files`, 4 KiB not individually retained | The residual is the directory inode, not an unretained file. |

   The producer establishes this independently of the fixtures: `crates/tidemark-scan/src/agg/linkset.rs:48` chooses a representative, `:57` charges at the LCA, `:74` counts each instance, and `:102` emits one notable candidate. Directory blocks enter charged totals at `agg/mod.rs:163`; own totals are produced at `:543` and `:550`. These implement `03-link-semantics.md` §§2–4 and `05-storage.md` §1.

   When a retained multi-link representative is directly listed, the new unknown wording is conservative whether its LCA is local or an ancestor, including incomplete link sets. That fixes the original `/r/a` example. It does not detect unretained link sets or representatives in a different subtree. Retention by threshold instead of top-N does not change the representative/LCA distinction (`agg/notable.rs:21`): a two-inode tree with the shared 8 MiB inode and one 1 MiB single-link file at `/r/b`, retaining both candidates with either `top_files=2` or a 1 MiB threshold, still leaves `/r/b` with a numeric zero-byte remainder for its shared instance. Nonzero directory blocks merely add unrelated bytes to that number.

   Report an unavailable file-size remainder unless it can be established, or explicitly separate a residual **charged** total, directory blocks, and unlisted entry counts. The new test at `tests/w7a/browser.rs:852` covers only a listed shared representative with a parent LCA and zero directory blocks; it cannot establish the numeric branch's correctness. The counterexamples above are source-derived calculations, not additional executed tests.

2. **SHOULD-FIX — F4: PgUp at the first live page still removes `.`.** Reference: `finding:rc3fix-live-pageup`.

   Refresh a directory with one child whose size exceeds the directory's own charged bytes, using default Size sort. The live page initially shows the child followed by `.`. Press PgUp, or Up while the first child is selected. `tui/mod.rs:190` saturates the live offset at zero, but `:203`–`:213` installs that first child as a reverse cursor. `load()` at `:90` reloads the identical offset-based live page. `own_position()` at `:468` nevertheless rejects the own row because the cursor sorts before it. The row becomes inaccessible until pagination is reset, for example with Home.

   This applies to Size, Reclaim, Files and Newest when a child sorts before `.`, and to Name with a name such as `!`. Growth uses its separate offset/end calculation at `:445` and avoids this particular failure. Initial refresh now correctly resets pagination for all six sorts; ordinary forward progression and the original later-page Name scenario are repaired. The residual reverse-boundary defect predates this fix, but remains within the requested nearby variants. The added regression at `tests/w7a/browser.rs:1001` covers Name and forward pages only. A no-op live page change must not install an excluding cursor; live own-row placement should agree with the actual offset slice.

3. **NOTE — N1: navigation retains a stale Main-pane live message.** Reference: `finding:rc3fix-navigation-label`.

   After refresh, entering an existing snapshot child or ascending calls `navigate()` (`tui/mod.rs:498`), clears the live cache and loads snapshot rows, but retains `message = "live values; delta against snapshot"`. `tui/draw.rs:141` displays that stale footer on Main. The Files override at `:138` correctly replaces it, so refresh → navigation → Enter on `.` now has consistent snapshot file details. This is a preexisting presentation issue, not a new regression; clear or replace the live message when navigation returns to snapshot data.

For F2, refresh preserves the current snapshot directory ID (`tui/mod.rs:395`), and own-file details fetch that directory from the snapshot at `:565`. Child navigation resolves by snapshot path at `:626`, parent navigation uses snapshot IDs at `:837`, and a newly live-only child is refused with an explanatory message. Thus those navigation paths do not feed live IDs/counts into the fixed file pane. Snapshot title/time and the post-snapshot-change notice also apply to the `f` pane.

For F3, a single activating → active transition either remains activating for this iteration or is caught by the second active probe (`deploy/mod.rs:529`). Activating → failed and inactive throughout produce failed service checks; stable enabled/stopped state retains the early start hint. Probe errors propagate into failed checks and the existing deadline retry, rather than success. Unhealthy verification still maps to exit 6 (`cmd/deploy/verify.rs:36`). The probes are not atomic across arbitrary repeated restarts; no remaining error was established for the requested single-transition scenarios. F5's restart recipe matches `deploy/mod.rs:409`. F6 preserves the Precondition/exit-5 path and recorded skip reasons (`cmd/snapshots.rs:339`). F7's warning matches the unchanged disabled-export branch (`tidemarkd/src/metrics.rs:711`).

Validation ran offline with pinned Rust 1.98.0 in the approved checkout, uid 0, without privileged operations. The default PATH initially lacked Cargo; validation used the existing pinned toolchain and Cargo cache. An initial formatting invocation could not create the default Rustup home; setting `RUSTUP_HOME` to the existing installation resolved it without changing source.

| Command | Observed result | Evidence log |
| --- | --- | --- |
| `cargo test --offline -p tidemark-cli --lib -- --nocapture` | 134 passed, 0 failed, 1 scale ignored | `library/cli-lib.log` |
| `cargo test --offline -p tidemark-cli --test w2c --test w6a3 -- --nocapture` | w2c: 14 passed, 1 scale ignored; w6a3: 34 passed | `library/cli-integration.log` |
| `cargo fmt --check` | Passed | `library/fmt.log` |
| `cargo clean --offline` | Completed; removed build artifacts | `library/clean.log` |

Logs are in this attempt's worker-output directory. Socket-dependent bodies can return early: both schedule-socket tests reported bind EPERM, and integration output also names unavailable socket/root-mount-identity paths. Passing harness counts therefore do not establish those branches. F8's nonblocking acceptance loop (`tests/w7a/browser.rs:1137`) and five-second accept/read/write limits (`:800`) remove the historical indefinite accept/join path by inspection. Its no-client test uses a monotonic 30 ms deadline and a five-second elapsed assertion (`:1177`), so severe runner starvation can fail that assertion; no normal-runner hang was established. Fixed timestamps use UTC arithmetic, and private fixture directories use explicit mode 0700, so no new timezone or umask dependency was found.

No product source/tests were changed or new reproducer tests injected. No full-workspace CI, Clippy, cargo-deny, uid-1000/umask-022 gate, GitHub Ubuntu 24.04 job, real systemd transition, RPM install or privileged/XFS VM run was performed. The fixes add no scan metadata operations, unsafe code, filesystem traversal or metadata mutation. No additional supported new wrong-exit-code or safety regression was found in the reviewed changes; the remaining F1/F4 cases above still need implementation and regression coverage.
