# Handoff — W2-B-fix: Bulk review corrections

- Attempt: attempt-62ee2caf0be878f23bf8a954928d18a5553c3b59d8e978565ddc219cf81b6363
- Branch: hp-62ee2caf0be878f23bf8a954928d18a5553c3b59d8e978565ddc219cf81b6363
- Date: 2026-10-11
- Status: implementation and local evidence submitted for independent verification; VM acceptance pending.

## Findings checked against the current baseline

| Finding | Status | Implementation and test evidence |
|---|---|---|
| 1 — Phase-3 replacement mount | fixed now | `tidemark-fs/src/walk/getdents.rs::read_dir_entries_checked`, exported from `walk/mod.rs`; bulk `names.rs` retains and checks phase-1 directory identities. `checked_read_rejects_symlink_replacement_and_outside_root`, `replaced_directory_names_remain_unresolved`; privileged `checked_read_refuses_mounts_added_after_namespace` compiles, pending VM. |
| 2 — Worker/batch memory | fixed now | Bulk `mod.rs::worker_memory` / `validate_worker_memory` reject buffer combinations before namespace traversal; sort threads use the same effective worker count. `worker_memory_boundary`; join `reference_spill_cap_is_checked_before_allocation`. |
| 3 — Duplicate directory identity | fixed now | `join.rs::matched` fails with `DirectoryIdentityRace { ino, races }`, counting all conflicting directory self-references in the error before dispatch. `duplicate_directory_identity_is_a_counted_fatal_race` asserts two races and zero emissions. Existing legitimate hard-link duplicate tests still pass. |
| 4 — Excluded name alias | fixed now | `names.rs::resolve` applies the original exclusion glob set and explicit skipped mount paths before choosing an alias. `names_exclude_smallest_alias`; `excluded_alias_namespace_bulk_equals_walk` uses the real walker in both stat/namespace modes, production join and resolver, and compares all required rows. |
| 5 — Cancellation | fixed now | Join checks cancellation before source advancement, per inode, gap/tail flushes, final emission and completion; bulk checks after sorting. `cancellation_between_gap_and_tail_batches` asserts exactly one callback for both source cases. Phase-1 normalization was already fixed in baseline `before_phase2`; `cancellation_during_namespace_setup_is_normalized` adds gated-setup coverage. Existing name/cancel and source-cancel tests pass. |
| 6 — Pre-phase-2 classification | already fixed | Baseline bulk `mod.rs::before_phase2`, namespace scope mapping, sort creation and geometry validation already classify these failures. Baseline `namespace_failure_is_redoable_and_cancellation_is_not` covers missing-root fallback and cancellation; unchanged join error tests cover errors after iterator advancement. No change to that classification logic. New memory refusals use the existing `BeforePhase2` variant. |
| 7 — Misleading RSS smoke | fixed now | VM test renamed `bulk_equals_walk_million_file_timing_smoke`; removed process VmHWM and estimate/RSS output. It now measures only walk/bulk elapsed time and row equivalence. Isolated bulk RSS and the 20% acceptance criterion are explicitly pending VM. |

## Decisions and bounds

The checked helper opens root without following any component symlink, then opens each descendant component relative to the current descriptor with `O_DIRECTORY | O_NOFOLLOW | O_NOATIME` (EPERM fallback). Gated descriptor `statx(EMPTY_PATH)` checks device and available mount ID against the opened root before any getdents read. It checks the final inode against the phase-1 value. The pinned Rust 1.98.0 supports `io::ErrorKind::CrossesDevices`, used for device/mount/inode mismatches and paths outside root. Ordinary open errors, including symlink rejection, retain their I/O error. Existing `read_dir_entries` keeps its signature and behavior; only its enumeration body is shared with checked reads.

A compact phase-1 identity index retains discovery id, parent, raw name, inode and device, bounded by the existing one-million-directory limit. It sorts by `(parent, name)` and finds descendants iteratively by raw components, without retaining full paths or performing separate live path prechecks. Failed checked reads leave the requested names unresolved; `run_bulk` adds missing names to its race total. Names in unobserved directories also remain unresolved.

