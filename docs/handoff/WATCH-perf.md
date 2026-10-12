# Handoff — WATCH-perf: Cut watcher cost per event

## Needs from coordinator

Confirm the five daemon readiness tests in an environment allowing Unix sockets. Run the privileged scale measurement in the Rocky 8.8 and 8.10 guests; no fanotify or privileged tests were run on this workstation. Budget compliance is unmeasured. Review the proposed validation wording below. Workspace test/CI outcomes are recorded below, including sandbox limitations.

Branch: `hp-97ec75e9974206980a546a1f420eef389459641a14510f3805d1985865bbadea`. Date: 2026-10-12. Status: implemented candidate, pending review and VM measurement.

## What changed

The attribution cache now uses a bounded PID hash map and doubly linked LRU, with no full-cache scan or accumulating eviction queue. Starttime remains part of the validated identity. `lookup` retains exact validation for existing callers; `lookup_watcher` reuses a validation younger than one second. TTL remains measured from attribution load, independent of validation and LRU touches. Failed starttime reads invalidate the PID; failed identity loads are not stored. The mutex serializes validation/load and LRU updates to prevent concurrent lookups publishing an older process identity over a newer one.

`read_starttime` uses a 512-byte stack buffer and one `Read::read`; full buffers and incomplete records fail closed. The watcher reads its own PID once when constructing its processor. Its fd event path (`resolve_event`) requests BASIC_STATS and BTIME together with one statx, converts device major/minor with rustix's Linux makedev, and retains the same inode, signed size/blocks, mode and ctime. Missing birth time falls back to ctime. A failed/insufficient statx falls back to fstat and ctime, preserving the previous successful metadata behavior when birth-time statx failed.

The `/proc/self/fd/N` name is formatted through a cursor into a 32-byte stack buffer. Existing ring rows and unit-counter rows are looked up before cloning their keys. Ring caps, unique-file accounting, eight-unit candidate semantics and LIVE-hot leader behavior remain intact. The scale child uses append/create, writes four bytes and closes, still paced at 500 microseconds per close over 2,000 files. The parent reports ten-second CPU, observed events and microseconds/event throughout the load, and the aggregate 60-second result. Every existing threshold remains unchanged.

## Process scanner and internal compatibility

ProcScanner compares attribution starttime with each `OpenFile.starttime` and promises reused-PID identities become unknown. Its `ProcSource` still calls the exact `lookup`; neither scanner nor CLI nor daemon source changed. A separate watcher entry point preserves this guarantee even when the cache is shared. All existing public signatures remain unchanged; one documented method was added.

FID resolution is outside ownership and returns the non-constructible external `rustix::Stat` type. Consequently `fdmode::resolve` remains its legacy internal two-call adapter; the fd-mode watcher now calls `resolve_event` returning a local metadata struct. This avoids changing fidmode.rs or unsafe construction of rustix::Stat. The normal fd-mode event path uses one statx; FID behavior is unchanged.

## Expected syscall counts (fd mode)

For a successfully resolved event with a watcher attribution hit validated less than one second ago:

| Syscall | Expected count | Why it remains |
| --- | --- | --- |
| `statx` | 1 per event | Current descriptor inode/device/size/blocks and creation time are needed; no regular-file data read |
| `readlink` | Normally 1 per event | fd-mode fanotify supplies a descriptor, not its pathname; procfs supplies the current path |
| `close` | 1 per event | The kernel-created fanotify event descriptor must be released, including filtered/error events |
| `read` (fanotify) | 1 per transport batch | Retrieves events and creates their descriptors; batched over the 64 KiB buffer |
| `ppoll` | Approximately 1 per active batch | Waits on fanotify and shutdown; idle timeout polls remain |
| `getpid`, `fstat`, procfs `openat`/`read`/`close` | 0 on the normal fresh hit | PID captured at startup, combined statx, cached validation |

Thus about five calls/event for one-event batches, or three plus amortized poll/read for multi-event batches, compared with the supplied approximately fourteen-call baseline. This is a source-derived prediction, not a new strace measurement. Each older cache hit adds one procfs `openat`, one `read`, one `close`, at most once per second per continuously cached PID. A miss or TTL refresh additionally reads comm/cgroup/status and rechecks starttime after loading them; failed lookups continue doing I/O rather than being cached. A statx failure adds fallback fstat. Removed procfs descriptors also remove their close/deferred-fput costs. Long link targets can require std::fs::read_link buffer-growth retries. Mutex contention can add futex calls, and allocation/runtime calls are not a fixed per-event count. Path, metadata, descriptor closure and transport cannot be eliminated under the current fd-mode contract; poll/read batching can amortize transport but was not restructured here.

## Proposed plan text (not applied)

