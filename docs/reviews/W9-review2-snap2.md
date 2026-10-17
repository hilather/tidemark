VERDICT: BLOCKED
BLOCKER 4 / SHOULD-FIX 4 / NOTE 1

Reviewed exact candidate `f5fdcb663070bbdce12ca79b8e384809cad2d918` against base `3efed8fe8c38ab3f070f677b8db1adc2c1a17758`, and separately reviewed the requested v2-writer integration tree `f46d883a741d33195153d63f83c8adc328be3fac`. The recorded blind-review session names the former; the assigned checkout and deliverable contract name the latter. The former contains the cold changes but no v2 writer. Their only tree differences are `writer_v2.rs`, its tests/handoff, and its module declaration. Findings explicitly marked **integration only** are excluded from the receipt for the pinned cold candidate. No implementation files were edited.

Read `AGENTS.md`, `docs/plan/15-snapshot-v2.md`, the storage/link contracts, both requested handoffs, and the relevant implementation, callers and tests. Inspected both requested commit ranges and the integration log. Line references below are identical between the two trees for cold code.

## Findings

### B1 — BLOCKER: diff and explain attach the compressed container as SQLite

Reference: `finding:w9-review2-snap2-b1`. Locations: `crates/tidemark-store/src/reader.rs:332`, `crates/tidemark-store/src/diff.rs:252` and `:265`.

Cold opening correctly connects to the inflated leaf, but preserves the original `.db.zst` in `Snapshot.file`. `with_old` uses that accessor for SQLite ATTACH. Comparing an ordinary new snapshot against an older cold snapshot therefore fails with `file is not a database`; explain has the same failure. This breaks normal historical comparisons as soon as retained snapshots become cold. Probe `review_diff_and_explain_cold_old` confirmed both errors and successful hot/hot comparison. Keep the durable catalog filename separate from the actual SQLite backing filename, and exercise both cold comparison directions.

### B2 — BLOCKER: cleanup can delete the only valid SQLite copy

Reference: `finding:w9-review2-snap2-b2`. Locations: `crates/tidemark-store/src/cold.rs:221`–`:224`, `crates/tidemark-store/src/catalog.rs:827`–`:837`.

`validate_cold` establishes valid header metadata, zstd decoding/checksum and decoded byte count, but does not establish a valid SQLite snapshot or equivalence to the hot source. Cleanup treats that result as permission to repoint the catalog and unlink the hot file. In a deliberately malformed-container probe, a genuine header plus a checksummed zstd frame of the exact declared length containing all-zero bytes passed validation. Cold opening failed, hot opening succeeded, and cleanup then deleted the good hot file and selected the unreadable cold copy. This demonstrates the missing destructive-cleanup predicate; it does not claim the normal compressor emits such a frame. Before deleting the surviving hot copy, establish payload equivalence to it or validate the complete snapshot and its identity. Probe: `review_valid_frame_invalid_sqlite_deletes_hot`.

### B3 — BLOCKER: catalog rebuild hides a good hot copy behind a truncated cold copy

Reference: `finding:w9-review2-snap2-b3`. Location: `crates/tidemark-store/src/catalog.rs:363`–`:368`.

Rebuild skips a `.db` whenever its sibling cold header parses, then records the cold header without checking the frame. Truncating one byte from a cold payload preserves its header. Cleanup correctly leaves the good hot copy, but the next rebuild selects the broken cold file and omits the readable hot one. Probe `review_truncated_frame_shadows_good_hot_on_rebuild` reproduced exactly that sequence. Header-only rebuilding is required for cold-only snapshots; it should not suppress an available validated hot copy solely because a sibling header exists. Resolve duplicate forms using evidence of payload validity, or prefer the known-readable hot source until cleanup resolves the pair.

### B4 — BLOCKER: v2 sorting writes outside the state directory — integration only

Reference: `finding:w9-review2-snap2-b4`. Locations: `crates/tidemark-store/src/writer_v2.rs:166`, `:429`, `:487`–`:493`.

