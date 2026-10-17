VERDICT: BLOCKED
BLOCKER 0 / SHOULD-FIX 2 / NOTE 1

Reviewed integration `358e4687fe779e0f3fbf29ec05135896814ece40` and exact blind reader candidate `30883cbd23e05a380ed32790e72562a8e9a249f9`, both against `d7cd4f065f2d8f94018e87e4c25d0bef1d4e766d`. The store source/tests are identical between candidate and integration. Inspected the requested fixes/log and the whole wave-3 delta from `cab1bdc`. The heading counts describe the requested integration recheck. Findings S2 and S3 below apply to both trees; the automatic-retention coverage note applies to integration. The exact candidate also retains the earlier cold/list/fixture findings listed separately below (its totals are BLOCKER 0 / SHOULD-FIX 5 / NOTE 1). This is a review recommendation: the original blocker is fixed, but two corrupt-input cases remain before all requested fixes can be closed. It is not release acceptance.

Read `AGENTS.md`, the previous review, `15-snapshot-v2.md`, both fix handoffs, and the relevant storage/link contracts. No implementation or repository test was edited. Standalone probes and baseline-module builds live only in this attempt's output library.

## Remaining findings on the reader candidate

### S2 — SHOULD-FIX: TEXT chunk values allocate before the new length guard

Reference: `finding:w9-review6-snap3fix-s2`. Location: `crates/tidemark-store/src/reader_v2.rs:250`, also the projected query at `:742`; the later size/type checks are at `:60` and `:71`.

The original oversized-BLOB case is fixed, but SQLite's non-STRICT `data BLOB` column also accepts TEXT. Both fetch paths evaluate unguarded `length(data)` before checking its storage type. For TEXT, SQLite reads the payload to calculate character length. This can allocate beyond the intended 41 MiB compressed-input cap before the reader rejects the value.

Reproduction on a writer-produced snapshot:

```sql
UPDATE dir_chunks
SET data=CAST(zeroblob(67108864) AS TEXT)
WHERE first_id=0;
```

A fresh reader subprocess reached `VmHWM: 75248 kB`, then returned `Invalid("invalid v2 snapshot: dir_chunks.data is not a blob")`. A separate isolation run reached `72856 kB` from `SELECT length(data)` alone, which returned **0** because the TEXT begins with NUL. Fixture creation occurred in the parent, outside these measurements. The fixed 64 MiB BLOB probe reached only `9828 kB` and returned the input-ceiling error. This establishes allocation before rejection; no OOM is claimed.

Reject non-BLOB storage types before evaluating length or materializing data in both paths. A lazy type-guarded length expression can preserve the selected row and map the invalid type to `Invalid`; simply filtering malformed rows out would conceal corrupt intervals. Add malformed-type coverage that establishes bounded allocation, since asserting `Invalid` alone already passes. Evidence: `reader-extra-probes.log`, `text-length-isolation.log`, their `.rs` sources, and `original-reader-probes.log`.

### S3 — SHOULD-FIX: wrong-typed required v2 metadata still escapes as `Sql`

Reference: `finding:w9-review6-snap3fix-s3`. Location: `crates/tidemark-store/src/reader_v2.rs:199`–`:203`.

The new conversion handles absent keys and nonnumeric strings, but `r.get::<_, String>` still propagates its SQL conversion error before parsing. For either `plain_depth` or `hash_width`:

```sql
UPDATE meta SET value=x'32' WHERE key='plain_depth';
```

`Snapshot::open` returns `Err(Sql(InvalidColumnType(0, "value", Blob)))`. The generic metadata loader does not read these v2-only keys, so this reaches the changed backend loader. Normalize malformed persisted metadata types to `Invalid` naming the key, while preserving genuine database-operation errors. Add both keys to the regression matrix. Evidence: `review_blob_metadata_error_type` in `reader-extra-probes.rs/.log`. This is an incomplete S3 fix, not a newly introduced regression.

## Supplemental integration note

### N2 — NOTE: the automatic-deletion test also passes the old helper

Reference: `finding:w9-review6-snap3fix-n2`. Location: `crates/tidemark-scan/tests/cold_retention.rs:258`–`:272`.

The fixture creates equivalent hot/cold files, then invokes an entire subsequent scan. Startup calls cleanup at `orchestrator.rs:140`; equivalence handling at `catalog.rs:891`–`:895` switches to cold and removes hot before retention reaches the deletion helper. Thus the test does not require the new two-form deletion.

