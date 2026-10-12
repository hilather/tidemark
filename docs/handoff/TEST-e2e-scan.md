# Handoff — TEST-e2e-scan: consolidated operator scan scenarios

- Attempt: attempt-2123ced45d140c48d8039d8088cef9556c41981247dc68bff4e5f1dc7b4c4a1f
- Base: d47ac6165d8802a60aa1a81a80c20d29f25119c4
- Date: 2026-10-12; existing attempt branch retained.
- Status: evidence candidate; independent verification pending.

## What was built

`tests/e2e.rs` runs two scenarios against the actual Cargo-built `tidemark` binary. The combined TOML fixture contains the generated 49-set hard-link web, an outside link making one set incomplete, byte names and control characters, a sparse file, five symlinks including a cycle and an escaping target, and an empty mode-000 directory. Text and JSON scans use separate temporary state directories so each initial catalog has exactly one complete, degraded, latest row. Every directory is compared with separate GNU du and normalized apparent-size invocations and GNU find counts. Report queries exercise all nine sorts, composite strict ordering, depth and limits, live top, file thresholds and byte-preserving paths, link topology and filters, symlink classification and filters, catalog list/show, ncdu bytes/JSON exports, private file modes, overwrite refusal and state containment.

The lifecycle scenario waits for both the running catalog row and a progress line reporting collected directories, with a 60-second deadline. It sends SIGINT at 1 operation/second and requires termination within two seconds, exit 3, finalized failed/partial catalog state, cleanup, and exclusion of partials from latest. To prove growth-baseline exclusion, it publishes a complete current scan and moves the retained partial into the 24-hour historical window through the public Catalog API. A separate Python child holds flock and signals readiness before the CLI busy-lock assertion; invalid overrides must exit 2 without creating state.

## Shared-file edits and scope

No manifests, dependencies, lockfiles, module declarations, memory projections or production changes are included. Only the new E2E target, combined fixture, nine narrow-test removals and this note change. Temporary mutation edits to production sources were restored byte-for-byte after each run, including on failure; none are committed. No privileged operations, mounts, services, branches, merges or pushes were performed.

## Assumptions

- GNU du/find and Python 3 are required test prerequisites. Absence fails explicitly, with no early successful return. Python is used only for process-level flock coordination in temporary storage.
- A permission probe distinguishes root with DAC bypass from sandbox root without capabilities. Ordinary unprivileged and capability-free root children retain their identity; DAC-capable root runners drop the report child to UID/GID 65534. The binary is copied into private test storage so inaccessible checkout ancestry cannot prevent execution. The locked directory remains empty; temporarily restoring its permissions for the independent oracle does not reveal unscanned contents or change directory size/mtime.
- “ncdu and JSON” means ncdu's JSON format, with and without the global `--json` flag, plus byte-preserving and `--utf8-lossy` variants. §08 exposes only `export ncdu`; no invented export subcommand is used.
- §08 supplies commands, schemas, path/path_b64 adapters and exit codes, but does not enumerate all report payload fields. Stored row fields use the frozen core DTOs and §05 schema, with metric meanings from §03; renderer schema tests remain intact.
- Historical catalog timestamps are adjusted after real scans, avoiding sleeps of 24 hours and same-second scan-ID ordering assumptions. No snapshot metrics are fabricated.
- Polling sleeps wait for observable state and have deadlines; they are not fixed delays used as readiness assumptions.

## Acceptance evidence

| Requirement | Scenario assertion |
|---|---|
| Every directory agrees with disk/apparent/count oracles | `scan_to_every_report`: all top rows, separate per-subtree du and find calls, exact row count |
| Symlinks not followed; sparse and raw names preserved | Oracle equality and counts, loop class count, file DTO exact raw-path round trip, escaped text, raw/lossy export bytes |
| Incomplete hard links and topology | 49 sets/crossings, exact instance count, measured disk totals, region pair count, one incomplete set with found=2/nlink=3/complete=false, all snapshot paths |
| Every report and sort/filter | Both render modes, eight single-snapshot strict composite sorts and prefix limits, positive growth and strict signed-delta/path order, depth 0/1, files threshold, symlink filters |
| Permission degradation | Exit 3, complete/degraded catalog row, exactly one error row pointing at locked directory |
| Catalog and permissions | One initial complete row, public latest selection, list/show both modes, snapshot/export 0600 |
| Export protection | Outside export succeeds and charges add up; overwrite preserves bytes; inside export exits 5 and creates no output |
| Cancellation and partial retention | Progress-state synchronization, two-second post-SIGINT bound, exit 3, finished_at, failed/no snapshot or partial/file, no latest or temporary artifacts |
| Growth baseline refuses partial | Partial moved into historical window with a real complete current snapshot; growth exits 5 |
| Busy/invalid preflight | Separate child holds lock before query; exit 5; three invalid overrides exit 2 and state remains absent |

