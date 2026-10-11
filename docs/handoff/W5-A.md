# Handoff — W5-A: fanotify FFI and watcher

## Needs from coordinator

- Decide [SCR-001](scr-W5-A/SCR-001.md): the frozen watcher API has no per-event stream for foreground path/PID/comm/delta lines. The CLI now runs watchers, emits top-five completed-minute directory summaries, handles SIGINT/SIGTERM and maps startup preconditions to exit 5. **Per-event lines and the 50-lines/s omission summary are not implemented.** No public signature or manifest was changed to bypass this gap.
- Decide [SCR-002](scr-W5-A/SCR-002.md): exact rename pairs need a correlation-capable event/output contract; current MOVED_FROM/MOVED_TO counts remain independent.
- Run the privileged and scale fixtures on both Rocky VMs and record capabilities, allocation behavior, CPU use and fd stability. This attempt has no VM.
- Resolve existing CI timing/readiness failures outside W5-A: the parallel retry failed `tidemark-fs::throttle::tests::exact_reservations_preserve_outstanding_targets` at throttle.rs:574; a serialized retry passed that test but failed store `immutable_reader_opens_under_another_process_exclusive_lock` at storage.rs:983. The separate full workspace test run passed. Logs retain both failures; those paths were not edited.
- The optional workspace privileged Clippy check encounters an existing unwrap lint at `crates/tidemark-fs/tests/w1a/main.rs:117`; that path is outside this task. Required normal workspace Clippy and compilation of privileged tests are checked separately.

- **Branch:** hp-94e43d33441b0be22bbfd99c5d1027b40b38b1efd10c7ba4d92e9a2791c08d46
- **Agent/session:** attempt-94e43d33441b0be22bbfd99c5d1027b40b38b1efd10c7ba4d92e9a2791c08d46
- **Date:** 2026-10-11
- **Status:** core implemented; foreground event stream blocked on SCR-001; runtime acceptance pending VM. This is an evidence candidate, not independently verified task success.

## What was built

`tidemark-sys::fanotify` now owns real nonblocking notification groups, probes init and mount/filesystem mark combinations, resolves bounded file handles with O_PATH, and parses kernel metadata and FID/name records with length/version checks. Hand-transcribed UAPI layouts are pinned by size, alignment and offset tests. Event descriptors become OwnedFd before validation, and unread valid records are consumed when their iterator drops.

The watcher enforces the host namespace guard, negotiates DFID/name reporting with fd/mount fallback, polls a group and shutdown eventfd, and processes metadata without reading file data. It uses injected timestamps in the processor, inode-size LRU and ring logic; real clocks are isolated at worker/API boundaries. It prefers statx birth time with ctime fallback, seeds old files without a restart spike, ignores its own PID, uses AttributionCache, filters byte prefixes, bounds dirty directories, and records estimated deletions and move direction deltas from bounded path history. A bounded 100,000-entry handle/path LRU is cleared on move events to avoid stale descendant paths.

The 60-minute ring has three maps per bucket, 20,000 current keys per scope, smallest-half eviction, and 1,000 completed keys per scope. A bounded current-minute identity set counts distinct files per key; it is released on completion. Completed-minute draining returns retained rows once, and aggregate output is sorted by signed bytes then key bytes. CLI foreground summaries use configuration and terminate cleanly on signals or output errors.

## Shared-file edits

None. No dependency, Cargo.toml, Cargo.lock, crate-root module, core contract, plan or other task source edits.

## Deviations from the spec

