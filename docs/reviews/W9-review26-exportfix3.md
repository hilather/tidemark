VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 1 / NOTE 0

Reviewed exact candidate `ae718b4a4dc7903ecce75634f170c027515f1ca5` against base `cc4aa13b663d6a216b91f377b21b3b2022be0d87`. The supplied checkout `1075b93a62167120c68fab2ad881ba6659003531` has the identical tree, `f710b266f2f0df94b27708887c292c508d15ee52`; executions therefore cover both the candidate and requested `cc4aa13..1075b93` diff. Read review25, review24, the fix3 handoff, relevant storage/export/cancellation contracts, and affected implementation and tests. This verdict is a review recommendation, not verification or integration approval.

**SHOULD-FIX S1 — SQLite quota errors still exit 1 when the separate write probe succeeds.**

Reference: `finding:export-sqlite-quota-exit` (the destination-write variant retained in review25 S1). Locations: `crates/tidemark-store/src/export_sqlite.rs:186`–`195`, with the new classification calls at lines 295–308 and 322; regression assumption: `crates/tidemark-store/tests/export_sqlite.rs:628`–`633`, 652–654.

The new hooks correctly run classification before deleting the temporary database. They still infer the original syscall's errno from a different allocation. `pwrite64(...)=EDQUOT` becomes SQLite `IOERR_WRITE` (778); when the subsequent 1 MiB probe write and fsync succeed, `classify_with` returns the original `StoreError::Sql`, so the CLI exits **1**, not the advertised quota/space status **5**. EIO and EDQUOT are indistinguishable at this boundary. Preserving the database does not recover that missing information.

Reproduced with an unprivileged preload wrapper restricted to the export's private staging files. On a 30,000-instance fixture with 200-byte names, the copy-phase trace records `pwrite(offset=851968,count=32768)=EDQUOT`, successful `write(space-probe,1048576)` and fsync, then probe unlink and database unlink. The CLI reports code 778 and exits **1**, with no destination or leftovers. The same result occurs at the first initialization write and at each of seven index commits, ANALYZE and metadata writes; hot and actual cold sources reproduce it. Across the targeted matrix, all **31** SQLite-write EDQUOT cases with a successful probe exit 1. ENOSPC at the same stages exits 5; EIO retains exit 1; explicitly making the probe fail EDQUOT changes the quota cases to exit 5.

These are injected syscall failures, **not** a claim that a real persistent shared filesystem quota was configured. The original persistent **2 MiB aggregate staging-file quota model now passes**: its database failure leaves allocation present, the probe fails EDQUOT before cleanup, and the CLI exits 5. That cleanup-order defect is fixed. The separately requested and previously recorded EDQUOT/healthy-probe variant remains. The new regression substitutes a probe that reports EDQUOT merely when `.db.tmp` exists; its successful-probe control assumes code 778 originated from unrelated I/O, which this end-to-end variant disproves. Preserve the originating SQLite OS error through a safe boundary and test both EDQUOT and EIO with a successful probe.

**Review25 recheck and regression coverage**

| Item | Result |
|---|---|
| S1 cleanup before quota probe | Fixed for the persistent 2 MiB aggregate-quota reproduction. Trace proves the probe runs before database unlink and now fails before cleanup releases space. The original EDQUOT/healthy-probe variant remains as S1 above. |
| S2 initialization bypasses space classification | Fixed for SQLite DiskFull. Injecting ENOSPC on the first 32,768-byte staging-database write now exits 5 for hot/cold sources, with no leftovers. Creation's EDQUOT/healthy-probe case shares the remaining S1 limitation. |
| S3 repeated same-inode prefix traversal | Fixed. The same-inode query seeks on both `(ino,rowid)` components; the next-inode query advances through `li_ino`. Neither source query uses a sorter. Counts and ordered row/blob checksums match the reference stream. |

The new copy/finalization and creation regressions pass. Their hooks exercise classifier placement and synthetic SQLite errors, rather than actual failing database writes. The preload reproductions above independently exercise those writes. The complete base binary/test suite was not rebuilt: failing-before evidence for S1/S2 is the recorded review25 reproduction and inspected base control flow, not a claim that the new hook-based tests were executable unchanged on the base.

For S3, both exact base and candidate SQL were executed on the same ANALYZE'd v1-shaped fixtures with the candidate's bundled SQLite **3.51.1**:

