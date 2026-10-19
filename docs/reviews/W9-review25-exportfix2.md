VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 3 / NOTE 0

Reviewed exact candidate `02e1ad50ca6a898fd56ee4e3cdf4416ea32cffc6` against base `75aeb4726bd3ef5c6bb2a7330f470799911d8075`. The supplied checkout `917eccb52a2805a82db2f5054ae00199beb42fa0` has the identical tree, `1151b316b2e4e1313cd46f97dca487569fdbacaa`; executions therefore cover the candidate and the requested `75aeb47..917eccb` diff. Read review24, the fix2 handoff, storage/export/cancellation contracts, and the affected implementation and tests. This verdict is a review recommendation, not verification or integration approval.

**SHOULD-FIX S1 — The quota probe runs after cleanup has freed the failed database's space.**

Reference: `finding:export-quota-probe-after-cleanup`. Location: `crates/tidemark-store/src/export_sqlite.rs:216`–`223`; probe: lines 148–156; supporting destructor: `crates/tidemark-store/src/writer.rs:380`–`384`.

The closure consumes `writer` through `finish_with`. When copying or finalization returns an error, the writer is dropped and its `.db.tmp` is unlinked before `classify_with` invokes the new 1 MiB write probe. A quota that stopped a larger database can therefore admit the smaller probe. The original SQLite error remains `SQLITE_IOERR_WRITE` and exits 1 rather than the advertised space-failure status 5. This can happen without any external process freeing space or the quota being raised.

Reproduced with an unprivileged preload wrapper enforcing a persistent 2 MiB limit on the sum of regular-file lengths in the private staging directory. The source contains 30,000 instances with 200-byte names. The trace records `pwrite(offset=2097152,count=32768)=EDQUOT`, then unlink of `.db.tmp`, then successful `write(space-probe,1048576)` and fsync. The CLI returns **1**, code **778**, with no published output or leftovers. Applying ENOSPC at the same limit returns 5. The original review24 destination-write EDQUOT injection also still returns 1 for hot and actual cold sources when the probe succeeds; forcing the probe itself to fail EDQUOT produces 5. Preserve the original OS failure through a safe boundary rather than relying on a later allocation after cleanup. The synthetic classifier tests do not exercise this lifecycle.

**SHOULD-FIX S2 — Disk-full during writer initialization now bypasses classification.**

Reference: `finding:export-startup-space-classification`. Location: `crates/tidemark-store/src/export_sqlite.rs:182`–`185`, `214`.

The base applied `classify` to the result of the whole `export_inner`. The candidate moves classification inside the copy/finalization closure, but `SnapshotWriter::create(...)?` remains outside it. Its schema/PRAGMA writes can fail with SQLite `DiskFull`, which now escapes as `StoreError::Sql` and exits 1.

Reproduced by injecting ENOSPC on the first staging-database `pwrite64`, offset 0, length 32,768. Both hot and cold exports return **1**, diagnostic **SQLITE_FULL / code 13**, without running the probe. There is no destination or staging residue. This is a regression in a case the base's outer `DiskFull` mapping covered. Apply space classification to initialization as well as later writes, and add an end-to-end startup-write regression.

**SHOULD-FIX S3 — The new instance pages repeatedly scan the same inode's earlier rows.**

Reference: `finding:export-instance-keyset-rescan`. Location: `crates/tidemark-store/src/export_sqlite.rs:40`–`41`, `371`–`388`.

The exact new SQL avoids sorting, but SQLite does not seek on both tuple components here. On the candidate's bundled SQLite **3.51.1**, an ANALYZE'd v1-shaped fixture produces `SEARCH link_instances USING INDEX li_ino (ino>?)`; bytecode `SeekGE` has one key field, followed by `IdxRowid`/comparison/`Next` to reject the earlier rows. Every new 10,000-row page starts at the beginning of the current inode's index range. A single-inode set therefore requires quadratic traversal work, and the growing prefix scan also delays the next cancellation check.

The exact query took **7,070,144 VM steps** for 100,000 instances and **530,971,354** for **1,000,001** instances. The final empty page alone took **10,000,024** steps. On the same bundled library, the base's sorted stream used **18,000,031** steps for the million-row set; a single `ORDER BY ino,rowid` cursor used **8,000,016**. Recorded times were 69.508 / 2.412 / 0.750 seconds respectively, but the VM counts establish the regression independently of shared-host timing. These are source-query measurements, not whole-export timings. Retain a streaming cursor or use a traversal whose complete seek key is supported. The new plan test checks for a missing sorter but cannot detect repeated prefix scans.

**Review24 recheck and regression coverage**

