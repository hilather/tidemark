# Handoff — W1-D: Throttle, ioprio, procfs parsers, attribution cache

- **Branch:** hp-8d24e5ecd8957c907277627a4780cfb0bf672695913891ccabcc4ab884fba1d6
- **Agent/session:** Codex, attempt-8d24e5ecd8957c907277627a4780cfb0bf672695913891ccabcc4ab884fba1d6
- **Date:** 2026-10-11
- **Status:** implementation submitted as an evidence candidate; independent verification and VM runtime validation pending.

## Needs from coordinator

- Decide [SCR-001](scr-W1-D/SCR-001.md): stable positional leaf samples, utilization masking, bounded callback duration, and caller-owned discovery. The supplied gate constructor has no root DevId; W2-B must call the final `blockdev::resolve_leaves(DevId)` signature, convert unavailable discovery to an empty slice, and inject a sampler using `blockdev::read_disk_stats`. This task does not initiate device discovery or invent a root device.
- Provide Rocky VM access for the two privileged runtime tests and real cgroup-v1 captures. Neither root acceptance nor real EL8 fixture provenance is claimed here.
- Run cargo-deny on an equipped host; local CI explicitly skipped it.

## What was built

Implemented per-thread Linux idle/best-effort ioprio, a fixed token bucket, an adaptive AIMD controller, all reserved procfs readers/parsers and open-file scanning, and a bounded attribution LRU keyed by PID/starttime with TTL refresh. Buckets reserve nanosecond deadlines with atomic CAS and wait without spinning. The controller samples without holding gate locks, retains its rate on sampler errors, masks inappropriate utilization signals in the configured gate, records time-weighted rate statistics, and joins on stop or last drop. Procfs readers retain raw fd target bytes, inspect metadata only, skip disappearing/inaccessible processes, and recheck PID and descriptor identity.

## Shared-file edits

None. No dependencies, manifests, lockfiles, crate roots, frozen core types, or plan documents changed. Existing public signatures and OpenFile/AdaptiveParams field lists were retained; FixedGate's constructor and private gate/cache storage fill the previously unspecified owning-task shapes.

## Deviations from the spec

| Spec reference | Implementation / interpretation | Why | SCR? |
|----------------|----------------------------------|-----|------|
| 04 §6.2; W1-D leaf discovery note | Constructor consumes supplied leaves; empty leaves use a fixed gate at configured min_ops_per_sec | There is no root DevId in the constructor. W1-A remains a stub; no leaves is treated as unknown rather than idle real data. The referenced §6 specifies no numerical fallback rate, so the configured minimum is the conservative choice | SCR-001 |
| 04 §6.2; DiskStat and BlockDevInfo | Pure controller accepts aligned counter samples; factory masks io_ticks_ms for nonrotational/unknown or multiqueue leaves, inspecting at most two mq entries | Frozen counter type has neither device identity nor queue policy. Missing mq on a known rotational leaf is interpreted as legacy single queue | SCR-001 |
| W1-D stop acceptance | Wait is interruptible; stop joins synchronous callback and thread. Sampler must complete within the configured interval | Safe Rust cannot forcibly cancel an arbitrary blocking FnMut callback. Arbitrarily blocked sampler shutdown is not implemented or claimed | SCR-001 |
| W1-D bucket details | std::thread::park_timeout waits, with a deadline loop to tolerate unrelated unparks; parking_lot mutex/condvar control lifecycle | Per-worker parked waits avoid a shared bucket wait lock and thundering-herd wakeups; still sleeping, never spinning | No contract change |
| W1-D rate test | Spend the initial quarter-second burst before the two-second steady-state measurement | A full initial 250-token burst would otherwise add 12.5% to a 2s measurement. The documented burst is retained | No |
| W1-D parser fixtures | Actual worker Linux stat/fdinfo/trimmed status captures plus representative v1/v2 cgroup fixtures and constructed pathological comm | No EL8 VM was available; fixture provenance is disclosed in tests/w1d/data/README.md | No; real Rocky captures pending VM |
| Read-only/no-symlink scanner rules | Stat only the kernel's /proc/PID/fd/FD magic links, intentionally, to inspect an existing descriptor; never open or follow the readlink target text | The live procfs primitive must inspect unlinked open files. This is not traversal of a filesystem tree or an ordinary filesystem symlink | Task's explicit procfs requirement |
| Environment | Pinned Rust 1.98.0, private /tmp cargo registry copy, offline builds | crates.io unavailable; host registry remains untouched | No |

Documented caps: proc text 1 MiB, numeric IDs per listing 1,048,576, returned open-file records 1,048,576, retained leaf samples 4,096, and cache entries bounded by constructor capacity. Exceeding a proc scan cap errors rather than silently truncating. Sampler vectors are callback-owned; oversized results are rejected before retaining them. Zero cache capacity disables storage. Process comm uses lossy UTF-8 because its frozen contract is String; fd target paths remain raw bytes.

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|-----------|----------|--------|
| All tests pass; no unsafe outside tidemark-sys::ioprio | Offline workspace test and CI logs exit 0; exactly one new unsafe block, in ioprio::set, with SAFETY comment and module-local allowance; fs retains forbid(unsafe_code) | Local evidence collected; independent review pending |
| AdaptiveGate::stop joins within one interval | controller_stop_drop_and_sampler_failure interrupts a 10s interval in <100ms; adaptive_rates_stats_and_stop_during_sampling joins a released in-flight callback in <200ms | Passed locally for bounded callbacks; arbitrary blocking callback gap in SCR-001 |
| Four threads at 1,000/s for 2s within ±5% | four_thread_fixed_rate asserts 1,900–2,100 acquisitions after consuming initial burst | Passed locally |
| Parser, controller, PID reuse and deleted open file cases | Tests listed below | Passed locally; actual Rocky cgroup captures pending VM |
| Root idle priority, get result, other users' fds | Privileged tests compile in workspace build; idle test uses ionice's ioprio_get against worker TID | Pending VM |

