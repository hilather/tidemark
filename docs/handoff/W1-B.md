# Handoff — W1-B: Parallel walker (walk + namespace modes)

- **Branch:** hp-ca51fc9a80557319dfe68d24d4b5199c578e0024805b06206c2125935b05f64c
- **Agent/session:** attempt-ca51fc9a80557319dfe68d24d4b5199c578e0024805b06206c2125935b05f64c
- **Date:** 2026-10-11
- **Status:** implementation delivered for review; workspace CI blocked by an unchanged CLI assertion. Independent verification and VM acceptance remain pending.

## Needs from coordinator

`crates/tidemark-cli/tests/surface.rs:215` still asserts that `tidemark probe` exits 1. The implemented probe in the supplied baseline exits 0, including when facts are unknown. Both the full workspace test command and `tools/ci.sh` fail on `binaries_and_bash_completions` for this reason. The assertion is present verbatim at base commit `e2eb059ac925982f1f2430d9a21027fa577a2dbf`; CLI sources, tests and the probe/mount modules were not changed by this attempt. Please reconcile that test in its owning task. This task cannot edit it under the assigned write paths.

## What was built

Replaced all walker stubs with safe rustix metadata traversal. Workers use local LIFO crossbeam deques, an injector, FIFO stealing, atomic in-flight accounting and bounded consumer backpressure. Walk mode emits directory metadata, direct single-link statistics, hard-link instances, age buckets and notable-file candidates; namespace mode records non-directory dent inode numbers without per-file stat calls except DT_UNKNOWN classification. Queued children are checked against their expected inode, device and mount ID before reading entries.

Directory opens use O_NOFOLLOW on every component, O_PATH for ancestors, and O_NOATIME on the directory being read, retrying without O_NOATIME only on EPERM. A shared opening lock limits temporary ancestor descriptors; directories close before output sends. Names and paths remain bytes; paths are reused in the entry loop and retained names are allocated only when needed. Entry arrays are sorted, and discovery IDs/output arrival order remain nondeterministic as specified for later aggregator renumbering. No unsafe code, regular-file content reads or metadata writes were introduced.

`read_dir_entries` now returns sorted byte-preserving names/inodes/kinds. The benchmark creates 100,000 files and reports entries/s. WalkError's owner-defined shape now includes Cancelled, Io, Limit and Disconnected, retaining the old unavailable variant for compatibility. Public signatures and field lists are unchanged.

## Shared-file edits

None. No manifests, lockfile, crate module declarations, frozen contracts, test-support sources or plan files changed. All dependencies were already attached.

## Deviations from the spec

| Spec reference | Implementation choice | Reason |
|---|---|---|
| 04 §2.2 long-path fallback | Always open components safely; ancestor handles use O_PATH | Prevent ancestor symlink traversal as well as supporting paths beyond PATH_MAX without recursion. Initial root metadata and error recovery use O_PATH. |
| 04 §2.2 one fd per worker | Serialize component opening; at most two temporary ancestor descriptors beyond currently active worker directories | Safe component traversal needs a replacement descriptor briefly; the absolute threads + 8 bound is tested. Opening serialization can reduce deep-tree throughput. |
| AGENTS rule 10 | Explicit caps: 65,536 in-flight directories, 64 MiB in-flight path bytes, 1 MiB per path, one million direct entries per directory/live reread, top_files <= 200,000, 4 KiB–4 MiB getdents buffers, 1–64 workers, output capacity 1–1,024 | WalkConfig has no queue/memory-cap fields. Caps fail explicitly rather than dropping work or growing indefinitely; very wide directory frontiers can therefore fail. |
| 04 §2.2 metadata source | statx with EMPTY_PATH on directory handles rather than a separate fstat, also requesting mount ID | Gets directory metadata and mount identity in one gated syscall. |
| 04 §4 unknown mount discovery | Fail explicitly if both mountinfo and statx mount IDs are unavailable | Device IDs alone cannot detect same-device bind mounts. With a mount ID, the mountinfo error is logged and traversal continues safely; unmatched mount IDs emit Gone(OtherFs). |
| Output backpressure | send_timeout with cancellation/failure checks | Avoid hanging forever when a connected consumer stops reading during cancellation. |
| Unprivileged special fixture | When sandbox denies Unix socket creation, disclose the omission and still test the FIFO | Same permission handling as test-support; socket runtime coverage remains pending an unrestricted host/VM. |
| Build environment | Pinned Rust 1.98.0, offline host registry copy in /tmp/w1b-cargo | Cargo binaries/cache default locations are not writable in this sandbox. Host cache was not modified. |