The unchanged estimate reserves 64 MiB. Admission assigns at most 32 MiB of that to bulk workers: `workers * (64 + batch * (192 + size_of::<BulkInode>()) + 2 * message_bytes + 2 MiB stack) + 3 * message_bytes`. Effective workers are `min(threads, agcount)` for sorting and streaming. Each message reserves 4,096 joined layouts plus 1 MiB for spilled directory IDs; the calculation includes replacement/producer messages, two channel slots and the collector's message. Join batches flush at 65,536 accumulated namespace references and reject a single inode group above that count before allocating spill vectors. Capacity growth and a group crossing the flush threshold are included; stale single-link groups release their directory vector after transferring races. The other 32 MiB remains an estimate allowance for other overhead, not a hard process-memory guarantee. The cgroup remains the hard limit under D3. Very large link groups now fail explicitly rather than allocating outside this overhead share; u32 link counts remain intact.

Duplicate directory identities abort the scan with a typed error carrying the raced-reference count, avoiding both duplicate directory callbacks and publication of unreconciled duplicate subtrees. No recovery policy is introduced.

## Scope and compatibility

Only owned paths changed. The sole `walk/mod.rs` edit exports the approved helper. No dependency, Cargo manifest/lock, core type, plan, CLI/config, other lib.rs, branch creation, merge or push changes. Bulk's local WalkConfig copy and owned test initializer now include the baseline's existing `max_record_bytes` field; this is a compatibility fix required to compile the owned bulk code, not a contract addition. MEMORY.md and memory projections were not edited; canonical receipts contained no pending deliveries.

## Validation

All Cargo runs used the pinned toolchain, host cache and `--offline` / `CARGO_NET_OFFLINE=true`. Logs and exit files are in this attempt's output `library/` directory.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 101: baseline W1-B bench/test compilation failures below |
| `cargo test --offline --workspace` | exit 101: baseline W1-B test compilation failures |
| `cargo build --offline --workspace --features privileged --tests` | exit 101: same baseline W1-B test compilation failures |
| `tools/ci.sh` with offline environment | exit 1: source checks and fmt pass; workspace Clippy stops on baseline failures |
| `cargo test --offline -p tidemark-scan -p tidemark-fs --lib --test w2b` | exit 0: 89 passing invocations, 2 existing scale ignores |
| `cargo clippy --offline -p tidemark-scan -p tidemark-fs --lib --test w2b -- -D warnings` | exit 0 |
| `cargo build --offline -p tidemark-scan --features privileged --test w2b` | exit 0, compile only |
| `cargo clippy --offline -p tidemark-scan --features privileged --lib --test w2b -- -D warnings` | exit 0 |
| `cargo test --offline -p tidemark-fs --features privileged --lib --no-run` | exit 0; checked-helper mount regressions compile only |
| `git diff --check` | exit 0 |

Baseline failures left untouched: `crates/tidemark-fs/benches/walk.rs:15` and `tests/w1b/main.rs:15` omit the existing `WalkConfig.max_record_bytes`; W1-B assertions at lines 599 and 726 pattern-match `WalkError::Limit` as `&str` although its field is `String`. These tests belong to another task except for new checked-read coverage, which is placed in owned getdents unit tests so it can run/compile independently. Workspace validation and CI therefore remain incomplete. CI did not reach cargo-deny.

## Pending VM and uncertainty

No privileged tests, mount commands, destructive commands, services, LVM operations or VM connections were executed. Checked-helper tests cover same-device bind mount and another-filesystem bind mount introduced after recording the directory identity, including mount-only rejection without an expected inode when mount IDs are available (different-device rejection is unconditional). As specified by the approved helper contract, a kernel without mount IDs cannot detect a same-device bind that preserves the expected final inode; that boundary remains a limitation. They and existing real-XFS equivalence, ftype refusal and scale tests remain pending on Rocky 8.8/8.10. No isolated RSS or 20% acceptance measurement is claimed. Full workspace CI needs the outside-scope baseline compile fixes before it can complete.
