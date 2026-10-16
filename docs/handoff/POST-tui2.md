# Needs from coordinator

- Independently review this candidate and run the uid 1000, umask 022 socket-enabled and Ubuntu 24.04 gates. This worker runs as uid 0; listener-dependent test bodies can skip when sockets are denied.
- Privileged runtime checks remain pending VM; only their test executables are built here.
- Run cargo-deny with the tool and advisory database available if CI reports it unavailable.

# Handoff — POST-tui2

## Changes and regression evidence

1. Snapshot paging retains one lower-bound child separately from the visible page. Reverse keyset loads fetch at most 1,001 children, retaining the extra preceding child as the lower bound; forward loads use their existing exclusive cursor. Empty page attempts restore this bound with the other paging state. Visible child pages remain capped at 1,000. The own row belongs to its forward page independent of arrival direction: insertion rank r belongs at child offset floor(r/1000)*1000; the existing final-page convention retains an own row after all children on the final child page. Live and Growth behavior is preserved.

   `post_tui_disjoint_traversals_all_sorts` failed on the original code with `Up must move one row`, where both selected IDs were the own row (0). It passes with the fix. The matrix now uses 2,500 children and own ranks 0, 999, 1000, 2000 and 2500 for Size, Files, Reclaim, Newest and Name, snapshot and completed live refresh. Growth retains its own-last convention and is also exercised. Names start with `!` before the own rank and `z` afterwards, reproducing the review's Name trace at rank 1000. Tests assert page membership in both directions, PgDn/PgUp reversals, and exact adjacent logical IDs for Up/Down boundary reversals, as well as full row sequences and child coverage. The live seam installs a completed bounded refresh result; no terminal or real refresh-scan claim is made.

2. Default selection and missing-path advice share `default_entry`, preserving the complete/root-specificity/start-time/ID rules. Confirmed evidence always offers `use --snapshot <id>`. The additional wording `or drop --snapshot` appears only if default selection chooses that same confirmed snapshot. Missing or unreadable default candidates never justify dropping the option.

   `post_tui2_remedy_requires_working_default` failed before the remedy change because it offered `drop --snapshot`. Both cases pass afterwards: A lacks the path, B contains it, and newer C omits it; and B has root `/` while C has the more specific root corresponding to `/data`. The latter uses a private temporary `data` directory so it needs no host `/data`. Both offer B explicitly and omit dropping the option. Existing `missing_path_explicit_old_snapshot_does_not_deny_newer_evidence` covers the positive default=B case, retaining both alternatives. No confirmed evidence retains `run: tidemark scan <subject>` and optional schedule detail. Error category remains Precondition/exit 5.

3. `REMEDY_CANDIDATE_LIMIT = 20` bounds the newest-first search before snapshot opens. It limits eligible catalog candidates, including missing-file or failed-open candidates, so at most 20 opens occur. Sorting/listing remains subject to the existing catalog bound; this does not claim a catalog-query latency bound.

   `post_tui2_remedy_search_is_capped` first failed without the cap: among 25 snapshots, the second-oldest contained the path and produced an explicit-ID remedy. With the cap it passes, returning the existing scan remedy without claiming no complete snapshot exists. The same fixtures confirm implicit selection still chooses the newest missing-path snapshot.

## Assumptions and scope

- Task-specific instructions govern commits on the supplied branch; no branch creation, merge or push.
- Root `/` versus a private temporary `data` root exercises the same component-aware specificity rule as `/` versus `/data`, without modifying host paths.
- The catalog is immutable evidence for selection, with the same ordinary deletion-after-check race as before; advice cannot guarantee later file availability.
- No contract, flag, JSON/schema name, manifest, dependency, shared-file or deploy/operator-guide edit. No new unsafe code, scanned-file content reads, scanning metadata syscalls or host metadata changes.
- No new privileged test is needed for these pure browser/diagnostic changes. Existing privileged suites are compiled only; execution remains pending VM.

## Validation

All long commands run under `timeout 1500`, with the pinned Rust 1.98.0 installation and host Cargo cache, offline. Logs are in the attempt's worker-output `library/`. Final command results and harness counts are recorded below after execution.

| Command | Result | Evidence |
| --- | --- | --- |
| `cargo fmt --check` | exit 0 | `library/fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 0 | `library/tests.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; build only | `library/privileged.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `library/doc.log` |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | exit 0; all available checks passed; cargo-deny unavailable | `library/ci.log` |

Workspace test harness summaries total 1,004 passed, 0 failed and 14 ignored (87 summaries, including subprocess harness output; not a unique-test count). The CLI library reports 146 passed, 0 failed and one ignored scale test. The targeted paging run reports one pass; targeted remedy/cap runs report two passes. The CI repeat passes all three regressions, repeats fmt/Clippy/workspace tests/privileged compilation, and reports two strict-ancestry test passes. Source checks report 150 owner tags and a passing forbidden-symbol scan. Cargo-deny was unavailable and skipped with a warning.

Before-change evidence: `library/paging-before.log`, `library/remedy-before.log`, `library/cap-before.log`. After-change targeted evidence: `library/paging-after.log`, `library/remedy-after.log`, `library/cap-after.log`; the workspace/CI runs include the final stronger direction-reversal assertion.

## Final state and limits

Implementation and local validation are complete as evidence for review, not independent task verification or release approval. Only owned paths changed; there are no shared-file edits, new dependencies or spec changes. No privileged test was executed. Socket-enabled uid 1000, GitHub Ubuntu 24.04 and VM runtime checks remain with the coordinator. Catalog listing/sorting still uses its preexisting million-row cap and has no new latency measurement. No interactive terminal or million-child responsiveness claim is made.
