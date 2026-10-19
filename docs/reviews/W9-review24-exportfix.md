VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 3 / NOTE 0

Reviewed candidate `ea17ae675956ee8937c516462d91005abe1212ed` against base `d905c47b5f7782f62b4900cb8fc2a29237a0a0ec`. The supplied checkout `9bb94f8cd3d1b1501c8cbaf9afd5b00987f6c19d` has the identical tree, `73f2e5f9305b7f20a0df1fa3842486c1ea1aba7b`; executions therefore cover the exact candidate. Reviewed `git diff d905c47..9bb94f8`, review22, the fix handoff, the v1/v2 storage and writer-cancellation contracts, and the affected reader/writer/CLI paths. This verdict is a review recommendation, not verification or integration approval.

**SHOULD-FIX S1 — Signals during finalization are swallowed and the export is published.**

Reference: `finding:export-finalization-cancellation`. Location: `crates/tidemark-store/src/export_sqlite.rs:179`–`188`; supporting implementation: `crates/tidemark-store/src/writer.rs:313`–`315`.

The last cancellation check precedes `writer.finish(&meta)`. That method deliberately supplies an always-false callback to `finish_with`, bypassing the existing checkpoints after each index and immediately before publication. Export then performs its outer rename without another check. The new CLI handlers suppress normal termination throughout this interval.

Reproduced with a preload wrapper delivering SIGINT, SIGTERM, or SIGHUP at the staging `.db.tmp` file's fsync, before either publication rename. For every signal, on both hot and cold sources, the CLI returned **0**, printed success, and published a valid output. The original staging/inflate leftovers did not recur. As a control, delivering the same signals during early destination writes returned **3** with no output or temporary leaves; cold-inflation signals also returned 3 after inflation completed. Pass cancellation through writer finalization and check before the outer publication, preserving success only after publication has occurred. The existing signal regression accepts success and does not exercise finalization.

**SHOULD-FIX S2 — The v1 instance stream still materializes a whole large link set inside SQLite.**

Reference: `finding:export-link-sort-unbounded`. Location: `crates/tidemark-store/src/export_sqlite.rs:324`–`328`; supporting settings: `crates/tidemark-store/src/reader.rs:229` and `crates/tidemark-store/migrations/snapshot-v1.sql:79`–`80`.

The new uncapped query orders by `ino,dir_id,name,rowid`. The v1 schema has only single-column `li_ino` and `li_dir` indexes, and the source connection forces `temp_store=MEMORY`. With all rows sharing one inode, SQLite sorts the entire set before returning the first row; the 10,000-row Rust buffer does not bound that memory or permit cancellation during the sort.

The exact schema/query reproduction with 200-byte names added **24,820 KiB** RSS for 100,000 rows and **249,812 KiB** for 1,000,001 rows before fetching the first result. This RSS measurement used Python SQLite 3.53.4, not the full exporter. Independently, the candidate's bundled SQLite **3.51.1** produced the same query plan on an exported v1 database: `SCAN link_instances USING INDEX li_ino` followed by `USE TEMP B-TREE FOR LAST 3 TERMS OF ORDER BY`. Larger valid stored sets therefore scale memory with their row count. The old convenience query also sorted; the cap refusal is fixed, but the proposed bounded replacement is incomplete. Use an index-supported traversal such as inode/rowid order where permitted, or a bounded external sort, without changing public query behavior.

**SHOULD-FIX S3 — Quota exhaustion during SQLite writes still exits 1.**

Reference: `finding:export-sqlite-quota-exit`. Location: `crates/tidemark-store/src/export_sqlite.rs:113`–`120`; related mapping: `crates/tidemark-cli/src/cmd/snapshots.rs:398`–`405`.

The new classification recognizes SQLite `DiskFull` and raw I/O `EDQUOT`, but SQLite translates a failed `pwrite64(...)=EDQUOT` into `SQLITE_IOERR_WRITE` (extended code 778), not `DiskFull`. Reproduced by injecting EDQUOT only for destination staging-file writes: CLI exit **1**, diagnostic `disk I/O error: Error code 778`, with no final output or staging leftovers. Identical ENOSPC injection returned **5**, as did ENOSPC at staging-file fsync. Preserve/classify SQLite's underlying quota error so this advertised space failure also exits 5; do not classify every unrelated SQLite I/O error as exhaustion.

**Review22 recheck and regression coverage**