| Single-inode fixture, 10,000 rows/page | Base | Candidate |
|---|---:|---:|
| Instances returned | 1,000,001 | 1,000,001 |
| SQLite VM steps | 530,971,354 | 8,991,022 |
| Final empty page VM steps | 10,000,024 | 26 |
| Elapsed source-query time | 63.770 s | 1.646 s |
| Index rows visited, separately instrumented build | 52,500,002 | 1,000,001 |

Row visits were measured by rebuilding the same SQLite 3.51.1 source with `SQLITE_ENABLE_STMT_SCANSTATUS`; VM counts and timings above use the unmodified bundled library. These are source-query measurements, not whole-export timings. Candidate plans show `li_ino (ino=? AND rowid>?)` with a two-field `SeekGT`, and `li_ino (ino>?)` with a one-field `SeekGT`; no temporary b-tree or sorter opcode appears. At 500 rows/page, the million-row candidate uses 9,018,466 steps. The new 30,001-row regression's work limits also reject the old query: base/candidate counts are 1,350,093/260,105 at 10,000/page and 9,929,334/270,142 at 500/page. Mixed-inode fixtures, boundary inode/rowid values and raw/nullable names preserve order and content across page boundaries.

**Review24/25 cancellation and behavior outside export**

All **162** real-signal/control cases pass for hot/cold v1 and v2. SIGINT, SIGTERM and SIGHUP at early writes, each of seven index commits, ANALYZE, metadata commit, final database fsync, and staging-directory fsync return 3 without output or leftovers. Cold-inflation signals clean up. Signals at the destination-parent fsync after outer publication retain exit 0 and a valid exported database. Index signals are delivered at commit writes before the cancellation checkpoint; sorter-internal execution was not separately intercepted. Cold catalog paths were verified as `.db.zst`; an initial signal pass that selected retained hot siblings is excluded from these counts.

No reader query or cap changed. `reader.rs`, `reader_v2.rs`, `writer_v2.rs`, error definitions and the v1 schema are byte-identical between base and candidate. With default hooks, initialization follows the original operations, and the original `finish_with` body is identical to `finish_steps` after its no-op hook. Cancellation checkpoints, close/sync/rename ordering and Drop behavior remain intact for normal scans. Store compatibility/writer tests and scan-backed CLI tests provide behavioral coverage; this is not a rerun of the reader latency gate.

**Validation and limits**

- `cargo test --offline -p tidemark-cli --test snapshots_export --test snapshots_cold --test surface`: **6 + 3 + 13 passed**.
- `cargo test --offline -p tidemark-store --lib --tests -- --skip state_dir::tests::readable_storage_warns_once`: **248 top-level tests passed, 0 failed, 8 scale tests ignored, one test explicitly filtered**, across 15 top-level harnesses. A nested conversion subprocess also passed its selected test. This includes all 13 export regressions, the new query-plan/work-bound tests, and reader/writer compatibility, cancellation, corruption and diff coverage. The sandbox-sensitive readability-warning test was skipped as recorded in the handoff; it was not rerun.
- `cargo fmt --all -- --check` and candidate `git diff --check` passed.
- External fault testing includes the **148-case** targeted lifecycle matrix: 37 ENOSPC cases exit 5, 37 EDQUOT-with-failed-probe cases exit 5, 37 EIO cases exit 1, 31 SQLite EDQUOT-with-successful-probe cases exit 1, and six explicit file/staging-directory fsync EDQUOT cases exit 5. All injected failures leave no output or staging residue. A separate **54-run** replay of the prior fault matrix reached **38** injected failures and includes the persistent-quota model, successful controls and unreached variants. Truncate injection is **unreached**; normal exports do not call it. SQLite fsync/truncate extended-code coverage remains synthetic; the exercised fsync failures are explicit Rust file/directory syncs.
- Rust 1.98.0, cached dependencies, offline, development ancestry mode. `cargo clean --offline` removed build output; generated external probe binaries and databases were removed while retaining sources and logs. No production code/tests changed, privileged commands, real quota setup, mounts, host configuration changes, branches, merges or pushes. No full-workspace CI/clippy, release/strict-ancestry build, VM run, or syscall-level source-state immutability audit.

Supporting sources and logs are in this attempt's worker-output `library/`: `cli-tests.log`, `store-tests.log`, `fmt.log`; `fault-review/` contains fixture/preload sources, `signals-results.json`, `phase-results.json`, `faults-{hot1,cold1,large1}-results.json` and individual traces; `query-review/` contains query/fixture sources, bundled/instrumented measurements and plans; `writer-review/static-comparison.json` records the unchanged reader blobs and finalization comparison.
