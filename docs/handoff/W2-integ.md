# Handoff — W2-integ: Wave 2 scan pipeline integration

## Needs from coordinator

- Run the feature-gated XFS tests on disposable Rocky 8.8 and 8.10 VMs: real bulk/walk equivalence, automatic bulk selection/fallback, ftype=0 refusal, and scale/RSS evidence remain **pending VM**. No privileged runtime commands were executed here.
- The known outside-scope `probe_json_has_all_sections_and_unknown_postgres` assertion prevents a clean workspace test/CI result on this baseline. No PostgreSQL, probe, store, live, manifest, or plan files were changed.
- Run the mixed-special oracle in an environment permitting Unix socket creation. This sandbox returns EPERM; the test explicitly reports that fixture as pending and continues the other seven mandatory fixtures.
- The advisory/license audit remains outstanding; cargo-deny is unavailable here.

- **Branch:** hp-357dd7a5d488ff89866c45c756f5b71e9ee7fd2c8511b239e81dc7261df57ef5
- **Attempt:** attempt-357dd7a5d488ff89866c45c756f5b71e9ee7fd2c8511b239e81dc7261df57ef5
- **Date:** 2026-10-11
- **Status:** implemented and locally exercised; submission is evidence for independent verification, not an integration or release decision.

## Changes per coordinator decision

**D1 / SCR-008:** Added exactly the approved `errors_total: u32` fields immediately after `errors` in core `DirRecord` and `NsRecord`. The walker snapshots its worker-local error counter before each directory, checks the difference fits u32, and preserves it in namespace output. Aggregation, progress, model conversion, and the independent reference consume the total rather than the sampled detail length. The bulk result privately carries the namespace walker summary without extending BulkStats. The orchestrator compares the rolled record total with `WalkSummary.errors` in both modes, logs any disagreement, and marks the snapshot degraded without silently replacing the record total. The walker cap regression now covers two directories, an exhausted shared detail cap, summed totals, and u32 overflow. A walk/namespace regression verifies 20,140 errors with only one detail row.

**D2:** Implemented the approved `BulkProgress` and callback argument, namespace noatime fallback totals, and `BeforePhase2(String)`. Namespace collection, sorting-worker creation, and geometry validation failures are classified before any phase-two iterator call; cancellation retains its own type. Progress shares one two-second cadence across phases, including timeout observations while collectors are waiting and bounded, joined observation workers for sorting and name resolution. Auto falls back only on Unsupported/BeforePhase2, changes the recorded mode/reason, and constructs a fresh aggregator for the walk. Cancellation takes precedence over fallback and bulk never publishes partial output. Help identifies keep-partial as walk-only. Updated all local and VM call sites to the new interface.

**D3:** Retained the existing candidate/channel admission formula and resource refusal; no WalkConfig budget field was added. Logs identify the mode working-set estimate, configured admission budget, and process peak RSS observed after publication/series/retention work. VmHWM is process-wide and includes earlier or concurrent work; it is not an isolated scan allocation measurement. The service cgroup supplies the hard limit. WalkConfig gained only Clone to reuse traversal settings on fallback.

**D4:** Finish-time AlreadyExists now becomes typed `ScanError::PublicationCollision`, with no retry. A regression constructs a writer, introduces the final-path collision, calls the production finish wrapper, and checks preservation of existing bytes and removal of the temporary file. The existing writer-creation retries remain intact.

**D5:** Non-dry-run CLI prune opens a writable catalog, deletes retention-selected files, then rebuilds globally from remaining snapshot files. The regression checks the deleted catalog row disappears and the current row survives. Rebuild help explains that --root validates the path while rebuilding all roots. No store interface changed.

Removed all four `needs W2-A` ignore markers and the unused NotYetImplemented variants in tidemark-scan. Updated obsolete skeleton documentation. No dependencies were added.

## Seams found and resolved

