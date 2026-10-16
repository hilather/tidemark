# Handoff — PROCFS-fdskip2

## Needs from coordinator

- Independently verify and integrate this evidence candidate; run uid 1000 / umask 022 and Ubuntu 24.04 gates. This worker runs as uid 0 with umask 022.
- Pending VM: actual hung NFS/dead remote filesystems and privileged runtime tests. Privileged tests are compiled only here.
- Run cargo-deny if unavailable in this worker's CI; see validation results below.

## Changes

- Branch: `hp-2d8f1dbb93c0ef1c03432fc9933f642570713ef7ee549341ce4615ce8062f808`
- Agent: Codex; date: 2026-10-15 (America/Toronto).
- Status: implementation and local validation evidence candidate, awaiting independent review.

Item 1(a) is withdrawn. Commit `f213724` is the explicit revert of `d241328`, with the requested message. Review7 found that mountinfo device identity can differ from a regular file's `st_dev` (OverlayFS and Btrfs), and mount IDs can be reused within a single pass. Cached negative mount-map decisions could therefore hide watched files. An empty source diff against `d241328^` confirms that the revert restores the exact pre-prefilter procfs source. All namespace/mount-map parsing, cache lookup and the extra 8 MiB proc-text reservation are removed. `Inventory.scratch` is again `ID_SCRATCH_BYTES`. The retained per-fd classifier from 1(b) and the earlier fixture guard remain; fixture.rs was not edited.

A private `read_descriptor` helper executes exactly the `feb6fb3` observation order. `scan_open_files` supplies these callbacks (procfs.rs:368–370):

```rust
|| fs::metadata(&path),
|| read_fd_target(pid, fd),
|| read_fdinfo_flags(pid, fd),
```

The helper calls them in this order (procfs.rs:292–314):

```rust
let Some(metadata) = per_fd(metadata_read())? else {
    return Ok(None);
};
if !metadata.is_file() || !devs.contains(&metadata.dev()) {
    return Ok(None);
}
let Some(target) = per_fd(target_read())? else {
    return Ok(None);
};
let flags = if want_flags {
    let Some(flags) = per_fd(flags_read())? else {
        return Ok(None);
    };
    Some(flags)
} else {
    None
};
// Reject descriptor reuse detectable by inode/device changes during the reads.
let Some(check) = per_fd(metadata_read())? else {
    return Ok(None);
};
if metadata.dev() != check.dev() || metadata.ino() != check.ino() {
    return Ok(None);
}
```

Flags are read only when requested, strictly between both metadata calls. No mount prefilter remains. The helper adds no retained allocation or public API.

Item 2 extends `per_fd` to propagate typed `ErrorKind::OutOfMemory`, including allocation errors without raw errno, as well as raw ENOMEM/EMFILE/ENFILE. Item 3 replaces the final starttime `.ok()` with explicit validation in private `finish_process`: only transient errors discard the process; all other errors abort the inventory. Successful changed starttime also discards that process.

## Error policy

`transient()` remains exactly ENOENT/ESRCH/EACCES/EPERM. EIO is not transient.

| Step | Skip / discard process / abort |
| --- | --- |
| Open `/proc` | Any open error aborts. |
| Enumerate numeric PID/fd directory entries | Transient entry error skips that entry; other errors abort. |
| Initial starttime / fd-list read | Transient error discards process; any other error aborts. |
| First metadata / readlink / requested flags / metadata recheck | Typed OutOfMemory or raw ENOMEM/EMFILE/ENFILE aborts; every other I/O error skips descriptor (including EIO/ENOTCONN/ESTALE). Malformed requested flags also skip. |
| Nonregular/unwatched metadata or dev/ino change | Skip descriptor. |
| Final starttime read | Transient error discards process; every other error aborts. Unchanged identity retains records; changed identity discards process. |
| Inventory admission / numeric-ID caps | Abort; no successful truncated inventory. |

Discarding a process truncates only its appended records and refunds their target-capacity accounting. Already retained processes remain intact when a later process disappears. An abort returns `Err`, never an incomplete successful inventory.

## Regression evidence

Four new tests in `tests/w1d/fdskip2.rs`:

- `replacement_before_first_metadata_uses_replacement_flags`: observes read-only A's real fdinfo, models replacement of the descriptor slot with writable B immediately before first metadata, and verifies that the accepted B observation carries B's writable flags. Callback assertions constrain the observation order. Uses real temporary regular-file metadata and procfs flags, without sockets, descriptor-allocation races or timing assertions.
- `flags_are_optional_and_inode_replacement_is_rejected`: no flags callback when not requested; distinct-inode replacement between metadata reads rejects the descriptor.
- `typed_and_raw_out_of_memory_abort`: creates a deterministic failed `try_reserve(usize::MAX)`, converts it to the same typed I/O representation used by readers, checks absent raw errno, and verifies propagation alongside all three raw resource errnos. Failed before the fix (`Ok(None)`); passes afterward.
- `final_starttime_errors_discard_only_transient_processes`: injects EMFILE/ENFILE/ENOMEM/EIO/EINVAL and typed OOM (all abort), all four transient errnos (discard process), and same/changed starttime. Verifies prior-process retention and path accounting. Failed with the old `.ok()` behavior (`Ok(())` on EMFILE); passes afterward.

Focused regression run: **4 passed, 0 failed**. Existing per-fd error, starttime parsing, deleted-open-file and inventory budget tests are retained.

## Validation

Workspace tests passed: **1,015 passed, 0 failed, 14 ignored** across 87 printed summaries, including child-process summaries. The first Clippy run reported type complexity in the new private helper; commit `9a31573` introduced a private type alias and the rerun passed. All required command results are recorded below. Full logs and before/after failure evidence are in the attempt output's `library/` directory. All long commands use `timeout 1500`, pinned Rust 1.98.0, host Cargo cache, and offline mode.

| Command | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed after private type-alias correction |
| `cargo test --offline --workspace --no-fail-fast` | Passed; 1,015 passed, 0 failed, 14 ignored; 87 printed summaries |
| `cargo build --offline --workspace --features privileged --tests` | Passed; build only, runtime pending VM |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Passed |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | Passed all available checks; 1017 passed, 0 failed, 14 ignored; 88 printed summaries. cargo-deny unavailable and skipped. |

No failures remain in the required locally available checks. Printed counts include child-process summaries. Source CI counted 150 owner tags and passed the forbidden-symbol scan. `git diff --check` passed. `cargo clean` and clean repository status are recorded in the external report after the final handoff commit.

## Assumptions and limitations

- The canonical task overrides the generic branch convention: no branch creation, merges or pushes.
- Helper-level injection is the requested deterministic alternative to live fd reuse; it models one descriptor slot and does not claim kernel-atomic observations.
- A hung hard-mounted NFS export can still block either metadata stat. This is a known limitation; withdrawing the unsound prefilter deliberately restores metadata observation on every candidate descriptor.
- Same-inode replacement, flag changes after observation and changes after final validation remain best effort.
- No new privileged regression is needed for these injected error/ordering cases. Existing privileged tests are build-only; runtime remains pending VM. No privileged commands were run.
- No dependencies, manifests, shared files, public items or frozen contracts changed. Owned paths only.
