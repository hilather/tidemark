VERDICT: BLOCKED
BLOCKER 3 / SHOULD-FIX 4 / NOTE 0

Reviewed integration commit `4122cbd36ced9973b2f96429219ce62ac94d5a19`, including the requested diff and log from `1b0a06c4ad12a6567d5fab719dd6d13a739f7ddd`. The blind-review session names exact candidate `4efebefa1e8480763b2b3c6e1cd59649164bef01`. Its query implementation is byte-identical to the integration tree; the only later changes are `diff.rs`, `tests/diff_walk.rs`, and `SNAP-diff-walk.md`. Findings B1, B2, S1, and S2 apply to that exact candidate. B3, S3, and S4 are supplemental findings about the requested integration diff walk, and must not be attributed to `4efebef`.

Read `AGENTS.md`, `15-snapshot-v2.md` (including §§3–6 and §8), normative link semantics in `03-link-semantics.md`, and both requested handoffs. This is a review evidence candidate, not an integration or release approval. No production source was edited.

**B1 — BLOCKER: cross-chunk link ordering is trusted without validation.**

Reference: `finding:w9-review8-snap4-b1`. Locations: `crates/tidemark-store/src/reader_v2.rs:982`, `:1049`, `:1616`.

`check_adjacent` compares only inode bounds between link chunks. `decode_links_chunk` checks row count and first/last inode, while the codec checks `(ino, dir_id, name)` order only within each chunk. Two one-row chunks for inode 42 and directory 0 can therefore contain `z` followed by `a` and pass every check. In a scratch reproduction, swapping just the two valid payloads caused `link_instances(42)` to return `Ok([z,a])`. Using the cursor actually returned by the first forward page, `(dir=0,name=z,rowid=1)`, a reverse page returned `Ok([])` even though `a` qualifies: the first `z` triggers the early stop. This is successful wrong data from a malformed layout, contrary to the corruption requirement. Validate full tuple ordering across relevant chunk boundaries before relying on early termination. The existing corruption tests do not mutate that boundary ordering.

**B2 — BLOCKER: a missing final file chunk silently removes notable files.**

Reference: `finding:w9-review8-snap4-b2`. Locations: `crates/tidemark-store/src/reader_v2.rs:1427` and `:1016`.

`subtree_files` iterates whichever overlapping chunks remain; it does not check their coverage against the directories' stored `file_lo`/`file_count`. Neighbour validation detects interior gaps, but accepts the final remaining chunk when it has no successor. On the existing 20-file sample, applying `DELETE FROM file_chunks WHERE first_row=(SELECT MAX(first_row) FROM file_chunks)` and reopening made the root query with default filters and limit 100 return `Ok(18 rows)`. It should report invalid storage rather than incomplete successful results. Directory file spans still describe the missing rows, so the inconsistency is observable without changing the format. The tests at `tests/reader_v2.rs:1791` accept individual `Ok` results provided some exercised query detects corruption; that does not establish per-query integrity.

**B3 — BLOCKER: diff retains quadratic path bytes along a deep chain.**

Reference: `finding:w9-review8-snap4-b3`. Integration only. Locations: `crates/tidemark-store/src/diff.rs:615`, `:628`, `:660`, `:697`.

Each active frame owns two `Kids` objects, and each keeps a cloned final `DirRow` in `last`, including when `done=true`. On a single-child chain, these retained rows contain the complete path at every depth. With depth D and 255-byte components, the two cursor paths alone retain approximately `256 * D * (D+1)` bytes. At D=4096 this exceeds 4 GiB even with `limit=1`, before chunk caches, row overhead, and the result heap. These are supported byte paths: the reader expressly supports paths exceeding `PATH_MAX` and allows individual reconstructed paths up to 256 MiB (`reader_v2.rs:117`). Thus constant page size does not make actual traversal memory O(limit + depth); path storage is quadratic in depth. This violates the bounded-memory requirement for trees thousands deep. Use compact cursors and shared/reused path storage, including for buffered rows. This finding is a source-level retention proof; no multi-gigabyte stress run was attempted.

