# Handoff — W1-D-fix: Review fixes

- Branch: hp-71f0eb70b323405278b5c1d450dd877cb906f09e2d634d92e70345d8fa3afe71
- Agent/session: Codex, attempt-71f0eb70b323405278b5c1d450dd877cb906f09e2d634d92e70345d8fa3afe71
- Date: 2026-10-11
- Status: implementation evidence candidate; independent verification pending.

## What changed

1. Waiting acquisitions now follow rate decreases and increases. The bucket holds cumulative issued and earned tokens; their difference is outstanding debt. Each acquisition reserves a cumulative target under a mutex and sleeps on a condition variable until earned tokens reach that target. Before changing rates, the controller accounts elapsed time at the previous rate, caps surplus to 0.25 seconds at the new rate, and notifies waiters to recalculate. Idle earnings are capped to the same burst allowance. State is constant-size, with no waiter queue; waits release the mutex. Stop retains its separate interruptible controller wait and bounded-sampler join behavior.
2. UID parsing selects the ASCII `Uid:` line from bounded bytes and decodes only its first field. The invalid-UTF-8 `Name:` regression returns UID 1000. The other strict whole-file readers were cgroup and fdinfo: their private text helper now uses lossy decoding, matching existing comm/stat handling, while preserving ASCII delimiters and numeric validation.
3. `FixedGate::new` is private. Its existing four-worker throughput test moved from integration tests into the throttle module. No public signature was added.

## Evidence

- `throttle::tests::concurrent_waiters_follow_rate_decrease`: 64 concurrent unit acquisitions, 100/s dropping to 1/s, asserts grants against the actual time-weighted rate plus the 25-token initial burst and a 2-unit scheduling/rounding tolerance. Recovery releases and joins all workers.
- `throttle::tests::bulk_waiter_follows_decrease_and_recovery`: a 1000-unit request, 1000/s dropping to 1/s and recovering to 1000/s, remains blocked during overload and completes after recovery. Grants are bounded by the actual rate integral plus the 250-token initial burst and the same 2-unit tolerance.
- `throttle::tests::time_weighted_debt_and_burst`: injected Instant values verify exact piecewise earnings (350.7 then 1050.7 tokens, tolerance 1e-9) and bounded idle accumulation.
- `procfs::tests::uid_parser_and_error_policy`: invalid UTF-8 outside `Uid:`, valid captured status, empty/negative/overflow UID fields.
- `throttle::tests::four_thread_fixed_rate`: preserved 1900–2100 grant bound over two seconds at 1000/s after consuming initial burst.
- Existing integration tests retain controller rate/statistics, stop/drop, and released in-flight sampler coverage.

## Shared-file edits and dependencies

None. Only throttle.rs, procfs.rs, tests/w1d/main.rs, and this handoff changed. No dependency, manifest, lockfile, crate-root, core contract, plan, attribution or ioprio change.

## Deviations and limitations

The task explicitly replaces the wave's immutable atomic-deadline implementation guidance with rate-aware token debt. A mutex and condition variable synchronize accounting and notifications without spinning or storing waiters. Accepted SCR-001 is treated as accepted per the task, despite older handoff/SCR text. Samplers must still finish within one interval; arbitrary blocked callbacks cannot be cancelled by safe Rust.

No privileged runtime tests or Rocky VM checks were run. Existing privileged tests remain pending VM; no new privileged behavior was introduced. Independent verification and integration are not claimed.

## Validation

All commands used Rust 1.98.0 on PATH, `CARGO_HOME=/tmp/tidemark-w1dfix-cargo` populated from the host registry, and `CARGO_NET_OFFLINE=true`. Cargo commands additionally used `--offline` where directly invoked.

| Command | Observed result |
| --- | --- |
| `cargo fmt --check` | Exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --offline --workspace` | Exit 0 |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0, compilation only |
| `tools/ci.sh` | Exit 0; 124 owner tags and forbidden-symbol scan passed; all available checks passed |
| `cargo test --offline -p tidemark-fs -p tidemark-sys` | Exit 0; fs unit 21/21, W1-D integration 8/8, sys unit 11/11; doc tests passed |
| `git diff --check` | Exit 0 |

CI explicitly skipped cargo-deny because it is not installed. Advisory/license auditing remains pending an equipped host. No unrelated workspace failure occurred. Logs and exit-code files are in this attempt's worker-output `library/` directory.
