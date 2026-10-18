VERDICT: BLOCKED
BLOCKER 1 / SHOULD-FIX 7 / NOTE 1

Reviewed candidate `65fb3eab818f04097d4296202899c521727772ac` against `21846e540ba7aca2b9deb5a3f35e36fe3e0fc5e4`: the three `SNAP-export-sqlite:` commits `84152a1`, `addad67`, and `65fb3ea`. The provided checkout is integration `cd83d7951e1ed15a77a08500a9f71aadcf8d89d5`; its export code and store dependencies are identical to the candidate. Only later scan-orchestrator changes, their test, and their handoff differ. Those changes were excluded from review. Executions below used this checkout, so the CLI test executable also contains those later scan changes.

Read the export handoff, snapshot-v2 contract §§7/9, relevant v1 schema/write/read contracts, CLI exit-code contract, and affected reader/writer/CLI paths. Findings below are reproduced unless explicitly marked as source-proven. The verdict is a review recommendation, not an integration or verification decision.

**BLOCKER B1 — A staging-name collision deletes an existing directory.**

Reference: `finding:export-staging-collision`. Location: `crates/tidemark-store/src/export_sqlite.rs:77`–`84`, cleanup at `:41`–`43`.

`Staging` owns the deterministic path before `create` succeeds. When `.tidemark-export-<pid>-<scan_id>` already exists, `create` returns `AlreadyExists`, then the guard recursively removes that existing directory. Reproduction: create that directory with a `preexisting-sentinel` file, then export a valid snapshot to a different, absent filename in the same parent. Result: `Err(Io(... AlreadyExists ...))`, but `preexisting_marker_survived=false`. An existing directory from another operation, or a concurrent same-process export of the same scan, can lose its contents. Establish cleanup ownership only after successful exclusive directory creation; a collision must leave the existing entry untouched.

**SHOULD-FIX S1 — JSON output fails after publishing a relative destination.**

Reference: `finding:export-relative-json`. Location: `crates/tidemark-cli/src/cmd/snapshots.rs:225`.

Run `tidemark --config CONFIG --json snapshots export --sqlite ID relative.db` from a writable directory outside state. Export completes, but the JSON branch passes the original relative argument to `RawPath::from_path`, which requires an absolute path. Observed exit status **1**, diagnostic `path must be absolute and contain no NUL`, and an existing `relative.db` with `user_version=1`. Retrying now encounters an existing output. Resolve the response path before publication, or serialize the resolved absolute destination.

**SHOULD-FIX S2 — Ordinary termination signals leave both temporary copies.**

Reference: `finding:export-interruption-cleanup`. Location: `crates/tidemark-cli/src/cmd/snapshots.rs:217`–`221`; `crates/tidemark-store/src/export_sqlite.rs:41`–`43`, `:90`–`91`.

Export installs no SIGINT/SIGTERM cancellation path and relies on destructors. Using a cold synthetic snapshot with 200,000 notable-file rows, wait until the staging `.db.tmp` appears and send SIGTERM. The CLI exits from signal 15 without a final output, leaving `.tidemark-export-<pid>-<id>/<id>.db.tmp` beside the output and `<id>.<pid>.0.db` in the source `.inflate` directory. Both leftovers were observed. This is a normal interrupt, not SIGKILL or a power-loss scenario. Wire cancellation through copying, inflation, and writer completion so normal termination unwinds cleanup.

**SHOULD-FIX S3 — The v2 file buffer is not bounded by BATCH.**

Reference: `finding:export-wide-file-buffer`. Location: `crates/tidemark-store/src/export_sqlite.rs:143`–`164`.

Source-proven: the callback pushes every file returned by `v2_dir_files`, while the `files.len() >= BATCH` check runs only after the entire directory has streamed. `reader_v2.rs:1958`–`1989` visits the full stored `file_count` without a per-directory row cap. A directory with two million notable files and a suitably configured `max_notable` accumulates all two million `FileRow`s and copied names before its first flush; the allocated vector capacity remains afterward. The row/config contract permits this size. Flush inside the callback to enforce the advertised 10,000-row bound. No peak-RSS measurement was made for this case.

**SHOULD-FIX S4 — v2 link instances without link_sets rows are silently omitted.**

Reference: `finding:export-v2-orphan-instances`. Location: `crates/tidemark-store/src/export_sqlite.rs:201`–`216`.

The exporter visits instances through `link_sets`, then copies leftover instances only for v1. Reproduction uses the public `SnapshotWriterV2`: write the normal six-directory fixture, four `LinkInstanceRow`s, no link sets, and finish. The writer and reader accept it; `link_instances(2000)` returns two rows. Export returns success with `link_instances: 0`, and SQL confirms the output contains zero instance rows. This is a writer-produced synthetic input; occurrence in an actual interrupted scan was not established. The corresponding v1 case is expressly preserved by the leftover pass. Copy all stored v2 instances, or reject an unsupported relationship explicitly instead of successfully dropping rows.