**S1 — SHOULD-FIX: ncdu parity fixtures hide production file ordering.**

Reference: `finding:w9-review8-snap4-s1`. Locations: `crates/tidemark-store/src/export_ncdu.rs:55`, `:155`; `crates/tidemark-store/tests/reader_v2.rs:963`.

V1 exports notable files by original file ID; v2 streams name-ordered chunk rows. Production aggregation assigns IDs in `(disk DESC, dir_id, ino)` order (`crates/tidemark-scan/src/agg/notable.rs:15`, `:29`; also normative §4 Step G). A directory containing single-link notable files `z` with disk 8192 and `a` with disk 4096 therefore exports `z,a` in v1 and `a,z` in v2. The scratch reproduction confirmed both exports succeed with that difference. Counts and byte totals match, but byte-identical output does not. Parity fixtures sort files by `(dir_id,name)` before assigning IDs, masking ordinary scanner behavior. The restriction acknowledged in the handoff excludes ordinary scanner output. Preserve the required export ordering or obtain an explicit compatibility decision; test scanner-style IDs. The documented v2 `FileRow.id` change itself is not a new finding.

**S2 — SHOULD-FIX: malformed plain tables return the wrong error category.**

Reference: `finding:w9-review8-snap4-s2`. Locations: `crates/tidemark-store/src/reader.rs:962`, `:973`, `:985`; `crates/tidemark-store/src/reader_v2.rs:1666`.

The new v2 plain-table paths propagate rusqlite conversion errors directly. Reproductions after reopening returned `StoreError::Sql`, not `Invalid`, for all four cases: skipped `reason='bogus'`, errors `op='bogus'`, annotation `value=x'00'`, and link-set `complete=2`. The precise errors were `FromSqlConversionFailure`, `InvalidColumnType`, and `IntegralValueOutOfRange`. Queries reject these rows and did not panic, so this is an error-contract gap rather than silent wrong data. V1 already uses these shared conversions; fix the required v2 classification without casually changing v1 compatibility. Existing new tests concentrate on chunk envelopes and do not cover these plain-table mutations. Arbitrarily large plain-table value allocation was not stress-tested.

**S3 — SHOULD-FIX: diff pagination repeatedly scans all children.**

Reference: `finding:w9-review8-snap4-s3`. Integration use of a pre-existing reader behavior. Locations: `crates/tidemark-store/src/diff.rs:603`; `crates/tidemark-store/src/reader_v2.rs:568`, `:404`.

Every diff page calls `children_page`, which invokes `visit_children` from the start and visits all siblings before retaining the next page. Pages grow from 256 to a fixed maximum of 16,384. Consequently a directory with W children costs O(W * ceil(W/16384)) sibling visits per v2 side, instead of the linear traversal required by §6. A million-child directory requires about 68 complete sibling scans. Chunk payloads can also be decoded repeatedly when the working set exceeds the cache. A cursor that seeks directly to the next sibling would avoid this amplification. The ordinary diff width test has only 1,500 children (`tests/diff_walk.rs:692`); the ignored two-million-directory fixture has fanout at most 999 and depth three (`:774`). Neither exercises the required 100k+ fanout shape.

**S4 — SHOULD-FIX: the reported diff latency misses the integration gate.**

Reference: `finding:w9-review8-snap4-s4`. Integration only. Location: `docs/handoff/SNAP-diff-walk.md:5`.

The handoff reports these medians and ranges on 2,000,101 directories with 1% changed:

| Pair and implementation | Median | Range |
|---|---:|---:|
| v1/v1 old ATTACH SQL | 8.1 s | 6.9–13.1 s |
| v1/v1 walk, historical measurement | 48.6 s | 36.6–56.6 s |
| v2/v2 walk, warm | 14.0 s | 9.7–22.1 s |
| v2/v2 walk, fresh Snapshot open | 25.4 s | 23.0–27.3 s |