Confirmed by compiling the pre-fix `d7cd4f0` orchestrator as a standalone library and running the current `automatic_deletion_removes_both_published_forms_in_both_pointer_directions`: **1 passed**, including both loop branches. Test a pair cleanup preserves, or exercise the helper with both forms still present. This is a coverage gap, not evidence that S4 remains broken. Evidence: `baseline-auto-regression.log`, `cold-baseline-probes.md`, `run-cold-baselines.py`.

## Recheck of each original finding

| Original finding | Recheck |
|---|---|
| B1 overlap/cache-dependent wrong results | Fixed. Original individually codec-valid overlap now returns `Invalid` immediately. Both new overlap/gap tests fail on the old reader. The current suite exercises cold/warm cache orderings and full/projected ranges. |
| S1 shallow subtree scans all descendants | Fixed for selection. The original deep-chunk sentinel now returns the same three shallow rows as v1. New regression fails old reader. All eight sort keys retain metric/name/id ties. Path materialization still reads ancestor chunks (`reader_v2.rs:289`); the new comment saying no chunks are decoded is too broad, but the former all-descendant scan is gone. |
| S2 BLOB checked after allocation | Original BLOB case fixed, including an independently probed oversize chunk beyond root on the projected path. New regression fails old reader. Malformed TEXT still allocates before rejection, as above. |
| S3 malformed coordinates/required metadata | Original negative coordinate now returns `Invalid`; missing-key regression and wrong-type/range coordinate cases pass. New test fails old reader. Required metadata storage-type errors remain, as above. |
| S4 selected scan resurrected by surviving form | Fixed by code inspection and original CLI replay: both forms disappear and one retained catalog row remains. New CLI regression fails old code. Automatic helper has the same four-name deletion but its test has N2's weakness. |
| S5 one unavailable size aborts listing | Fixed. Original missing-file replay exits 0, lists both entries, uses null sizes for the missing file and prints its error. Current tests cover text/JSON plus corrupt cold headers; old implementation fails the first missing-file case. |
| S6 historical upgrade comparison | Fixed without changing the historical fixture. Old test fails the comparison; fixed test result is recorded below. Historical config still lacks cold keys and retains the historical metrics destination; config-load coverage checks cold defaults. |
| N1 quarantine/list/prune contract discrepancy | The revised `15-snapshot-v2.md:58`–`:64` now agrees with selected-scan deletion and diagnostic listing; retained scans' quarantine leaves remain protected. |

S4's helpers only derive four same-stem leaf names within the selected snapshot directory. Descriptor-relative trusted-directory traversal and no-follow, regular/single-link leaf checks remain in force. No ordinary retained scan deletion, link following or state-directory escape was established. Same-euid hostile state mutation is outside the existing storage-lock threat model; no privileged mount/race test was run here.

S5 refresh is bounded to one catalog lookup. Missing stat and unreadable header failures retain stderr diagnostics with scan ID, path and underlying error; initial catalog-list errors propagate. I did not establish an operator-silent size failure. A deterministic concurrent publication/refresh success test is still absent.

The extra valid-input matrix compares v1/v2 full rows across 75 writer combinations: five topologies (including root-only), chunk sizes 1/2/3/7/4096, and shallow depths 0/2/4. It covers first/last/single chunks, partial final chunks, empty file/link tables, missing children, all sorts, signed timestamps/ties, relative depths and zero/small limits. No valid snapshot rejection by the neighbor checks was found. A directory table without a root is not a valid scan snapshot.

## Findings retained only in the exact blind candidate

The exact candidate has **not** incorporated `SNAP-cold-daemon-fix`. Its CLI snapshot module, scan orchestrator, w6c test and v2 contract are byte-identical to `d7cd4f0`. Thus the isolated old-module probes above are also evidence about `30883cb`, not merely historical failures. The following existing findings remain in that candidate tree and belong in its receipt, although they are resolved at integration:

