VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 1 / NOTE 0

Reviewed exact candidate `9e86c4e77ab9c8ee17d33169cd7cc6a482972128` against base `316e3292b6368ea7a2375bd84cc052737343ee3a`. The supplied checkout `c153620fea380c369def44f482c03cd4f761ecc8` has the identical tree, `13e2996cc81a53e208d904567189059ae5608706`; repository tests ran there. Inspected the requested `316e329..c153620` diff, review16, the fix5 handoff, reviews 8–15 and relevant link/storage/v2 contracts. This report is evidence, not integration or release approval. No production source or repository tests were edited.

**S1 — SHOULD-FIX: eager root validation can decode the requested chunk twice within a fresh nonempty query.**

Reference: `finding:w9-review17-gate8fix4-s1`. Location: `crates/tidemark-store/src/reader_v2.rs:1168`–`:1171`; cache eviction at `:446`–`:449`.

`raw` initially decodes the requested directory's chunk. The newly eager `path` call then loads its ancestors before `scan` starts. If those ancestors exceed the 32-chunk or 8 MiB LRU bound, they evict the requested chunk, and `scan` decompresses that chunk again. Previously, the scan already held the chunk's `Arc` while the first admitted row triggered ancestor reconstruction. The original pre-rewrite reader consumed the cached requested chunk during its range scan before materializing results and loading ancestors.

Reproduced with fresh readers and identical results in all three revisions:

| Fixture and query | Original `7cde8e6` | Immediate base `316e329` | Candidate |
|---|---:|---:|---:|
| Chain IDs 0–32, one directory per chunk; `wide_dirs(32,0,1)` | 33 | 33 | 34 |
| Chain IDs 0–33, one directory per chunk; `wide_dirs(33,0,1)` | 34 | 34 | 35 |
| Default 4,096-directory chunks, 32,769 directories; `wide_dirs(32768,0,1)` | 9 | 9 | 10 |

The default-size fixture has `plain_depth=2`, root `/r`, and a depth-eight chain of final children named `z`. Each chain ancestor has 4,095 preceding leaf children named `a0000` through `a4094`. Chain IDs are 0, 4096, …, 32768. Each chain span ends at 32768; other leaves end at their own IDs. The last `z` is a leaf with `entries=1`. Its eight full ancestor chunks exceed the byte cap and evict its initially loaded chunk. The query returns that one leaf, including the same full path and every other `DirRow` field as v1.

This is additional work beyond both the original implementation and the accepted decisions: the query is fresh, has limit 1 and returns a row; both comparison implementations already validate the same ancestors. The extra decode rereads the requested chunk, rather than providing newly required ancestor validation. No repeated-query or empty-result exception is being reopened. Retain the already decoded requested chunk across validation, or otherwise avoid rereading it while keeping the eager rejection checks. This is a decode-work defect under the stated bound; no elapsed-latency gate failure is claimed.

**Review16 S1 is fixed; no dropped rejection was observed.**

The requested-root corruption matrix ran 10,130 calls per revision on separate copied snapshots with fresh readers, across plain depths 0–4. Chunk mutations use the real codec to re-encode the changed row while preserving envelopes and other rows. Both chunk-only and synchronized shallow-copy variants cover empty/dot/dot-dot/slash/NUL component names, malformed full-root paths, requested and ancestor parent/depth/span changes, and malformed ancestor payloads. Queries cover `wide_dirs` limits 0/1 and thresholds 0/2, `subtree_dirs` relative depths 0/1/4 and limits 0/1, `metrics_candidates` depths 0/4 and limits 0/1, and leaf/internal `children` at both limits. Filtered-empty child pages are recorded without reopening their accepted narrowing. Subtree and metrics APIs have no entries filter; zero-limit calls exercise their empty output.