The corresponding §8 threshold is 10.125 s. Warm and fresh medians exceed it by about 1.38x and 2.51x. These are reported noisy optimized-test-build measurements, not an independently rerun prescribed benchmark; the required depth/fanout distribution and complete query/RSS matrix are not established. They support a likely gate miss, not a claim that §8 passed. The plan says a miss stops Tier 2 integration until coordinator reporting to the owner; no resolution of that condition is established here. The retained v1/v1 SQL fallback at `diff.rs:261` is justified by the reported sixfold v1 walk slowdown. Current scale-test calls labeled v1 “walk” now use this fallback, so that historical walk timing cannot be reproduced through current dispatch as written.

Other review coverage and limits:

- For valid inputs, the new file/link heaps use raw containing-directory path bytes for ties, not preorder; NULL names rank before present names. Filters, result limits, and correctly ordered links spanning chunks have useful differential coverage. No further supported valid-data defect was found in `subtree_files`, `link_sets_under`, `link_instances`, `link_instances_page`, `skipped_under`, `errors_under`, `annotations`, `crossing_sets`, or `link_report`.
- V1 query branches preserve their valid-tree behavior by inspection. Ncdu now performs directory primary-key lookups instead of a sequential SQL cursor; its v1 performance was not benchmarked here. At exact candidate `4efebef`, diff/explain are still v1-only and guarded by `v1_conn()` (`diff.rs:263` there), returning `NotYetImplemented` for v2. The integration adds the walk. No other remaining v2 `NotYetImplemented` query path was found in the requested methods.
- The directory cache remains bounded by 32 chunks/8 MiB; file and instance caches each use at most four chunks/8 MiB, with a separate path cache bounded by 4 MiB of path bytes and 65,536 entries. Codec/input ceilings remain in place. Narrow file queries decode overlapping payloads, although their SQL scans chunk metadata because its overlap predicates lack an index; link lookups binary-search metadata. No general panic guarantee or complete malformed-input proof is claimed.
- Diff/explain ranking, relative depths, classes, missing-target/differing-root checks, and contributor counts appear consistent with the old implementation for valid trees. The copied SQL oracle is called directly only for v1/v1, now the retained fallback. Identical generated trees across pair kinds and the separate path-map model provide useful transitive coverage. `ties_order_by_raw_path_bytes_not_preorder` explicitly checks `a-c` before `a/x`, and would catch the stated preorder bug. The explain test does not assert `share_of_parent`; most error tests check occurrence rather than exact error variants.
- Diff rejects output limits above 1,000,000 and pages at no more than 16,384. Its loop does not impose an erroneous million-directory traversal cap, and contributor counting is independent of retained top-N rows. Execution on more than one million immediate children was not performed; source-level cap handling does not resolve the time and memory issues above. Ignored scale tests were not rerun.

Validation in this attempt:

- `cargo test --offline -p tidemark-store --test reader_v2 --test diff_walk --test w3c`: passed, exit 0. `diff_walk`: 5 passed/1 ignored (19.13 s); `reader_v2`: 29 passed/1 ignored (223.44 s); `w3c`: 6 passed/1 ignored (0.24 s). Used the pinned Rust 1.98.0 toolchain directly because Cargo was absent from the initial PATH. Ran `cargo clean --offline` afterward.
- Four scratch reproducers compiled with that toolchain against the unmodified integration libraries. All four observation assertions passed: ncdu order mismatch, cross-chunk link ordering/paging failure, missing final file chunk, and four plain-table error categories. These are defect reproductions, not acceptance tests. Query implementation files match the blind candidate exactly.
- Reproducer source and logs are under this attempt's output `library/`: `review_repro_additions.rs`, `review_repro.rs`, `review-repro.log`, `focused-tests.log`. No source or test files in the repository were changed. No privileged commands, branch creation, merges, or pushes were used. Full workspace CI, VM tests, scale tests, and the prescribed §8 benchmark were not run.