## Mutation evidence

Each mutation was applied individually to production in this worktree, tested against its scenario, then restored. Full output and the reproducible mutation driver are in the attempt output `library/`.

| Scenario | Mutation | Observed failure |
|---|---|---|
| Reports | Reader subtracts one byte from `t_disk` | Per-directory du equality fails |
| Reports | Links JSON reports zero incomplete sets | Expected one incomplete set fails |
| Reports | Export containment guard disabled | Expected exit 5 receives successful export |
| Lifecycle | Failed finalization leaves status running | Expected failed catalog status fails |
| Lifecycle | Catalog latest admits partial rows | Expected no latest row fails |
| Lifecycle | Growth baseline selector admits partial rows | Expected exit 5 receives successful growth report |

No unmutated product bug was exposed; no assertion was weakened or marked ignored.

## Retired tests and replacement mapping

| Removed test | Replacement assertion |
|---|---|
| w2c::scan_then_top_files_and_snapshots_end_to_end | Scan/queries and schema envelopes; byte fixture; live_disk populated; live top creates no catalog row |
| w2c::real_fixture_cli_all_single_snapshot_sorts_and_growth | Text/JSON reports and all sort ordering/depth/limits, real second scan with positive growth |
| w2c::sigint_records_failed_and_leaves_no_snapshot | Lifecycle no-keep branch: exit 3, finalized failed row, no file/snapshots/temp |
| w2c::sigint_keep_partial_publishes_partial_and_is_not_latest | Lifecycle keep branch: exit 3, partial metadata/file, no latest, baseline refusal |
| w2c::invalid_scan_overrides_exit_two_without_catalog_mutation | Lifecycle invalid threads 0/65 and malformed exclusion, no state created |
| w3b::scanned_hardlink_web_crossings_and_paths | Exhaustive web totals, measured bytes, region pair, incomplete filter, every stored path, text sections |
| w3b::symlink_filters_preserve_total_counts | Full totals, dangling filter and limit, exact target and class, usage error without target checking |
| w3c::scanned_link_fixture_additive_export | ncdu tree disk sum equals stored root charged total |
| w3c::scanned_nonutf8_preserved_or_lossy_and_escaped | Raw invalid bytes retained, lossy output parses and contains replacements, newline/backslash/quote escapes |

Kept deliberately: w3a/reports.rs tests (hand-built diff schema/count goldens); w1b::link_semantics_basic_exact_records (per-record std::fs oracle); w2c::non_xfs_modes_and_scan_lock_are_explicit (bulk precondition/auto fallback beyond this scenario); w3c::export_mode_overwrite_state_protection_and_wide_json (wide report, aliased state, depth truncation and malformed-snapshot cleanup); all synthetic store tests, aggregation properties and orchestrator checkpoint unit tests. Unreferenced lifecycle/debug helpers and the additive-export sum helper were removed with their tests; retained tests were not modified.

## Test counts

The affected pre-existing CLI targets contain 27 test declarations before and 18 after (W2C 16→11; W3B 5→3; W3C 6→4, including its one feature-gated test). The new E2E target adds two: 27→20 total, nine retired and two added. Unprivileged cases across those targets: 26→19. Workspace enumeration lists 502 current tests, including 13 existing scale ignores; baseline count is 509 inferred by adding back the nine retired cases and subtracting the two new cases. The complete current run reports 489 passed, zero failed, 13 ignored.

## Validation

All Cargo operations use the installed pinned Rust 1.98.0 toolchain with `--offline` or `CARGO_NET_OFFLINE=true`, explicit host `CARGO_HOME` and `RUSTUP_HOME`, and the installed toolchain bin directory on PATH. The default shell lacks Cargo on PATH and the projected Rustup home is read-only; no toolchain or dependency installation was attempted.

| Required check | Final result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline --workspace` | exit 0; 489 passed, 13 existing scale ignores |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only |
| `tools/ci.sh` with offline environment | exit 0; repeats format, Clippy, workspace tests and privileged build |
| `git diff --check` | exit 0 |

The initial Clippy invocation rejected three non-octal zero permission literals in the new tests. They were changed to `0o0`; both standalone Clippy and the final CI invocation pass. Initial and final logs are retained separately. CI reports that cargo-deny is not installed and skips that optional check under the existing xtask policy; dependency advisory/license validation was not performed. Privileged runtime is **pending VM**; no privileged runtime test was executed. No new VM-only behavior is introduced by this test-only task.

## Remaining uncertainty

Independent verification/integration is pending submission review. The UID/GID 65534 fallback for a fully privileged root runner was not exercised here: this sandbox is UID 0 with no effective capabilities and only UID 0 mapped. The mode-000 permission probe correctly retained that identity and the scenario observed permission denial. The workstation run does not establish Rocky/XFS VM behavior; its oracle assertions intentionally measure the active fixture filesystem.