In particular, ID 40 name `a/b`, ID 0 name `/r\0bad`, and ID 40 parent 0 now return `Invalid` for wide queries at both limits and when every row is excluded by `min_entries=2`, at every tested plain depth. Subtree queries reject these changes at both limits; metrics rejects the corrupt scan root at both limits. The candidate adds 720 rejections over the immediate base, all in wide queries, and loses none in the exercised matrix. The summary script separately asserts 230 expected rejections, including review15 S2's shallow ID 41 omission/type/depth examples and chunk 32 payload corruption at limits 0 and 1.

The handoff's empty-children gap remains unchanged: a malformed requested leaf name can still yield an empty success at limits 0 and 1, and a malformed internal parent's name can succeed at limit 0. All three tested revisions behave identically. A nonempty limit-1 listing of that internal parent rejects it. This retained behavior is not a new finding. Prior sibling/name/completeness, root-copy, chunk-envelope, file/link, mixed-format diff and ncdu checks remain covered by the inspected code and focused tests; this is not an exhaustive malformed-input proof.

**Valid results and work matrix.**

The balanced 121-directory matrix ran 9,280 fresh queries per revision: plain depths 0–4; nine root/internal/leaf IDs; all eight sorts; relative depths 0/1/2/4/`u32::MAX`; limits 0/1/3/1000; wide thresholds 0/1/2. Another 756 queries exercise chains of depths 30/31/32/33/34/64/128 with chunk sizes 1/2/4, and six queries use the default-size fixture above. All 30,126 full-result comparisons with v1 passed, and result digests agree across the three revisions. Metrics compares both rankings and eligible count, checking each snapshot's own filename separately.

The balanced matrix has no work regression outside the accepted decisions: 962 increases over original zero-limit work fit the same candidate query's limit-1 work, and 45 nonzero filtered-wide increases are requested-root validation. The added fixtures expose 25 nonempty fresh-query cases of S1. Representative balanced counts:

| Query, plain depth 0 | Original | Immediate base | Candidate |
|---|---:|---:|---:|
| `wide_dirs(40,2,1)` | 2 | 1 | 3 |
| `wide_dirs(120,2,1)` | 2 | 1 | 4 |
| `children(40,Name,1)` | 1 | 1 | 1 |
| `subtree_dirs(0,4,Disk,0)` | 1 | 8 | 8 |
| `subtree_dirs(40,0,Name,0)` | 1 | 3 | 3 |
| `metrics_candidates(4,0)` | 8 | 8 | 8 |

**Validation and limits.**

- `cargo test --offline -p tidemark-store --lib --test reader_v2 --test diff_walk --example snapbench --message-format=json`: exit 0; library 57 passed/1 ignored, reader integration 42/1, diff walk 10/2, example 2/0. Total 111 passed, zero failed, four ignored scale tests. The example runs its small tests; the 2M benchmark was not run.
- Five scratch tests per revision passed: balanced results/work, deep-cache results/work, default-chunk results/work and two corruption observation tests. The passing observation tests do not establish acceptance of S1. `compare_matrices.py` asserts the cross-revision results, retained rejection cases and exact finding counts.
- Scratch harnesses extract each revision's `reader_v2.rs`, `reader.rs`, `chunk.rs` and crate entry point, reference unchanged remaining checkout modules, and add tests plus a counter at `Columns::open` for directory chunks. Counts include ordinary, projection and streaming decompression. No production behavior is substituted. Sources, build commands/logs, three sets of work/corruption logs, `matrix-summary.log`, regression lists and `focused-tests.log` are retained in this attempt's output `library/`.
- Used pinned Rust 1.98.0 and the existing offline Cargo cache. No privileged commands, VM/release tests, ignored scale runs, full workspace CI, fmt/clippy or latency/RSS/file-size benchmark were run. No release-readiness conclusion follows from these checks.

`cargo clean --offline` completed (2,036 files / 1.0 GiB removed); all three scratch executables were removed. `git diff --check` is clean. The only repository change is this report. No branches, merges or pushes were created.