1. W1-B capped error details while W2-A interpreted detail length as the directory total. D1 supplies the missing attribution; global, subtree, and direct counts now agree independently of sampling.
2. W2-C could not observe bulk phases or decide whether to redo them. D2 supplies the callback and typed pre-phase-two classification; fallback discards the consumed aggregator.
3. Cancellation can arrive during walker setup before any root record is emitted. Preparing an empty aggregator returned `invalid directory tree`. With keep-partial, the orchestrator seeds only already-observed root metadata in this case. It publishes a partial snapshot with zero observed entries; default latest selection excludes it. No extra scan syscall is needed.
4. Two successful scans can finish within the same second. The frozen dir_series primary key excludes scan_id, so the second scan previously failed after publishing its snapshot. The direct recorder now uses a bounded scan helper to preserve the first sample for a path/second and log omitted duplicate samples; both snapshots and catalog rows remain available. No schema or store API changes were made. A future daemon recorder should use the same policy/helper when it persists scan series. This is an explicit local behavior choice: historical series coalesce to the first observation within a second, while immutable snapshots retain every scan.
5. Prune previously retained catalog entries for deleted files. D5 performs a global rebuild and requires a writable catalog connection.
6. The apparent-size normalization lived only in the orchestrator test. Moved it into the bounded, metadata-only `test_support::oracle::du_apparent_normalized` helper and exercised it against the measured golden reference. It probes local GNU du behavior rather than checking its version.
7. Text top/files output lacked snapshot provenance. Added a renderer header with snapshot ID, mode, age, status, and degraded marker, as required by 03 §7; JSON shapes remain unchanged.
8. Catalog recency ties use scan ID after second-resolution timestamps; the hash suffix is not chronological. The growth test now explicitly selects the newly returned scan ID, rather than assuming that an immediate third scan sorts after an unchanged second scan. The frozen catalog ordering remains intact.
9. The newly enabled mixed-special test assumed Unix sockets could be created. It now handles only that fixture's PermissionDenied as an explicit environment gap, matching test-support's existing treatment. Other fixture materialization failures remain failures.

## Shared-file edits

- `crates/tidemark-core/src/scan.rs`: only the two D1 fields and their approved rustdoc.
- `crates/tidemark-scan/src/lib.rs`: obsolete skeleton documentation updated; no exports or module declarations changed.
- No Cargo.toml, Cargo.lock, outside-scope lib.rs, schema, config key, CLI flag, metric, plan, or AGENTS.md edits.

## End-to-end tests and evidence

- `mandatory_walk_fixtures_match_du_find_for_every_directory`: run_scan, temporary recorder/state, immutable store reader; every directory compared with GNU du disk, normalized apparent, and find entry/directory/inode counts. Checks snapshot summary/catalog values and file mode 0600. Seven fixtures run here; mixed-special socket creation is explicitly pending.
- `random_disk_snapshot_matches_independent_reference`: seeded materialized tree with 100 directories, 3,000 inodes, 350 hard-link sets, 30% incomplete-set probability, and symlinks. Measures metadata into the independent generated model and compares every directory's disk/apparent/charged/reclaimable/shared/count/error/age/newest/width metrics through run_scan and the snapshot reader.
- `scan_then_top_files_and_snapshots_end_to_end`: enabled real non-UTF-8 scan/query/live CLI coverage.
- `real_fixture_cli_all_single_snapshot_sorts_and_growth`: assert_cmd with a temporary config/state; real hard-link fixture scans, snapshots list, files, depth-two top with all eight single-snapshot sorts, with/without JSON, then added allocated data and growth queries. The older catalog timestamp is explicitly injected to now-minus-24h so the unchanged frozen growth window can be tested without waiting a day; snapshot metrics are produced by real scans. The growth query explicitly selects the newly returned scan ID to avoid same-second catalog ties.
- Enabled `sigint_keep_partial_publishes_partial_and_is_not_latest`; retained SIGINT-without-partial cleanup/failed-record checks.
- `non_xfs_modes_and_scan_lock_are_explicit`: actual mount-type conditional non-XFS bulk refusal (exit 5) and auto walk reason, plus a real CLI scan attempt while the host lock is held.
- `prune_rebuilds_catalog_for_remaining_files`: real published snapshots, retention deletion, and catalog row checks.
- Enabled synthetic bulk/walk equivalence. Added pre-phase-two namespace failure versus cancellation and progress during blocked namespace setup tests. Real bulk execution remains pending VM.
- `finish_collision_preserves_existing_snapshot_and_removes_temporary_file`: production publication wrapper against an actual existing final path.

## Validation

All Cargo commands use the pinned Rust 1.98.0 and cached registry, with --offline/CARGO_NET_OFFLINE=true. Evidence logs and exit files are in the attempt output library. The table below records the completed final checks.

