VERDICT: BLOCKED
BLOCKER 5 / SHOULD-FIX 3 / NOTE 1

Reviewed exact candidate `2e9818cf8f8c3e7205ebdba6ca807b8a2ba7e3d6` against base `5135a9a3fe27c4f6d12c832acbeba23e6baf5dbc`, including its whole tree. Integration head `451fdcab3c7896e6aae7a009399569f985565306` has the identical tree (`a582ded145591fc17cf021e68362b7e4d5ed1e06`). Also reviewed the requested diff/log `5c4fc54..451fdca` and skimmed the wave since `1b0a06c`. Consequently all findings below apply to the exact blind-review candidate; some concern inherited reader behavior rather than newly introduced code.

Read `AGENTS.md`, review8, `15-snapshot-v2.md` including §§3–6 and §8, normative link semantics, and both requested fix handoffs. This report is an evidence candidate, not integration or release approval. Production source and repository tests were not edited.

**B1 — BLOCKER: review8 B1 still permits wrong reverse pages across three link chunks.**

Reference: `finding:w9-review9-snap4fix-b1`. Location: `crates/tidemark-store/src/reader_v2.rs:1699` (early-stop validation through line 1717).

The two-chunk regression is fixed, but early termination checks only the immediately following boundary. Reproduced with three one-row chunks for inode 42/directory 0, payload names `z`, `zz`, `a`: forward limit 1 returns `(rowid=1,z)` successfully. Passing that actual cursor to a reverse page returns `Ok([])`, although `a` qualifies. Both calls inspect the sound `z <= zz` boundary and miss `zz > a`. Full `link_instances` detects the disorder. The handoff acknowledges the limitation, but it still yields successful wrong data for the same query class as review8 B1. Validate the remaining relevant order before relying on early termination, or avoid the ordering-dependent early stop. The existing regression has only two chunks.

**B2 — BLOCKER: compact children bypass name validation and diff invents normalized paths.**

Reference: `finding:w9-review9-snap4fix-b2`. Locations: `crates/tidemark-store/src/reader_v2.rs:474`, `crates/tidemark-store/src/diff.rs:709`, `:562`.

`ChildEntry.name` is copied without `RawName` validation. The walk appends those bytes, then `RawPath::new` normalizes them rather than validating a component. Reproduced by changing only a v2 shallow child's name to `../escaped`: `children(0, Name, 10)` returns `Invalid`, while diff succeeds and reports a new `/data/escaped` outside the snapshot root `/data/\xFF`, plus removal of the original child. The former materialized-child path rejected the component. Validate names before merge/path construction, including rows that will not enter top-N. Current child-cursor corruption tests do not cover invalid names.

**B3 — BLOCKER: sibling ordering is not checked across child pages.**

Reference: `finding:w9-review9-snap4fix-b3`. Locations: `crates/tidemark-store/src/reader_v2.rs:472`, `:504`, `:514`.

Each call resets `prev`; the lookahead row is returned as a cursor without checking its name against the previous page. Reproduced with 257 root children by swapping shallow names of IDs 256 and 257, precisely across diff's initial 256-row page boundary. Both pages succeed. Diff returns `/data/\xFF/b000255` twice: once new with value 5 and once removed with value 4. A sorted merge needs global sibling order, not merely order within each page. Carry/check the previous name or validate boundary adjacency before consuming the next page. The existing parity and decode-count tests use sound ordering and miss this case.

**B4 — BLOCKER: missing shallow children are silently reported as removals.**

Reference: `finding:w9-review9-snap4fix-b4`. Location: `crates/tidemark-store/src/reader_v2.rs:499` (shallow loop and successful exhaustion through line 510).

The shallow cursor neither checks expected sibling IDs nor verifies that exhaustion reaches the parent's declared subtree end. Reproduced on a root with one child: deleting only that child's `shallow_dirs` row, while retaining its directory chunk, makes diff return the child as removed. The intact chunk and parent span make the missing copy detectable. This incomplete-shallow-table weakness is inherited from the older `visit_children` implementation; the new cursor preserves it. Check coverage against the authoritative chunk structure and return `Invalid`, including for missing first/last children. The deleted-directory-chunk test does not exercise deleted shallow rows.

**B5 — BLOCKER: corrupted parent depth panics during diff.**

Reference: `finding:w9-review9-snap4fix-b5`. Location: `crates/tidemark-store/src/reader_v2.rs:477`.

