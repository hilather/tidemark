# Handoff — W0-C: Test support: fixtures, oracles, loop-XFS helper

- **Branch:** hp-dad2e63e6bd67c81a69c30e65da730093402b406050f52be7a28e81b6ca2ddb6
- **Agent/session:** attempt-dad2e63e6bd67c81a69c30e65da730093402b406050f52be7a28e81b6ca2ddb6
- **Date:** 2026-10-11
- **Status:** implemented; independent verification and privileged execution pending

## What was built

Implemented the byte-safe fixture DSL, deterministic model/disk tree generation,
real core walk-record conversion, all-metric brute-force reference, GNU and
LVM/filesystem reporting oracles, privileged loop XFS/LVM lifecycle helpers,
and VM runner. The reference computes fresh subtree/inode sets and determines
charges by containment of all discovered instances, independently of any
aggregator or scanner conversion. The golden Rust table reproduces every
number in 03 §6; integration comparisons measure directory metadata at runtime.
Fixture/model item caps, checked model byte totals and a 32 MiB oracle-output
cap bound memory; tree traversal and generation are iterative. The privileged
helpers roll back partial setup and preserve backing storage on cleanup failure.

## Shared-file edits

| File | Change | Why |
|------|--------|-----|
| crates/test-support/Cargo.toml | Attach tidemark-core, tempfile, serde, toml, rustix from workspace dependencies | Frozen scan/path types; owned temporary storage; TOML parsing; safe FIFO/device creation and uid checks |
| Cargo.lock | Record those five existing dependency edges | No new package versions or registry crates introduced |
| xtask/src/main.rs | Wire vm-test match arm to vm_test::run | Sole allowed entry-point wiring change |

Workspace Cargo.toml and xtask/Cargo.toml were unchanged. All additional source,
fixture and README changes are within the task's owned paths. SCR-006 is the
explicitly authorized spec-change request in docs/handoff; no frozen types or
plan files were modified.

## Deviations from the spec

| Spec reference | Decision | Why | SCR? |
|----------------|----------|-----|------|
| 03 §3 / 09 §3 apparent oracle equality | Preserve directory-inclusive reference; expose raw du output and separately probe whether directory st_size must be normalized in comparisons | GNU 9.11 omits directory apparent sizes; observed golden root differs by measured 480 directory bytes | SCR-006, pending |
| 09 §2 mixed_special device entries | Keep FIFO/socket fixture and a separate root-only device extension; exercise both in VM smoke test | Prevent unprivileged suite from creating device nodes even when a worker happens to be root | No; root-only cases retained |
| Socket integration | Report PermissionDenied skip for socket fixture only; FIFO, byte-name, sparse and mode assertions still run | Sandbox denies AF_UNIX socket creation with EPERM | No; VM test requires socket success |
| Random generator unspecified parameter details | dirs includes root; files counts distinct inodes; link_sets picks regular inodes; discovered links number 2..=max; incomplete sets have one outside link; symlink ratio applies to unlinked inodes; Fixed/Uniform size distributions | Deterministic documented interpretation closest to 03 and 09 | No |
| Model/disk metadata | Model allocation rounds to synthetic 512-byte blocks, with fixed injected timestamps and editable metadata; disk fixtures measure actual metadata | Filesystem allocation and creation times cannot be inferred from a seed | No |
| Oracle skipping | Tests call gnu_available before numerical oracles; fallible variants expose errors, infallible wrappers panic on unexpected failures | A u64 return cannot encode a skipped test without risking false comparisons | No |
| Test-support diagnostics | Skip/cleanup diagnostics go to stderr without requiring a tracing subscriber | Test runners must display the mandatory explanatory skip messages | No |
| Offline builds | Copied the host registry cache to /tmp/tidemark-w0c-cargo, pinned Rust 1.98.0 on PATH, CARGO_NET_OFFLINE=true and --offline | Explicit attempt constraint; host cache left unchanged | No |
| VM availability | Wrote runner and feature-gated smoke tests; compiled, never contacted any host or ran mount/LVM helpers | Explicit no-VM/no-host-privileged-operation instruction | No |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|-----------|----------|--------|
| materialize link_semantics_basic reproduces tree; du /r/a and /r match 03 §6 plus measured directory blocks | materialize_link_semantics_basic_matches_du_with_measured_directories checks inode identity, kernel nlink, symlink target, exact allocated file bytes, all six directory du/find comparisons and Drop cleanup | Observed locally |
| ModelTree reference reproduces 03 §6 exactly | reference_reproduces_03_section_6_exactly checks all seven published columns for every row, plus apparent, age, newest, errors and widest | Observed locally |
| random_tree identical listing for same seed | random_model_and_disk_structure_are_seed_deterministic compares canonical byte listings and link identities for root/outside, model equality and real DirRecords; checks different seed changes model | Observed locally |
| LoopXfs and thin LoopVg smoke tests write files | loop_xfs_smoke and loop_lvm_thin_smoke, feature-only gating, no ignore; explicit cleanup assertions | Compiled; pending VM |
| Fixture DSL and mandatory files | dsl_bytes_sparse_specials_and_modes, dsl_rejects_escapes_duplicates_symlink_parents_and_cycles, generated_deep_and_link_web_and_symlink_cycles | Observed locally, socket-specific skip |
| Large generated fixture | generated_wide_has_120000_entries explicitly run with --ignored; 1 passed, 120,000 entries | Observed locally; scale test remains ignored in ordinary suite |
| Every reference metric / boundary behavior | reference_age_boundaries_and_directory_bytes; reference_handles_same_directory_links_races_negative_mtimes_and_errors; random_reference_invariants_across_seeds (100 seeds) | Observed locally |
| Frozen walk record interface and validation | walk_records_preserve_negative_mtimes_names_and_skipped_width; caller_models_reject_invalid_structure_and_byte_overflow | Observed locally |
| VM arguments/command construction | validates_arguments_without_contacting_host; constructs_cargo_command_with_safe_shell_quoting_and_sudo_path; constructs_both_script_suites | Observed locally; no SSH/rsync execution |