| Command | Local result | Evidence |
|---|---|---|
| cargo fmt --check | exit 0 | checked-fmt.log / .exit |
| cargo clippy --offline --workspace --all-targets -- -D warnings | exit 0; CI Clippy also passed after the final test correction | checked-clippy.log; checked-ci.log |
| cargo test --offline --workspace | exit 101: known outside-scope PostgreSQL probe assertion | checked-test.log / .exit |
| cargo build --offline --workspace --features privileged --tests | exit 0; compilation only | checked-privileged.log / .exit |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | exit 0 | checked-doc.log / .exit |
| tools/ci.sh with offline environment | exit 1: source policy, fmt and Clippy passed; tests stopped at the same probe assertion | checked-ci.log / .exit |
| cargo test --offline -p tidemark-scan -p tidemark-fs -p test-support | exit 0: 129 passed, 4 scale ignores | checked-owned-test.log / .exit |
| cargo test --offline -p tidemark-cli --test w2c | exit 0: 13 passed, no ignores | checked-cli-w2c.log / .exit |
| git diff --check | exit 0 | local whitespace check |

The named passing targeted runs include the final production changes and the corrected growth selection. Scale ignores were not enabled; no dependency ignores remain. The mixed-special skip is printed in oracle-fixtures.log (--nocapture) and is a recorded coverage gap, not a successful socket scan.

Earlier logs are intermediate and retained separately; the table cites passing checks on the final implementation and explicitly identifies the remaining external failures.

## Fixture example

The following output is pasted from an actual temporary fixture run; paths, timestamps, duration, and measured sizes are host-specific observations, not filesystem-independent assertions.

```text
$ tidemark scan /tmp/tidemark-w2-integ-l0bss6mg/r --mode walk --throttle max
Mode: Walk (walk explicitly requested)
Duration: 0.04s
Directories: 6  Files: 7
Disk: 1.02 MiB  Reclaimable: 1.02 MiB
Link sets: 3 (1 incomplete)
Errors: 0  Races: 0
Snapshot: /tmp/tidemark-w2-integ-l0bss6mg/state/snapshots/_tmp_tidemark-w2-integ-l0bss6mg_r/20261011T125224Z-6478a717.db
```

```text
$ tidemark top /tmp/tidemark-w2-integ-l0bss6mg/r --depth 2
Snapshot 20261011T125224Z-6478a717 · Walk · 0s ago · Complete
PATH                                         SIZE      RECLAIM   SHARED      FILES
/tmp/tidemark-w2-integ-l0bss6mg/r            1.02 MiB  1.02 MiB  H 8.00 KiB  7
├─ /tmp/tidemark-w2-integ-l0bss6mg/r/a       1.00 MiB  1.00 MiB  H 4.00 KiB  2
├─ /tmp/tidemark-w2-integ-l0bss6mg/r/b       12.0 KiB  0 B       H 12.0 KiB  2
└─ /tmp/tidemark-w2-integ-l0bss6mg/r/c       12.0 KiB  12.0 KiB  0 B         3
   ├─ /tmp/tidemark-w2-integ-l0bss6mg/r/c/d  12.0 KiB  0 B       H 12.0 KiB  1
   └─ /tmp/tidemark-w2-integ-l0bss6mg/r/c/e  12.0 KiB  0 B       H 12.0 KiB  1
H = hard links shared outside this directory (see tidemark links)
```

## Remaining uncertainty and work not done

- VM execution, Rocky compatibility, true bulk fallback/cancellation observations, scale performance, and isolated bulk RSS measurement are not done.
- The mixed-special Unix socket oracle is not executed in this sandbox.
- Workspace tests/CI still depend on the parallel PostgreSQL-removal task resolving the known probe assertion; advisory/license auditing is not complete.
- Process VmHWM is diagnostic evidence, not a per-scan memory guarantee. Existing fixed aggregator/walker caps and candidate admission estimates remain unchanged.
- The direct recorder's same-second coalescing policy must be considered by the later daemon recorder; snapshots themselves retain every successful scan.
- No privileged commands, mounts, LVM changes, service/configuration deployment, merges, pushes, source-checkout edits, or memory-projection edits were performed.

## Definition of Done evidence

Only owned paths were edited, with the approved D1 core change listed above. All four dependency ignores were removed. Local check outcomes, external failures, environment skips, and pending VM work are reported rather than treated as independent verification.
