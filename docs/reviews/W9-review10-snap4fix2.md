VERDICT: BLOCKED
BLOCKER 2 / SHOULD-FIX 1 / NOTE 1

Reviewed exact candidate `abf9a0ab4770db3f9557d15f7c05e28e485fd4bf` against base `6fde327756ac8c9732b9edfc75cedf02f554441a`, including the candidate tree. The supplied integration checkout `36560dc4f90e22133207a10a403f0b40e1e55db6` has the identical tree, `7a9f4d559d3e4da4f1a0f10dccc199a4591da205`; compiled checks ran there. Reviewed the requested `6fde327..36560dc` diff/log and skimmed the wave since `1b0a06c`. Findings below apply to the exact candidate, including inherited behavior explicitly identified below.

Read `AGENTS.md`, review8, review9, `15-snapshot-v2.md` (including §§3–6 and §8), normative link semantics, and `SNAP-reader2-fix3b.md`. This report is a review evidence candidate, not integration or release approval. Production source and repository tests were not edited.

**B1 — BLOCKER: shallow subtree and metrics queries still accept incomplete or misplaced rows.**

Reference: `finding:w9-review10-snap4fix2-b1`. Locations: `crates/tidemark-store/src/reader_v2.rs:778` (shallow subtree enumeration), `:842` (shallow eligibility enumeration, consumed by `metrics_candidates` at `:946`).

The new completeness checks protect `visit_children`, but these two shallow-table paths still iterate whatever rows remain without validating coverage or hierarchy. Reproduced with a root and one child, `plain_depth=2`: delete only `shallow_dirs.id=1`, retaining all chunks. `children(0, Name, 100)` correctly returns `Invalid`; `subtree_dirs(0, 1, Name, 100)` succeeds with only the root, and `metrics_candidates(1, 100)` succeeds with root-only used/reclaimable lists and count 1 instead of 2. `dir(1)` still returns the intact child from its chunk.

A second reproduction changes a depth-2 shallow row's `parent_id` to 0 and `depth` to 1, retaining its chunk and the true first child's subtree span. Direct child APIs reject the overlap, but `subtree_dirs(0, 1, Name, 100)` accepts it, returning both IDs 1 and 2 at `/data/\xFF/a` and depth 1. Thus this is successful wrong subtree data, not merely a different corruption error. Validate shallow range coverage/hierarchy too, or consult the authoritative chunks. The new `s2_b4_shallow_rows_are_validated` regression covers only `children`, `children_count`, and `children_page`. This inherited gap remains within the requested broader shallow-table check; the original review9 diff-removal reproduction itself is fixed.

**B2 — BLOCKER: mixed-format diff still normalizes malformed v1 child names into invented paths.**

Reference: `finding:w9-review10-snap4fix2-b2`. Locations: `crates/tidemark-store/src/reader.rs:617` (unchecked v1 child component), `crates/tidemark-store/src/diff.rs:709` (append), `:562` (normalization).

The v2 compact cursor validates `RawName`; its v1 counterpart still copies unchecked bytes. Reproduced using equivalent two-directory snapshots rooted at `/data/\xFF`, with a v2 new snapshot and v1 old snapshot. Change only the old child's `dirs.name` to `../escaped`, leaving its stored path and counters intact. Mixed diff succeeds, reporting removed `/data/escaped` with old value 1 and a new original child with value 1. Neither snapshot contains `/data/escaped`; it lies outside the root. A v1/v1 SQL control against the same damaged old snapshot returns both actual stored paths as unchanged. Validate v1 compact child components before the walk consumes them, without normalizing a component into a different path. This is inherited from the earlier compact-cursor walk, not introduced by this patch; valid-input v1 behavior is unchanged. The new name-validation regressions exercise v2 only.

**S1 — SHOULD-FIX: v2 ncdu export bypasses notable-file component validation.**

Reference: `finding:w9-review10-snap4fix2-s1`. Locations: `crates/tidemark-store/src/export_ncdu.rs:168`, `:176`; `crates/tidemark-store/src/reader_v2.rs:1521`.