Both connections request `temp_store=FILE`, but no trusted state-local SQLite scratch directory is configured. Large index builds consequently spill through SQLite's environment/default temporary-directory selection, violating AGENTS rule 9 and bypassing state-directory usage accounting. Probe `review_writer_sort_scratch_outside_state` inserted 204,800 link instances with 240-byte names, then observed a live `etilqs_* (deleted)` descriptor outside the writer's state directory during finish. The subprocess deliberately set `SQLITE_TMPDIR` to a separate private `/tmp` fixture, keeping the experiment isolated. The pinned SQLite source also confirms the ordinary environment and `/var/tmp`/`/tmp` fallback selection. Keep bounded disk sorting, but confine its scratch to trusted Tidemark state.

### S1 — SHOULD-FIX: cold-only recovery does not repair a stale hot catalog pointer

Reference: `finding:w9-review2-snap2-s1`. Locations: `crates/tidemark-store/src/catalog.rs:827`–`:837`, `crates/tidemark-store/src/timeseries.rs:151`, `crates/tidemarkd/src/lib.rs:1554`–`:1556`.

The catalog update uses WAL `synchronous=NORMAL`; cleanup then unlinks hot and fsyncs its directory. Startup retains another catalog connection, so closing cleanup's temporary connection does not guarantee a final-connection checkpoint. A power failure can leave durable cold data and unlink while losing the catalog update. The next cleanup only repairs the pointer inside the `hot exists` branch, leaving it pointed at a missing `.db`. The data remains recoverable through explicit rebuild. Probe `review_stale_hot_pointer_not_repaired_for_cold_only` constructed that post-crash state and confirmed cleanup leaves it unchanged. The possible WAL loss is a source-level durability analysis, not an executed power-failure test. Durably order the update before deletion or reconcile cold-only entries on startup.

### S2 — SHOULD-FIX: process death leaves the full v2 staging file — integration only

Reference: `finding:w9-review2-snap2-s2`. Locations: `crates/tidemark-store/src/writer_v2.rs:133`, `:452`–`:455`, `:670`–`:676`; `crates/tidemark-store/src/catalog.rs:728`–`:731`.

The writer creates `<scan>.db.stage.tmp`. Finish and Drop remove it, but abandoned-scan cleanup recognizes only `.db.tmp` and `.db.zst.tmp`. A process exiting without destructors leaves the uncompressed staging database indefinitely; repeated failures consume the state allowance. Probe `review_writer_crash_staging_leaf_not_swept` terminated a child after committing its input, then ran cleanup: `.db.tmp` disappeared while the 327,680-byte `.db.stage.tmp` remained. Include staging leaves in the existing trusted, scan-lock-protected cleanup.

### S3 — SHOULD-FIX: v2 conversion ignores the installed space guard — integration only

Reference: `finding:w9-review2-snap2-s3`. Locations: `crates/tidemark-store/src/writer_v2.rs:186`–`:200`, `:428`–`:449`, `:483`–`:664`.

Only input insertion invokes the space callback. Finish builds staging indexes/spans and the entire second database while retaining the first, without another check. Probe `review_writer_finish_does_not_recheck_space` installed an every-row guard, changed it to refusal after the final input row, and observed successful publication with no additional callback. V1 also omits checks during finish, but v2 adds a complete output conversion and its additional simultaneous storage. Check conversion boundaries and budget staging, output and scratch together. The probe demonstrates the omitted checks, not an actual full-filesystem event.

### S4 — SHOULD-FIX: descriptor exhaustion leaks an inflate leaf on an ordinary error return

Reference: `finding:w9-review2-snap2-s4`. Location: `crates/tidemark-store/src/cold.rs:275`–`:288`.

The leaf is created before the cleanup guard exists. Failure in flock setup or `output.try_clone()` returns without unlinking it. In an isolated child, probe `review_inflate_emfile_before_guard_leaks_leaf` lowered only its soft descriptor limit: at `RLIMIT_NOFILE=7`, open returned `Too many open files` and left one inflate leaf behind. A later sweep can remove it, but the claimed cleanup on every error is not met. Establish ownership/cleanup immediately after exclusive creation and avoid requiring an unguarded descriptor clone.

