# Handoff — W1-C: XFS geometry and bulkstat FFI

- **Branch:** hp-f5d55a58295503cb4bf394aa47ab8dc2c09a61e96cdda1b45427895175bc8a43
- **Agent/session:** Codex, attempt-f5d55a58295503cb4bf394aa47ab8dc2c09a61e96cdda1b45427895175bc8a43
- **Date:** 2026-10-11
- **Status:** implementation and local validation evidence candidate; runtime acceptance pending VM and independent review

## Needs from coordinator

No dependency, feature, shared-file or contract edits needed. Run the privileged suite in disposable Rocky 8.8 and 8.10 VMs and retain the kernel/v5 probe output. Actual advisory/license auditing needs cargo-deny on an equipped host.

## What was built

Replaced the XFS stubs with fstatfs detection, geometry-v1 with feature flags, independent one-record v1/v5 capability probes, checked AG inode arithmetic, and a bounded ascending bulkstat iterator. The iterator tries v5 first, uses its per-AG flag for exact AG intervals, and falls back to the complete v1 path on ENOTTY/EOPNOTSUPP/ENOSYS. Other errors preserve OS errno, including EPERM as PermissionDenied, yield once and terminate. It filters exclusive upper bounds and rejects invalid counts, cursor stalls, unordered records and unrepresentable metadata.

The EL8 ABI is hand-transcribed into the private gen module, with the supplied source package versions and measured sizes in its header. Upstream v5 definitions are explicitly identified as absent from the EL8 userspace header and unverified on EL8. Allocated-byte multiplication is centralized and checked; v1 timestamps retain signed whole Unix seconds and project IDs combine both 16-bit halves. Public signatures and field lists remain unchanged.

## Shared-file edits

None. No dependencies added. Only the five owned deliverable files changed: xfs.rs, gen/mod.rs, gen/bindings.rs, tests/w1c/main.rs, and this note.

## Deviations from the spec

| Spec reference | Implementation | Reason | SCR? |
|---|---|---|---|
| W1-C binding generation | Hand transcription, no bindgen | Explicit task instruction; no Rocky VM or bindgen available | No; authorized |
| 04 §5.1 nlink width and geometry number | v1 nlink is u16; EL8 FSGEOMETRY is nr 124/112 bytes; execution uses geometry-v1 nr 100 | Supplied measured EL8 reference overrides older prose | No; authorized |
| Iterator batch size | Valid nonempty scans require 1..=65,536; oversized/zero batches yield InvalidInput once | Documented cap bounds the ioctl buffer and converted batch; empty ranges issue no syscall | No public contract change |
| Invalid AG geometry | Returns (0, 0) for invalid geometry/groups or overflowing bounds | Frozen arithmetic signature has no error channel; avoids panics and wrapped ranges | No public contract change |
| Metadata Gate charging | Caller must charge geometry/probe/iteration syscalls to its Gate | Frozen sys interfaces have no Gate argument; no dependencies or signature changes introduced | No |
| Build environment | Pinned Rust 1.98.0, temporary cargo cache copy, offline builds | crates.io unavailable; follows W0-A | No |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| Privileged tests pass on Rocky 8.8 and 8.10 | All six new privileged tests compile with the workspace privileged build; no VM available | **pending VM**, not claimed |
| Probe correctly reports v5 availability on both VMs | `bulkstat_probe_records_kernel_support` prints kernel release and independent v1/v5 results; asserts v1 callable | **pending VM**, no EL8 availability finding yet |
| Unsafe only in tidemark-sys; every block commented | New unsafe blocks are only in xfs.rs and sys's VM test; each has SAFETY rationale for allocation, alignment, lifetime and initialization. CI source-policy scan passes | Observed locally |
| Exact supplied ABI sizes, alignments, offsets and widths | `rocky8_x86_64_layouts`, `rocky8_measured_field_widths` | Passed locally |
| Exact ioctl numbers and encoded sizes | `rocky8_ioctl_numbers_and_sizes` asserts all five supplied numbers and size fields | Passed locally |
| v5 ABI independent of EL8 layouts | `upstream_v5_layout`: header 64/8, record 192/8, request 0x8040587f | Passed locally; EL8 runtime unverified |
| AG arithmetic and conversions | Five pure xfs unit tests listed below | Passed locally |
| Format | `cargo fmt --check` | Exit 0 |
| Workspace lint | `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| Unprivileged workspace suite | `cargo test --offline --workspace` | Exit 0 |
| Privileged-feature compilation | `cargo build --offline --workspace --features privileged --tests` | Exit 0; not executed |
| Privileged sys lint | `cargo clippy --offline -p tidemark-sys --features privileged --all-targets -- -D warnings` | Exit 0 |
| Documentation | `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 |
| CI wrapper | `tools/ci.sh` with CARGO_NET_OFFLINE=true | Exit 0; cargo-deny unavailable and explicitly skipped |

The final workspace suite recorded 105 passed, zero failed and one existing scale test ignored. A privileged harness `--list` inventory confirmed the five integration tests plus the forced-v1 unit test are feature-enabled; listing executes no test bodies.

