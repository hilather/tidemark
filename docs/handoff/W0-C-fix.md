# Handoff — W0-C-fix: Filesystem-independent golden fixture

- **Branch:** hp-cb322cb00554321ebee056e9ee958c8deaaab76afc2b1cacd1e8aa553d525b26
- **Agent/session:** Codex / attempt-cb322cb00554321ebee056e9ee958c8deaaab76afc2b1cacd1e8aa553d525b26
- **Date:** 2026-10-11
- **Status:** implemented; evidence candidate awaiting independent verification

## What changed

The materialized link-semantics test measures allocated bytes for all five non-directory inodes, including the symlink, and inserts them into the reference model before computing rows. Link counts, apparent sizes, hard-link identity, and symlink target checks remain. When every measured inode allocation matches 03 §6, all seven golden-table columns are asserted after subtracting measured subtree directory bytes from disk, charged, and reclaimable totals. Otherwise one stderr line names each differing model inode, path, measured allocation, and golden allocation; only the golden-table assertion is skipped. The du and find oracle checks remain active.

The fixture writer now calls sync_all on each regular file, including sparse files, before returning it to tests. The sparse fixture test compares its measured allocation with du rather than asserting zero blocks. The privileged loop-XFS smoke test explicitly formats with 4096-byte blocks, writes and flushes the worked-example files on that mount, creates both inside and outside hard links and the symlink, and asserts exact normative allocations for inodes 100–104.

## Audit

- Reviewed every test in tests/support.rs: materialized golden allocations and the sparse zero-block assertion were the only filesystem-dependent literal allocation checks. Directory allocation was already measured. Apparent lengths, modes, link counts, entry counts, and pure-model size/age assertions remain.
- Reviewed all tests in tests/privileged.rs: existing file length checks are apparent-size checks; added intentional exact XFS allocation checks to loop_xfs_smoke.
- Reviewed src/golden.rs unit test reference_reproduces_03_section_6_exactly; unchanged, guards the normative in-memory numbers.
- Reviewed all six src/fixture.rs cleanup unit tests and src/lvm.rs refuses_non_test_vg_without_commands; no allocation assumptions.
- Reviewed reference.rs, oracle.rs, random.rs, xfs.rs, and lib.rs; no additional unit tests or literal measured-allocation assertions. Random model allocations are synthetic; disk-structure tests compare lengths and links, not model allocation with the filesystem.

## Shared-file edits and dependencies

None. Only the owned fixture source, two owned integration test files, and this handoff changed. No new dependency or contract change.

## Validation evidence

Offline commands use the host cache with CARGO_HOME=/home/brewerm/.cargo, RUSTUP_HOME=/home/brewerm/.rustup, the pinned 1.98.0 toolchain bin on PATH, and CARGO_NET_OFFLINE=true. Full results are recorded in the attempt output library/validation.log.

- cargo fmt --check — exit 0
- cargo clippy --offline --workspace --all-targets -- -D warnings — exit 0
- cargo test --offline --workspace — exit 0; test-support: 7 unit tests and 10 integration tests passed, 1 scale test ignored
- cargo build --offline --workspace --features privileged --tests — exit 0
- tools/ci.sh (offline inherited by its cargo invocations) — exit 0; 122 owner tags; forbidden-symbol scan passed; all available checks passed

## Filesystem dependence and remaining uncertainty

Allocation and directory apparent sizes remain measured filesystem properties by design; oracle normalization still probes the local GNU du behavior. Pure golden math retains fixed normative inputs. Only the privileged 4 KiB XFS test intentionally requires exact real allocations. Privileged tests are compiled, not executed here: **pending VM**. The reported Rocky 8 overlayfs-on-btrfs failure was not reproduced locally. Scale tests remain explicitly ignored as before. cargo-deny was unavailable and CI explicitly skipped it; dependency/advisory checking remains pending an equipped host.

## Definition of Done evidence

Owned paths only; public contracts and pure golden test unchanged; no new unsafe code, library unwraps, or dependencies. Required local check results and remaining VM/tooling gaps are recorded with the submission report. Independent verification and integration remain outside this attempt.
