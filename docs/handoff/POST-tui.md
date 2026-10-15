# Needs from coordinator

- Run the uid 1000, umask 022 socket-enabled gate and GitHub Ubuntu 24.04 gate. This attempt runs as uid 0 in the restricted worker; socket-listener test bodies may skip when bind is denied.
- Privileged runtime/VM checks remain pending VM; this attempt only builds their test executables.
- Run cargo-deny with an installed tool and advisory database: `tools/ci.sh` warned that cargo-deny is unavailable and skipped that check.
- Independent review and acceptance remain with the coordinator.

# Handoff — POST-tui

Date: 2026-10-15. Attempt: attempt-d387e2c41d592a780eb88b2695462b0a87093204581dd1de250cb1e580607e15. Base: `409f49c0180463c82d5b3212efc180dcc14a9864`.

## Changes and regression evidence

1. **Paging:** End now starts at `((count.saturating_sub(1)) / 1000) * 1000`. All pages share the Home/forward boundaries. The snapshot reverse query used by End takes only `min(1000, total - offset)` children; live and Growth slices already take the same final suffix. At 2,500 children, End/PgUp/PgUp visits offsets 2000, 1000, 0, with no overlapping child slices. A 2,000-child directory ends at offset 1000. Empty directories retain the own-files row.

   `post_tui_disjoint_traversals_all_sorts` failed before the fix with `one-row boundary movement Size live=true`; it passes after. Its matrix covers Size, Reclaim, Files, Newest, Name and Growth, 2,500 and 2,000 children, snapshot and completed-refresh paths. Names and numeric fields position `.` at rank 750 for the five non-Growth sorts (including Newest sentinel ties); Growth keeps its existing last-page own row. Each matrix case checks Home/PgDn and End/PgUp child coverage and exactly one `.`, plus full forward/down and reverse/up row sequences across boundaries. Loops are explicitly bounded, including full final pages.

2. **80-column files pane:** title is now `Files | snapshot <id> at <UTC time>`. Sort/filter detail is omitted from this pane's title; snapshot id and UTC time remain in both title and body. The final explanation is split into `(includes directory blocks and hard-linked files` and `whose links share this directory as common ancestor)`.

   `post_tui_files_pane_fits_80_columns` failed before with a clipped title ending `1970-01-12 13:4`. It passes after, asserting the complete snapshot id and timestamp in the title and the explanatory ending in an 80×24 buffer.

3. **Snapshot remedy:** missing-path diagnostics search newer complete catalog snapshots, newest first, and confirm the path in each immutable snapshot. On a match the remedy reads `; newer complete snapshot <id> contains this path; drop --snapshot or use --snapshot <id>`. Without readable complete evidence, the existing `; run: tidemark scan <subject>` remedy and optional scheduling detail remain.

   The strengthened `missing_path_explicit_old_snapshot_does_not_deny_newer_evidence` failed before with the old scan remedy despite snapshot `20260102T000000Z-00000000` containing the path. It passes after. `post_tui_missing_path_keeps_scan_remedy_without_complete_evidence` checks a newer complete snapshot without the path and a newer partial snapshot with the path. Existing partial-selected-snapshot and recorded-boundary diagnostics still pass.

## Assumptions and scope

- The task-specific instructions override the generic branch instruction: no branch creation, merge or push; work stays on the supplied attempt branch.
- Refresh regression coverage uses the established bounded completed-result channel seam (`install_refresh` / `poll_refresh`), avoiding worker timing or host filesystem dependencies. It exercises the post-R installation and paging logic, rather than synthesizing terminal input or running another scan.
- Boundary movement coverage invokes the same `page`, selection and state movement operations as the Up/Down input branches. No interactive terminal or VM claim is made.
- No flags, exit codes, JSON/schema names, dependency or manifest changes. No shared-file edits and no spec changes. Only owned paths changed.
- Existing owner headers remain in place. No new unsafe blocks, regular-file content inspection, scan syscalls or metadata writes were added.

## Validation

All long commands use `timeout 1500`; Cargo uses the host cache offline with the pinned Rust 1.98.0 toolchain. Environment: uid 0, umask 022. Logs are in this attempt's worker-output `library/`, including before/after regression logs.

| Command (offline cache; each timeout 1500) | Result | Log |
| --- | --- | --- |
| `cargo fmt --check` | exit 0 | `library/cargo-fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `library/cargo-clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 0 | `library/cargo-tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; executables only | `library/cargo-privileged.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `library/cargo-doc.log` |

| `CARGO_NET_OFFLINE=true tools/ci.sh` | exit 0; all available checks passed; cargo-deny unavailable | `library/tools/ci.sh-ci.log` |

Workspace harness summaries total 998 passed, 0 failed, 14 ignored (includes subprocess harness output; not a unique-test count). The CLI library reports 140 passed, 0 failed, 1 scale ignored. Targeted paging and render runs each report 1 passed; targeted missing-path coverage reports 6 passed. The CI repeat also reports 140 CLI library passes and passes all three new regressions, including the strengthened full-explanation assertions. Its strict-ancestry feature check reports 2 passed.

No privileged runtime tests were added: these changes concern browser state, rendering and snapshot diagnostics. Existing privileged suites compile; runtime validation remains pending VM. The uid 1000/socket-enabled and Ubuntu gates were not run here.


## Final state

The three requested fixes and regression tests are implemented in focused POST-tui commits. This handoff is local execution evidence, not independent task verification or release approval. No remaining implementation dependency outside the owned paths was identified. All requested commands exited zero; cargo-deny and the environment-specific gates listed at the top remain follow-ups.