`p.depth + 1` is unchecked. Reproduced with a three-directory chain: re-encode the chunk with directory 1's depth set to `u32::MAX`, leaving its shallow copy at the valid depth. Root paging accepts that shallow copy; expanding directory 1 reads the corrupt chunk and panics with `attempt to add with overflow` when visiting directory 2. A scratch `catch_unwind` confirmed the panic against the unmodified debug library. Without overflow checks the expression wraps; that build was not executed. Use checked depth arithmetic and return `Invalid`. This violates the corrupt-data/no-panic contract and is not covered by current cursor corruption tests.

**S1 — SHOULD-FIX: review8 S3 remains for mixed v1/v2 diff.**

Reference: `finding:w9-review9-snap4fix-s1`. Locations: `crates/tidemark-store/src/reader.rs:600`, `crates/tidemark-store/migrations/snapshot-v1.sql:76`, `crates/tidemark-store/src/diff.rs:618`.

The v1 branch filters by parent and name cursor, then sorts by name. The schema has only `dirs_parent(parent_id)`, so each page scans the parent's siblings. An exact-schema/query SQLite reproduction reports `SEARCH dirs USING INDEX dirs_parent` and `USE TEMP B-TREE FOR ORDER BY`. For a fixed 65-row request, increasing width from 1,000 to 10,000 raises VM instructions from 10,124 to 91,121 initially, and from 7,471 to 61,471 near the end. Thus mixed-format diff still has O(W × pages) work on its v1 side. Pure v1/v1 retains the SQL fallback. The handoff already discloses this gap; its O(page) method rustdoc and the diff handoff's per-side O(W) claim are too broad. `FETCHED` counts returned entries, not SQL rows examined, so the new test cannot detect this amplification. Provide a compatible seek strategy/index and a work-sensitive regression.

**S2 — SHOULD-FIX: malformed shallow rows still return `Sql`, not `Invalid`.**

Reference: `finding:w9-review9-snap4fix-s2`. Location: `crates/tidemark-store/src/reader_v2.rs:503`.

The new shallow cursor uses `dir_row(row)?` without the new v2 `plain_row_error` mapping. Setting a child's `shallow_dirs.t_disk='bogus'` and reopening made diff return `Sql(InvalidColumnType(21, "t_disk", Text))`. The original four review8 S2 cases are fixed, but this newly exercised plain-table path still violates the requested corruption error category. Apply the v2 conversion mapping here without changing v1 behavior.

**S3 — SHOULD-FIX: the ncdu ordering fix removes bounded streaming for a directory's notable files.**

Reference: `finding:w9-review9-snap4fix-s3`. Locations: `crates/tidemark-store/src/export_ncdu.rs:158`, `:162`, `:167`; `crates/tidemark-store/src/reader_v2.rs:1496`.

V2 now clones every named single-link notable file into one vector before emitting any of them. There is no export row/byte budget; `dir_files` walks the complete stored `file_count`. Scanner `max_notable` defaults to 200,000 but accepts values up to `u32::MAX` (`crates/tidemark-core/src/config.rs:828`), and its value is not enforced by the reader. A supported two-million-file directory with 255-byte names retains at least 510 MB of name bytes plus about 96 MB of tuples, allocation overhead and sorting storage. V1 continues streaming 256-row pages. Corrupt input with matching chunks can induce the same retention before emission checks reject inconsistent totals. This is a source-level allocation proof, not an RSS measurement. Restore a bounded sorting/streaming strategy or an explicit supported export memory limit. The scanner-style order regression proves ordering, not this resource bound.

**N1 — NOTE: the handoff still misses the wide-directory §8 diff threshold.**

Reference: `finding:w9-review9-snap4fix-n1`. Location: `docs/handoff/SNAP-diff-walk-fix2.md:28`.

Reported five-run medians, release with debug assertions:

| Shape | v1 SQL | Threshold max(1.25 × v1, 100 ms) | v2 warm | v2 fresh open |
|---|---:|---:|---:|---:|
| 2,000,101 directories, 1% changed | 3,889 ms | 4,861.25 ms | 1,539 ms | 1,732 ms |
| One 150,000-child directory, 1% changed | 184.5 ms | 230.625 ms | 725.8 ms | 719.1 ms |

The large-tree medians are below their threshold; wide medians exceed theirs by approximately 3.15× and 3.12×. The handoff also reports plain-table versus chunk child paging at 312/52 ms for one side, and whole diffs at 678/116 ms. These are handoff measurements, not independently repeated timings. The large fixture has depth three and maximum fanout 999; the separate wide fixture has depth one. Neither establishes the prescribed median-depth-seven shape and complete §8 query/RSS matrix. The reported `v1/v1 walk` measurement actually dispatches to the SQL fallback. Per this review's instruction, the timing miss is a NOTE and is not the reason for the BLOCKED verdict.

