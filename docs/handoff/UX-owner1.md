# Needs from coordinator

- cargo-deny is absent; run the required dependency-policy gate in the coordinator environment.
- Review this evidence candidate; run the uid 1000 / umask 022 gate with sockets and the Ubuntu 24.04 gate. This attempt ran as uid 0 with listening sockets unavailable.
- Consider a store follow-up for a paged direct-directory notable-file accessor. The CLI currently filters a bounded subtree query and refuses when it reaches 1,000,000 retained records rather than silently truncate direct results.
- Real service/Rocky VM acceptance remains pending VM. No privileged tests or host service/package/mount commands were run.

# UX-owner1 — First-server CLI rough edges

- Branch: precreated canonical attempt branch `hp-33c98bd1270d7d64f41710c261b3bab7e8ad80eca46ae7791a7e37bbbc547a47`.
- Date: 2026-10-14. Status: implementation candidate, validation performed; coordinator review pending.

## Per-item implementation and evidence

1. The TUI inserts `.` by disk, files or byte-name order, using charged bytes, apparent bytes and direct file counts; unavailable values remain `-`. Wide terminals show `./ (files here)`. APPARENT is included for both directory and own rows. Bars use the existing charged-byte scale. Child caches stay at 1,000 entries per page; cached filtered counts and cursor boundaries place the own row once, including End and reverse navigation at exact page boundaries. Enter opens all retained direct notable records below the query cap, with path, allocated size, apparent size and UTC mtime, then saturating count/charged-byte remainder. Esc returns through the same close-pane function tested directly. `files --direct` filters by stored directory ID before the user limit, preserving unresolved names and the existing table/JSON schema. Tests: `own_row_sorts_and_enter_lists_direct_files`, `direct_retained_files_filter_before_limit_and_show_utc`, `own_row_appears_once_across_pages_and_sorts_between_children`, `end_includes_own_row_at_an_exact_page_boundary`, `own_files_columns_and_info`, and `files_direct_flag_keeps_existing_json_shape`. The operator guide documents the flag next to `files`. There is no tracked CLI man directory; `cargo xtask man` generated the flag in `target/man/tidemark-files.1`, and runtime bash completions contain it.
2. `top`, `files`, `wide` and TUI use one absent-path formatter naming the requested path, selected complete snapshot root/ID and exact scan remedy. Recorded mount, other-filesystem and exclusion reasons explain skipped coverage; absence alone never asserts a different filesystem. Configured roots get the daemon's `scan.status` next_due in UTC when reachable, with one-second client deadlines; failure omits scheduling. Exit remains Precondition (5). Tests: `ancestor_snapshot_missing_path_names_scan_remedy` uses a catalog with only a `/` snapshot for top/files/wide; TUI fixture tests cover the shared formatter, recorded filesystem boundary, offline omission and configured-root daemon schedule. The socket test prints one line and returns when bind is prohibited.
3. Verification keeps `FAIL service staged, waiting for start` and adds a `WARN service_start` line containing the requested hint. This preserves existing JSON field names while adding an explanatory check entry. Inactive enabled services stop polling immediately and skip the unnecessary API ping. Activating services continue normal polling. Production probes distinguish systemd ActiveState via bounded `show`; ordinary status probes are bounded to three seconds, the activating probe to one second. Tests: `staged_inactive_verify_returns_with_start_hint_without_waiting`, existing `verify_missing_stale_unhealthy_healthy_and_api_failure`, and `verify_timeout_is_bounded` now explicitly uses the activating seam. No real systemctl invocation was performed by this attempt.
4. Metrics wraps trusted-directory failures at the exporter boundary. Missing collector ancestry names the complete configured collector directory and creation/disable remedies. Other writer failures name the textfile; the shared state-directory helper is untouched. Tests: `missing_collector_names_textfile_directory_and_remedy`, `other_export_errors_name_the_textfile`.
5. POSIX post-install prints the requested configuration/deploy/service-name message only when the install argument is 1. `${1:-0}` tolerates an absent argument. No alias/unit change or service action was added. `metadata_and_scriptlet_contract` permits only the exact literal printf line and still rejects start/enable/restart text in every other line; existing shell syntax checks cover post.sh.