| Spec reference | Implementation / limitation | Reason | SCR |
|---|---|---|---|
| W5-A foreground per-event lines | Completed-minute top-five summaries only | Frozen Watcher API omits an event stream; CLI has no sys dependency | SCR-001 pending |
| 07 §3.3 approximation flag | Eviction and identity-cap approximation conservatively set existing `lossy` fields | HotEntry/HotMinute have no separate `approx` field | No contract change |
| 07 §3.3 unique files | Current bucket tracks at most 200,000 scope/key/inode identities; further identities make it lossy | Additional documented bound; completed buckets release identity tracking | None |
| 07 §3.4 renames | MOVED_FROM and MOVED_TO are counted individually with directory deltas, without explicit paired records | No correlation cookie or paired-rename output contract exists | SCR-002 pending |
| HotEntry.top_unit | None (unknown), rather than incorrectly labeling the first observed unit as dominant | Exact dominant-unit accounting is not implemented | None; known gap |
| Load fixture | Ten-minute scale fixture drives batches of 2,000 closes with a one-second pause | Fixture is not a calibrated 2,000-events/s CPU benchmark; measurement still required | None |
| Build environment | Offline Cargo with registry copied to `/tmp/w5a-cargo` | Host cache is readable; profile cache lacked base64; crates.io unavailable | None |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| fd mode works on both VMs; FID implemented/tested if supported, fallback otherwise | Real init/marks, capability negotiation, FID/name decoding and handle resolution; compiled `fanotify_fd_mode_sees_close_write`, `fanotify_probe_reports_capabilities`, `fid_mode_create_delete_rename` | Implementation and compile evidence only; both VM runs pending |
| No fd leaks over ten-minute load | `descriptors_close_on_error_and_abandoned_batch` checks exact descriptors and fd count in an isolated subprocess; `synthetic_batch_fd_count_stable` processes 2,000 synthetic events with unchanged fd count; ten-minute fixture compiled | Synthetic checks passed; ten-minute VM acceptance pending |
| No regular-file content reads | Source audit: event fds only fstat/statx/readlink; handle fds O_PATH; FID names statat/statx with SYMLINK_NOFOLLOW; only fanotify group fd read | Local source evidence |
| Namespace refusal | `namespace_ok` compares proc namespace readlink identities before probing/marking; privileged unshare test compiles | Runtime pending VM |
| Bounded rings / overflow | `window_rollover_lossy_and_drain_once`, `key_caps_shrink_and_deterministic_ties`, `sixty_buckets_and_clock_rollback`, synthetic processor overflow assertion | Unprivileged passed |

## Tests

Unprivileged tests added in owned modules:

- sys: `uapi_layout`; `multiple_overflows_and_truncated_tail`; `fid_with_and_without_name_and_invalid_lengths`; `descriptors_close_on_error_and_abandoned_batch`.
- live fdmode: `new_files_old_files_eviction_and_signed_delta`; `resolve_only_metadata_and_raw_path`.
- live fidmode: `remembered_delete_estimates_and_capacity`; `handle_path_lru_and_invalidation`.
- live rings: `window_rollover_lossy_and_drain_once`; `key_caps_shrink_and_deterministic_ties`; `sixty_buckets_and_clock_rollback`.
- live processor: `component_prefixes_and_hot_depth`; `synthetic_event_processing_exclusions_attribution_and_cleanup`; `synthetic_batch_fd_count_stable`.
- CLI: `signed_summary`.

Privileged tests, all feature-gated and **pending VM**:

- sys `fanotify_probe_reports_capabilities` prints capability findings.
- live `fanotify_fd_mode_sees_close_write`: child PID, path, live comm lookup, measured allocated-byte delta and one-second arrival deadline.
- `exclude_prefixes_respected`: component prefix separation and shutdown deadline.
- `watcher_namespace_guard`: unshare child refusal.
- `watch_overflow_marks_lossy`: distinct-file 100k workload induces an unread kernel group overflow. Ring loss propagation/thread processing is separately synthetic-tested; this fixture does not force the actual running watcher to overflow.
- `fid_mode_create_delete_rename`: conditional capability test of masks, directory handle resolution and names.
- `writer_child`: helper test for child-owned writes; no-op without its fixture environment variable.
- `watcher_sixty_second_load_and_ten_minute_fd_stability`: ten-minute fixture, `#[ignore = "scale"]`; pending VM. No privilege tests are ignored for privilege.

No W5-A tests are ignored pending another task. The workspace retains existing W2-B/W2-C tests ignored with `needs W2-A`; the workspace total also includes existing scale ignores. No privileged tests were executed on the host.

