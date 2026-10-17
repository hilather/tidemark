VERDICT: BLOCKED
BLOCKER 1 / SHOULD-FIX 6 / NOTE 1

Reviewed exact blind candidate `3059ead47ccda1eaf1b4e7a1b5badec4cd6c615b` against `cab1bdc37abcb9e5ee87712313b38c9841a68f73`. Also reviewed the requested cold-wiring integration at `a64c25bdf15b89f9fbcace55318fa1f58681abbb`, the supplied checkout. The reader implementation, legacy-query adapters and reader tests are identical between these two candidates. Cold wiring is absent from the blind candidate's delta: S4–S6 and N1 below concern the supplemental integration tree and must not be attributed to the reader candidate. The verdict is a review recommendation, not integration or release acceptance.

Read `AGENTS.md`, `15-snapshot-v2.md`, both named handoffs, the relevant link/storage contracts, `git diff cab1bdc..3059ead`, `git diff cab1bdc..a64c25b`, and the requested integration log. Only this report changes the repository. Probes and logs are in the current attempt's output `library/`, described below.

## Findings on the exact reader candidate

### B1 — BLOCKER: overlapping chunks return cache-dependent paths and totals

Reference: `finding:w9-review5-snap3-b1`. Location: `crates/tidemark-store/src/reader_v2.rs:127` and `:133`.

The cache accepts any chunk covering the requested ID; a miss chooses the greatest `first_id <= id`. Loading a chunk verifies its own decoded endpoint but never rejects overlap with another chunk. Consequently a structurally corrupt snapshot can produce successful, contradictory answers rather than `Invalid`.

Reproduced with an original chunk covering IDs 0–3 and a second, individually codec-valid chunk covering 2–3. The alternate row 2 has a different valid name and `t_disk=999`. Two consecutive calls on one fresh reader returned:

```text
dir(2): path=/data/\xFF/b000001-alt t_disk=999
dir(2): path=/data/\xFF/b000001     t_disk=2
```

The first lookup chooses the later chunk; path reconstruction loads parent 0 from the earlier chunk and changes cache order. The second lookup then selects that earlier chunk. This is a corrupt-input case, not a claim that the writer generates overlaps. It meets the review's wrong-data blocker criterion and violates the required corruption behavior. Validate neighboring chunk intervals before admitting/using a chunk, with regressions for both cold and warmed caches and range queries. Evidence: `reader-review-probes.rs`, `reader-review-probes.log`.

### S1 — SHOULD-FIX: shallow subtree queries decode every descendant chunk

Reference: `finding:w9-review5-snap3-s1`. Location: `crates/tidemark-store/src/reader_v2.rs:536`–`:548`.

`subtree_dirs` always passes the whole subtree interval to a projected or full chunk scan, even when the requested absolute depth is covered by `plain_depth`. Thus root `top` at depth 2 with `plain_depth=2` still decompresses every directory chunk in a multi-million-directory snapshot. The shallow SQL fast path promised by §3 is used for children/metrics, but not this method.

A sentinel probe corrupted only the depth-4/5 chunk of a depth-12 tree: v1 returned the three requested depth-0/1/2 rows, while the v2 shallow query decoded the excluded chunk and returned `Invalid`. This establishes unnecessary access; it is not a latency benchmark. Use `shallow_dirs` for eligible subtree queries while preserving v1 ties, then run the §8 representative latency gate. Evidence: `reader-review-probes.log`.

### S2 — SHOULD-FIX: compressed-input size is checked after allocation

Reference: `finding:w9-review5-snap3-s2`. Location: `crates/tidemark-store/src/reader_v2.rs:133`, `:627`; `crates/tidemark-store/src/chunk.rs:261`.

Both chunk fetch paths convert the entire SQLite BLOB into `Vec<u8>` before the codec enforces its 41 MiB input ceiling. The 8 MiB cache limit does not bound this allocation. A corrupt but SQLite-valid `zeroblob(67108864)` in the root chunk was rejected only after a fresh reader subprocess reached `VmHWM: 140440 kB`. Larger blobs can impose correspondingly larger allocations; no allocation-failure reproduction is claimed.

