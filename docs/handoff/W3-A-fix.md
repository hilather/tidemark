# Handoff — W3-A-fix: bounded explain query memory

- Attempt: `attempt-2c966fbcbe4f7903b1ce9cf8472590be7192b7971780606b7fa062fc1e77758c`
- Branch: `hp-2c966fbcbe4f7903b1ce9cf8472590be7192b7971780606b7fa062fc1e77758c`
- Date: 2026-10-11
- Status: implementation and local evidence candidate; independent verification pending.

## Findings

| Finding / requested audit | Status | Implementation and evidence |
|---|---|---|
| `finding:w3-a-explain-window-memory` — child window retains all children | Fixed now | `crates/tidemark-store/src/diff.rs`: `joined_sql` builds a separate streaming `COUNT(*)` over the child union and an ordered query with a bound LIMIT. `wide_explain_uses_streaming_count_and_bounded_top_n` runs explain on 200,000 children in both directions and checks the exact production query bytecode. Existing W3-A tests retained. |
| Audit other queries in `diff.rs` for width-dependent scratch storage | Checked; no additional offending query found | Subtree diff uses the same limited query builder. Regression checks all three metrics for both subtree and child variants. No other window, GROUP BY, materialized CTE or unlimited ORDER BY is present. ATTACH/DETACH and bounded mmap PRAGMAs are unchanged. |

## What changed

Removed `SUM(...) OVER ()`. A separate aggregate counts children with deltas in the current direction; the top contributor query retains at most `max(top, 1)` rows. Runtime parameters remain bound, full BLOB paths still accompany hashes, and raw-byte ties and symmetric shrink behavior are unchanged. Totals already come from individual target directory rows and own charged counters; no child SUM is needed. Target totals include own files and hard-link LCA charges; a sum of direct-child totals cannot replace them.

The regression inspects SQLite VDBE bytecode rather than global allocator statistics, avoiding unsafe code and process-global measurement interference. It rejects `SorterOpen` and `OpenAutoindex`, rejects any ephemeral table in the aggregate, and requires each limited sorter to have the `IfNotZero` / `Last` / `IdxLE` / `Delete` eviction sequence before insertion. This distinguishes bounded top-N sorting from full-set temporary storage despite both being labelled TEMP B-TREE by EXPLAIN QUERY PLAN. Counts and five raw-byte-ordered unit contributors are checked for growth and shrinkage. The shared synthetic row constructor moved unchanged to `tests/w3a/row.rs` for use by both integration and crate-private plan tests.

## Shared-file edits and deviations

None. No dependency, public signature, reader, manifest, lockfile, core contract, CLI or plan change. No privileged/destructive operation executed. The scale test uses temporary Tidemark snapshot files only. The explicit task permits query-plan evidence instead of SQLite high-water statistics; the regression establishes bounded sorter cardinality, not a numeric process RSS bound.

## Measurements

`cargo test --offline -p tidemark-store --lib wide_explain -- --ignored --nocapture`: exit 0, 1 passed, 46.59 seconds including construction of both 200,001-row snapshots with 237-byte child paths. Marked `#[ignore = "scale"]` because this exceeds a few seconds. Query plan assertions cover disk, reclaimable and files metrics, both query variants, and the streaming count. Evidence: attempt output `library/scale.log` and `scale.exit`.

## Needs from coordinator

No contract or scope change needed. Independent verification/integration and VM execution remain coordinator gates.

## Final local validation

Rust 1.98.0; all Cargo work offline, with a private writable copy of the host registry cache at `/tmp/tidemark-w3afix-cargo`.

| Command | Exit | Evidence |
|---|---:|---|
| `cargo fmt --check` | 0 | `library/cargo-fmt.log`, `cargo-fmt.exit` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `library/cargo-clippy.log`, `cargo-clippy.exit` |
| `cargo test --offline --workspace` | 0 | `library/cargo-test.log`, `cargo-test.exit` |
| `cargo build --offline --workspace --features privileged --tests` | 0 | `library/cargo-build.log`, `cargo-build.exit` |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | 0 | `library/ci.log`, `ci.exit` |

Retained W3-A store/CLI regressions and the real scan/mutate/scan executable story passed in the workspace run. The new wide regression was explicitly executed separately. Privileged compilation includes the retained feature-gated XFS story; execution remains **pending VM**. No new privileged test is appropriate for this SQL-only change.

An earlier overlapping validation run encountered a missing rebuilt test executable, and initial compilation exposed two owned errors that were corrected. Final table/logs supersede those runs. No out-of-scope fix was needed.

## Definition of Done evidence

- Only owned paths changed; no shared files or dependencies changed.
- Existing public contracts and source owner headers retained. No unsafe code introduced.
- One review finding fixed; other query forms audited and included in the plan regression.
- Local checks are evidence candidates, not independent verification, integration or release approval.

CI reports `all available checks passed`; cargo-deny is not installed and its advisory/license audit was skipped. This remains an equipped-host check.