## Measurements

No Rocky 8.8/8.10 capability findings, calibrated 2,000-close/s CPU measurement, peak memory measurement, or ten-minute runtime fd result is available. The synthetic fd tests compare counts before/after, rather than assuming a host-specific count. Allocation assertions measure metadata blocks rather than assuming filesystem-dependent literals.

## Known gaps and follow-ups

- Foreground per-event output and line limiter require the coordinator's event-stream contract decision.
- Rename pairs and exact dominant-unit tracking remain unimplemented; directory `top_unit` is None, while the separate unit scope is attributed. Deletion deltas are estimates only when a path was previously seen; unknown deletions count events with zero bytes.
- Concurrent rename/delete races may prevent handle/name resolution; these minutes are conservatively lossy.
- Metadata version/event/info lengths are validated. If the event boundary itself is corrupt, the remaining buffer is discarded because there is no trustworthy boundary from which to decode more fds. Kernel-produced valid records, malformed records with valid boundaries, abandoned iterators and processor return paths are covered; arbitrary corruption of record boundaries is not a recoverable protocol.
- Shutdown checks a flag between events and wakes poll with eventfd, but the one-second requirement still needs VM evidence when metadata/procfs operations are stalled.
- Probe findings do not prove that every actual rich event mask works; start retries actual marks and then falls back to fd mode.
- Completed minutes older than the retained 60-minute ring are discarded if no consumer drained them; buffering is intentionally bounded.

## Notes for the reviewer

Unsafe code exists only in the owned sys module, each block has a SAFETY comment. Handwritten bindings use native-endian byte parsing, so buffers need no alignment. The decoder prefers a named directory record over a file FID, and a file FID over a nameless directory FID. The watcher never enables unlimited queues or permission/content hooks. All newly added library public items have docs and all source headers identify W5-A.

## Validation results

Environment: Rust 1.98.0, offline Cargo, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/tmp/w5a-cargo`, `CARGO_NET_OFFLINE=true`. Logs and exit-code files live in the canonical attempt output `library/` directory. The final results below are populated after the settled-source check run.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits.
- [ ] `cargo xtask ci` green: final retries failed outside task scope.
- [x] Public documentation and W5-A ownership headers present.
- [x] Privileged fixtures written and compilation checked; runtime pending VM.
- [x] No production unwrap/expect or unsafe outside sys.
- [ ] All runtime acceptance criteria evidenced: pending VM.
- [ ] Full foreground event output: blocked on SCR-001.


| Command | Result | Evidence in canonical output library |
|---|---|---|
| `cargo fmt --check` | exit 0 | fmt.log / fmt.exit |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | clippy.log / clippy.exit |
| `cargo test --offline --workspace` | exit 0; 261 passed, 0 failed, 9 existing ignored | test.log / test.exit |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compile only | privileged.log / privileged.exit |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | doc.log / doc.exit |
| `tools/ci.sh` normal retry | exit 1; unrelated throttle assertion | ci-parallel.log / ci-parallel.exit |
| `RUST_TEST_THREADS=1 tools/ci.sh` | exit 1; unrelated store child readiness assertion | ci.log / ci.exit |
| Targeted throttle failure retry | exit 0; 1 passed | throttle-retry.log / throttle-retry.exit |
| Privileged Clippy for sys/live/CLI | exit 0 | owned-privileged-clippy.log / .exit |
| `git diff --check` | exit 0 | diff.log / diff.exit |

An earlier tools/ci.sh run passed before the final `top_unit=None` correction; its cargo-deny step was skipped because cargo-deny is unavailable. Final CI is **not green**, despite the separate settled-source workspace tests passing. The final owned-crate fallback test result is appended below after it completes. No privileged runtime or release acceptance claim is made.

Final fallback: `cargo test --offline -p tidemark-sys -p tidemark-live -p tidemark-cli` exited 0; 75 passed, 0 failed, 2 ignored (library/owned-tests.log and owned-tests.exit).
