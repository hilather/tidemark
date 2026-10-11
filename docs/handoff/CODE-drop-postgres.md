# Handoff — CODE-drop-postgres: Remove database-specific tracking

## Needs from coordinator

- `crates/tidemark-scan/tests/w2a/main.rs:363` retains the generic annotation test label `pg.db`; replace it with a generic label in the owning task. This file is explicitly outside this attempt's scope. It has no dependency on a removed type or crate.
- Historical handoffs and reviews outside the owned paths retain references to the removed implementation. Their exact search matches are recorded in the attempt's `library/remaining-mentions.txt`; they were not rewritten.
- ADR-019 and R2 in `docs/plan/12-decisions-and-risks.md` still mention retaining the performance gate, contrary to this task's explicit owner instruction. No gate was added; plan edits remain with the coordinator.

Date: 2026-10-11. Attempt: `attempt-a12966d7248dbee773014e1327eb6e31d2469e921f765c4000d9cc4eaccc1bb8`. Status: implementation and local validation evidence candidate; independent verification pending.

## What changed

Removed tracking rather than retaining a disabled collector. Generic annotations, exclusions, ordinary directory scanning and disk-protection mechanisms remain. No schema migration is introduced because this is an unreleased schema.

| Category | Removed or updated |
|---|---|
| Crate | Entire `crates/tidemark-pg`, including collector, queries, labels, exports, unit tests, SQL fixture and privileged temporary-server tests |
| Dependencies | Workspace member and internal dependency, CLI dependency, workspace `postgres`; offline lockfile pruning of client-only dependencies |
| Types | `PostgresConfig`, `PgSample`, `PgSlot`, `PgNow`, `PgParams`, `PgResult` |
| Fields | `Config.postgres` and input/default/validation plumbing; `NowReport.pg`, `NowInputs.pg`, `AlertInputs.pg` |
| Config | `[postgres]`, all its keys, `watch.exclude_postgres`; reference example copied directly from current 08 §3 |
| Alerts | `pg_wal_bytes`, `pg_slot_retained`, `pg_archiver_failing`; evaluation, availability handling, validation, default rules and tests; nine configurable kinds remain |
| Store | `pg_samples` DDL, `TimeSeries::insert_pg`, `query_pg`, `PG_QUERY`, compaction and retention SQL; sample/query-plan tests now cover remaining families |
| CLI | `tidemark pg`, argument/dispatch/module and renderer implementation; command-tree-derived help/completions omit the command |
| Probe | CLI collector composition and PostgreSQL JSON/text section; filesystem report documentation updated |
| Live report | WAL/archiver/slot text and collector degradation message; degraded snapshot and inputs updated |
| Daemon | Reserved `services/pg.rs` stub and module declaration |
| Tests | Deleted CLI database renderer tests and slot disappearance/unreachable tests; removed database portions of store, alert and report tests; made annotation/mount/cgroup fixtures generic |
| Probe regression | Replaced `probe_json_has_all_sections_and_unknown_postgres` with `probe_json_has_remaining_sections`, asserting exactly host/roots/recommendations without host mount assumptions or stub behavior |
| New regressions | `removed_database_table_is_an_unknown_key` checks helpful unknown-field error through `Config::load`; `example_matches_interface_contract_byte_for_byte` pins fixture to plan |
| VM prerequisites | Removed server package from `xtask/README.md` |

There were no implemented password-file packaging, deployment pre-flight, first-boot component or database metrics elsewhere in the baseline; deleting the collector removed its password-file support. No owner-tag/source-check hard-coded file lists required adjustment. Packaging had no matching references.

## Shared-file edits

| File | Change | Reason |
|---|---|---|
| `Cargo.toml` | Removed member, internal dependency and external client dependency | Approved removal |
| `Cargo.lock` | Cargo refreshed offline | Remove packages only needed by collector |
| `crates/tidemark-cli/Cargo.toml` | Removed collector dependency | Approved removal |
| CLI `cli.rs`, `cmd/mod.rs`; daemon `lib.rs` | Removed command/module declarations | Remove dangling declarations |