Re-encode a valid file chunk after replacing its first root file's name `a` with `../escaped`, keeping name ordering, directory association, counts and byte totals valid. `subtree_files(0, default_filter, 100)` returns `Invalid`, but ncdu export succeeds and emits `"name":"../escaped"`. `dir_files` passes raw names to the exporter, which copies them into the arena and JSON without `RawName` validation. Reject invalid components on this path too. This inherited validation inconsistency is separate from the accepted per-directory buffering policy. No ncdu import or downstream filesystem effect was tested, so none is asserted.

**N1 — NOTE: current-candidate §8 timings remain unavailable.**

Reference: `finding:w9-review10-snap4fix2-n1`. Location: `docs/handoff/SNAP-reader2-fix3b.md:4`.

The requested handoff reports no new wide-directory timing: both `scale_wide_directory_old_sql_vs_walk` and `profile_wide_split` failed setup because `/` was owned by unmapped UID 65534. It provides no new 2M-directory measurement either. These benchmarks were not rerun in this review. The earlier `SNAP-diff-walk-fix2.md:28` supplies only historical five-run medians, from an optimized build with debug assertions:

| Historical shape | v1 SQL | max(1.25 × v1, 100 ms) | v2 warm | v2 fresh open |
|---|---:|---:|---:|---:|
| 2,000,101 directories, 1% changed | 3,889 ms | 4,861.25 ms | 1,539 ms | 1,732 ms |
| One 150,000-child directory, 1% changed | 184.5 ms | 230.625 ms | 725.8 ms | 719.1 ms |

Historical large-tree timings meet their threshold; historical wide timings miss it. The old split measurements (one side 312/52 ms and whole diff 678/116 ms for plain depth 2/0) motivate the new unconditional chunk cursor, but do not measure this candidate. No current latency pass or miss is established. The historical large fixture has depth three and fanout at most 999; the prescribed median-depth-seven shape and full query/file-size/RSS matrix remain unestablished. This evidence gap is a NOTE and is not the reason for the BLOCKED verdict.

Review9 disposition and regression strength:

| Review9 item | Recheck |
|---|---|
| B1, three link chunks | Fixed. `li_visit` continues checking all boundaries in the inode's run after its visitor stops. `b1_misordered_boundary_beyond_the_next_chunk_is_invalid` covers `z, zz, a` with full/forward/reverse/supplied-cursor queries. Its first forward-page assertion distinguishes the old early-stop logic. |
| B2, invalid v2 child name | Fixed on the v2 compact cursor. Library `b2_child_names_are_validated` isolates invalid components with surrounding `0x01`/`0xfe` names. Replaying the original shallow-only `../escaped` mutation now gives a correct unchanged diff from chunks; the ordinary child listing returns `Invalid`. The analogous v1 compact-cursor defect is B2 above. |
| B3, cross-page sibling order | Fixed. `Id(next_sibling, last_name)` carries the preceding name, and every consumed child must sort strictly after it. Library and diff regressions swap chunk names across the 256-row boundary. Replaying the original shallow-only swap now returns 258 unchanged diff rows correctly. |
| B4, deleted shallow child | Fixed for the original diff and for direct shallow-child APIs. Original deletion replay returns the intact child in diff; the new first/middle/last-deletion regression rejects ordinary child queries. A scratch misplaced-row test also makes all three direct child APIs return `Invalid`. Remaining shallow subtree/metrics gap is B1 above. |
| B5, parent depth overflow | Fixed. Child depth uses checked addition. New library test directly queries the `u32::MAX` parent; replay with a corrupt chunk depth and intact shallow copy makes diff return `Invalid`, with `catch_unwind` confirming no panic. |
| S2, shallow conversion category | Fixed at all shallow `dir_row` call sites through `plain_row_error`. New library test returns `Invalid` for malformed `t_disk` in children/count. Original malformed-shallow diff replay now correctly uses intact chunks. V1 error mapping was not changed. |
| S3, ncdu retention | Resolved according to the accepted coordinator decision: compact tuples and one shared name arena, freed per directory and bounded structurally by `file_count`. Existing scanner-order parity regression passes. No new fail-before memory regression was added, and this review did not measure arena RSS. The accepted policy is not re-raised. |