Reject oversize `length(data)` before materializing the BLOB, for both single-chunk and projected queries. Preserve the codec's independent validation. Evidence: `reader-review-probes.rs`, `reader-review-probes.log`; the measured child only opened/read an already-created fixture.

### S3 — SHOULD-FIX: corrupt SQL chunk coordinates escape as `Sql`

Reference: `finding:w9-review5-snap3-s3`. Location: `crates/tidemark-store/src/reader_v2.rs:133`, `:622`–`:623`; metadata loading at `:116` has the analogous missing-key issue.

Persisted fields are read with fallible SQL conversions and propagated directly. After `UPDATE dir_chunks SET last_id=-1 WHERE first_id=0`, opening succeeds and `root()` returns `Err(Sql(IntegralValueOutOfRange(1, -1)))`, not `StoreError::Invalid`. Missing required v2 metadata similarly leaks `QueryReturnedNoRows`. Normalize malformed stored values/required metadata into `Invalid`, while retaining the deliberate v1-compatible missing-directory error for a legitimate caller ID. The negative-bound case was reproduced in `reader-review-probes.log`; no panic was observed.

## Supplemental integration findings

### S4 — SHOULD-FIX: prune reports deletion but rebuild restores the surviving form

Reference: `finding:w9-review5-snap3-s4`. Location: `crates/tidemark-cli/src/cmd/snapshots.rs:328`–`:338`, with rebuild at `:201`.

A crash between publishing `.db.zst` and unlinking `.db` legitimately leaves both files. Prune removes only the catalog-selected form plus `.bad` siblings; it neither reconciles the pair first nor removes the other published form. Its subsequent catalog rebuild rediscovers the survivor.

Reproduced with two complete scans, both forms of the older scan, and `recent=1,daily=0,weekly=0`. Prune exited 0 and included the old ID in `pruned`, but the cold file and old catalog row remained: two rows after pruning. Delete/reconcile both trusted published forms of a selected scan and test both catalog-pointer directions. The automatic deletion helper at `crates/tidemark-scan/src/orchestrator.rs:1190` shares the omission, although pre-scan cleanup normally resolves equal pairs first. No loss of a retained snapshot was demonstrated. Evidence: `cli-review-probes.log`.

### S5 — SHOULD-FIX: optional size collection makes all snapshot listing fail

Reference: `finding:w9-review5-snap3-s5`. Location: `crates/tidemark-cli/src/cmd/snapshots.rs:389`–`:391`.

`entry_sizes` propagates missing-file and cold-header errors, so a single problematic entry prevents both text and JSON listing. Listing does not hold the scan lock: a daemon can cold-publish/unlink a hot file after `catalog.list()` captures its old filename and before this stat. That race can therefore fail an ordinary list operation even though a valid replacement exists.

The deterministic missing-file case was reproduced: two catalog entries, one hot file removed, list exits 1 with empty stdout instead of listing the healthy entry and available metadata. The actual concurrent interleaving was established by source inspection, not a timing-dependent test. Use a bounded refresh or unknown sizes plus a diagnostic. Add missing-file and corrupt-header coverage. Evidence: `cli-review-probes.log`.

### S6 — SHOULD-FIX: packaged cold-default comments break the upgrade-fixture gate

Reference: `finding:w9-review5-snap3-s6`. Location: `packaging/config/tidemark.toml:95`–`:96`; failing comparison `xtask/tests/w6c.rs:442`.

The new comments change the current packaged config while the test still derives an exact historical fixture from that file. `cargo test --offline -p xtask --test w6c upgrade_fixture_options_and_drift` failed at that comparison, independently confirming the handoff's known gate failure. Historical config parsing itself passes. Update the test's historical-compatibility expectation without rewriting away the historical scenario; the current integration cannot be reported as having a clean workspace test gate. Evidence: `upgrade-test.log`.