Final required checks used CARGO_HOME=/tmp/tidemark-w1d-cargo and CARGO_NET_OFFLINE=true. Logs and exit files are in this attempt's worker-output/library directory.

| Command | Local result | Evidence |
|---------|--------------|----------|
| cargo fmt --check | Exit 0 | fmt.log, fmt.exit |
| cargo clippy --offline --workspace --all-targets -- -D warnings | Exit 0 | clippy.log, clippy.exit |
| cargo test --offline --workspace | Exit 0 | test.log, test.exit |
| cargo build --offline --workspace --features privileged --tests | Exit 0 | privileged.log, privileged.exit |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | Exit 0 | doc.log, doc.exit |
| tools/ci.sh | Exit 0; cargo-deny skipped, unavailable | ci.log, ci.exit |
| cargo clippy --offline -p tidemark-fs -p tidemark-sys --all-targets --features privileged -- -D warnings | Exit 0 | privileged-clippy.log |

## Tests

New unprivileged tests:

- sys::ioprio::tests::linux_priority_encoding: Linux bit encoding and rejected best-effort level.
- fs::attribution::tests::pid_reuse_ttl_and_lru: cache hits, reused PID, exact TTL expiry, LRU eviction, missing PID invalidation with injected starttime/clock.
- fs::attribution::tests::zero_capacity_and_racing_exit: zero capacity and failed identity recheck.
- fs::procfs::tests::uid_parser_and_error_policy: recorded Uid and malformed/overflow inputs, transient versus fatal errno handling.
- w1d::parser_samples: captured stat/fdinfo, comm parentheses/spaces, v1/v2 units, user scopes, irrelevant hierarchies, malformed lines, append/write flags, live comm/starttime/UID readers.
- w1d::controller_table: additive increase/max clamp, utilization/latency decrease, min clamp, worst of multiple devices, threshold equality, counter reset, empty samples and invalid dt.
- w1d::four_thread_fixed_rate: four concurrent workers at steady-state 1,000/s for two seconds.
- w1d::controller_stop_drop_and_sampler_failure: sampler error preserves rate, interruptible stop, repeated stop and last-drop join.
- w1d::config_profiles_and_fallback: reserved max, custom profile, unknown name, minimum-rate unknown-device fallback without invoking sampler.
- w1d::finds_deleted_open_file_without_reading_contents: unlinked sparse temporary file held open, correct PID/fd/inode/size/nlink/raw target/flags; empty device set.
- w1d::exiting_processes_are_tolerated: concurrent short-lived subprocesses while scanning.
- w1d::adaptive_rates_stats_and_stop_during_sampling: in-flight callback join, decrease/min/mean statistics.
- w1d::nonrotational_util_is_ignored_by_config_gate: high synthetic utilization does not reduce a nonrotational gate.

Privileged tests, gated only by features (no ignore):

- sys::ioprio::tests::idle_class_as_root — pending VM; worker-only priority change and ionice/ioprio_get idle-class assertion. Requires util-linux ionice.
- w1d::root_sees_other_user_open_file — pending VM; a setpriv child at UID/GID 65534 opens a temporary file, publishes PID/fd, and root scan finds it. Requires setpriv and Python 3. No mounts or host configuration writes.

Initial local compilation found missing type annotation/Clone use; initial Clippy found test slice clones. Those were corrected. A test's exact floating-point mean-rate comparison also exposed rounding at ~1e-13 and was corrected to tolerance. Final checks passed after corrections.

## Measurements

Rate test bounds steady-state acquisitions to 1,900–2,100 in two seconds at 1,000 units/s across four threads. This is a functional timing test on the worker, not a Rocky pilot disk-performance measurement.

## Known gaps and follow-ups

- No privileged test execution, Rocky 8.8/8.10 runtime validation, or real EL8 cgroup captures: pending VM.
- SCR-001 is pending: stable sample ordering and bounded sampler lifetime are required assumptions, not coordinator-approved policy.
- Actual W1-A discovery/read_disk_stats integration remains caller work; this baseline still contains W1-A stubs.
- PID/fd metadata checks reduce races but cannot make procfs a coherent snapshot; same-inode descriptor reopen cannot always be detected. Kernel starttime is the specified PID-reuse key.
- Cache LRU operations are bounded but linear in configured capacity; no high-rate watcher benchmark was performed.
- Cargo-deny/advisory and license checks were not available locally.

## Notes for the reviewer

No regular-file contents are read by production code. File opens read only fixed procfs metadata files; fd targets are only readlink/stat-ed. No new dependency or shared-file edit was made. The controller holds an Arc to its internal state, never the AdaptiveGate itself, so last-drop can join without creating a self-retaining cycle. Joining remains serialized across concurrent stop calls. Atomic bucket reservations do not hold a shared lock while waiting; rate changes apply to new reservations.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits
- [x] Local format, Clippy, workspace tests, privileged-feature test build and documentation checks passed
- [x] tools/ci.sh passed its available checks; deny skip disclosed
- [x] Privileged tests written, compiled and listed as pending VM
- [x] Public items documented; owner headers present
- [x] No unexplained TODOs or production unwrap/expect calls
- [ ] Privileged runtime acceptance and real Rocky fixture provenance pending VM
- [ ] SCR-001 disposition and independent verification pending coordinator
