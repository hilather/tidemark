# Handoff — W2-C-fix2: interrupted scans

- Attempt: `attempt-1f69666654a1efb87f329e77de84488b0c5be9a92a5261281c7af0248e8fd976`
- Base: `37792b7ebb233d06ca7b7ae0d9eafd0501696230`
- Date: 2026-10-11
- Status: implementation evidence candidate; independent verification and VM runtime pending.

## Cause

Commit `9336614` (STORE-seams) changed cancellation finalization from `failed` to `cancelled`, which the frozen catalog rejects, leaving the row `running`; it also passed the already-set walk cancellation flag to `finish_with`, so kept partial candidates were immediately discarded. The CLI Context change is unrelated: reproduction on the assigned baseline showed **exit 3 in both tests**, with stderr `invalid storage data: invalid scan status` and failures on the catalog status assertions, rather than the exit assertion described in the task. Without keep-partial the state held config, lock and catalog files only; with keep-partial it additionally held an empty snapshot root directory, with neither `.db` nor `.db.tmp` (full process/catalog/directory diagnostics in `library/reproduction.log`).

## Fix

The common finalizer records `failed` for any unacknowledged unsuccessful scan, preserving cancellation as the returned typed error and CLI exit 3. Immediately before store publication, walk keep-partial atomically consumes the cancellation that selected Partial and re-arms the flag; subsequent cancellation remains visible to every `finish_with` checkpoint and discards both complete and partial candidates. Bulk and non-keep scans never consume cancellation and publish nothing.

The SIGINT harness drains stderr on a thread and waits for a progress report with a positive collected-directory count before signaling, replacing the 400 ms sleep. Deadline waits are failure bounds, not assumptions about when scanning has progressed. Original CLI outcome assertions are unchanged. The two STORE-seams unit-test expectations change from `cancelled` to `failed`: the W2-C Tests section in `docs/plan/waves/wave-2-scan-engine.md` explicitly says “SIGINT during a scan leaves no `.db` and a catalog row with `status = failed` (or `partial` in walk mode with `--keep-partial`)”; `05-storage.md` §4 explicitly defines `running|complete|partial|failed`, and `04-scanner.md` §8 says “A cancelled or failed scan deletes its `.db.tmp`, unless walk mode is running and `--keep-partial` is given, in which case the snapshot is written with `meta.status = partial` and is never chosen as \"latest\" by default.” The publication regression now consumes an initial keep-partial cancellation, injects a fresh cancellation after the first index, and verifies failed catalog completion, no candidate or temporary file, and preservation of an existing snapshot.

The W2-C integration test recorder now delegates directory samples to the production `record_dir_series` helper. Its direct unconditional insert bypassed the documented first-observation-per-second policy, making the retention test fail when two scans started in the same second. No duration wait or weakened retention assertion was introduced.

## Shared edits and deviations

Only orchestrator.rs, the two owned W2-C test files and this handoff changed. No shared files, dependencies, manifests, lockfile, contracts, store/core code, plan, AGENTS.md or memory projections changed. No branches, merges, pushes, privileged operations or host configuration writes were performed. Used pinned Rust 1.98.0 with a temporary copy of the host registry cache at `/tmp/tidemark-fix2-cargo`, `--offline` and `CARGO_NET_OFFLINE=true`.

## Validation

Logs and exit summaries are in this attempt's worker-output `library/`.

| Command | Observed result |
|---|---|
| Baseline `cargo test --offline -p tidemark-cli --test w2c sigint -- --nocapture` | Both exit 3; both fail on catalog status, as diagnosed above |
| Fixed SIGINT command | 2 passed |
| `cargo test --offline -p tidemark-scan --lib --test w2c` | 35 unit tests passed, 2 existing scale tests ignored; 4 integration tests passed |
| `cargo test --offline -p tidemark-cli -p tidemark-scan --test w2c -- --nocapture`, five consecutive runs | Every run exit 0: CLI 15 passed plus one scale ignored; scan 4 passed |
| `cargo test --offline -p tidemark-cli --test w2c -- --ignored --nocapture`, five consecutive runs | Every run exit 0: million-row growth-cap scale regression passed (126.37, 137.85, 102.86, 83.95, 67.99 seconds) |
| `cargo fmt --check` | Exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --offline --workspace` | Exit 0 |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only |
| `tools/ci.sh`, with offline environment | Exit 0; 135 source owner tags, forbidden-symbol scan, fmt, Clippy, tests passed |
| `git diff --check` | Exit 0 |

The repeated mandatory fixture test explicitly reports `mixed_special pending unrestricted sandbox: Unix socket creation denied (Operation not permitted (os error 1))`; its existing sandbox-aware branch skips that fixture. Other mandatory fixtures pass their du/find comparisons. Privileged W2-C bulk selection is excluded from unprivileged repetitions and compiled by the privileged build.

## Remaining uncertainty

CI reports `warning: cargo-deny is not installed; skipping cargo deny check`; advisory/license auditing remains pending an equipped environment. Privileged tests are compiled only; existing loop-XFS/bulk runtime coverage is pending VM. This handoff is evidence for independent review, not a claim of verified task success.

## Completion evidence

- Owned paths only; no shared-file or dependency edits.
- Formatting, workspace Clippy, workspace tests, privileged test compilation and CI observed exit 0.
- All unprivileged W2-C tests, including the ignored CLI scale regression, observed passing five consecutive runs.
- Cancellation during publication is injected deterministically; catalog completion, candidate cleanup and existing-file preservation are asserted.
- Handoff and attempt report written; final committed tree cleanliness and submission receipt recorded in attempt artifacts.
- Independent task verification and VM gate remain external to this evidence candidate.
