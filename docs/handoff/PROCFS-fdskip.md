# Handoff — PROCFS-fdskip

## Needs from coordinator

- Run `cargo deny check` on a host with cargo-deny installed; repository CI explicitly skipped that unavailable tool.
- Run the uid 1000 / umask 022 gate and Ubuntu 24.04 CI; this worker ran as uid 0.
- Pending VM: real dead FUSE/NFS behavior and private mount namespaces. No mounts, sockets, user namespaces, or privileged test execution were used here. Injected errors and stat callbacks cover the fault without requiring mounts.
- Independent verification and integration remain with the coordinator. No caller changes, new public API, skipped-fd counter, dependency, or shared-file edits are needed.

## Changes and evidence

1. **1(b), commit `2f5fc09`:** private `per_fd` seam skips descriptor metadata, readlink, fdinfo and recheck errors except ENOMEM, EMFILE and ENFILE, which propagate unchanged. `transient` and process enumeration policy remain unchanged. `injected_descriptor_errors_skip_or_abort` injects ENOTCONN/EIO/ESTALE and resource shortages; the existing test still asserts EIO is not transient. Focused fs tests passed before this commit.
2. **1(a), commit `d241328`:** read fdinfo before target stat, resolve `mnt_id` using the process's mountinfo and `libc::makedev`, and cache sorted mount maps by the device/inode of `/proc/<pid>/ns/mnt`. Known unwatched devices skip stat. Unknown ids, malformed mappings and nontransient mount-map unavailability fall back to stat; transient lookup failures skip the affected process's descriptors. Resource shortages abort. The dev/ino recheck remains and rereads fdinfo to filter a reused descriptor before its second stat. Tests cover escaped mountpoints, optional fields, high major/minor numbers, missing ids, malformed input, mapping failure policy, cache accounting and injected stat callbacks proving the unwatched branch does not stat. Focused fs tests: 93 passed, 1 existing scale test ignored; includes deleted-open and exiting-process tests.
3. **2, commit `44dde20`:** only `substitution` changed in fixture.rs. A local Drop guard holds directory handles and restores victim and child to 0700 before TempDir destruction, including assertion unwinding. Test-support tests: 22 passed, 1 existing scale test ignored.

## Cache bound and charging

The cache lives for one inventory and shares the existing 128 MiB `MAX_INVENTORY_BYTES` budget with open-file records and target capacities. Namespace vector capacity and each mount-pair vector capacity are charged to `Inventory.scratch`, including allocator-returned capacity. Growth is admitted with both old and requested replacement buffers present; failure returns the existing `InventoryBudgetExceeded` with descriptor count and accounted bytes. Cache vectors start at 16 slots and grow geometrically, so the byte budget bounds both namespace and mount counts. No unbounded hash map is introduced. A separate conservative 8 MiB scratch reservation covers capped proc-text reads and UTF-8 conversion buffers, in addition to the existing 8 MiB ID-vector reservation. Existing budget tests remain unchanged and pass; the new cache test proves rejection before allocation and actual-capacity charging.

## Assumptions and limitations

- The task's explicit fallback requirement applies when mount mapping is unavailable. Such descriptors may still stat an unhealthy filesystem. A kernel descriptor-reuse race between fdinfo and stat cannot be eliminated through these procfs paths; both stat attempts prefilter and the existing inode/device comparison rejects observable reuse.
- Mount maps are snapshots retained for this inventory only. New/unknown mount ids fall back to stat. Live mount topology changes and namespace inode reuse are not made atomic by procfs.
- Per-fd fdinfo read failures skip that fd; malformed/missing mnt_id alone falls back. Requested but malformed flags skip the fd, consistent with item 1(b).
- The supplied canonical task governs scope and commit order; no branch creation, merge or push was performed. All Cargo commands use the pinned 1.98.0 toolchain, host cache and offline mode, with long commands under `timeout 1500`.

## Workspace validation

Full transcripts are in the attempt output's `library/` directory. Commands ran offline under `timeout 1500` as uid 0.

| Command | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --workspace --no-fail-fast` | Passed; 1,014 passed, 0 failed, 14 ignored across 87 printed summaries (including child-process summaries) |
| `cargo build --workspace --features privileged --tests` | Passed; compiled only, execution pending VM |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Passed |
| `tools/ci.sh` | Passed all available checks; cargo-deny absent, audit skipped |

No new dependencies, manifest edits, public contract changes or unsafe code. No privileged tests added: the regression seams and parser tests run unprivileged, and real unhealthy mounts/private namespaces remain pending VM. Only owned paths changed. Independent verification is not claimed.

Repository CI printed 1016 passes, 0 failures and 14 ignored tests across 88 summaries, including strict-ancestry and child-process summaries.