No dependencies were added. Core edits are limited to the approved removals and formatting their surrounding expressions.

## Remaining mentions

Production code, workspace manifests and lockfile have no matching database references. The required negative config test contains `[postgres]` and its expected unknown-field message. The out-of-scope scan-test annotation label remains as listed above. Historical documents retain obsolete evidence; the complete line-level search is in `library/remaining-mentions.txt`, grouped by path below. This handoff necessarily names the removed items.


## Deleted tests

| Location | Tests removed |
|---|---|
| Collector unit tests | `versions_decode_and_guard_sessions`, `standby_skips_slots_and_old_versions_fail`, `backoff_retains_last_good_and_recovers`, `relations_are_qualified_and_sequential`, `query_failure_publishes_no_partial_pass`, `first_failure_has_unreachable_state_and_selection_skips_other_databases`, `counter_rates_handle_growth_resets_new_databases_and_clock_edges`, `malformed_rows_are_errors`, `configured_password_paths_and_pgpass_matching`, `probe_reports_versions_missing_privileges_and_unreachable`, `password_rejects_wide_permissions_and_symlink` |
| Label/query unit tests | `maps_labels`, `sql_text_is_pinned`, `version_branches` |
| Collector privileged tests | `postgres_monitor_collects_sizes_slots_relations_and_labels`, `postgres_probe_reports_missing_monitor_privileges` |
| CLI renderer tests | `text_lists_database_wal_slots_archiver_temp_and_relations`, `json_is_versioned_and_keeps_null_retention` |
| Alert tests | `vanished_slot_resolves_but_unreachable_does_not`; WAL availability assertions and removed kinds in the all-kinds test |

## Validation environment and limits

Used pinned Rust 1.98.0 from `/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin`, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup` and `CARGO_NET_OFFLINE=true`. All Cargo dependency resolution/build/test/doc commands used the existing offline cache. Initial attempts without explicit environment failed because cargo was absent from PATH and the projected rustup home was read-only; the checks below use the corrected environment.

No privileged runtime tests or host mutation commands ran. Existing remaining privileged suites are compiled, with execution **pending VM**. The removed collector's privileged tests were deleted with its crate. No new privileged behavior needs a new VM test.

## Acceptance evidence

- Workspace and lockfile contain no collector/client packages; Cargo pruned the lockfile offline. No dependencies added.
- Nine alert kinds are covered by the remaining exhaustive enum matches and `all_rule_predicates_and_unavailable_collectors`; defaults are generated only for remaining kinds.
- Config rejection and byte-for-byte fixture tests pass. The CLI command/flag tree and generated completion tests pass after removal.
- Probe JSON/text snapshots and `probe_json_has_remaining_sections` pass. Report snapshots omit database-specific data while retaining watcher/procscan degradation.
- Store roundtrip, compaction, retained-history and query-plan tests cover filesystem/LVM samples and generic annotations.
- `git diff --check` and the supplied scope checker pass locally.
- Outstanding edits are confined to the coordinator items above; independent verification/integration is not claimed.

## Required checks

All commands below exited 0 locally. Detailed output is saved under this attempt's output `library/` directory.

| Command | Log |
|---|---|
| `cargo fmt --check` | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | `clippy.log` |
| `cargo test --offline --workspace` | `test.log` |
| `cargo build --offline --workspace --features privileged --tests` | `privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | `rustdoc.log` |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | `ci.log` |

CI reported `source checks: 120 owner tags; forbidden-symbol scan passed` and `ci: all available checks passed`. It explicitly skipped cargo-deny because that executable is unavailable; dependency advisory/license auditing remains pending an equipped host. Scale tests retain their existing ignore status. Remaining privileged runtime acceptance is pending VM. No out-of-scope validation failure occurred.

## Definition of Done evidence

Owned paths only; no branches created, merged or pushed. Formatting, Clippy, workspace tests, privileged compilation, rustdoc and CI passed locally. Required example/config and probe regressions pass. Handoff and evidence logs are written. Coordinator follow-ups and validation limits are explicit; this report does not assert independent verification or integration.
