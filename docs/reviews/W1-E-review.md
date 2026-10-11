**CHANGES REQUESTED** — 2 BLOCKER, 2 SHOULD-FIX, 1 NIT.

Reviewed candidate `cff2dc91fcd2b8f290f6bd94f0711158e2b52a51` against base `8d23839528fbe514b5460ad4cadbc7d40df735ea`, limited to the candidate diff. The supplied checkout is `cebac02abdac11b0b1ca13038af24e7693887dab`; its store sources, tests, migrations, plan documents, and W1-E handoff are identical to the candidate. Builds below use that checkout, including the other landed Wave 1 implementations. This report is review evidence, not task acceptance or integration approval.

## Findings

1. **BLOCKER — Preserve the latest complete snapshot even when `recent = 0`.**
   `crates/tidemark-store/src/retention.rs:44`.
   Reference: `05-storage.md` §7 and the review's explicit requirement that retention never delete the latest complete snapshot.

   The deletion condition has no unconditional protection for the newest complete entry per root. Zero is accepted for `SnapshotRetention.recent`. A direct API probe supplied one complete `/data` snapshot named `only`, started at timestamp 0, policy `{ recent: 0, daily: 14, weekly: 8 }`, and `now = 100 * 86400`. `retention_plan` returned `["only"]`. Pruning this plan removes the root's last usable snapshot. The all-zero-policy assertion in `tests/storage.rs:477` also endorses deleting every complete entry.

   Retain index zero unconditionally after each root's newest-first sort, then apply the configured rules to the rest. Add regressions for zero recent retention with expired daily/weekly windows and for multiple roots; update the existing all-zero expectation. Evidence: `library/timeseries_probe.rs` and `.log`.

2. **BLOCKER — Implement the contracted glob boundary filter.**
   `crates/tidemark-store/src/reader.rs:432`.
   Reference: `05-storage.md` §3, `13-contract-decisions.md` SCR-001, and W1-E's complete reader API acceptance criterion.

   A valid `LinkFilter { boundary: vec!["/data/*".into()], boundary_depth: None, ..Default::default() }` always returns `StoreError::NotYetImplemented`. This was reproduced against a populated snapshot. Consequently, an explicitly contracted filter cannot produce any answer. The handoff discloses the gap, but a missing dependency attachment does not complete the interface.

   Arrange the permitted dependency attachment and implement boundary-region matching and crossing selection. Preserve the existing rejection of simultaneous globs and depth. Add matching/nonmatching, overlapping/nested-region, and raw-byte-path cases. Evidence: `library/reader-probe.rs` and `.log`.

3. **SHOULD-FIX — Avoid repeated full resolution-range scans in sample queries.**
   `crates/tidemark-store/src/timeseries.rs:312`; the same construction appears at lines 346 and 376.
   Reference: `05-storage.md` §6's retained sample queries, AGENTS.md's resource constraints, and `01-requirements.md` NFR-4.

   Each candidate sample runs correlated existence checks whose resolution inequalities prevent seeking the timestamp portion of the primary key. `EXPLAIN QUERY PLAN` shows the coarser check using only `(mount=? AND res>? AND res<?)`, repeatedly scanning that range. A 400-day fixture containing 8,760 hourly, 9,504 five-minute, and 5,760 raw samples for one mount has only 24,024 rows; the actual `query_fs("/data", 0, 400*86400)` call did not finish within a 90-second timeout (exit 124). This is a debug-library measurement with concurrent builds, not a controlled production benchmark; the query plan independently identifies the repeated work.

   A reproducible SQLite instruction-budget comparison removes the timing ambiguity: the extracted candidate SQL exhausted 100 million VM instructions without completing; an equivalent fixed-resolution query for this requested tier returned all 24,024 rows in approximately 1.1 million instructions. Split the tier checks into fixed `res = 1` / `res = 2` branches with exact bucket-timestamp primary-key lookups. Give finer-tier fallback checks explicit resolution equality and bounded bucket timestamp ranges. Apply the change to filesystem, LVM, and PostgreSQL queries, preserving mixed-tier results, and add a representative retained-history performance regression. Evidence: `library/timeseries_probe.{rs,log,exit}`, `library/timeseries_query_plan.{py,log}`, and `library/timeseries_sql_cost.{py,log}`.

4. **SHOULD-FIX — Complete the corrupt-catalog recovery path after resolving its contract request.**
   `crates/tidemark-store/src/catalog.rs:125`.
   Reference: `05-storage.md` §4's missing-or-corrupt database fallback; `docs/handoff/scr-W1-E/SCR-1.md` is still pending.

   With non-SQLite bytes in `timeseries.db`, `Catalog::open(state_dir, true)` returns `file is not a database`. No `Catalog` exists on which to call `rebuild_from_files`, so the promised recovery cannot run even when published snapshots survive. The probe confirmed that the corrupt bytes are preserved, which is appropriate, but recovery is absent.

   Resolve SCR-1 before extending the frozen API, then implement an explicit recovery entry point that preserves the original database and sidecars before rebuilding the catalog from valid snapshots. Keep ordinary opens nondestructive. Test corrupt shared storage alongside valid snapshot files and assert preservation of recovery artifacts. Evidence: `library/catalog-space-probe.rs` and `.log`.