No public-contract changes or spec-change requests were needed. Process-level nice priority belongs to the caller/service; this walker performs the specified best-effort per-worker ioprio calls only.

## Acceptance criteria evidence

Evidence directory: `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-ca51fc9a80557319dfe68d24d4b5199c578e0024805b06206c2125935b05f64c/library/`. Every named command has a log and an exit-code file.

| Criterion | Evidence | Local status |
|---|---|---|
| Golden walk matches exact records | `link_semantics_basic_exact_records`: all six directory records, 12 direct entries, own/age statistics, sorted LinkRefs and notable records compared with independent symlink_metadata measurements; no errors/races/skips | Passed |
| All walker tests pass | `targeted.log`: 15 integration tests passed. `stored_errors_are_capped_but_totals_are_not` also passed as a library unit test | Passed locally; privileged runtime pending VM |
| Peak open fds <= threads + 8 | `wide_dir_memory_and_fd_bound`, isolated process, four workers; measured peak 6, baseline 5, asserted absolute limit 12 | Passed; 1 ms sampling, plus structural descriptor bound |
| Wide directory memory | 120,000-entry directory; latest isolated sample RSS delta 1,757,184 bytes, asserted delta < 256 MiB | Passed locally |
| Format | `cargo fmt --check`, fmt.log, exit 0 | Passed |
| Clippy/no unsafe | `cargo clippy --offline --workspace --all-targets -- -D warnings`, clippy.log, exit 0; source search found no unsafe/unwrap/expect in walker source | Passed |
| Full unprivileged workspace | `cargo test --offline --workspace`, test.log, exit 101, unchanged CLI assertion above | Blocked outside scope |
| Remaining workspace | `cargo test --offline --workspace -- --skip binaries_and_bash_completions`, test-excluding-baseline-cli.log | Passed locally; explicitly excludes one baseline CLI test |
| Privileged test compilation | `cargo build --offline --workspace --features privileged --tests`, privileged.log, exit 0 | Compiled; pending VM |
| Rustdoc | `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps`, doc.log, exit 0 | Passed |
| CI wrapper | `tools/ci.sh` with offline environment, ci.log, exit 1 at the same unchanged CLI assertion | Blocked outside scope; cargo-deny stage not reached |

## Tests

Unprivileged integration tests in `tests/w1b/main.rs`:

- `link_semantics_basic_exact_records`
- `metadata_fixtures` — symlink_loops, nonutf8, hardlink_web, sparse and depth-200 fixtures; complete directory-set comparison and exact direct metadata/statistics
- `namespace_and_live_names` — names match independent directory reads byte-for-byte; live names are sorted and inode numbers match metadata
- `exclusions_and_mounts` — full-path exclusions for directories and files; artificial mount skip; skipped entries remain counted
- `cancellation_backpressure_and_disconnect` — cancellation while a consumer is blocked, prompt joins, disconnected receiver
- `namespace_avoids_file_stats` — counting Gate confirms exactly 100 extra per-file stat charges for 100 files in walk mode
- `symlink_ancestor_is_rejected`
- `random_tree_records` — four deterministic random fixtures, independent complete directory and metadata comparisons
- `wide_dir_memory_and_fd_bound` — 120k entries, isolated sampling process, RSS and absolute descriptor limits
- `paths_beyond_path_max` — 200 components of 24 bytes, more than 4,800 path bytes; descriptor-relative fixture construction
- `deleted_queued_child_is_a_race` — Gate coordinates a background deletion after directory discovery; Gone(Vanished), one race, zero errors
- `mixed_special_records_when_supported` — full fixture when permitted; FIFO fallback and explicit Unix socket omission here
- `notable_threshold_and_worker_top_files`
- `settings_and_channel_limits_are_explicit`
- `worker_panic_joins_instead_of_hanging` — deliberately panicking Gate; panic is caught as a walk failure and workers join

