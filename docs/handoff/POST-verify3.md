# POST-verify3 — sampler grace diagnostics and deterministic polling

## Needs from coordinator

Independent review, the uid 1000 / umask 022 gate with sockets, and GitHub ubuntu-24.04 remain pending. Privileged deployment/reboot execution remains pending VM; only compilation is performed here. This handoff is an evidence candidate, not release approval.

## S1 — retain earlier sampler context

The loop retains one previous result and the monotonic time of the last routine sampler observation. If a later API call fails, the failing core item also names that earlier observation. A newer successful nonroutine status supersedes it. The live healthy predicate, exit codes, flags, JSON fields and schemas are unchanged.

New failure wording in the deterministic expiry test: the API item retains `status budget expired`; the core item retains the existing unavailable-health/report-start explanation and appends `; last status 900 ms ago: sampler=degraded (measurement pass in progress)`. This is explicitly historical context and never certifies current health.

The retry probe floor is **250 ms of total remaining API allowance**, before splitting connect/exchange. Below that floor, return the previous result instead of starting another socket request. The loop checks after polling and sleeping; the poll checks again after synchronous service/file work. The initial probe is retained even for a short explicit wait, since no previous result exists. Socket transport deadlines are unchanged.

`verify_sampler_then_timeout_keeps_history` failed before the diagnostic fix (exit 101, missing historical sampler assertion). Its final version injects 100 ms for the successful observation, sleeps 500 ms, asserts the next API receives exactly 400 ms, and consumes that allowance with a timeout. It passes with two calls, a failed verdict, both diagnostics and exactly one second on the fake clock. `verify_retry_probe_floor` checks 249 ms remaining produces one call and the original failed sampler result, while exactly 250 ms permits a second call and healthy recovery.

## S2 — deterministic sequence policy

A private clock/sleeper trait and timing struct replace real elapsed-time/sleep calls in the polling seam, including subtraction of synchronous probe time before API budget propagation. Production uses `Instant` and `thread::sleep`. Tests advance a `Cell<Duration>` clock without sleeping. No public API or dependencies were added.

The pre-fix negative control introduced the clock/sleeper seam and fake-clock tests while deliberately retaining the existing `Instant` elapsed-time checks. `verify_sampler_grace_sequences` failed (exit 101, `poll loop ignored fake deadline`), demonstrating that the real clock ignored simulated scheduling/time passage. Switching elapsed-time accounting to the injected clock passed. This was a negative control of the existing timing algorithm with a new seam, not an unmodified baseline rebuild.

The sequence matrix now asserts exact API counts (2, 3, 1, 1, 1, 2, 2), with no wall-clock bound. Recovery, persistent progress, unrelated sampler failure, another failed core, duplicate sampler, ordinary explicit-wait recovery and explicit wait shorter than grace remain covered. `verify_wait_rechecks_live_health` and the existing `verify_timeout_is_bounded` policy case also use fake time. Existing transport tests remain separate and unchanged.

## Assumptions and scope

- The specific POST task supersedes generic wave selection. Read the complete review and prior handoff, W4-B and its listed deployment contracts, playbook and handoff template.
- 250 ms is a useful minimum total allowance for a retry; it is not a promised transport latency. Existing configured per-phase limits still apply.
- Preserve sampler context for any subsequent API failure, including an immediate error as well as budget exhaustion. A nonzero explicit wait retains its existing retry policy within the floor.
- N1 remains unchanged: synchronous service/filesystem operations and scheduling prevent a hard whole-command bound. The operator guide does not promise a hard whole-command ten-second bound, so it was not edited.
- No new privileged scenario is needed for this CLI policy change. Existing privileged tests are built only, pending VM execution.
- No branches, merges, pushes, privileged commands, service actions or host configuration changes. No shared files, manifests or dependencies changed. Only owned deploy source/tests and this handoff changed.
- Validation uses host Cargo/Rustup caches, offline mode, uid 0 and umask 022. Socket-dependent coverage on other runners remains a coordinator gate.

## Validation

All long commands use `timeout 1500`; Cargo runs offline against the host cache. Logs, failing controls and focused results are retained in the attempt output `library/`.

- `cargo fmt --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- `cargo test --offline --workspace --no-fail-fast`: passed; 1,008 passed, 0 failed, 14 ignored across 87 summaries (including child harnesses). CLI library: 150 passed, 1 ignored.
- `cargo build --offline --workspace --features privileged --tests`: passed; execution pending VM.
- `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps`: passed.
- `tools/ci.sh` with `CARGO_NET_OFFLINE=true`: passed; 1,010 passed, 0 failed, 14 ignored across 88 summaries (including strict-ancestry/child harnesses). Source checks (150 owner tags), formatting, Clippy, strict-ancestry, workspace tests and privileged-feature build passed. `cargo-deny` is absent, so dependency-policy validation was skipped.
- Focused verifier run: 12 passed, 0 failed, 139 filtered; no fake-clock policy case sleeps or asserts a wall-clock deadline.
- S1 and S2 pre-fix controls: each expected exit 101, 0 passed / 1 failed; see the per-item explanation above.
- `git diff --check`: passed. All edits are confined to owned paths. Build cleanup and final clean-status checks are recorded in the attempt report.