Validation transcripts and per-command exit files are in this attempt's worker-output/library directory. Initial lint failures (redundant i64 cast and manual is_multiple_of) were corrected before the final check pass.

## Tests

Nine new unprivileged tests:

- `rocky8_x86_64_layouts`: all supplied offsets and size/alignment numbers for both geometry structs, bstime, bstat and bulkreq.
- `rocky8_measured_field_widths`: every supplied measured field width.
- `rocky8_ioctl_numbers_and_sizes`: geometry-v1, EL8 geometry, bulkstat, single-bulkstat and inumbers constants.
- `upstream_v5_layout`: separate v5 request/header/record ABI.
- `ag_ranges_use_ceiling_block_log`: non-power-of-two AG lengths, contiguous groups, different inode sizes, invalid inputs and overflow.
- `blocks_are_filesystem_units_and_times_are_signed_seconds`: units, signed pre/post-2038 times, negative/overflow errors, project ID halves and maximum v1 nlink.
- `v5_preserves_wide_links_and_project_id`: v5 wide nlink, project ID, signed time, blocks and overflow rejection.
- `batches_truncate_at_exclusive_end_and_reject_duplicates`: exclusive end, ascending ordering, repeated/out-of-range inodes, cross-batch lower bound and termination.
- `empty_range_is_inert_and_invalid_batch_fails_once`: no ioctl for an empty range; zero/oversized batch errors terminate.

Six new privileged tests, gated only by `feature = "privileged"`, no ignores, all **pending VM**:

- `geometry_matches_xfs_info`: geometry fields and ftype match xfs_info; v5/reflink enabled fixture and nonzero UUID; is_xfs recognizes the mount.
- `bulkstat_covers_all_inodes`: fixture with directories, allocated file, hard link, symlink, sparse file and pre-epoch mtime; all pathname-reachable inode tuples match a no-follow statx walk. Unnamed filesystem metadata inodes are filtered from this pathname comparison.
- `bulkstat_blocks_match_statx`: 10,000 deterministic pseudorandom-size files, including sparse and partially allocated files, flushed to resolve delayed allocation; compares all ten thousand inode byte counts to direct statx blocks times 512.
- `bulkstat_ag_ranges_partition`: exact four-AG intervals with batch size one match a full scan and have no duplicates.
- `bulkstat_probe_records_kernel_support`: records kernel and v1/v5 availability; verifies v1 available and iterator returns metadata.
- `legacy_v1_matches_metadata` (sys unit test): forces the v1 backend even on a v5-capable kernel, uses an allocated file with a sparse tail, and compares size, blocks, nlink, mode and mtime to filesystem metadata.

VM invocation: `cargo test --offline -p tidemark-sys --features privileged -- --nocapture` inside a disposable root VM with xfsprogs and test-support prerequisites. Never run that command on the development host.

## Measurements

Maximum batch 65,536 records. V1 ABI buffer is 8.5 MiB; v5 buffer is 12 MiB plus a 64-byte header; one converted batch is 3.5 MiB. The v5 allocation is released before allocating the v1 fallback buffer. Geometry is fetched once per nonempty iterator. These are allocation-size calculations, not VM/RSS or throughput measurements.

## Known gaps and follow-ups

- Rocky 8.8/8.10 runtime ioctl behavior, block units, geometry flags and v5 backport status are **pending VM**. No XFS ioctl or privileged setup command was executed on this host.
- The legacy ABI has a 16-bit nlink field and cannot represent larger link counts faithfully. V5 preserves its u32 nlink when supported. This is an ABI limitation, not a changed public field.
- Per-syscall Gate charging remains with scan/probe callers, as required by the existing sys signatures.
- Binding measurements target x86_64 EL8; other architectures were not validated.
- cargo-deny license/advisory audit was skipped by the existing CI wrapper because cargo-deny is not installed.
- Integration, independent verification and task acceptance remain coordinator work.

## Notes for the reviewer

V1 lastip starts at start.saturating_sub(1), whereas v5 starts at the inclusive start and resumes from the returned next inode. Both stop on zero output or at the exclusive end. A bounded batch is initialized before FFI; only validated output counts are consumed. V1 bstat buffers use MaybeUninit with zero-initialized storage; v5 uses 8-byte-aligned u64 storage for the flexible-array request. No regular-file contents are read by production code, no paths are interpreted, and the wrappers mutate no filesystem metadata. VM fixture content writes and timestamp setup are confined to the disposable loop fixture.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits
- [x] Format, workspace lint, unprivileged tests, privileged build, documentation and available CI checks passed locally
- [x] Privileged tests written, feature-gated, compiled and listed
- [x] Public items documented; owner headers and SAFETY comments present
- [x] No unexplained TODOs or unwrap/expect outside tests
- [x] Handoff evidence and deviations recorded
- [ ] Runtime acceptance on Rocky 8.8 and 8.10 — pending VM
- [ ] Independent verification/integration — pending submission review