| Review24 item | Result |
|---|---|
| S1 finalization cancellation | Fixed in the exercised paths. SIGINT, SIGTERM and SIGHUP at early writes, each of all seven index commits, ANALYZE, metadata commit, final database fsync and staging-directory fsync before outer publication returned 3 with no output or leftovers, for hot/cold v1 and v2. Cold inflation signals also cleaned up. Signals at parent fsync after publication returned 0 with a valid database. All 162 signal/control cases passed. |
| S2 source sort memory | The original memory growth is fixed. With 200-byte names, Python SQLite 3.53.4 source-query RSS increases were 10,160 / 10,236 KiB at 100,000 / 1,000,001 instances, versus 33,740 / 258,740 KiB for the base SQL. Bundled SQLite confirms no sorter, but exposes S3 above. These measurements do not bound whole-export peak RSS. |
| S3 quota exit classification | Not fixed end to end: see S1. Moving the classifier also introduces S2. Later ENOSPC writes and explicit file-fsync ENOSPC/EDQUOT return 5; unrelated EIO retains exit 1. |

The added cancellation regression targets index presence and the gap after the inner rename; its conditions are reached after the base's last cancellation check, so the base implementation would return success. The new query-plan assertion rejects the base SQL's temporary sorter. The quota tests inject result codes and a probe return value, bypassing both failing lifecycles above. Candidate tests were executed; the complete base binary/test suite was not independently built or run. Base/current SQL were both executed directly against the same fixtures and bundled SQLite.

**Behavior outside export**

The diff changes only `export_sqlite.rs`, its integration test, and the handoff. `reader.rs`, `reader_v2.rs`, `writer.rs`, `writer_v2.rs`, public query bodies and result caps are unchanged. No hot reader query path changed. The new `finish_with` call is confined to export; valid scan publication still uses the unchanged writer implementation. CLI scan-backed export/cold tests and store compatibility/writer tests provide supporting coverage, not a rerun of the reader latency gate.

**Validation and limits**

- `cargo test --offline -p tidemark-cli --test snapshots_export --test snapshots_cold --test surface`: **6 + 3 + 13 passed**.
- `cargo test --offline -p tidemark-store --lib --tests -- --skip state_dir::tests::readable_storage_warns_once`: **244 top-level tests passed, 0 failed, 8 scale tests ignored, one test explicitly filtered**, across 15 top-level harnesses. A nested conversion subprocess also passed its selected test. This includes all 11 export regressions, the four new export unit tests, and reader/writer compatibility, cancellation, corruption and diff suites. The filtered sandbox-sensitive test was skipped as recorded in the handoff; it was not rerun.
- External source-query probes used 100,000 and 1,000,001 instances, ANALYZE, the exact old/new queries, query plans, bytecode, VM-step counts and process RSS. The bundled-library measurements used the candidate's SQLite 3.51.1; the separate Python measurements used SQLite 3.53.4.
- External CLI probes used real signal delivery and preload failures restricted to private staging files. Index signals were delivered at schema-page commit writes before the post-index cancellation checkpoint; sorter-internal timing was not separately intercepted. The effective fault matrix has 54 runs across hot v1, cold v1 and a larger hot v1 fixture; 38 reached an injected failure, with successful controls and unreached variants reported separately. Cold source paths were checked against the fixture catalogs. No actual filesystem quota or space exhaustion was induced.
- Truncate injection was **not reached**: normal traced exports never called `ftruncate`. SQLite fsync/truncate extended-code classifier coverage remains synthetic; the exercised destination fsync is the writer's explicit file sync. Do not treat those attempts as end-to-end truncate validation.
- Rust 1.98.0, cached dependencies, offline, development ancestry mode. `cargo clean --offline` removed build output; external compiled probes and generated query/CLI fixtures were removed, retaining sources and logs. No production code/tests changed, privileged commands, mounts, host configuration changes, branch creation, merges or pushes. No full-workspace CI/clippy, release/strict-ancestry build, VM run, actual quota setup, or syscall-level source-state immutability audit.

Supporting sources and logs are in this attempt's worker-output `library/`: `store-tests.log`, `cli-tests.log`, `fixtures.rs`, `signals-faults.c`, `signals-probe.py`, `signals-results.json`, `signals-fixtures.json`, `faults-quota.c`, `faults-probe.py`, `faults-{hot1,cold1-actual,large1}-results.json`, individual fault logs, `faults-cold-source.json`, `query_probe.py`, `query_bundled.c`, `query-results.json`, `query-bundled-results.log`, and `query-bundled-plan.log`. The earlier `signals-results-first-pass.*` and `faults-cold1-*` runs selected a hot catalog entry and are not counted as cold evidence; corrected reruns supply all cold claims above.
