# Handoff — TEST-e2e-deploy: fake-host deployment scenario table

## Needs from coordinator

The required non-local-state row exposes a gap: stage succeeds when the injected state filesystem is nfs. `deployment_scenario_nonlocal_state` retains the expected exit 5 and whole-tree no-change assertion under `#[ignore = "bug: stage does not reject a non-local state filesystem"]`. Running it explicitly fails with actual 0 / expected 5. No production fix or weakened assertion is included. Decide whether to add state-filesystem eligibility or clarify the task: 08 §5 explicitly requires local filesystems for tracked roots, while this task additionally requires a non-local-state scenario.

Independent review and VM acceptance remain pending. Existing privileged deployment/foreign-owner/firstboot tests are retained and compiled only. No production or VM verification is claimed.

- Task/attempt: TEST-e2e-deploy / attempt-b228ea51475aa796de8e20922bb779eec4780828b63e726a5ab7ad0d19a771b9
- Branch: existing canonical attempt branch, no branch creation, merge or push.
- Date: 2026-10-12
- Status: local implementation and validation evidence candidate.

## What was built

`deploy/tests.rs` includes a new test-only `scenarios.rs`. A single active table executes 28 isolated fake-host scenarios, with one additional ignored bug row using the same runner. Each row uses a temporary installation tree, a recording service manager, actual no-follow file publication/compensation/purge code, and injected environment facts. Fake installation artifacts go through the same installation probe as RealEnv; fake ownership goes through the actual eligibility predicate.

The active rows cover valid install/0600 backup/hash/0700 directories/one enable, validation and malformed config failures, 0777/0750 state, foreign ownership, full state filesystem, each missing binary/unit/service-manager artifact, installation/root/mount/capacity probe failures, differing and identical destination symlinks after a prior stage, identical restage, repaired enablement, record-publication and enable failures with compensation, unstage retention and purge including outside symlink targets, unsafe unstage/purge, and healthy/unhealthy/absent API sockets. Service mutations and query counts are asserted. Expected statuses are 0/1/5/6 from 08 §1.3 and §5; malformed stage source is a stage pre-flight failure (5), consistent with retained CLI subprocess tests.

Unchanged rows SHA-256 the entire fake tree before and after, including sorted raw names, file types, modes, owner/group, mtime, file contents and symlink targets. Length framing prevents ambiguous concatenation; reads never follow symlinks. Atime/ctime and filesystem-dependent directory sizes are excluded. Compensation assertions follow R2-7: original bytes/service state restored, new backups and publication records removed, new empty state/deploy directories remain. Compensation is explicitly not asserted to leave the whole tree unchanged.

Healthy/unhealthy API fixtures use connected Unix socket pairs and exchange a bounded newline JSON status request/response through an injected ApiPing. The absent row uses production SocketPing against a missing temporary path. Stream read/write deadlines and joined server threads avoid sleeps or timing-based readiness assumptions. No precondition silently skips a row.

## Every non-test-file edit

No manifests, dependencies, crate roots, CLI flags, configuration keys, core contracts or command adapters changed. `mod.rs`, purge and firstboot sources are unchanged.

| File and final lines | Edit and reason |
|---|---|
| `deploy/real.rs` 71–80 (replaces former inline installation body 71–91) | Delegate to an internal installation helper with the identical four production paths, preserving probe order and behavior. Tests supply four temporary paths instead. |
| `deploy/real.rs` 186–204 | Move the same presence/writability/state-handle checks into `installation(cfg, installed, required)`. Formatting the error via Path::display gives the identical error for all four fixed ASCII production paths. |
| `deploy/real.rs` 206 | Widen local-filesystem classifier to `pub(super)` so the injected filesystem kind uses the real allowlist. Predicate unchanged. |
| `deploy/files.rs` 391–399 | Under `cfg(test)` only, replace st_uid for one explicitly selected path before the unchanged real eligibility predicate. Other stat fields and all ordinary tests are untouched. |
| `deploy/files.rs` 446–450 | Test-only thread-local override, default None. A test guard clears it on success or panic; thread isolation prevents contamination. The helper is also included by the daemon, so the override lives in this shared file rather than referring to CLI-only test code. |

## Retired scenarios and coverage

Only these six tests in `deploy/tests.rs` were removed. No integration/firstboot tests or individual-check unit tests were removed.

| Removed test | Covering table rows |
|---|---|
| `blocking_failures_change_nothing` | installation/root/mount/capacity failure; whole-tree hash, original config, zero calls/queries |
| `invalid_config_changes_nothing` | malformed config; validation failure |
| `private_install_backup_hash_and_idempotence` | valid backup; stage twice (including returned timestamp/hash and exact unchanged tree) |
| `failed_enable_and_record_publication_restore_previous_files` | enable compensation; publication compensation |
| `unstage_retains_data_and_purge_removes_only_state` | unstage keep; unstage purge, including nested data and outside symlink target |
| `installed_symlink_blocks_identical_and_differing_config` | destination symlink; identical destination symlink, both after an initial stage |

