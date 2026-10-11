**CHANGES REQUESTED** — 1 BLOCKER, 1 SHOULD-FIX, 0 NIT.

Reviewed candidate `88864a2568e14fc25580642036be82027501e82d` against base `8d23839528fbe514b5460ad4cadbc7d40df735ea`, scoped to the candidate diff. Builds used the supplied checkout at `2fdc0027b8510b2759ab4ce4f1ea22160e953a58`; its entire `crates/tidemark-sys` subtree is identical to the candidate. No implementation files were changed for this review.

1. **BLOCKER — legacy fallback exposes truncated link counts as complete metadata.** `crates/tidemark-sys/src/xfs.rs:263`, with fallback at line 356. Reference: `03-link-semantics.md` §§2,4; `04-scanner.md` §3.2; AGENTS.md rules 2–3. Receipt reference: `finding:W1-C-legacy-link-count`.

   `convert_v1` widens the ABI's `u16` count to `BulkInode.nlink: u32` without detecting or communicating information loss. The legacy kernel producer directly assigns its full link count to the 16-bit field; see [Linux v4.18 xfs_bulkstat_one_int](https://raw.githubusercontent.com/torvalds/linux/v4.18/fs/xfs/xfs_itable.c). A count of 65,537 therefore arrives as `1` on that path. The independent ioctl mock reproduced successful public records with `nlink=1` and `disk_bytes=8192` after v5 returned ENOTTY. This models the verified producer narrowing; it is not a Rocky VM execution.

   Concrete impact under the normative algorithm: place one of those links inside the scan root and the other 65,536 outside. The emitted count directs the caller into the single-link path, making 8,192 bytes appear reclaimable when none are. Preferring v5 does not protect kernels using the required v1 fallback. The limitation disclosed at `docs/handoff/W1-C.md:88` is not a contract decision permitting incorrect completeness.

   **Recommended change:** retain the measured 16-bit C layout, but file an SCR resolving how v1 obtains trustworthy link counts. Require a metadata/statx or walk fallback before these counts can drive completeness. Until that route exists, fail the v1 metadata path explicitly rather than emit records presented as exact; coordinate the required fallback-contract change. Neither widening the ABI field nor capping/recognizing only large returned values fixes the ambiguity: a returned `1` can already be truncated. Add a forced-v1 regression with 65,537 links and an external-link case; retain the v5 wide-count test.

2. **SHOULD-FIX — required ABI offset assertions are incomplete.** `crates/tidemark-sys/src/gen/bindings.rs:187` and `:245`. Reference: W1-C implementation notes, `04-scanner.md` §5.1, and the review's every-offset criterion. Receipt reference: `finding:W1-C-abi-offset-coverage`.

   The geometry-v1 and geometry tests cover only 8/20 and 9/21 field offsets respectively. Each omits `rtextsize`, `logblocks`, `sectsize`, `imaxpct`, `datablocks`, `rtblocks`, `rtextents`, `logstart`, `sunit`, `swidth`, `logsectsize`, and `rtsectsize`. The v5 record test checks only 6/30 offsets, omitting fields consumed by conversion, including `bs_size`, `bs_xflags`, `bs_uid`, `bs_gid`, and `bs_projectid`.

   An isolated mutation swapped the v5 `bs_uid` and `bs_gid` declarations. All four existing binding tests still passed, although this mutation would interchange returned owners and groups. The candidate's current layouts do match the independent reference; the defect is missing required regression protection for hand-written FFI.

   **Recommended change:** assert every field offset of both geometry structs and `xfs_bulkstat`, including reserved/padding fields, against the C ABI reference. Preserve the existing size/alignment/ioctl assertions. The isolated full-offset probe provides the missing reference values.

Checked and found correct:

- Scope: exactly `xfs.rs`, `gen/mod.rs`, `gen/bindings.rs`, `tests/w1c/main.rs`, and `docs/handoff/W1-C.md` changed. No manifest, lockfile, or other task's path changed. Public function signatures and public data fields match the Wave 0 stubs and W1-C contract. No `docs/handoff/scr-W1-C/` exists.
- All 107 current field offsets, seven sizes and seven alignments match independent x86_64 calculations from the [legacy C header](https://raw.githubusercontent.com/torvalds/linux/v4.18/fs/xfs/libxfs/xfs_fs.h) and [v5 C header](https://raw.githubusercontent.com/torvalds/linux/v5.10/fs/xfs/libxfs/xfs_fs.h). The measured legacy sizes 16/136/32/112 bytes and the four required ioctl numbers match. This checks the supplied Rocky measurements against references; it does not reproduce them on Rocky.
- Every new unsafe block was inspected. Borrowed descriptors and synchronous buffer lifetimes are sound; v1 storage uses initialized `MaybeUninit` slots, v5 storage has sufficient size/alignment, and returned counts are checked before consumption. The 65,536-record cap keeps integer casts and allocation arithmetic bounded. No production panic, content read, metadata write, pathname traversal, recursion, or clock dependency was introduced.
- AG ranges use ceiling block logarithms and checked overflow. Signed whole-second mtimes, project-ID halves, block multiplication and overflow rejection are handled consistently. Actual bulkstat/statx block-unit equality remains a VM check.
- Sixteen independent public-API ioctl-mock scenarios exercised v5 continuation, v1 fallback for ENOTTY/ENOSYS/EOPNOTSUPP, start/end truncation, exact AG selection and exhaustion, zero count, invalid count, stalled cursor, fused errors, EPERM in both backends, non-XFS geometry failure, independent availability probes, and the wide-link reproduction. No real ioctl ran in these probes.
- All four named privileged correctness scenarios exist, plus availability reporting and a forced-v1 test. They are feature-gated without ignores and compile; metadata comparisons use measured statx/filesystem values. Low-level wrappers accept existing descriptors, so opening flags, mount exclusions and Gate charging remain caller responsibilities under the frozen interfaces; scan integration was not assessed here.

Build and test results (pinned Rust 1.98.0, temporary copy of the host Cargo cache, `CARGO_NET_OFFLINE=true`):

| Check | Result |
|---|---|
| `tools/ci.sh` | Exit 0: source policy, formatting, Clippy with `-D warnings`, and workspace tests passed. 130 tests/doctests passed, 0 failed, 2 existing scale tests ignored. All nine W1-C unprivileged tests passed. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compiled only, no privileged tests executed. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0. |
| Independent ABI probe | All current offsets/sizes/alignments passed; the UID/GID mutation also passed the candidate's incomplete tests. |
| Independent ioctl probe | All 16 scenario assertions passed, including reproduction of the incorrect v1 count. |

Evidence artifacts are under this attempt's worker-output `library/`: `ci.log`, `privileged-build.log`, `doc.log`, their exit-status files, `abi-probe/`, and `iteration-probe/`. Attempt: `attempt-85e436e856fb6291fb66349bd612ea39cb94e73d17c6de49cfd38b06293b60d3`.

Not checked: Rocky 8.8/8.10 runtime behavior, real XFS block-unit equality, v5 backport availability, and non-x86_64 ABI behavior. No VM, mount, LVM, or privileged operation was attempted. cargo-deny was unavailable and the CI wrapper explicitly skipped that audit. Those runtime acceptance items remain pending VM as the handoff states. This report is review evidence, not task acceptance or integration approval.
