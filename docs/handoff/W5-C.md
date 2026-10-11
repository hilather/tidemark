# Handoff — W5-C: PostgreSQL collector

## Needs from coordinator

- Update the out-of-scope `crates/tidemark-cli/tests/w1a/main.rs:26` test `probe_json_has_all_sections_and_unknown_postgres`. It requires the implemented PostgreSQL probe to continue returning unknown/null. No server is present, so the real probe correctly reports unreachable/false. This blocks the mandatory unfiltered workspace test and tools/ci.sh. The test was not edited or marked ignored.
- Decide [SCR-001](scr-W5-C/SCR-001.md) (credential-file DSN details) and [SCR-002](scr-W5-C/SCR-002.md) (timed label refresh). No shared contract was changed.
- Run the PostgreSQL runtime tests on each EL8 AppStream version. Their temporary initdb fixture must be invoked as an unprivileged VM test user with PostgreSQL binaries in PATH or PG_BINDIR; root initdb is deliberately unsupported. The ordinary root-wide `xtask vm-test` invocation needs a separate unprivileged invocation for this crate.

- **Branch:** hp-59b6485dd67c96328dc7776a6fcf6d65affc8787e4b411b3503fbe5082d2536d
- **Attempt:** attempt-59b6485dd67c96328dc7776a6fcf6d65affc8787e4b411b3503fbe5082d2536d
- **Date:** 2026-10-11
- **Status:** core implementation provided as an evidence candidate; runtime acceptance and coordinator decisions remain pending.

## What was built

Implemented a synchronous, text-row PostgreSQL collector using the already attached postgres client, with no new dependencies. Startup options and session SETs enforce read-only transactions, application_name, statement timeout and a one-second lock timeout. SQL selects metadata only, adapts slot columns for PG 10/12 versus 13/16, skips slot retention on standbys, and rejects pre-10 servers while exposing the discovered version in probes.

Fast collection publishes complete passes atomically. Timed failures back off 1, 2, 4, 8, 16, 30, 30 minutes, preserve previous values, and expose the first failure timestamp even before a successful pass. Counter rates ignore reset/new database counters and use injected time. Hourly queries connect sequentially to eligible non-template databases, qualify relation names with the database, and provide dead-tuple samples. The additive `collect_relations_for_database` method supports CLI --db without opening unrelated databases.

Path labels use RawPath lexical normalization and never traverse the filesystem. Credential paths come from keyword-form DSN passfile options; final symlinks, nonregular files, non-root ownership, wrong permissions and oversized files are refused by checking the opened descriptor. Credential contents and DSNs never appear in client error messages. The CLI renders databases, WAL, slots, archiver status/counters, temp-byte counters and optional relations in text and versioned JsonDoc output.

## Shared-file edits

None. Cargo manifests, lockfile, crate roots, core types, flags and config fields were not edited. All original public signatures and explicit fields remain intact. PgError retains its preparation compatibility variant; implementation variants and collector private fields are task-owned.

## Deviations from the spec

| Reference | Implementation / reason | SCR |
|---|---|---|
| 07 §5.1, task password-file wording | Extract keyword-form passfile from the DSN because the frozen config has no separate file field and postgres does not parse libpq passfile. Support raw one-line passwords without colons or matching pgpass records. URL passfile and multi-host pgpass matching are unsupported; inline passwords are refused. | SCR-001 |
| 07 §5.2 general failure backoff | Fast/relation methods use their injected clocks; path_labels has no clock and returns failures without updating retry state. | SCR-002 |
| Bounded everything | 10,000 rows per streamed query; 10,000 relation/dead-tuple samples per hourly pass; configured top_relations capped at 1,000. Exceeding row/sample caps returns an error rather than publishing a partial pass. Password files are capped at 4,096 bytes; connect timeout is five seconds. | documented local constants/caps |
| Hourly database enumeration | Exclude templates and databases that disallow connections, so template0 cannot break every hourly pass. | no contract change |
| CLI scheduling | Explicit pg runs once even when the background collector is disabled in config. --db filters database counters/output, and limits relation connections; WAL/slots/archiver remain cluster-wide. JSON includes now plus samples, retaining counters absent from PgNow. | no flags changed |
| Sandbox | All Cargo builds/checks use offline cache. No PostgreSQL was installed or started, and no privileged command was executed. | retained environment instruction |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| Works against EL8 AppStream PostgreSQL versions | VM tests below compile; no server/VM is available. No PostgreSQL runtime version has been tested. | pending VM |
| Version-adaptive text SQL and row decoding | versions_decode_and_guard_sessions, standby_skips_slots_and_old_versions_fail, sql_text_is_pinned, version_branches, malformed_rows_are_errors | local unit coverage |
| Backoff, retained last-good values and unreachable timestamp | backoff_retains_last_good_and_recovers, query_failure_publishes_no_partial_pass, first_failure_has_unreachable_state_and_selection_skips_other_databases | local unit coverage |
| Labels, probe and safe sessions | maps_labels, probe_reports_versions_missing_privileges_and_unreachable, versions_decode_and_guard_sessions, relations_are_qualified_and_sequential | local unit coverage; live privileges pending VM |
| Credential safety and output | password_rejects_wide_permissions_and_symlink, configured_password_paths_and_pgpass_matching; two CLI render tests | local unit coverage; root-owned positive-file authentication pending VM |