**SHOULD-FIX S5 — A query result cap prevents exporting a larger stored link set.**

Reference: `finding:export-large-link-set`. Location: `crates/tidemark-store/src/export_sqlite.rs:202`; cap in `crates/tidemark-store/src/reader.rs:1043`–`1055`.

Reproduction: a synthetic v1 database with one link set whose `nlink`/`found` are 1,000,001 and that many distinct named instances. Export returns `Invalid("query exceeds 1,000,000-row result cap; narrow the query")` instead of producing the database. These counts fit the frozen u32 row contract; the export interface has no query-narrowing option. This was a database fixture, not a million-link filesystem experiment. The handoff acknowledges the limitation, but it remains inconsistent with exporting any snapshot. Use bounded instance paging/streaming, rather than the convenience API that materializes and caps a whole set.

**SHOULD-FIX S6 — Space failures do not consistently exit 5.**

Reference: `finding:export-space-exit-code`. Location: `crates/tidemark-cli/src/cmd/snapshots.rs:217`–`218`, `:382`–`393`.

With a valid cold snapshot and valid `[retention] min_free_pct = 100.0`, export reports `state space exhausted: state filesystem free-space floor would be crossed` and exits **1**. The error from `open_snapshot` bypasses `export_error`. Separately, source inspection shows that `StoreError::Sql(SqliteFailure(SQLITE_FULL, ...))` from destination SQLite writes also falls through as a runtime error; only `Space` and raw I/O ENOSPC are mapped. This contradicts the documented full-disk/precondition status **5**. Apply the mapping during source preparation as well as export, including SQLite's disk-full classification. Actual destination-disk exhaustion was not injected.

**SHOULD-FIX S7 — An untrusted v1 schema can panic in a debug build.**

Reference: `finding:export-malformed-schema-panic`. Location: `crates/tidemark-store/src/export_sqlite.rs:127`–`136`, calling the assertion at `crates/tidemark-store/src/writer.rs:123`–`125`.

Reproduction: copy a v1 fixture, replace `dirs` with `CREATE TABLE dirs AS SELECT * FROM old_dirs`, append a second root row, and drop `old_dirs`. This intentionally removes the primary-key constraint; it is malformed input, not a normal scan. Snapshot opening succeeds, but export panics with `directory ids must increase`, process status **101**. Validate the input ordering/uniqueness before calling the writer, or return an error from the writer invariant. This finding is limited to builds with debug assertions; no release-build panic was established.

**NOTE N1 — The RPM manifest omits the new command's manual.**

Reference: `finding:export-rpm-manpage`. Location: `packaging/rpm/rpm.toml:37`–`41`, introduced command at `crates/tidemark-cli/src/cmd/snapshots.rs:32`.

Source-proven: the manifest explicitly lists the other snapshots subcommand manuals but not `tidemark-snapshots-export.1`. `xtask/src/man.rs:35`–`38` generates a page for every visible command, so the export manual is generated but excluded from the RPM assets. This is the unresolved packaging follow-up already identified in the handoff. No RPM was built.

Validation performed:

- `cargo test --offline -p tidemark-store --test export_sqlite`: **4 passed**. Covers fixture row equality across v1/v2, byte paths/hashes, every side table/meta, v1 indexes/version/mode, existing file and dangling-symlink refusal, ordinary error cleanup, and hot/cold equivalence.
- `cargo test --offline -p tidemark-cli --test snapshots_export --test surface`: **3 + 13 passed**. Covers CLI scan/export/open, mode 0600, basic refusal/errors, JSON with an absolute output, cold cleanup on success, and help/argument surface.
- Added external probes only; repository source/tests were not edited. A regular file created after staging began was preserved and caused exit 1; a symlink created at the same point and its target were also preserved. Both race failures removed the staging directory. This supports the final `RENAME_NOREPLACE` behavior.
- The collision, relative JSON, signal, orphan-instance, large-link-set, malformed-schema, and cold-space results above were executed against temporary fixtures. Probe source and logs are retained in this attempt's worker-output `library/` directory.
- Cargo used the installed 1.98.0 toolchain and cached dependencies, offline. The environment required an explicit toolchain PATH and `CARGO_HOME=/home/brewerm/.cargo`. No privileged commands, mounts, host configuration changes, branches, merges, or pushes were used. Build output was removed with `cargo clean --offline` after testing.

Limits: no full-workspace suite, clippy, release/strict-ancestry build, VM run, arbitrary corruption fuzzing, or large-tree memory benchmark was performed. Tests ran in the sandbox's development ancestry mode. Catalog access is read-only by inspection; cold opening deliberately creates its guarded temporary inflate leaf. Persistent state/catalog immutability was not separately traced at syscall level. The original coordinator benchmark table is not available as independent evidence in the supplied material, so faithful transcription of `docs/perf/snapbench.md` could not be established; no benchmark was rerun or performance conclusion independently claimed.