### N1 — NOTE: normative quarantine policy and new operator behavior disagree

Reference: `finding:w9-review5-snap3-n1`. Location: `docs/plan/15-snapshot-v2.md:58`–`:60`; `crates/tidemark-scan/src/orchestrator.rs:1197`.

The v2 contract says automatic retention never removes `.bad` leaves, while the new helper removes quarantine siblings of deleted scans and the other updated docs describe that behavior. The cold handoff reports a task requirement for this behavior, but that statement does not reconcile the normative document. Record the policy resolution explicitly. The same v2 paragraph's catalog-only-list/no-quarantine-prune description is also stale at the integration tree. This review does not infer authorization to change the contract.

## Checks and limits

All Cargo execution was offline, using Rust 1.98.0, host dependency caches, and the isolated target directory `/tmp/tidemark-review-716c0c4a-target`. Tests ran on integration `a64c25b`; equality of the six reader implementation/test files with the exact candidate was checked separately. These are not claims of an exact-candidate full-workspace build.

| Validation | Observed result | Evidence |
|---|---|---|
| Store `reader_v2` and `cold` integration tests | 9 + 25 passed | `store-tests.log` |
| Store `storage`, `w3a`, `w3b`, `w3c` regressions | 33 + 7 + 7 + 6 passed; four existing scale tests ignored | `v1-tests.log` |
| Scan `cold_retention` | 3 passed | `scan-tests.log` |
| CLI `snapshots_cold` | 1 passed | `cli-tests.log` |
| Core `config_load` | 10 passed | `config-tests.log` |
| Focused upgrade-fixture gate | 1 failed, exit 101 | `upgrade-test.log` |
| Reviewer probes, compiled outside the repository against built libraries | Reader findings above reproduced; CLI probes 2 passed | `reader-review-probes.log`, `cli-review-probes.log` |
| Required build cleanup | `cargo clean --offline`, exit 0; 4.4 GiB removed | `cargo-clean.log` |

The reviewer probe assertions deliberately confirm observed defects; their passing status does not mean those defects are fixed. An initial allocation-measurement probe failed because `/usr/bin/time` is unavailable; the retained `reader-review-probes-initial.log` records that harness failure. The final probe reads the child process's `/proc/self/status` instead. No repository test or implementation was edited, and the standalone probe binaries were removed after use.

Inspection and passing fixtures support valid-input ordering/ties/filter/paging/path/hash parity, including the existing 50k-directory, deep-child, hash-collision and empty-list cases. Deep children hop siblings; `dir_by_path` probes hash candidates and their ancestor chains. The retained chunk cache itself enforces 32 chunks/8 MiB. Deferred query adapters reject valid v2 invocations before legacy SQL; v1 query regressions passed. No valid-writer arithmetic overflow or panic was established.

Cold publication follows compress/fsync → NOREPLACE rename/directory fsync → FULL catalog update → trusted unlink/directory fsync under scan ownership. Compression refusal/failure preserves the published scan; existing cancellation/space tests passed. Deletion is confined to trusted snapshot directories and no-follow leaves. Config defaults, boundaries, unknown fields, old config loading, decoded-length estimates and additive list fields have passing coverage. The quarantine-collision starvation regression passed in the 25-test cold suite; its continuation fix exists at integration `a64c25b`, not the exact reader candidate.

No full workspace CI, clippy, privileged execution, Rocky VM, crash/power-cut campaign or §8 2M-directory latency/RSS benchmark was run. Cancellation after entry into post-compression validation was not stress-tested. The supplied handoff's remaining gates are not independently accepted here. No privileged commands, source-checkout edits, branch creation, merge or push were performed.

Evidence directory: `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-716c0c4a975d46ec09a56a3da530a02ddead2314cec2bdee2feb6ec52a442847/library/`. It also contains `candidate.diff`, `integration.diff`, `integration-log.txt`, the probe sources and artifact hashes. Review receipt findings for the exact blind candidate are B1 and S1–S3 only; supplemental findings remain explicitly scoped to the integration report.
