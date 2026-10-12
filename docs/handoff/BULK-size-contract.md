# Handoff — BULK-size-contract: Bulk apparent size and fixture writeback

## Needs from coordinator

Run the privileged equivalence and writeback tests on Rocky Linux 8.8 and 8.10 disposable guests. Privileged runtime validation is pending VM; this workstation only compiles those tests. Re-run workspace tests and CI in the coordinator’s socket-capable environment: this sandbox reproduces the five expected W6-A1 readiness-deadline failures. Local workspace/CI results are recorded below; independent verification remains pending.

- Branch: precreated `hp-2715c6a96737d58e7ccad316eb598029c7494e7d4f7e0ac2e1b7d742ed159e55`.
- Date: 2026-10-12. Status: implementation and local validation evidence candidate.

## What changed

1. Recorded accepted R3-16 and reconciled apparent-size/quiescent-tree definitions throughout the plan, including the normative link-semantics document. Regular-file bulk apparent sizes follow XFS's on-disk inode; walk uses in-memory sizes. No production writeback or corrective stat calls are introduced.
2. Added the short lag clause to `crates/tidemark-cli/src/cmd/scan.rs` mode help and pinned it in the existing command-tree test in `crates/tidemark-cli/tests/surface.rs`. No help snapshot pins this text. Documented `BulkInode.size` in `crates/tidemark-sys/src/xfs.rs` and the `apparent: i.size` assignment in `crates/tidemark-scan/src/bulk/join.rs`.
3. Added a small rustix `syncfs` setup helper in `crates/tidemark-scan/tests/w2b/vm.rs`. The mandatory-fixture test flushes the destination loop XFS after `cp -a`; the million-file timing test flushes after generation and before its child measurement and timed scans.
4. Added privileged `vm::bulk_apparent_size_follows_writeback`: writes 1 MiB without fsync, checks file and root disk agreement and bulk apparent size <= walk, then flushes and compares rows with the existing projid/link-name normalization. Both scans must report zero races/missing names. It permits spontaneous kernel writeback and never requires zero apparent size.
5. Replaced per-file `sync_all` in `crates/test-support/src/fixture.rs` with one `rustix::fs::syncfs` after all fixture entries, links and directory modes have been written. The root handle is opened relative to the owned directory with the existing no-follow/no-atime helper. Outside entries share the fixture filesystem and are flushed too. Errors propagate through the unchanged public signatures.

## Every changed plan location

- `docs/plan/13-contract-decisions.md`: R3-16 problem, accepted decision, contract, plan locations and affected tasks.
- `docs/plan/01-requirements.md`: FR-3 and NFR-1 quiescence/apparent-size wording.
- `docs/plan/03-link-semantics.md`: §2 apparent definition; §3 apparent oracle and invariant 6. This avoids leaving the normative definition contradictory to R3-16; worked-example metrics and algorithms are unchanged.
- `docs/plan/04-scanner.md`: introduction, mode table, oracle sentence applying to §1, §5.1 bulkstat size semantics, §10 performance comparison setup.
- `docs/plan/08-interfaces.md`: §1 scan-mode guidance for operators, including auto-selected bulk.
- `docs/plan/09-testing.md`: §3 bulk/walk oracle and fixture flush/test explanation; §5 bulk-equivalence VM table.
- `docs/plan/11-rollout-runbook.md`: first-night mode-choice sentence.
- `docs/plan/waves/wave-2-scan-engine.md`: W2-B mandatory-fixture test setup.

## Test and caller audit

The requested searches (`bulk_sink`, `run_bulk`, `--mode", "bulk`, `mode='bulk'`) and a broader bulk search covered `crates/*/tests` and `xtask/tests`.

- `w2b/vm.rs::bulk_equals_walk_mandatory_fixtures`: added destination syncfs after copy.
- `w2b/vm.rs::bulk_equals_walk_million_file_timing_smoke`: added syncfs after generation; its `bulk_memory_child` inherits that flushed fixture and only checks counts/RSS.
- `w2b/vm.rs::bulk_apparent_size_follows_writeback`: deliberately unflushed first scans; flush before final equivalence.
- `w2b/main.rs::synthetic_bulk_equals_walk` and W2-A model tests: synthetic inode data, no real bulkstat/writeback comparison; unchanged.
- `w2b/main.rs::excluded_alias_namespace_bulk_equals_walk`: real namespace, but synthetic bulk attributes copied from stat metadata, not XFS bulkstat; empty file, no pending data; unchanged.
- Other `w2b/main.rs` run_bulk tests and `vm::bulk_refuses_ftype_zero`: refusal, cancellation or progress tests, no size oracle; unchanged.
- `tidemark-cli/tests/w2c/main.rs::non_xfs_modes_and_scan_lock_are_explicit`: empty tree, checks non-XFS refusal and lock; unchanged.
- `tidemark-scan/tests/w2c/main.rs::auto_xfs_mount_root_and_small_subdir_record_reasons`: checks selection/reason, not size equivalence; unchanged.
- `tidemark-cli/tests/w3b.rs`: synthetic stored bulk snapshot with live name resolution; no bulk scan size comparison; unchanged.
- `tidemark-sys/tests/w1c/main.rs`: broader search found direct bulkstat/statx tests outside this task's writable scope. `fixture` already syncs its data-bearing file, and `bulkstat_blocks_match_statx` already syncs each file before measurement; no missing data flush found. Sparse set_len has no data awaiting writeback. Unchanged.
- No qualifying test in `tidemark-fs/tests`, `tidemarkd/tests`, or `xtask/tests` needed a flush.

