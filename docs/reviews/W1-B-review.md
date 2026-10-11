**CHANGES REQUESTED** — 0 BLOCKER, 1 SHOULD-FIX, 0 NIT.

Reviewed candidate `8e606ab180018f1d16db474ccb6bb30e3b6bb878` against base `e2eb059ac925982f1f2430d9a21027fa577a2dbf`, limited to the candidate diff. Read the W1-B contract, its scanner/link-semantics references, contract decisions, Wave 0 signatures, and handoff. There is no `docs/handoff/scr-W1-B/` in the reviewed tree.

## Finding

**SHOULD-FIX — Retain size-tied candidates needed by deterministic global top-N.** `crates/tidemark-fs/src/walk/worker.rs:564` (especially lines 568–570). Reference: `03-link-semantics.md` §4 Step G and invariant 6; `04-scanner.md` §2.2 notable-file admission. Receipt reference: `finding:w1b-notable-ties`.

The worker heap orders equal-sized files by filename and rejects a candidate whose name is equal to or less than the current cutoff name. The required final selection uses `(disk desc, final directory id asc, inode asc)`. These orders disagree, and the worker can discard the required winner before the aggregator receives it. Sorting the emitted records cannot recover a discarded candidate.

An unprivileged probe created directories `a031` through `a000` in descending order, each containing one empty single-link file named `same`. It measured equal allocated sizes, then scanned with one worker, `top_files = 1`, and `big_file_threshold = u64::MAX`. The walk completed with 33 directories, 64 entries, zero errors and zero races. Its only notable candidate was `a031/same`; `a000/same`, required by byte-name directory preorder, was absent. The observed traversal order is filesystem-dependent; the probe records the actual omitted winner. No filesystem-dependent size literal is used as its oracle. The existing notable test only exercises unequal sizes and misses this case.

Change admission so it preserves every possible winner under the normative order. A straightforward bounded implementation keeps the size heap but emits every candidate tied at its size cutoff, leaving final tie selection to the aggregator. Alternatively, use a tie key that exactly reproduces directory preorder and inode order. Add regressions with equal-sized files in different directories and reversed discovery order, plus same-directory inode ties; assert that every required global winner is emitted, independent of worker assignment.

## Checks without additional findings

- Scope: all seven changed files are within the owned walker, benchmark, tests and handoff paths. No manifest, lockfile, core contract, or other task file changed. Public configuration/summary fields and function signatures match Wave 0; the error type was reserved for this task.
- Filesystem access: component-by-component opens reject symlinks, final readable directory opens request `O_NOATIME` with a gated EPERM retry, entry stats use no-follow/no-automount flags, and opened directories are checked for device, inode and mount identity. Production walker code contains no regular-file reads, metadata writes or unsafe blocks.
- Resource/concurrency inspection: directory iteration is iterative; task count, retained path bytes, per-record entries, heaps, buffers and output capacity have explicit limits. Temporary component descriptors are serialized, directory handles drop before output backpressure, and active-task guards release in-flight counts on worker panic. Getdents refill charging agrees with the cached rustix implementation.
- All 15 walker integration tests and the stored-error cap unit test passed. The golden fixture compares complete direct records with independent metadata, including hard links, sizes, ages and entries. Required raw-byte, symlink, namespace, exclusion, deletion, wide/deep, cancellation and worker-failure tests exist and pass.
- Three additional deterministic probes passed: a queued child replaced by a symlink; a queued grandchild's ancestor replaced by a symlink; and an opened directory renamed before getdents. They emitted the expected `Gone(Replaced)`/`Gone(Vanished)` records, with one race, zero errors, and no outside-file records.

## Build and test evidence

Commands ran on the assigned review checkout `434b69dacb85e4c54e6d9c1424f70cdb59f0a86c`. Every candidate-owned W1-B file is byte-identical there. This checkout also contains later CLI and W1-D fixes outside the review scope, so these are checkout-wide results, not a claim that the entire older candidate tree passed CI.

Used pinned Rust 1.98.0, a private copy of the host registry at `/tmp/tidemark-w1b-review-cargo`, `CARGO_NET_OFFLINE=true`, and `/tmp/tidemark-w1b-review-target` for build outputs.

| Check | Observed result |
|---|---|
| `tools/ci.sh` | Exit 0: source checks, format, Clippy with warnings denied, and all unprivileged workspace tests passed. Cargo-deny was unavailable and explicitly skipped. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; privileged tests compiled, not executed. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0. |
| Independent notable probe | Confirmed missing required top-file candidate; probe exits 0 when the defect is reproduced. |
| Independent safety probes | All three passed. |

Evidence directory: `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-11047da2fb67421aff41b5efdbd94d847fc338f8bab73f104fa3cf4e150ef436/library/`. Build logs: `ci.log`, `privileged-build.log`, `rustdoc.log`, with corresponding `.exit` files. Probe sources/results: `notable_probe.rs`, `notable-probe.log`, `safety_probe.rs`, `safety_probe.log`.

## Limits

No privileged commands or VM access were attempted. Strictatime, real bind mounts, ftype=0 XFS, Rocky-specific runtime behavior, forced O_NOATIME permission fallback, real EIO faults and mount-change races remain untested. The descriptor/RSS test passed but its fixture has only two directories, so it does not demonstrate all workers holding descriptors simultaneously; the general descriptor bound was inspected structurally. The benchmark was inspected and compiled, not timed. No advisory/license audit ran. This report is a review recommendation and evidence candidate, not integration approval.
