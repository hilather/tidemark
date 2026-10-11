# Handoff — W1-B-fix: notable-file cutoff ties

- **Branch:** hp-116642e8255e3a37b10f4e1943b2ec841ab9d53683c0bc89139ed2b080b1c9d7
- **Attempt:** attempt-116642e8255e3a37b10f4e1943b2ec841ab9d53683c0bc89139ed2b080b1c9d7
- **Date:** 2026-10-11
- **Status:** implementation delivered as an evidence candidate; independent verification and integration pending.

## What changed

Worker heaps now store only allocated sizes. Every candidate equal to the current size cutoff is emitted, while only a strictly larger candidate replaces a full heap's minimum. This avoids filename-based rejection and leaves final `(disk desc, final directory id asc, inode asc)` ordering to the aggregator. Threshold admission and zero-top-files behavior remain covered by the existing test.

Added regressions for 32 equal-sized files in reverse-created directories and 2,048 equal-sized files in one directory. Both measure allocated sizes with independent metadata and run with 1, 2, 4 and 8 workers. The directory test requires every candidate (including the first byte-preorder directory's global top-1 winner); the inode test requires every tie and checks the smallest inode winners for top-1 and top-7. Requiring all ties makes the assertions independent of assignment/discovery order even when the filesystem's directory enumeration differs from creation order.

## Memory bound

No worker-wide tie collection exists. The heap has at most `top_files` size values (configuration already caps this at 200,000); it no longer allocates names. Ties are retained only in the current `DirRecord`, which already fails explicitly with `WalkError::Limit("million-entry directory cap")` beyond 1,000,000 direct entries. Thus a record contains at most 1,000,000 notable candidates, including threshold files and provisional candidates later displaced from the heap. No candidate is silently truncated.

There are at most `threads` records held by producers, including records blocked on send, and `out.capacity()` queued records; those existing validated bounds are 64 and 1,024. Walker-retained notable cardinality is therefore at most `(threads + out.capacity()) * 1,000,000`, plus `threads * top_files` heap size values. Vec allocation capacity and bounded filesystem component names add constant factors. Consumer-retained records are the consumer's responsibility. This is a conservative, large finite ceiling, not a measured RSS guarantee for worst-case ties. Millions of ties across directories are streamed through the bounded channel; a directory exceeding its existing entry cap aborts the scan rather than publishing an incomplete selection as success. No new truncation cap, traversal-dependent omission, contract change or SCR is needed.

## Shared-file edits and dependencies

None. Only `src/walk/worker.rs`, `tests/w1b/main.rs` and this handoff changed. No dependency, manifest, frozen type, module declaration or plan change.

## Test timing

Profiled each original test in a separate invocation of the compiled unprivileged binary, excluding compilation. The original 15 tests took **10.329 s** total on this host; `wide_dir_memory_and_fd_bound` dominated at **8.068 s**. Next largest were `metadata_fixtures` (1.013 s), cancellation (0.536 s), and long paths (0.337 s). The 120,000-file fixture is an inherently large resource/scale test. Marked it `#[ignore = "scale"]`, keeping its fixture, 120,001 total-entry assertion, 120,000 direct-entry assertion, 256 MiB RSS assertion and `threads + 8` fd assertion intact. Its isolated child invocation now includes `--ignored`, so explicit scale execution actually runs its assertions.

Afterward the same serial profiling method, including both new regressions, took **3.366 s** (16 executed tests, one ignored); the ordinary concurrent targeted run took **0.72 s**. After profiling overlapped workspace compilation, so these are local observations rather than controlled benchmarks. Explicit scale execution passed in **11.69 s**, with RSS delta 1,568,768 bytes and peak fds 6 versus limit 12.

The reported >200 s Rocky Linux 8 run was not reproduced on this host. Docker is installed but access to `/var/run/docker.sock` is denied; container before/after measurements remain pending. No privileged commands were run.

## Validation evidence

Logs and exit-code files are in the attempt output `library/`. Used pinned Rust 1.98.0, a private host registry copy at `/tmp/w1b-fix-cargo`, target directory `/tmp/w1b-fix-target`, and `CARGO_NET_OFFLINE=true`; all explicit cargo build/test/lint commands used `--offline`.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --offline --workspace` | Exit 0 |
| Targeted W1-B tests | 16 passed, 1 ignored; `targeted.log` |
| Explicit resource scale test | Exit 0; `scale.log` |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compiled, not executed |
| `tools/ci.sh` with offline environment | Exit 0; cargo-deny unavailable and explicitly skipped |

`before-times.txt` and `after-times.txt` record each test's wall time and output. Existing privileged W1-B tests remain unchanged and are **pending VM**; this metadata admission change needs no additional privileged fixture. Docker/VM runtime validation and independent review remain outstanding. No claim of verified task success or integration is made.

## Definition of Done

- [x] Owned paths only; no shared edits or dependencies
- [x] Format, Clippy, unprivileged workspace and new regressions passed locally
- [x] Bounded cutoff ties documented; no silent truncation
- [x] Before/after local timing and container limitation disclosed
- [x] Public contracts unchanged; no unsafe or production unwrap/expect added
- [x] Privileged test compilation and CI wrapper passed locally (cargo-deny audit skipped)
- [ ] Rocky container and privileged runtime — pending equipped host/VM
- [ ] Independent verification/integration — pending coordinator