All materializing callers were searched, including `random_tree` and crate-internal tests. Constructors are synchronous and return only after completion; no callback or exposed fixture allows measurement while population is underway. Call sites occur in test-support fixture/random/support/privileged tests, tidemark-fs W1-B/W3-B/worker tests, tidemark-cli W2/W3/e2e reports, and tidemark-scan W2-B/W2-C/orchestrator tests. Their post-return stat/du/find measurements retain resolved delayed allocation. Explicit fixture copies still need their own destination flush, now present in W2-B. No caller needed a signature or behavior workaround.

## Measurements

Command: `timeout 1500 cargo test --offline -p test-support --test support generated_wide_has_120000_entries -- --ignored --exact --nocapture`.

Temporary, removed instrumentation measured `Instant` from entry to `try_materialize` to just before returning the completed fixture, excluding fixture cleanup and Cargo build time. Single samples: before **2.386178 s**, after **3.936948 s**. End-to-end test times were 4.02 s and 6.79 s respectively; the uninstrumented baseline test was 3.67 s. These are workstation samples, not a speedup claim: fixture storage is private tmpfs on kernel `7.2.5-3-omarchy`, with no XFS per-file durability latency. The after sample flushed before directory-mode application; the final code places the same sole flush after modes (wide_dir has no modes). Supplied guest evidence is 23 minutes with per-file syncs for the complete mandatory test, not a measurement made here. Coordinator should measure the final guest implementation.

## Assumptions and scope

The coordinator's supplied Rocky/XFS experiment is accepted evidence; no web research or production probing was needed. About 30 seconds is explanatory, never a guaranteed deadline. Ordinary mode-only projid and unresolved link-instance-name differences still use the existing equivalence normalization. The task's explicit plan ownership authorizes the narrow normative clarification in 03. No manifest, dependency, core type, module declaration, scan behavior, schema, flag or output field changed. Rustix already supplies syncfs with workspace fs features; no fallback command was needed.

All builds use the pinned direct toolchain at `/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin`, `CARGO_HOME=/tmp/bulk-cargo` copied from the host registry cache, and offline mode (`--offline` or `CARGO_NET_OFFLINE=true`). Every long command uses `timeout 1500`. No privileged tests were executed and no host mounts, services or LVM were changed.

## Guest commands — pending VM

On each disposable Rocky 8.8/8.10 guest, from the final checkout with the pinned toolchain and populated Cargo cache (guest privilege only):

```sh
sudo env "PATH=$PATH" CARGO_NET_OFFLINE=true timeout 1500 cargo test --offline -p tidemark-scan --features privileged --test w2b vm::bulk_equals_walk_mandatory_fixtures -- --exact --nocapture --test-threads=1
sudo env "PATH=$PATH" CARGO_NET_OFFLINE=true timeout 1500 cargo test --offline -p tidemark-scan --features privileged --test w2b vm::bulk_apparent_size_follows_writeback -- --exact --nocapture --test-threads=1
```

Coordinator wrapper alternatives: `cargo xtask vm-test --host <rocky88-or-rocky810> --filter bulk_equals_walk_mandatory_fixtures --nocapture` and the same command with `--filter bulk_apparent_size_follows_writeback`. The wrapper uses the guest's configured cache/network policy; the direct commands above explicitly stay offline.

## Validation

All logs are retained in the attempt output `library/`; `results.txt` contains exit codes and timings.

| Command (offline, timeout 1500) | Result |
|---|---|
| `cargo fmt --check` | exit 0 (2 s) |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 (34 s) |
| `cargo clippy --workspace --all-targets --features privileged -- -D warnings` | exit 0 (11 s) |
| `cargo test --workspace` | exit 101 (286 s): 492 passed, 5 failed, 13 ignored before fail-fast stopped later targets |
| `cargo build --workspace --features privileged --tests` | exit 0 (50 s); privileged runtime not executed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | exit 0 (11 s) |
| `tools/ci.sh` | exit 1 (247 s): 140 owner tags and forbidden-symbol scan passed; fmt/clippy passed; tests reproduced the same five readiness failures; wrapper stopped before its later build/deny stages |
| `cargo test --workspace --no-fail-fast` | exit 101 (338 s): **544 passed, 5 failed, 13 ignored**, only `tidemarkd --test w6a1` failed |
| `cargo test --workspace -- --list` | exit 0; **562** enumerated unprivileged tests including doctests/ignored tests |
| Explicit unprivileged wide_dir scale measurement | passed before and after; excluded from ordinary suite execution by scale ignore |
| `git diff --check` | passed |

The five failures in each workspace/CI run are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`: readiness deadlines in the documented Unix-socket-restricted sandbox. No daemon source/test changes were made.

The `du` fixture oracle `test-support::support::materialize_link_semantics_basic_matches_du_with_measured_directories` passed. Test-support unit tests: 12 passed; support integration: 10 passed, 1 scale ignored. Scanner W2-B: 21 passed; scanner W2-C: 5 passed, including mandatory disk oracles. CLI `surface` help contract passed.

Counts before/after: **562 → 562** unprivileged tests (after count enumerated; before inferred from unchanged unprivileged test definitions, not a separate baseline workspace run). W2-B alone remains **21 → 21** unprivileged tests; with privileged enabled **25 → 26** test definitions, including the scale test and memory child (baseline/final VM module has 4/5 test definitions). The added test is compile-only here. Independent verification and both guest runs remain pending.

## Shared-file edits and final state

No manifests or module declarations changed. Only the owned plan documents, fixture implementation, two source comments, CLI help/test, W2-B VM tests, and this handoff are changed. The deliverable is committed with the required task prefix; Cargo artifacts are cleaned and the final repository status is checked before submission. This is an evidence candidate, not a declaration of verified integration success.
