# Handoff — RC3-fix: rc3 review findings

## Needs from coordinator

- Run uid-1000/umask-022 socket-enabled and Ubuntu 24.04 gates. This sandbox is uid 0 and Unix socket bind returns EPERM; socket bodies print one skip line. F8's historical hang was not reproduced here, and its no-client deadline body needs that gate.
- Privileged execution and real systemd activation/restart remain pending VM. Only compile privileged tests here.
- No missing accessor, contract change, dependency, or shared-file edit is required. Retained `FileRow.nlink` permits conservative hard-link wording without discovering exact charge ownership.

## What changed and regression evidence

All finding commits use `RC3-fix: F<n> ...`. Exit codes, flags, JSON fields, schemas, scanner/store/daemon/core and manifests are unchanged. Evidence logs are in this attempt's output `library/`.

| Finding | Change | Regression and observed before/after |
| --- | --- | --- |
| F1 | Mark retained multi-link rows `[hard links]`; if any is listed, report remaining size unknown because link charge location is uncertain. Single-link subtraction remains compatible with own charged totals. | `own_files_hardlink_remainder_is_unknown`: deterministic `/r/a/shared`, `/r/b/shared`, unretained `/r/a/small` snapshot representing `top_files=1`, 1 GiB threshold. Before: 1 other file, 0 B; after: unknown with hard-link marker. `f1-before.log`, `f1-after.log`. |
| F2 | Fetch the snapshot directory row for own counts/charges. File pane text/title identifies snapshot ID/time and excludes subsequent changes; suppress live title/footer there. | `own_files_after_refresh_use_snapshot_observation`: snapshot A=8 MiB, live replacement B=16 MiB delivered through refresh seam. Before: bogus 8 MiB remainder; after: snapshot A, zero remainder, snapshot label and no live label. `f2-before.log`, `f2-after.log`. |
| F3 | Recheck active and activating before the stopped-service terminal hint. | `verify_activation_between_probes_does_not_report_stopped`: fake Systemctl completes activation between probes. Before: service Fail; after: service/API Pass, no stopped hint. Existing inactive and activating timeout checks also pass. `f3-before.log`, `f3-after.log`. |
| F4 | Clear stale cursor/direction when installing live results at offset zero. | `live_refresh_from_later_page_restores_own_row`: 2,500 digit-named children, name sort, next page, refresh result. Before: own position unavailable; after: position zero and exactly one own row over all live pages. `f4-before.log`, `f4-after.log`. |
| F5 | Metrics and unwatched-mount config recipes now give `sudo tidemark deploy stage --from ./tidemark.toml --start-now`, explaining service restart and stage-only deferral. Initial install recipe already explicitly reboots; upgrade section explains restart. | `operator_changed_config_recipes_apply_to_running_daemon`: documentation assertions fail before and pass after. Confirmed `stage` calls `sys.restart(UNIT)` only for `opts.start_now`. `f5-before.log`, `f5-after.log`. |
| F6 | Missing-path wording describes only the selected snapshot, using its actual status. Retain recorded skip reasons and scheduling advice. | `missing_path_explicit_old_snapshot_does_not_deny_newer_evidence` and `missing_path_partial_snapshot_is_labelled_partial`: real catalog with newer complete snapshot containing path; explicit old/partial selection. Both fail before and pass after. `f6-before.log`, `f6-after.log`. |
| F7 | Upgrade guidance distinguishes omitted and explicit metrics destinations and explains stale collector files, deletion, mtime and `node_textfile_mtime_seconds`. Exporter behavior unchanged. | `operator_upgrade_explains_metrics_defaults_and_stale_export`: documentation assertions fail before and pass after. `f7-before.log`, `f7-after.log`. |
| F8 | Schedule fixture uses nonblocking accept with five-second deadline, plus read/write timeouts before joining. | `schedule_socket_accept_without_client_has_deadline` checks TimedOut with no client. Both it and the existing schedule test skip socket bodies here (EPERM). No failing-before runtime reproduction claimed. `f8-after.log`. |

## Assumptions and limitations

- Hand-built metadata snapshots isolate CLI behavior from filesystem block-size differences and scanner selection. F1 stores only the shared representative, as top_files=1 selects; the missing small file is reflected in own totals, with zero directory blocks per the review. F2/F4 inject completed live scan results via the existing bounded channel rather than scanning a live fixture.
- Returning selected-snapshot evidence unconditionally is more precise than making an unproven catalog-wide absence claim. No catalog search is added to error rendering.
- Hard-link remainder is deliberately unavailable even if a particular link set might be charged locally. Numeric single-link remainder can include directory blocks or unlisted link-set charges; it does not subtract retained bytes charged elsewhere.
- Documentation assertions establish recipe presence, not real service behavior. Actual stage/restart is prohibited in this worker.
- No VM, uid-1000, Ubuntu, real systemd or socket-enabled acceptance is claimed. No new privileged tests are needed for these CLI/doc changes.

## Proposed plan text

For coordinator consideration only; no `docs/plan` edits: “Enter on own files uses only the selected snapshot's retained direct file representatives and directory own totals, labelled with snapshot ID/time even after live refresh. Multi-link representatives are marked and uncertain unretained byte totals are displayed as unknown. Live result installation resets snapshot pagination. Staging a changed configuration does not apply it to an already-running daemon unless `--start-now` is used (restart); disabled textfile export leaves previous collector output for the operator to remove.”

## Validation

All long commands use `timeout 1500`, offline host cache (`CARGO_NET_OFFLINE=true` for the CI wrapper) and pinned Rust 1.98.0. No privileged commands executed.

| Check | Observed result | Log |
| --- | --- | --- |
| `cargo fmt --check` | Passed after formatting the updated existing assertion | `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed on final source/tests | `clippy-final.log` |
| `cargo test --offline --workspace --no-fail-fast` | Exit 0; 992 passed, 0 failed, 14 ignored across 87 harness summaries (includes nested crash-test child summaries). CLI library: 134 passed, 1 ignored. Socket bodies can return early with a printed line. | `workspace-tests-final.log` |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; built only, pending VM execution | `privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 | `doc-final.log` |
| `tools/ci.sh` | Exit 0; all available checks passed. 994 passed, 0 failed, 14 ignored across 88 harness summaries, including strict-feature checks and nested children. cargo-deny absent: warned/skipped. | `ci-final.log` |

The first broad test run failed only the two existing assertions expecting the old F2/F6 behavior; both were updated and the fresh full run passed. The initial CI invocation failed formatting the updated assertion, then it was formatted; that log is retained as `ci-initial.log`. F1–F7 targeted before/after logs predate the broader final pass; F1's fixture was subsequently expanded with both link instances and consistent charged totals. After the standalone full pass, the F2 timestamp/notice were split across lines and rendering assertions were strengthened at 80 and 160 columns (`f2-widths-after.log`); final CI covers that committed adjustment.

## Items not done

- F8 failing-before runtime reproduction and socket-enabled deadline execution: sandbox bind EPERM; coordinator gate required.
- uid-1000/umask-022, Ubuntu 24.04, VM and real-service validation: not available in this worker.
- cargo-deny audit: tool is not installed; CI warns and skips it.
- No plan changes proposed above have been applied; they require coordinator review.

## Submission preparation

Only owned paths changed. No dependencies, manifests or module declarations changed. All source/test changes are committed per finding; handoff/report record remaining external gates. Cleanup and clean-status evidence are retained in the attempt output. This is an implementation evidence candidate, not independent verification or release approval.