## Tests

Final validation used Rust 1.98.0, CARGO_HOME=/tmp/tidemark-w0c-cargo,
CARGO_NET_OFFLINE=true. All commands exited 0:

- cargo fmt --check
- cargo clippy --workspace --all-targets --offline -- -D warnings
- cargo test --workspace --offline
- cargo build --workspace --features privileged --tests --offline
- RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --offline
- tools/ci.sh (source checks: 120 owner tags; forbidden-symbol scan passed; ci: all available checks passed)
- cargo clippy -p test-support --features privileged --all-targets --offline -- -D warnings
- cargo test --offline -p test-support --test support generated_wide_has_120000_entries -- --ignored

The ordinary test-support suite had 1 golden unit test and 10 passing integration
tests, with the one scale test ignored by default. xtask had 6 passing tests
(3 existing source checks and 3 new VM-runner tests). Full transcripts are in
this attempt's worker-output library/validation.log and library/scale.log.
Cargo-deny was not installed; CI explicitly reported the permitted skip.

Privileged tests, all compiled and **pending VM**:

- loop_xfs_smoke — 512 MiB loop XFS, file write, unmount/detach/removal
- loop_lvm_thin_smoke — linear and thin LV, writes to both, cleanup
- special_fixture_smoke — socket and root-only character device metadata
- lvm::tests::refuses_non_test_vg_without_commands — private guard unit test, compiled only; no host commands required by its body

## Measurements

The wide fixture produced exactly 120,000 entries; its explicit test completed
in 4.95 seconds on this sandbox (not a performance claim). GNU coreutils 9.11
reported 1,073,156 apparent bytes for the golden root, versus 1,073,636 including
measured directory st_size. Both allocated-byte golden and model table checks
passed. Directory block counts were measured, never hard-coded.

## Known gaps and follow-ups

- Disposable Rocky 8.8/8.10 VM execution, cleanup behavior under actual command failures, and root special-node smoke tests — pending VM.
- SCR-006 apparent-oracle clarification — pending coordinator decision; normative metric preserved meanwhile.
- cargo-deny license/advisory audit — pending equipped host.
- install/fault script suites belong to later tasks; runner reports a missing suite instead of silently succeeding.
- Oracle find counts include mount-point directories and do not apply exclusions; callers must adjust for scanner skip policies as 09 describes. Owned fixtures contain no mounts.
- No non-test scanner logic, Gate bypass, unsafe code, production metadata reads/writes, host mounts, LVM operations, service changes, branch creation, merge or push were performed.

## Notes for the reviewer

Fixture validation rejects paths escaping temporary storage, conflicting names,
symlink parents (including hardlink-target parents), missing targets and cycles.
Permissions are applied after creation and restored shallowest-first during
Drop. Oracles never parse filenames, preserving newline/non-UTF-8 correctness.
Privileged cleanup stops on error instead of deleting mounted backing storage;
VG names are private, unique, prefix-guarded and checked for pre-existing names.
The VM runner shell-quotes remote paths/filters, explicitly preserves PATH for
sudo, rejects filters that could be libtest flags, excludes target and .git from
rsync, streams output and propagates numeric subprocess exit codes.

## Definition of Done

- [x] Only owned paths changed, plus shared edits and SCR listed above
- [x] cargo xtask ci / tools/ci.sh passed locally (cargo-deny unavailable)
- [x] Privileged tests written, compiled and listed as pending VM
- [x] Public items documented; owner headers present
- [x] Acceptance items have local evidence or explicit pending status
- [x] No unexplained TODOs, unwraps or expects; unwraps only in tests
- [ ] Privileged runtime evidence pending VM
- [ ] Independent verification pending submission review
