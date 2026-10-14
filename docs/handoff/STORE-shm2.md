# Handoff — STORE-shm2: isolate scheduler catalog trust regression

## Needs from coordinator

Rerun the exact candidate through the uid-1000, umask-022 gate. This worker runs as uid 0 and did not change identity. The original failure's component detail was not captured, so its exact error remains inferred rather than reproduced here. Privileged execution remains pending VM.

## Cause

Established by inspection: the fixture inherited `Config::default()` roots `/data` (`every 6h`, window `00:00-06:00`) and `/` (`daily 03:30`). It injected wall time zero but used local-time scheduling. An empty catalog makes the every schedule immediately due when its window permits. The scheduler publishes `ok` at startup, during initialization, and after catalog admission. An unrelated scheduled scan can then fail and publish `degraded`; its retry record keeps `scheduler_health` degraded on later successful catalog opens. A 10 ms polling observer can miss the earlier healthy publication. Host timezone, scan permissions, mount presence and scan I/O therefore affect a test intended only to exercise catalog admission.

Inferred explanation for the reported gate failure: default host scan activity overwrote initial `ok` before the observer caught it. The exact scan error and gate timezone are unknown. The code publishes startup `ok` unconditionally; "never published" is not the normal path for this test, although an initial catalog refusal could also immediately replace it. The new timeout diagnostics expose actual component state/detail for any remaining failure.

Trust inspection found no reason to loosen product policy: storage directories/leaves require effective-uid ownership and no group/world writes; ordinary leaves require one link. Group/world readability warns rather than rejects. Ancestors accept root/euid; writable ancestors require sticky protection and safe owned children. Debug exceptions apply to namespace overflow owners/read-only foreign ancestors, not storage ownership or writable leaves. Directory `O_NOATIME` falls back on EPERM. The private fixture is mode 0700 owned by its creating uid. Mode 0620 deterministically violates the write rule for either uid; restoring 0600 trusts it again. No product fault outside the owned test was established.

## Change

Only the existing scheduler regression changed. Clear configured roots to keep the real scheduler idle while it still opens the catalog every pass. For healthy waits, require `ok` with `waiting for scheduled scan`, which is published only after successful admission; startup health cannot prematurely satisfy readiness or recovery. Degraded waits still require the writable-group trust error. Timeout and early-exit messages include the observed scheduler component. The test still publishes daemon readiness, induces a subsequent real catalog-open trust refusal, checks unset shutdown, restores trust, observes recovery, and joins normally.

## Assumptions

- No user questions, identity switching, branch creation, merge or push.
- Empty scan-root configuration is appropriate for this catalog-only regression; the production scheduling loop continues to open the catalog with no roots.
- Keep the five-second deadline and ten-millisecond polling interval; observe a stable admitted state rather than increasing timeouts.
- Use the host Rust 1.98.0/cache with `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, offline Cargo and timeout 1500 for long checks.

## Validation

All commands below exited 0 on the final source; logs are in the canonical attempt output's `library/` directory. Every long command used `timeout 1500` and offline Cargo.

| Check | Result / log |
| --- | --- |
| `cargo fmt --check` | passed; `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | passed; `clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | passed; `test.log` |
| `cargo build --offline --workspace --features privileged --tests` | passed, build only; `privileged.log` |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | passed; `ci.log` |
| Focused scheduler regression | passed; `focused.log` |
| Focused regression with umask 022, `TZ=UTC` | passed; `focused-utc.log` |
| Focused regression with umask 022, `TZ=America/Toronto` | passed; `focused-toronto.log` |

No privileged tests were run; pending VM. The anticipated socket restrictions did not cause workspace failures in this run. CI skipped cargo-deny because it is not installed; dependency policy audit remains pending an equipped host. No new privileged test is needed for this test-only correction.

## Scope and review

Owned paths only: `crates/tidemarkd/src/scheduler.rs` and this note. No shared-file edits, dependencies, frozen-contract changes, unsafe code, product trust changes or memory-projection edits. This is an evidence candidate for coordinator review, not independent gate acceptance.