Unit test in `walk/worker.rs`: `stored_errors_are_capped_but_totals_are_not` — 10,017 errors counted, 10,000 details stored.

Privileged tests in `tests/w1b/privileged.rs`, feature-gated only (no ignore):

- `noatime_no_writes` — strictatime loop XFS, directory atime seconds/nanoseconds unchanged, zero fallback count — **pending VM**
- `real_bind_mount_is_skipped` — same-device bind mount is excluded via mountinfo and recorded Skip(Mount) — **pending VM**
- `ftype_zero_walk_and_namespace` — legacy crc=0/ftype=0 loop XFS, DT_UNKNOWN classification and both output modes — **pending VM**

Only ordinary tests were executed here. No mount, loop, mkfs, LVM, service or privileged runtime commands were run.

## Measurements

Latest `cargo bench --offline -p tidemark-fs --bench walk --profile dev`: 100,100 entries (100,000 files plus 100 child directories) in 105.217570 ms, **951,360 entries/s**. Warm cache, development build, eight workers, NoopGate, empty files, Linux x86-64 sandbox host reporting 12 CPUs. This is not a cold-cache, HDD RAID, optimized-build or Rocky/XFS performance claim. RSS sampling assumes the host's 4 KiB pages and samples every millisecond; allocator/fixture effects influence deltas.

## Known gaps and follow-ups

- Full workspace tests and CI are not green until the coordinator fixes the unchanged CLI probe-exit assertion. Advisory/license auditing was not reached in CI.
- All three privileged runtime tests need a disposable VM. No Rocky 8.8/8.10, real bind mount, strictatime or ftype=0 runtime acceptance is claimed.
- Unix socket fixture creation is denied in this sandbox; that portion of mixed_special remains pending an unrestricted host/VM.
- Limits are fixed constants rather than configurable fields. A large/deep frontier or exceptionally large direct directory fails explicitly. The walker has bounded allocations but does not enforce a caller's overall memory budget; queued output records can also be large.
- Recoverable directory-open and per-entry syscall errors are represented in records when metadata can be recovered; unrecoverable root/directory identity/stat failures abort rather than fabricating metadata. Real EIO faults and permission-recovery races were not injected end-to-end.
- O_NOATIME EPERM fallback exists but was not deterministically forced in a dedicated ownership/capability test here.
- Mount ID fallback is used on this host because W1-A's mount parser rejects a non-path nsfs root. If both mount discovery mechanisms fail on an older kernel, scanning fails explicitly. VM coverage must confirm mountinfo support on EL8.

## Notes for the reviewer

Directory IDs can have holes and parents can arrive after children, as the contract permits. The aggregator must renumber by byte-name preorder and retain global top-N from emitted notable candidates; the walker emits candidates admitted at any point to each worker's heap, including candidates later displaced. Error detail caps apply globally while summary totals are uncapped. In-flight RAII guards release counters and notify sleepers on panic; output retry loops stop on cancellation/fatal failure. All retained descriptors are OwnedFd and drop before output backpressure. Fixtures and benchmark write only their own temporary directories; production walker code is read-only.

## Definition of Done

- [x] Only assigned paths changed; no shared edits or new dependencies
- [x] Public signatures/field lists preserved; owner headers and rustdoc present
- [x] Ordinary walker tests, format, Clippy, privileged compilation and rustdoc passed locally
- [x] Privileged tests written and listed as pending VM
- [x] Acceptance evidence, limits, deviations and omissions recorded
- [ ] Full workspace/CI green — coordinator fix required above
- [ ] Privileged runtime acceptance and independent verification

This handoff is an evidence candidate, not a declaration of verified task success or integration.