“Watcher attribution may reuse cached PID/starttime validation for less than one second, independently of the attribution TTL. On an older hit, read starttime once; invalidate the cached identity if the read fails or the value differs, then load new attribution with a final identity recheck. A recycled PID may retain its predecessor's attribution for at most one second. Failed attribution lookups are not cached. Process scanning and existing exact-validation callers continue to validate starttime on every lookup.”

## Acceptance evidence

- `attribution::tests::validation_reads_reuse_ttl_and_failures`: counting starttime seam proves no read at 999 ms, revalidation at 1 s, exact-call validation within that window, PID replacement, independent TTL load counts, invalidation and uncached failures.
- `attribution::tests::ten_thousand_entries_constant_work_and_lru`: 10,000 entries/hits, no loaders on hits, at most six keyed map operations per hit (counting BuildHasher), capacity, oldest eviction after touching PID 1, and full linked-order invariants. Source has no iteration or retain on cache lookup; map operations are expected constant time.
- `attribution::tests::zero_capacity_and_racing_exit`: disabled storage and missing PID.
- `procfs::tests::starttime_single_read_and_oversize_failure`: one read, parse identity, reject full 512-byte buffer/incomplete record.
- `watch::fdmode::tests::single_statx_matches_fstat_and_ctime_fallback`: one injected metadata call, real temporary regular-file metadata parity for device/inode/size/blocks/mode/ctime, birth parity with old call, missing BTIME and failed-statx fallback.
- Existing watcher synthetic processing/cleanup and LIVE-hot/ring tests retained. Privileged load and stability test compiled only, pending VM.

## Guest measurement command

On each disposable Rocky guest, from the coordinator's checkout:

```sh
cargo test --release --offline -p tidemark-live --features privileged --test w5a privileged::watcher_sixty_second_load_and_ten_minute_fd_stability -- --exact --ignored --nocapture --test-threads=1
```

Run under the VM's root test account. The test lasts ten minutes; report its six ten-second intervals, minute CPU/events/cost, child achieved rate, final fd count and assertion results. A guest with a configured dependency source may omit `--offline`. Keep the 2% scale assertion and the plan's 1% resource budget.

## Assumptions and deviations

- The task's coordinator decisions authorize watcher-only one-second validation; exact consumers retain their previous contract.
- A procfs stat line must end in newline and fit below 512 bytes. Full buffers are rejected even if they happen to end with newline; this conservatively rejects the boundary rather than silently accepting possible truncation.
- Existing inode-history/ring unique-file and event-stream allocations are preserved. No dependency, manifest, shared module or contract change; no new unsafe code.
- Hash-map lookup has expected constant cost; keyed operation counts prove no cache-size-dependent LRU operations, not a worst-case hash-collision bound.
- Guest kernel timing and actual budget attainment require coordinator measurements; local test seams cannot establish CPU cost.

## Validation

| Command | Outcome |
| --- | --- |
| `cargo fmt --check` | 0; repeated after final source refinements |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0; also rerun by CI against final source |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0; repeated against final source |
| `cargo test --offline --workspace` | 101: 502 passed, five daemon readiness failures, 13 ignored |
| `cargo build --offline --workspace --features privileged --tests` | 0; compile only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 |
| `tools/ci.sh` | 1: same five daemon readiness failures; 502 passed and 13 ignored; source checks (140 owner tags), format and lint passed; stopped before deny |

Before/after counts: the retained baseline `docs/handoff/LIVE-hot.md` records 499 passed, five daemon readiness failures and 13 ignored; this attempt's workspace run records 502 passed, the same five failures and 13 ignored. This is a comparison with retained baseline evidence, not a new full baseline run. Source test declarations in the three changed test-bearing modules increased from five to eight (attribution 2→3, procfs 1→2, fdmode 2→3). All eight pass in the workspace run. The failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog` in tidemarkd's w6a1 suite, each with `deadline exceeded`, matching the sandbox limitation stated in the task.

Logs are in the attempt output's `library/`: checks.txt, fmt.log, clippy.log, clippy-privileged.log, final-clippy-privileged.log, tests.log, build-privileged.log, docs.log and ci.log. No cargo-deny pass is claimed if CI stops at tests. Commands use the pinned 1.98.0 toolchain, host cached dependencies, `CARGO_NET_OFFLINE=true`/`--offline`, and `timeout 1500` for each long command. Privileged binaries are only compiled. No privileged/destructive host commands were run.

## Definition of Done evidence

- [x] Owned source paths only; no dependencies or shared-file edits.
- [x] Format, workspace Clippy and privileged Clippy clean.
- [ ] Workspace tests/CI fully green: five known sandbox readiness failures remain.
- [x] Privileged tests compiled; runtime pending VM.
- [x] Public addition documented, owner headers present, no unsafe or new production unwrap/expect.
- [x] Functional and operation-count evidence recorded; handoff and report written.
- [ ] Guest performance and privileged acceptance established by coordinator.

Handoff and report are evidence candidates, not independent verification of task success.