5. **NIT — Do not round the free-space floor downward at large integer sizes.**
   `crates/tidemark-store/src/space_guard.rs:160`.
   Reference: `05-storage.md` §8's percentage floor and `01-requirements.md` NFR-7.

   The `u64 -> f64 -> u64` calculation can lose bytes before `ceil` runs. The pure-function probe used `size = 2^62 + 512`, `avail = 2^62`, `min_free_pct = 100`, zero estimate/usage, and `max_state_size = u64::MAX`. It returned `Ok` despite being 512 bytes below the required floor. This is an extreme synthetic filesystem size, hence the low severity.

   Compute the ceiling percentage with checked integer/rational arithmetic or conservative rounding that cannot underestimate the floor, and add this boundary to the decision table. Evidence: `library/catalog-space-probe.rs` and `.log`.

## Checked and found correct

- Scope: only store-owned implementation/migration/test paths and W1-E handoff files changed. No manifest, lockfile, or module declaration changed. Although the wave table omits `error.rs`, `docs/handoff/W1-prep.md:20` explicitly assigns it to W1-E; its error shape was left to the owning task. Public signatures and `ScanEntry` fields match the frozen stubs.
- The schema test reads the actual `05-storage.md` through `include_str!` and compares executed document DDL with the implementation. Independent comparison confirmed all 15 noninternal snapshot objects and 13 time-series objects match, including column declarations, nullability, constraints, and indexes.
- Writer inspection confirmed the specified PRAGMA order, tables before secondary indexes, at-most-50,000-row transactions, final indexes/ANALYZE/metadata, connection close, file sync, `RENAME_NOREPLACE`, and directory sync. Drop/abort and poisoned-writer tests pass. An extra probe verified mode `0600` under umask `0777`, page size 32768, schema version 1, and rejection of a concurrent same-ID creator without damaging the original writer.
- Immutable read-only URI handling works with non-UTF-8 filenames and `?#%`. Queries bind values; dynamic ordering uses fixed enum mappings. Hash collision probes confirm stored path bytes are compared. Separate corruption probes reject negative unsigned values and over-width inode/depth/entry/counter values. Unresolved file paths remain `None`; link counts count stored instances.
- Catalog selection excludes partial snapshots from automatic selection, preserves raw-byte roots/file paths, and rebuilds a healthy catalog transactionally. Root slug escaping is injective for the exercised byte cases.
- WAL/NORMAL, the five-second busy timeout, transactional sample inserts, backed-up initial migration, 20,000-row directory-history selection, and ordinary last-value/idempotent compaction checks pass. Retention is deterministic and uses per-root UTC days and Monday-based ISO-week buckets; running entries are preserved.
- State usage traversal uses directory-relative handles, no-follow flags, mount IDs, `O_NOATIME` with EPERM fallback, bounded iterative traversal, and checked allocation/cap arithmetic. No production `unsafe`, target regular-file contents, host configuration changes, or uninjected clocks were introduced. This traversal measures Tidemark's own state, not scanner input.

## Validation and limits

Rust 1.98.0, `CARGO_HOME=/tmp/tidemark-w1e-review-cargo` (copied host cache), and `CARGO_NET_OFFLINE=true` were used. Logs and standalone probe sources are retained under this attempt's worker-output `library/` directory.

| Check | Result |
|---|---|
| `tools/ci.sh` | Exit 0: source policy checks, format, all-target Clippy with warnings denied, and workspace tests passed. Cargo-deny was unavailable and explicitly skipped. |
| Store tests within CI | 2 unit tests, 17 integration tests, and 2 compile-checked doctests passed; the scale test was separately ignored by default. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; privileged tests compiled only. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0. |
| `cargo test --offline -p tidemark-store --release insert_2m_directory_rows_under_60_seconds -- --ignored --nocapture` | Exit 0; insertion took 6.790184945 s; the whole test including publication took 12.40 s. |
| Additional probes | Schema, restrictive-umask writer, byte-path/collision/conversion checks passed; missing filter, retention deletion, corrupt-catalog, and floor cases reproduced. The mixed-tier query exceeded its 90-second timeout. |

No privileged test, VM connection, mount, LVM operation, service action, or destructive host command was run. Power-loss durability and Rocky/XFS runtime behavior were not exercised; sync ordering was inspected and feature-enabled tests compiled. The privileged publication test verifies ownership, mode, and reopening, not an actual power cut. Cargo-deny auditing remains unperformed.

The existing depth-overflow test changes `depth` while leaving a previously invalid `ino = -1`; its assertion does not isolate the width error. The independent probe restored each field between cases and confirmed width rejection. The membership semantics of `link_sets_under` for a child containing one instance whose LCA is above that child are not explicit in §3; no finding is asserted on that ambiguity.