| Review22 item | Candidate result and evidence |
|---|---|
| B1 staging collision | Original old-name regression passes. Additional injection at the actual new random staging name forced existing directory+sentinel, dangling symlink, and FIFO collisions; all were preserved and export retried successfully. |
| S1 relative JSON | Regression passes: relative output publishes and JSON succeeds. |
| S2 interruption leftovers | Existing tests pass; direct probes at inflation/early destination writes clean both temporary copies. Finalization cancellation remains defective as S1 above. The CLI regression permits exit 0 without requiring signal delivery. |
| S3 wide file buffer | The callback now flushes at 10,000 rows by inspection. Additional v2 probe exported 20,001 files in one directory. There is still no regression measuring/enforcing the buffer bound; the preexisting equality test also passed before the fix. |
| S4 orphan v2 instances | Regression passes. Additional large v2 probe preserved four orphan instances alongside a populated set. |
| S5 large link set | Existing v1 1,000,001-instance regression passes; additional v2 probe exported 1,000,001 instances. Both retain the public query cap. The v1 SQLite sort remains unbounded as S2 above. |
| S6 space exit status | Cold-source floor regression, ENOSPC SQLite-write injection, and ENOSPC file-fsync injection all return 5. SQLite EDQUOT remains defective as S3 above. Publication-rename ENOSPC was not executed: the sandbox refused unprivileged child tracing with EPERM. Linux publication uses `renameat2`, not a link syscall. |
| S7 malformed v1 | Both regressions pass. Additional duplicate root/child IDs, duplicate paths, negative counters, out-of-range IDs, missing tables and missing columns returned errors without export panics or leftovers. |
| N1 RPM manual | Manifest includes `tidemark-snapshots-export.1`; fixed by inspection. No RPM or packaging regression was run. |

The requested failing-before regression coverage for every item is not present: the handoff explicitly lacks a bound test for S3, N1 has no regression, and S2/S6 omit the failing variants above. This review ran candidate tests and inspected their relationship to the base changes; it did not independently build/run the base. Review22 supplies the recorded original reproductions.

**Behavior outside export**

No public reader query body or result cap changed. `reader.rs` adds only an internal export adapter; `reader_v2.rs` adds an internal chunk visitor and widens one helper's internal visibility. Existing hot query paths were not changed. `writer.rs` changes only rejection of non-increasing directory IDs: its former debug assertion becomes an error using the existing rollback/poisoning path. Valid increasing writes retain their behavior. The scan-backed CLI tests and the reader/writer tests below exercise valid inputs.

**Validation and limits**

- `cargo test --offline -p tidemark-store --test export_sqlite -p tidemark-cli --test snapshots_export --test surface --test snapshots_cold`: **10 + 6 + 13 + 3 passed**.
- `cargo test --offline -p tidemark-store --lib --tests -- --skip state_dir::tests::readable_storage_warns_once`: **239 top-level tests passed, 0 failed, 8 scale tests ignored, one test explicitly filtered** across 15 top-level harnesses. A nested conversion subprocess also passed its selected test. The filtered test is the sandbox limitation recorded in the handoff; it was not rerun. Reader v2 compatibility, valid storage/writer behavior, cancellation, corruption, and diff suites passed.
- External Rust probe: 20,001 single-directory files, 1,000,001 instances in one set, four orphan instances; exported counts matched, public query stayed capped, bundled SQLite query plan confirmed.
- External preload probes: actual staging collisions, SIGINT/SIGTERM/SIGHUP during inflation/early destination writes/final fsync, ENOSPC write/fsync, EDQUOT write. Fixtures and injections affected only this attempt's private files and processes. Malformed-schema probes returned `Err`; their harness exit 0 means the probe completed, not that export succeeded.
- Rust 1.98.0 and cached dependencies, offline; development ancestry mode. No production code/tests changed, privileged commands, mounts, host configuration changes, branches, merges or pushes. Build output was removed with `cargo clean --offline`; external probe executables were also removed, retaining their sources and logs.

No full-workspace CI/clippy, release/strict-ancestry build, VM run, RPM build, actual filesystem exhaustion, or syscall-level source-state immutability audit was performed. The allocator measurement covers the source SQL sorter, not whole-export peak RSS. ENOSPC at publication remains inspection-only after the tracing restriction. Supporting sources and logs are under this attempt's worker-output `library/`: `targeted-tests.log`, `store-tests.log`, `faults.c`, `fault_probe.py`, `fault-results.json`, `variants.py`, `cold-results.json`, `malformed_probe.py`, `malformed-results.json`, `reader_sort_probe*`, `v2_stream_probe*`, and the refused `rename_fault.py` probe.
