# Handoff — W23-fix-scan: walker merge, provenance and isolated bulk RSS

## Needs from coordinator

N1's post-index/post-sync timestamp remains blocked by the frozen, outside-scope `SnapshotWriter::finish_with` API in `crates/tidemark-store/src/writer.rs`. It takes `&SnapshotMeta`, builds indexes, writes that metadata, closes and syncs, then publishes. Its cancellation callback cannot replace metadata. Please assign the storage owner an approved finalization metadata/clock seam and coordinate its timestamp semantics. This attempt does not mutate a published immutable snapshot or duplicate the writer's private index/publication logic. The candidate moves the timestamp after rows, but still before indexes and sync; it does **not** close all of N1.

- Attempt: `attempt-ba34a3c4da6a14d2ca1022bfabeb5741f96c3d3fcf971613264fee3773633697`
- Base: `5ac9979d183fee13afa478fa30a7e3c4412f39a7`
- Date: 2026-10-12
- Status: implementation evidence candidate; independent review and VM acceptance pending.

## Findings

| Finding | Status | File and test evidence |
|---|---|---|
| S1: notable merge exceeds R2-1 byte bound | fixed now | `fs/src/walk/worker.rs::merge_notable` drops the directory heap, retains eligible top candidates in place and transfers their allocation into the record. Extra capacity is charged for the entire coexisting new allocation before `reserve_exact`. `notable_merge_power_of_two_plus_one_peak` enforces live vector/heap capacities and name payloads plus baseline against both the 16 MiB bound and accounting, for 65,537 top candidates with zero/4,096 threshold candidates. `notable_merge_charges_coexisting_reserve_before_allocation` checks that refusal precedes allocation. |
| N1: phase duration, finish timestamp and OS provenance | not done (partial implementation) | `scan/src/orchestrator.rs` preserves `WalkSummary.elapsed`; failed partial walks use time starting at collection rather than scan setup. `walk_phase_preserves_measured_duration` prevents overwrite. Private `run_scan_with_environment` injects clock and OS discovery without changing public signatures/structs. `read_os_release` reads at most 16 KiB from host `/etc/os-release`; missing/read failure yields empty metadata. `os_release_read_is_bounded_and_replaceable` covers fixture, failure and cap; `scan_provenance_uses_injected_environment` confirms deterministic start/after-row timestamps and saved fixture OS metadata. The index/sync boundary needs the storage seam above. |
| S9: isolated bulk memory acceptance harness missing | fixed now (harness only) | `scan/tests/w2b/vm.rs::bulk_equals_walk_million_file_timing_smoke` launches the already compiled test executable's exact `vm::bulk_memory_child` after fixture creation and before any walk/comparison storage. The child runs bulk only and writes to a discard sink. It logs dirs/files, filesystem used-inode count, estimate and process VmHWM, validates the output counts, and asserts the 120% envelope. Runtime remains pending VM. |

## Reproduction

Before the merge fix, the extracted production merge operations failed `notable_merge_power_of_two_plus_one_peak`: `peak=18286599 bound=16777216`. That peak uses actual Rust vector/heap capacities and seven-byte RawName payloads, plus 2,048 bytes of baseline; it is an allocation-lifetime/capacity proof rather than a whole-process RSS measurement. Before the phase fix, `walk_phase_preserves_measured_duration` failed with actual 999 versus measured 17 milliseconds. Both regressions passed after the fixes. S9's absence was verified against the unchanged baseline source; a failing real XFS/RSS invocation cannot be run on this workstation under the task's explicit prohibition. Its new harness is compiled only.

## Scope and compatibility

Only the two assigned implementation files, owned W2-B VM tests and this handoff changed. No shared-file edits, new dependencies, manifests, lockfiles, core/schema/config changes, public signatures/struct fields, uid/group checks, branches, merges or pushes. Earlier W23-fix-gate cancellation and charged no-follow helpers remain in use. OS release is host metadata, not scanned file content. No privileged tests or host configuration operations were run.

## Pending VM

VM command: `cargo test --offline -p tidemark-scan --features privileged --test w2b -- vm::bulk_equals_walk_million_file_timing_smoke --exact --ignored --nocapture`. Run the already built privileged W2-B target's ignored `vm::bulk_equals_walk_million_file_timing_smoke` with `--exact --ignored --nocapture` on the disposable XFS VM. The fixture contains 100 subdirectories, the root (101 directories total), and 1,000,000 empty files. Parent setup and earlier compilation never enter the child high-water mark; the earlier walk has not yet happened. The child holds no comparison rows and its sink discards output batches. It records `used_inodes = f_files - f_ffree`, `estimate_memory(used_inodes, 101)` and `peak_rss_bytes = VmHWM * 1024`.

Exact pending assertion: `u128::from(peak_rss_bytes) * 100 <= u128::from(estimate_bytes) * 120`. Count assertions require `summary.dirs == 101` and `summary.files == 1_000_000`. The measurement includes the child's runtime/bulk overhead and does not subtract a guessed baseline. Passing compilation does not establish that the RSS assertion will pass on Rocky Linux/XFS.

## Validation

Pinned Rust 1.98.0, `--offline` and `CARGO_NET_OFFLINE=true`, using a private copy of the host registry cache at `/tmp/tidemark-scan-cargo`. Final command logs are in the canonical attempt output's `library/`.

| Command | Observed result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline --workspace` | exit 0; 496 passed, 13 existing ignores across suite/doc-test summaries |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only |
| `tools/ci.sh` with offline environment | first run exit 1 at the outside-scope timeout test below; retry exit 0 |
| `cargo clippy --offline -p tidemark-fs -p tidemark-scan --all-targets -- -D warnings` | exit 0 |
| `cargo test --offline -p tidemark-fs -p tidemark-scan` | exit 0; 153 passed, 5 existing ignores |
| `cargo build --offline -p tidemark-fs -p tidemark-scan --features privileged --tests` | exit 0; compilation only |
| Privileged all-target Clippy for FS/scan | exit 101 at outside-scope W1-A `unwrap()` below |
| Privileged Clippy for owned libraries and W2-B target | exit 0 |
| Privileged W2-B `--list` | exit 0; exact `vm::bulk_memory_child` appears; no test body executed |
| `git diff --check` | exit 0 |

The first CI run failed at `crates/tidemark-fs/tests/w4a/lvm.rs:101` in `lvm::tests::timeout_kills_and_reaps_child`: its fake-command 100 ms timeout assertion did not return `Err(LvmError::Timeout)`. The same unchanged test passed in both the preceding workspace suite and the subsequent scoped suite. No actual LVM reporting/storage command is invoked by this test, and its source is outside this task's scope. The failure log is retained rather than overwritten.

The extra privileged-feature all-target Clippy check found an existing denied `unwrap()` at `crates/tidemark-fs/tests/w1a/main.rs:117` in the VM mount teardown test. That outside-scope file remains unchanged. The new VM harness and owned library code pass the narrower privileged Clippy check. Required default-feature workspace Clippy and privileged workspace compilation both pass.

The CI retry passed source ownership checks, fmt, workspace Clippy and workspace tests. Cargo-deny is not installed in this environment, so xtask explicitly skipped that check; no dependency/advisory audit result is claimed. All privileged checks above are compilation/lint/listing only. No VM test was executed.
