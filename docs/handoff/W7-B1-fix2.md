# Handoff — W7-B1-fix2: Finished fixture byte accounting

- Attempt: attempt-391699621ddf92feb1e3799e16df2a2150ca18b31df22f970d69d798eee1da9c
- Branch: hp-391699621ddf92feb1e3799e16df2a2150ca18b31df22f970d69d798eee1da9c
- Date: 2026-10-13
- Status: implementation evidence candidate; independent review and Rocky/XFS execution pending.

## Changes

Removed all byte/count accumulation during generation. All data writers and the fixed-size summary placeholder close before a filesystem flush; one final streamed metadata pass computes observed counts, apparent sizes and allocated blocks. Entries use lstat; the held root uses fstat because its procfs capability pathname is a symbolic link. Directories and symbolic links contribute their own bytes. Non-directory multi-link inodes are deduplicated by (device, inode); every duplicate contributes one alias count, with no retained path list. The flat generated tree is streamed with two directory iterators, without recursion or a width-dependent queue; unexpected nested directories and changed devices are refused.

xtask has no rustix dependency, so the authorized fallback invokes GNU `sync -f` directly without a shell. The path addresses the parent process's held directory descriptor under `/proc/<pid>/fd/<fd>`: `/proc/self/fd` would refer to the child, whose descriptors close on exec. The JSON overwrite retains the already allocated 4096-byte size, then closes and flushes again before returning. Both stdout and file label the snapshot `state: before_mutation`; bounded and continuous mutation leave this initial summary intact.

The benchmark previously flushed only for cold-cache preparation; it now flushes the fixture before baseline sampling and the first measured invocation, including warm runs. README documents this behavior in one revised sentence. No shared files, manifests, dependencies, unsafe blocks or product contracts changed.

## Memory estimate

Only multi-link identities occupy the HashSet. At the reports profile's 20% link share, 20M unique inodes require at most 4M keys (actually fewer because directories, controls and symlinks occupy part of the target). With 16-byte keys and one control byte per bucket, the standard HashSet's power-of-two table needs approximately 136 MiB at 8,388,608 buckets; overlapping old/new tables during growth peak below 204 MiB, plus small allocator overhead. This is below about 1 GiB. An explicit 20M-key cap bounds accounting for the supported 100M-inode generator ceiling; allocation failure is not recovered. These are implementation estimates, not measured RSS at scale.

## Acceptance evidence

- `summary_matches_du_and_inode_target`: 500 observed unique inodes, stdout/file equality, direct allocated-byte equality with `du -sx --block-size=1`; test flushes before both oracles. Apparent bytes compare with the existing `du_apparent_normalized` helper, which probes whether local du includes directory sizes as required by 03/09 (direct equality on Rocky's coreutils).
- `finished_summary_counts_three_links_once`: two regular files, one with three names spanning root/report, one symlink, two directories; checks counts, one link set/two aliases, hand-enumerated distinct metadata totals, and both du oracles.
- `mutation_is_confined`: asserts the summary's explicit pre-mutation state after a bounded run.
- `tiny_bench_matches_du`: warm benchmark allocation equality checks the measurement setup.

## Validation

Pinned Rust 1.98.0 tools were put on PATH with `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`; no cache installation or dependencies were added. Logs are retained in canonical worker output `library/`.

| Command | Outcome |
| --- | --- |
| `cargo fmt --check` | Exit 0 |
| `cargo clippy -p xtask --all-targets --offline -- -D warnings` | Exit 0 |
| `cargo test -p xtask --offline` | Exit 0: 17 unit, 9 packaging, 6 W7-B integration tests passed |
| `cargo test -p xtask --offline --features privileged --no-run` | Exit 0; compile only, pending VM |
| `tools/ci.sh` (offline environment) | Exit 1: source checks, format and workspace Clippy passed; workspace tests stopped at five known socket-restricted daemon readiness failures |
| `git diff --check` | Exit 0 |

The five failed w6a1 tests are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. Four report an API socket `PermissionDenied / Operation not permitted`; the binary test times out awaiting readiness. No unrelated fixes were attempted. An initial test run exposed the subprocess descriptor namespace and root procfs lstat issues; both were fixed before the recorded successful xtask checks.

## Remaining uncertainty / guest follow-up

No privileged commands, mounts, LVM changes, cache drops or services ran here. Small fixtures were confined to temporary directories. Local temporary storage cannot reproduce the supplied XFS allocation discrepancy; its cause split is not inferred. The coordinator should run on the Rocky/XFS guest:

```sh
sudo env "PATH=$PATH" cargo test --offline -p xtask --test w7b
```

The same three-link unit test can be run with `sudo env "PATH=$PATH" cargo test --offline -p xtask finished_summary_counts_three_links_once`. No full-scale memory or performance gate was run. Existing privileged staging tests remain pending VM.