## Tests added

Unprivileged (14 collector/SQL/label tests and two CLI tests):

- collector::tests::versions_decode_and_guard_sessions
- collector::tests::standby_skips_slots_and_old_versions_fail
- collector::tests::backoff_retains_last_good_and_recovers
- collector::tests::relations_are_qualified_and_sequential
- collector::tests::query_failure_publishes_no_partial_pass
- collector::tests::first_failure_has_unreachable_state_and_selection_skips_other_databases
- collector::tests::counter_rates_handle_growth_resets_new_databases_and_clock_edges
- collector::tests::malformed_rows_are_errors
- collector::tests::configured_password_paths_and_pgpass_matching
- collector::tests::probe_reports_versions_missing_privileges_and_unreachable
- collector::tests::password_rejects_wide_permissions_and_symlink
- queries::tests::sql_text_is_pinned (reviewable SQL fixture in tests/w5c/sql.txt)
- queries::tests::version_branches
- labels::tests::maps_labels
- cmd::pg::tests::text_lists_database_wal_slots_archiver_temp_and_relations
- cmd::pg::tests::json_is_versioned_and_keeps_null_retention

Privileged-feature integration tests, both **pending VM**, not ignored:

- postgres_monitor_collects_sizes_slots_relations_and_labels
- postgres_probe_reports_missing_monitor_privileges

The fixture uses only its temporary cluster/socket, disables TCP listening, creates a monitoring role/database/table/physical slot, and shuts down the temporary server. No host service or cluster is used. Invoke in a disposable VM as an unprivileged test user: `PG_BINDIR=<installed-version-bin-dir> cargo test --offline -p tidemark-pg --features privileged --test w5c -- --nocapture`. It prints server_version_num for recording the tested streams.

## Known gaps and follow-ups

- No live PostgreSQL query, authentication or permission behavior has been executed here; VM tests compile only. Acceptance remains pending.
- Password-file format/URL options and label-refresh retry policy require the coordinator decisions above.
- Labels describe lexical PGDATA/pg_wal/base/OID and reported tablespace locations. They do not resolve an externally symlinked WAL directory or enumerate version-specific tablespace database subdirectories; there is no unbudgeted filesystem traversal.
- PgNow's frozen field list has no separate relation-refresh timestamp or archived_count/temp_files/autovacuum fields. Relation updates retain fast sampled_at; selected extra columns are not invented as new sample kinds. A single CLI pass has no prior temp rate, so it prints cumulative temp-byte counters.
- The untouched W1-A test prevents a green mandatory workspace test/CI result. The filtered validation is supplemental evidence and does not replace that failure.

## Notes for the reviewer

No production unsafe code, writes, directory listing, regular scanned-file content reads, async API or TLS/OpenSSL dependency was added. The synchronous postgres dependency's transitive runtime already existed in the preparation. Linux O_NOFOLLOW/O_NONBLOCK numeric open flags are used only for the credential file because the task has no direct libc/rustix dependency. Client rows stream into bounded vectors; names/byte counts are validated before conversion into frozen sample fields. Failure tests exercise both connection failure and failure after partial query results.

## Validation

Evidence logs are in this attempt's canonical worker-output directory under library/. Required checks were executed offline against Rust 1.98.0 and the host Cargo cache. Final command results:


| Command | Exit/result | Evidence |
|---|---|---|
| cargo fmt --check | 0 | library/fmt.log |
| cargo clippy --offline --workspace --all-targets -- -D warnings | 0 | library/clippy.log |
| cargo test --offline --workspace | 101; stale W1-A probe assertion | library/test.log |
| cargo build --offline --workspace --features privileged --tests | 0; compile only | library/privileged.log |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | 0 | library/doc.log |
| tools/ci.sh with CARGO_NET_OFFLINE=true | 1; same W1-A test, source/format/Clippy checks pass before it | library/ci.log |
| cargo test --offline --workspace -- --skip probe_json_has_all_sections_and_unknown_postgres | 0; 239 passed, 0 failed, 6 existing ignored, 1 filtered | library/test-filtered.log |
| cargo clippy --offline -p tidemark-pg -p tidemark-cli --features privileged --all-targets -- -D warnings | 0 | library/clippy-owned-privileged.log |
| git diff --check | 0 | local whitespace audit |

The supplemental filtered run includes all 14 W5-C library tests and both CLI rendering tests. Five pre-existing tests are ignored for scale. `crates/tidemark-scan/tests/w2b/main.rs::synthetic_bulk_equals_walk` is also already ignored with reason `needs W2-A`; it was not changed. No W5-C test is ignored. Cargo-deny is not reached because CI stops at the stale test; no advisory/license audit claim is made.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits.
- [ ] Full CI green — blocked by the out-of-scope stale W1-A assertion.
- [x] Privileged tests written, compiled and listed as pending VM.
- [x] Public items documented; ownership headers present.
- [ ] All acceptance criteria evidenced — real EL8 PostgreSQL versions remain pending VM.
- [x] No production unwrap/expect, unsafe, unexplained TODO or system-clock logic added.