The pre-fix implementation was not built or executed in this attempt. Statements that a regression distinguishes old code are source comparisons, not newly measured old-code failures. The diff regression's isolated `../escaped` assertion also violates old within-page ordering and would already fail there; the library B2 test provides the meaningful component-validation coverage. The empty-name library case likewise also violates the surrounding order. These limitations do not invalidate the other isolated bad-component cases.

For valid snapshots, `children_by_name` now uses chunk sibling hopping regardless of `plain_depth`. Existing parity tests compare both formats' compact pages with `children(..., Name, ...)` at plain depths 0/2/4, chunk sizes 1/3/5/4096 and pages 1/2/7/1000, plus a 5,000-child case at plain depths 1/2. The loop begins at the cursor's next sibling and never rescans preceding siblings; cross-page order, names and checked child-depth arithmetic are enforced. This supports O(page) v2 sibling work with bounded chunk overhead for every plain-depth setting. The decode-count regression directly measures plain depth 0 only; the same work bound applies at default depth by source inspection, not a new timing regression.

The accepted review9 S1 decision is documented accurately in `reader.rs:581` and the handoff: very wide mixed v1/v2 diffs retain superlinear work on the v1 side. The S3 documentation accurately describes per-directory buffering; `max_notable` is a scanner-produced snapshot invariant, not an additional reader-enforced cap. One handoff wording is too broad: `shallow_dirs` is not used only by metric-sorted queries; ordinary name-sorted `children`/`children_page` and `children_count` still use it. Specifically the compact `children_by_name` cursor always uses chunks.

Valid-input diff/explain parity, raw-path tie order, cold-format cases, the 120,000-child oracle and the 4,096-deep bounded-memory regression pass. No further valid-input mismatch was established. Explain tests still do not assert `share_of_parent`. The v1 SQL fallback and production `diff.rs` are unchanged by this patch. Directory cache ceilings remain 32 chunks/8 MiB; file/link caches remain four chunks/8 MiB each; path cache remains 4 MiB/65,536 entries, with codec bounds on transient chunks. Arena offsets derive from successful appends; no new overflow was found there. This is not an exhaustive malformed-input or panic proof.

Validation and limits:

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --test w3c`: exit 0; lib 43 passed/1 ignored (35.80 s), diff_walk 9/2 (159.50 s), reader_v2 34/1 (186.22 s), w3c 6/1 (0.09 s). Total 92 passed, zero failed, five ignored scale tests. Used the pinned Rust 1.98.0 binary directory on PATH and `CARGO_HOME=/home/brewerm/.cargo` for the existing offline cache. Log: `library/focused-tests.log`.
- Six scratch observation tests compile against the unchanged candidate libraries and pass. They replay review9 shallow-name/order/deletion/type corruption and depth overflow, verify correct child-API rejection, and reproduce B1/B2/S1 above. Passing observation assertions demonstrate the stated outcomes; they are not acceptance tests for the defects.
- Scratch sources and logs are in this attempt's output `library/`: `review_repro_additions.rs`, `review_repro.rs`, `run_repro.py`, `repro-compile.log`, `review-repro.log`, and `focused-tests.log`. All mutated snapshots were temporary fixtures. The new repository three-chunk-link regression reruns review9's remaining reproduction.
- `cargo clean --offline`: exit 0, removed 2,046 files/1.0 GiB after all compiled checks. `git diff --check` is clean. The generated scratch executable was removed; sources and logs are retained.
- Full workspace CI, fmt/clippy, privileged/VM tests, ignored scale benchmarks, execution above one million children, and the prescribed §8 benchmark were not run in this review. No privileged commands, branch creation, merges or pushes were performed.