## Golden files updated

- `crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_80_24.snap`
- `crates/tidemark-cli/tests/w7a/snapshots/tidemark_cli__tui__tests__main_160_48.snap`

Both show the new own-row label/placement and APPARENT column. No machine-output golden changed.

## Assumptions and limits

- Retained notable files use the snapshot's representative directory ID for a hard-linked inode, as the frozen contract specifies; this is not a listing of every hard-link instance.
- Remainders follow the requested own_files/own_charged minus displayed records, saturating at zero. Own charged bytes also include directory-inode/LCA charges under normative link semantics; no new size semantics were introduced.
- Unknown own growth is last; unknown reclaim/newest use minimum sort values with byte-name ties. A child-name filter also filters `.`.
- The formatter says "selected complete snapshot" to remain accurate for explicit historical --snapshot IDs. For automatic selection this is the latest snapshot chosen by the existing containing-root policy.
- A paged direct-file store accessor is outside the owned scope. The explicit million-record refusal is the remaining scale limitation. No store/core/schema/manifest/dependency changes were made.
- The service hint is an extra check line because the deploy renderer is outside the allowed paths; its WARN prefix keeps the existing FAIL and JSON field names.

## Proposed plan text (not applied)

Replace W7-A's pinned own-files row description with: "The main list includes a `.` row sorted with children by the selected key, using own_charged, own_apparent and own_files where available. Enter shows retained direct notable files and the residual count/charged bytes. `files --direct` excludes descendant directories. Missing snapshot paths name selected coverage, recorded skip reason and a scan remedy, with configured-root next_due from a reachable daemon. Inactive enabled deployments report a start hint and do not wait for the entire verification interval."

## Validation

All long commands ran under `timeout 1500`, with `CARGO_NET_OFFLINE=true` or explicit `--offline`, host `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and toolchain 1.98.0. No host cache/dependency changes were requested.

| Required command | Result |
|---|---|
| `cargo fmt --check` | exit 0, final source |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, final source |
| `cargo test --workspace --no-fail-fast` | exit 0 standalone; rerun by final CI on final implementation source |
| `cargo build --workspace --features privileged --tests` | exit 0, final source; build only, pending VM execution |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | exit 0, final source |
| `tools/ci.sh` | exit 0; source policy, fmt, clippy, strict ancestry, workspace tests and privileged compile passed. Warning: cargo-deny is not installed, so dependency-policy check was skipped. |

Final CI workspace summaries with zero filtered tests total **966 passed, 0 failed, 14 ignored** across 83 summaries (includes doc tests; excludes nested filtered child-process summaries and the separate targeted strict-ancestry run). CLI library: **123 passed, 1 scale ignored**; TUI subset: **23 passed, 1 scale ignored**; CLI w2c: **14 passed, 1 scale ignored**; metrics regressions: **2 passed**; RPM w6c: **14 passed**. The final strengthened exact RPM contract was also rerun alone: **1 passed**. The configured-root daemon schedule test returned with `socket unavailable; skipping schedule message test: Operation not permitted (os error 1)` in this uid-0 restricted worker; the actual listener branch remains for the coordinator. The earlier standalone workspace run reported 962 passed/14 ignored before the final four regressions were added. Golden output was updated during focused TUI testing, then final CI passed without update mode.

Logs, command-status records, generated files man page and bash completions are retained in the canonical attempt output's `library/`. The source implementation was last committed at `58f626d`; subsequent handoff-only commits do not alter it.

## Items not done, shared-file edits and remaining work

No shared manifests, module declarations, unit, normative config, core/store files or plan documents changed. No dependencies added. All five items have implementations and regression evidence; scale accessor, non-root/listening-socket execution and real service VM evidence remain for coordinator review. No claim of independent verified task success or release readiness is made.