| Classification / reference | Exact-candidate location and supported scenario |
|---|---|
| SHOULD-FIX, `finding:w9-review5-snap3-s4` | `crates/tidemark-cli/src/cmd/snapshots.rs:332`: pruning a selected scan that has both `.db` and `.db.zst` leaves the other published form, allowing rebuild to restore it. The current CLI prune regression fails against this exact snapshot module. |
| SHOULD-FIX, `finding:w9-review5-snap3-s5` | `crates/tidemark-cli/src/cmd/snapshots.rs:389`: a catalog entry whose hot file vanished makes list exit 1 with empty stdout. The current list regression fails against this module; size errors still propagate. |
| SHOULD-FIX, `finding:w9-review5-snap3-s6` | `xtask/tests/w6c.rs:442`: deriving the historical fixture from packaged config retains the new cold-default comments, so `upgrade_fixture_options_and_drift` fails. The exact-candidate test source was reproduced in the isolated old-test run. |
| NOTE, `finding:w9-review5-snap3-n1` | `docs/plan/15-snapshot-v2.md:58`: the contract says automatic retention never removes `.bad` leaves while the candidate's deletion helper does remove selected scans' quarantine siblings. The integration contract resolves this discrepancy. |

These are carried-forward findings, not claims that the reader change introduced them. The integration's N2 regression-coverage note does not apply to the exact candidate, which does not contain that new test. No full workspace build of `30883cb` is claimed; the whole-workspace run uses `358e468`.

## Validation and limits

All Cargo execution was offline on Rust 1.98.0, using the existing host cache and isolated target `/tmp/tidemark-review-b372a650-target`. Builds/tests ran on unchanged integration `358e468`; source equality connects the reader evidence to the exact candidate. Setup adjustments are recorded in `validation-setup.md`.

| Validation | Observed result | Evidence |
|---|---|---|
| `cargo test --offline --workspace --no-fail-fast` | Exit 0; 1,111 passed / 0 failed / 14 ignored across 93 completed harness summaries, including nested subprocess summaries | `workspace-tests.log`, `workspace-summary.json` |
| Store `reader_v2` / `cold` | 14 / 25 passed as part of the workspace run | `workspace-tests.log` |
| Scan `cold_retention` / CLI `snapshots_cold` / core `config_load` | 4 / 3 / 10 passed | `workspace-tests.log` |
| `xtask` w6c | 14 passed, including `upgrade_fixture_options_and_drift` | `workspace-tests.log` |
| `cargo fmt --check` | Exit 0 after explicitly selecting the installed rustup home | `fmt-fixed-env.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 | `clippy.log` |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only | `privileged-build.log` |
| Additional reader probes | Six test functions passed; the metadata/TEXT tests assert observed remaining defects, not their correction | `reader-extra-probes.log` |
| Required build cleanup | `cargo clean --offline`, exit 0; standalone executables/rlibs also removed | `cargo-clean.log`, `probe-cleanup.log`, `baseline-cold-cleanup.log` |

The workspace **test** gate is clean at this integration head in this environment. I did not run the full `cargo xtask ci` wrapper, a cargo-deny advisory/license audit, or an independent documentation build. The totals above include nested helper processes and are not counts of distinct test functions.

The original probe assertions intentionally described the old bugs. Their replay exits 101 when the fixes change those outcomes: three reader assertions and both CLI assertions now fail for the expected corrected behavior. They are retained unchanged apart from the CLI helper's absolute source path; these exits are separate from the actual workspace gate.

Before/after sensitivity used standalone `rustc` harnesses with the changed reader, CLI snapshot module, scan orchestrator, or w6c source from `d7cd4f0`, plus unchanged supporting modules/current dependency libraries. All five new reader regressions fail on the old reader; CLI prune/list and w6c fail on old code; automatic deletion passes old code. These are isolated module comparisons, not full baseline workspace builds. Sources, exact harnesses, selected dependencies and logs are retained. Initial harness setup errors (a mixed-feature dependency selection and missing `anyhow`) were corrected before the reported probe runs.

No privileged execution, Rocky VM, independent crash/power-cut campaign beyond the workspace tests, or §8 2M-directory latency/RSS benchmark was performed. The matrix is correctness coverage, not the representative latency gate. No production v2 writer was enabled by this wave; deferred file/link/diff/export methods still deliberately reject v2. The broader wave skim found no additional supported regression. No branch creation, merge, push, source-checkout write or memory-projection edit occurred.

Evidence directory: `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-b372a650ce808b2ebc2d522aeeb1c6a639db881783504cff26953bcbb773ed35/library/`. The exact-candidate receipt includes residual S2/S3 and carried-forward S4/S5/S6/N1; N2 is supplemental integration evidence.