### N1 — NOTE: simultaneous cold opens have no shared capacity reservation

Reference: `finding:w9-review2-snap2-n1`. Location: `crates/tidemark-store/src/cold.rs:255`–`:289`.

Each reader checks capacity before creating its inflate leaf. The directory flock is shared during creation and released before output, so independent readers can approve the same free bytes. For example, two 8-GiB inflates can each pass with 12 GiB available and a 2-GiB floor, then exceed available space together. Default `max_state_size` is 20 GiB, so it need not prevent that example. This is a control-flow observation; no disk-exhaustion stress test was run. If concurrent cold readers must preserve the configured floor, add shared reservations or an equivalent concurrency policy.

## Coverage and validation

- Publication writes an exclusive private cold temporary, finishes/checksums and fsyncs it, renames NOREPLACE, and fsyncs the directory. The compressor leaves source unlink/catalog transition to its caller. Ordinary cancellation leaves a sweepable temporary. The destructive validation and recovery gaps are B2, B3 and S1.
- Cold leaves use ancestry checks, no-follow opens, ownership/mode/single-link validation and mount checks. `.inflate` is private and its creation is coordinated with sweeping by a directory flock; a retained per-leaf flock protects live readers. `Snapshot` closes its connection before dropping the inflate guard. No ordinary live-leaf deletion was established. Decode and caller-unwind paths have RAII protection; S4 identifies the earlier setup gap. Privileged bind-mount behavior was inspected but not executed.
- Normal cold opens call the space guard with the header's declared length before creating a leaf. Header JSON is capped at 1 MiB, decoder window at 128 MiB, output at declared length plus one byte; short/long output and trailing input fail before SQLite opening. Existing malformed-length and refusal tests passed. N1 addresses concurrent capacity, rather than a single lying header.
- Existing cold parity tests cover direct reader methods and catalog-row reconstruction from a healthy cold-only file. The additional comparison path fails under B1; the duplicate-corruption case fails under B3.
- V2 tables, column ordering, metadata, `(dir_id,name)` and `(ino,dir_id,name)` clustering, chunk bounds, `file_lo`/`file_count` across chunks and empty directories, lookup hashing, and shallow copies matched the contract for valid input. Incoming file IDs become clustered row numbers intentionally. Reviewed checked conversions did not reveal an additional narrowing or field-loss defect.
- Writer buffers contain at most one configured chunk; spans live in SQLite and caches are set to 8 MiB per connection. This avoids whole-tree Rust maps, but does not establish a measured 2M-directory/10M-instance peak. Cancellation, errors, ordinary Drop and publication collision tests passed; process-death cleanup and conversion space have S2/S3 gaps.

Commands and artifacts:

1. `cargo test --offline -p tidemark-store --test cold --test writer_v2 -- --nocapture`: **8 cold tests and 8 writer tests passed**, on integration tree `f46d883`. Cargo used pinned Rust 1.98.0 and a dedicated `/tmp/tidemark-review-2401-target`. Cold implementation is unchanged from the pinned candidate; a separate full build of that candidate was not performed.
2. An external Rust test harness copied the existing cold fixture helpers and added probes without modifying repository code. Seven initial probes passed by asserting the observed defects; the additional isolated EMFILE probe also passed. This is **eight reproduced observations**, not a passing correctness gate. Saved source: `library/review-probes.rs`; logs: `library/cargo-tests.log`, `library/probe-results.log`, `library/probe-emfile.log`, `library/probe-build.log` in this attempt's output directory.
3. `git diff --check 3efed8f f46d883`: passed. No full workspace CI, Clippy, privileged suite, power-loss injection, or §8 latency/2M/10M scale gate was run. Handoff claims were not treated as independent validation.
4. Build products were removed with offline `cargo clean`; the standalone probe executable was also removed. Source/log evidence remains in the attempt output library.

The receipt for `f5fdcb6` references B1–B3, S1, S4 and N1 only. B4, S2 and S3 belong to the separately requested integration review. This report is submitted as review evidence; no acceptance, release or integration decision is claimed.