Review8 disposition and regression strength:

| Review8 item | Recheck |
|---|---|
| B1 cross-chunk links | Two-chunk case passes its regression; incomplete fix, see B1 above and the reproduced three-chunk case. |
| B2 missing final file chunk | Fixed by checking queried chunk coverage against directory file spans; first/last deletion regression passes, including filtered/subtree/export queries. Source logic clearly distinguishes the old missing-tail success. |
| B3 quadratic path retention | Fixed in traversal state: compact pages/cursors, frame path lengths and one shared path. The 4,096-deep, 255-byte-component, limit-1 child-process test passes its VmHWM <192 MiB assertion. Old code was not executed; its >4 GiB retention follows from retained full paths at each frame. |
| S1 ncdu ordering | Scanner-style `(disk DESC, ino ASC)` order is restored; exact-byte regression passes across three layouts. Earlier artificial-ID fixtures now compare normalized ordering, so universal byte equality for arbitrary writer IDs is not established. New buffering concern is S3 above. |
| S2 plain-table errors | All four original malformed skipped/error/annotation/link-set cases pass their `Invalid` regression; v1 mapping is preserved. Shallow decoding remains a gap (S2 above). |
| S3 repeated child scans | V2 seek/hopping and directory-chunk decode-count test establish improvement. Mixed v1/v2 remains superlinear (S1 above). Counting returned entries alone does not prove the old paginator would fail the new assertion. |

For valid inputs, the four hot pair kinds and tested cold combinations pass model parity for diff and explain; v1/v1 also checks the copied SQL oracle. Other formats connect to that oracle transitively through equivalent generated trees. The 120,000-child v2/v2 and v2/v1 oracle tests pass. No further supported valid-input output mismatch was found. Explain tests do not assert `share_of_parent`; arbitrary format/shape combinations are not exhaustively proved.

Memory inspection supports O(depth × page) compact traversal entries plus one current path and retained top-N output paths. Pages are at most 16,384 and output limits at most 1,000,000; there is no million-directory traversal stop. This excludes the former quadratic stack-path retention; actual top-N byte storage still includes the lengths of retained output paths. Existing directory cache ceilings (32 chunks/8 MiB), file/link caches (four chunks/8 MiB each), and path cache (4 MiB/65,536 entries) remain in place; transient codec allocations are separately bounded. Those cache bounds do not bound the new ncdu sorting vector.

Validation and limits:

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --test w3c`: exit 0; lib 39 passed/1 ignored (27.30 s), diff_walk 8/2 (121.01 s), reader_v2 33/1 (155.28 s), w3c 6/1 (0.10 s). Total 86 passed, zero failed, five ignored scale tests. Used the pinned Rust 1.98.0 binary directory on PATH and `CARGO_HOME=/home/brewerm/.cargo` for the existing offline cache. Log: `library/focused-tests.log`.
- Separate scratch streaming fixture: 1,000,001 immediate children and 1,000,003 total directories, v2/v2 with `plain_depth=0`, against a root-only old snapshot. Whole-added target and matched root return the expected final/highest children for limits 1 and 50; requested output limit 1,000,001 returns `Invalid`; explain top-1 also includes the final child. One test passed, exit 0 (110.00 s including construction; not a latency benchmark). Sources/logs: `library/scale_traversal.rs`, `scale_traversal_compile.log`, `scale_traversal.log`. This establishes both >1M-child and >1M-subtree traversal for that v2 shape; mixed-pair >1M execution and the default shallow-table shape were not run.
- Ran `cargo clean --offline` after all compiled checks: exit 0, removed 2,046 files/1.0 GiB. `git diff --check` is clean. No privileged commands, branch creation, merges or pushes were used.
- Six scratch defect reproductions compiled with the pinned Rust 1.98.0 toolchain against the unchanged libraries. All six observation assertions passed: three-chunk link paging, invalid child name, cross-page ordering, deleted shallow row, shallow error category, and caught depth-overflow panic. These passing assertions demonstrate defects, not acceptance.
- Scratch sources and logs are in this attempt's output `library/`: `review_repro_additions.rs`, `review_repro.rs`, `run_repro.py`, `review-repro.log`, and `v1_children_paging_repro.py/.log`. Only temporary fixtures were mutated.
- The pre-fix implementations were not built or executed in this attempt. Reported old-code failures are identified by source analysis or attributed to the handoffs, not claimed as new experimental results. Full workspace CI, privileged/VM tests, the ignored repository scale benchmarks, and the prescribed §8 benchmark were not run.