The filesystem-type allowlist, unsafe-path list, capacity arithmetic, warning/skip rules, firstboot, detailed multi-point compensation, compensation errors, byte-path backups, start-now restaging, state child permissions, and real foreign-owner VM tests remain. All five W4-B CLI subprocess tests remain: the table does not subsume parsing/rendering or CLI confirmation checks.

Test counts: CLI library before 67 registered unprivileged tests, after 63 (62 active plus the one ignored bug); six removed and two added. `tests.rs` declarations before/after are 36/30 including the retained privileged owner test. W4-B integration before/after is 5/5. Workspace totals and complete validation transcripts are in the canonical worker artifact library.

## Mutation evidence

Each mutation was applied to production code in this working tree, compiled, and run against `deployment_scenario_table`. Each returned test failure (101), not a compilation failure or missing-socket precondition. The originals were restored in a finally block, and no mutation is committed.

| Mutation | Assertion caught |
|---|---|
| Invert `files::eligible` private-directory owner/mode predicate | valid backup becomes exit 1 instead of 0 |
| Skip the backup write in stage | required 0600 backup/file bytes assertion fails with missing file |
| Move enable before pre-flight completes | validation failure observes enable despite pre-flight refusal |
| Follow destination symlinks (remove read O_NOFOLLOW and use following destination stat) | destination symlink stage returns 0 instead of 5 |

Logs: `invert-private.log`, `skip-backup.log`, `early-enable.log`, `follow-symlink.log`; reproduction script retained in the artifact library. Baseline table passes after restoration.

## Validation

Final results are appended below. All Cargo operations use pinned Rust 1.98.0 and `--offline`, or `CARGO_NET_OFFLINE=true` for tools/ci.sh. Host cache was copied to `/tmp/tidemark-e2e-cargo`, with RUSTUP_HOME pointing at the read-only host toolchain. No Cargo file changed.

## Assumptions and limits

- The exact submission contract requires `7488/TEST-e2e-deploy.md` and rejects `docs/handoff/TEST-e2e-deploy.md`. This handoff uses the required artifact path instead of changing an unaccepted path.
- Treat the task's non-local-state row as blocking pre-flight exit 5; the root-only local-filesystem language in 08 §5 needs coordinator resolution as described above.
- A fake host invokes the deployment engine with injected interfaces. Exit assertions reuse CliError's mappings after engine execution, matching the command adapters; this is not a subprocess test of successful command rendering. Retained CLI tests cover parsing and existing error statuses.
- The sandbox rejects UnixListener::bind with EPERM. An initial strict listener run failed (table.log), so healthy/unhealthy sockets use socket pairs; no skip or ignore was added for this environmental restriction. Production pathname-connect success is not established by these rows. Existing client socket-pair unit tests retain framing coverage; real listener and service lifecycle need VM review.
- No sudo, systemctl, mount, LVM, filesystem creation tool, service enablement, or write under /etc, /var/lib, /run or /usr was executed. Privileged tests were compiled, never executed.
- A whole-tree hash covers stable metadata and bytes, not atime/ctime; legitimate reads can change atime. It is not a concurrent-host transaction oracle.

## Final local check results

| Check | Exit/result | Artifact |
|---|---|---|
| `cargo fmt --check` | 0 | `fmt.log`, `fmt.exit` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `clippy.log`, `clippy.exit` |
| `cargo test --offline --workspace` | 0; 484 reported passed, 14 ignored (13 existing + this bug) | `workspace-tests.log`, `workspace-tests.exit` |
| `cargo build --offline --workspace --features privileged --tests` | 0, compilation only | `privileged-build.log`, `privileged-build.exit` |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | 0; all available checks passed | `ci.log`, `ci.exit` |
| Explicit ignored non-local-state row | 101, intentional regression evidence: actual 0 / expected 5 | `nonlocal-bug.log` |
| Each of four final mutations | 101, intended table assertion failure | four mutation logs |
| Baseline CLI library at original deploy sources | 0; 67 passed | `baseline-cli-library.log` |
| Restored final table | 0; 1 passed, all 28 rows executed | `restored-table.log` |

CI source ownership/read-only checks pass. Cargo-deny is not installed; CI prints that the audit was skipped, so advisory/license auditing is not established. Earlier local compile/CI attempts exposed the daemon's shared helper inclusion and the W-prefixed owner-tag requirement; both test-infrastructure issues were corrected before these final checks. No product bug was fixed.

Final baseline/count and restored-table transcripts accompany the report. Required checks ran on the final source bytes; temporary mutation/baseline variants are restored byte-for-byte before committing. Source whitespace and scope are checked again at submission. All results remain evidence for coordinator review, not independent verification.
