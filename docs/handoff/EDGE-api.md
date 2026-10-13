# Handoff — EDGE-api: connection deadlines, handler ownership and socket paths

## Needs from coordinator

F03 is **incomplete**: add an internal cancellation-aware `ScanControl::start` entry point in the expressly excluded `scheduler.rs`, pass the API request budget into its scan-start acknowledgement wait, and cancel the admitted scan when that budget expires. Its existing eight-second acknowledgement limit and lifecycle shutdown checks remain, but it cannot currently observe request cancellation. Recorder waits should continue observing the admitted scan's cancellation. This candidate does not claim every acknowledgement wait receives the request token.

F04 changes startup eligibility. Update the unowned `tests/w6a1.rs::fixture` to create its outer temporary directory with mode 0700 before invoking the daemon. `tempfile::tempdir()` defaults to 0755 with umask 022, which cannot satisfy the assigned sticky-ancestor rule. All five socket-readiness cases use this fixture. Also audit `tests/w6a2/runtime.rs` outer temp fixtures when their API listener runs on an unrestricted host. This task expressly excludes both files; they were read but not changed. Use explicit private fixture permissions rather than weakening production validation or automatically chmodding a configured parent.

Listening-socket and guest acceptance remains pending. See commands below. This handoff and submission are evidence candidates, not verified task success.

## Status and scope

Attempt `attempt-6d8b9cdb2c21f4955e902ef86d8491aafcfb5fa9a36900a6353a15848e30bf45`, branch `hp-6d8b9cdb2c21f4955e902ef86d8491aafcfb5fa9a36900a6353a15848e30bf45`, baseline `3e38025004d79a8ee1f8225fbb7f7ec9fed49a44`. Only owned paths changed. No branch creation, merge, push, dependency, manifest, frozen core/wire/flag/config changes, unsafe code, privileged test execution or workstation service/storage changes.

Shared file `crates/tidemarkd/src/lib.rs`: only reload callback implementation/signature and lifecycle construction/SIGHUP call sites changed for F03. A zero-argument public adapter preserves the unowned W6-A1 direct reload tests; production API/SIGHUP use the explicit cooperative callback.

## Per finding

### F02 — implementation candidate

Authorize SO_PEERCRED uid immediately: a nonzero uid or failed lookup closes before request buffers are allocated or parsed. Direct `dispatch` still returns the frozen Forbidden error for unauthorized requests. The first incomplete frame expires ten seconds from accept; partial bytes cannot reset the deadline. Subsequent frames receive a fresh absolute budget after the previous response. Malformed complete frames retain BadRequest behavior. Response writes have a separate absolute ten-second budget and check shutdown between writes; socket syscall timeouts poll at most every 50 ms. The request ceiling remains 1 MiB, responses remain bounded, and admission/retained worker handles are capped at 32.

Regressions: `idle_and_drip_frames_have_absolute_deadlines`, `response_write_deadline_and_shutdown_interrupt_stalled_reader`, `peer_refusal_precedes_malformed_frame_and_credential_failure_closes`. Socketpairs exercise these without bind permission. The admission test now verifies recovery after all idle slots expire. Nonroot/kernel-credential transport tests now expect immediate EOF without submitting a request.

### F03 — partial implementation; scheduler seam outstanding

Every spawned handler has an owning join guard in its connection worker. Timeout sends Unavailable but retains the admission slot and worker until the handler actually exits. Every exit path, including shutdown and write failure, drops the guard and joins. The server joins completed connection workers while reaping and joins remaining workers before returning; accept errors also shut down/join workers.

`Cancellation` combines an absolute deadline, lifecycle shutdown and atomic request state. `ReloadCallback::call` explicitly receives it. `CooperativeReload` adapts production callbacks; a blanket zero-argument callback adapter preserves existing injected fixtures and documents that those callbacks cannot promise cooperation. Production reload checks the budget before reading config, after captured config load, before scheduler preflight, and at the commit boundary. An atomic compare/exchange chooses cancellation or commit ownership; once commit wins, publication finishes even if a transport timeout follows, whose result is uncertain. This is stronger than a bare flag check before publication, and does not promise to cancel in-flight kernel/SQLite I/O (R3-7).

The API response acknowledgement wait observes shutdown and the same handler deadline. The scheduler's private acknowledgement wait still observes only its existing scheduler shutdown/deadline and eight-second limit; request-token propagation requires the excluded file change described above.

Regressions: both formerly detached-callback tests now assert the service is still running while the callback is blocked, release it, then join. `cooperative_reload_exits_on_deadline_and_shutdown` proves callback exit before connection join, and `cancellation_and_commit_have_an_atomic_winner` covers cancellation, deadline, shutdown and commit winning. No promise is made that an arbitrary noncooperative injected callback or blocked syscall exits within ten seconds.

### F04 — implementation candidate; unrestricted/guest acceptance pending

Walk socket ancestry component by component with retained O_DIRECTORY|O_NOFOLLOW handles, O_NOATIME with EPERM fallback. Reject mutable untrusted ownership, group/other-writable nonsticky ancestors, and any sticky ancestor without an immediately owned 0700 descendant. The final parent must be trusted and nonshared. Read-only mounts with untrusted owners are safe because their owners cannot mutate them; this is required for the sandbox's immutable nobody-owned `/`. No directory is created or chmodded by production path validation. `/run` root 0755 with `/run/tidemark` root 0700 satisfies the checks by inspection, pending Rocky runtime.

Retain the parent handle through shutdown. Unix bind/connect have no *at API, so their address is derived solely from `/proc/self/fd/<parent>/<raw leaf>`, never the configured mutable ancestry. Cooperating startups serialize with nonblocking directory flock. Detect leaves with no-follow statat, compare device/inode before unlinkat and after creation/security checks, and clean up only the owned socket identity. A replaced leaf is left intact; a renamed/replaced parent cannot redirect cleanup. The active-socket probe uses a nonblocking rustix socket with an absolute deadline, including a full backlog.

Linux rustix `chmodat(..., SYMLINK_NOFOLLOW)` returns OPNOTSUPP, including the cached version. Pin the socket with O_PATH|O_NOFOLLOW, fstat/reject a symlink or unexpected owner/type, then chmod that pinned inode through its procfd using chmodat. Both before/after checks compare the leaf's device/inode. Substituting a symlink cannot redirect chmod to its target. This procfd adaptation uses safe existing APIs and adds no dependency. Identity-check/unlink is not an atomic compare-and-unlink kernel primitive; trusted ancestry excludes unprivileged replacement, and flock serializes cooperating servers. A hostile process already running as the daemon uid/root remains trusted and can interfere.

Regressions: `socket_parent_trust_and_no_follow_checks_without_listening` exercises private/sticky ancestry, writable-parent refusal and parent/leaf symlinks without bind. New listening tests cover renamed parent cleanup, substituted leaf preservation/unchanged symlink target permissions, a full backlog deadline and concurrent startup identity preservation. Existing non-socket/active/stale test is retained. Owned API/operator/privileged temporary fixtures explicitly set outer directory mode 0700.

## Tests updated and regression sensitivity

Workspace search found transport authorization assertions in daemon W6-A3 and direct dispatch checks in W6-A3/W6-A2; only transport assertions change. Unowned W6-A1 direct reload invocations and W6-A2 zero-argument callbacks retain compatible adapters.

Updated: `pair_controllable_timeout_and_shutdown`, `controllable_handler_timeout_keeps_admission_bounded`, `thirty_two_connections_then_rejects_without_queueing`, `injected_nonroot_peer_is_forbidden_over_real_socket`, `lifecycle_start_uses_kernel_credentials_over_real_socket`; private permissions in `socket_config`, both operator scenarios and privileged `daemon_end_to_end`.

Three temporary mutations were built/run and then fully restored before final checks: removing incomplete-frame deadline checks makes the idle/drip test fail with a read timeout; dropping the handler handle instead of joining makes the ownership test fail with “server abandoned its blocked callback”; bypassing writable-parent checks makes the no-listener ancestry test fail its refusal assertion. Each command exited 101 with assertion failures, not compile failures. Logs: `library/f02-request-deadline-without-fix.log`, `f03-handler-ownership-without-fix.log`, `f04-parent-mode-without-fix.log`. These are targeted mutation tests, not an execution of the entire old baseline. Listening identity/backlog tests cannot be mutation-verified in this sandbox.

Test declaration counts before/after: API unit 2 → 8; W6-A3 top-level integration 7 → 9; operator 2 → 2; privileged 1 → 1. Net eight added tests. No fresh full baseline execution was collected. Final harness counts and check outcomes follow.

## Assumptions and proposed plan text (coordinator only)

- Refusing a nonroot transport peer means immediate close with no response/frame parsing; keep Forbidden for direct dispatch authorization. This follows the assigned early-refusal instruction over older plan prose saying every peer receives Forbidden.
- Ten seconds begins at accept for the first frame and after the previous response for each later frame. Each handler and response write has its own absolute ten-second budget. Completing malformed frames may keep a connection open; incomplete drip traffic cannot.
- API shutdown completes after all owned handlers exit. Cancellation wins atomically before configuration commit or publication completes after commit ownership is claimed; timeout after the boundary means uncertain outcome. Deadlines do not interrupt kernel/SQLite I/O already running.
- Socket directories must have trusted no-follow ancestry. Sticky ancestors require owned 0700 descendants; custom shared parents are refused, not normalized. Descriptor-derived procfs addresses are the Linux bind/connect and pinned-inode chmod adaptation.
- A directory flock serializes Tidemark API instances in one parent directory, including different socket leaf names. This conservative restriction assumes one API runtime directory per daemon.
- Existing scheduler cancellation bounds remain until coordinator adds request-budget propagation. No plan files were edited.

## Coordinator / guest commands

On an unrestricted host, after the fixture correction above:

```sh
cargo test --offline -p tidemarkd --lib api::tests -- --nocapture
cargo test --offline -p tidemarkd --test w6a1 --test w6a2 --test w6a3 -- --nocapture
cargo test --offline --workspace --no-fail-fast
CARGO_NET_OFFLINE=true tools/ci.sh
```

On a Rocky guest only (not this workstation):

```sh
cargo test --offline -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture
```

Also verify default root-owned `/run/tidemark` runtime binding, socket 0600, stale detection and shutdown cleanup under the packaged unit. Privileged tests were compiled only here; no privileged test ran.

## Validation

All long commands use `timeout 1500`, offline cache, host CARGO_HOME/RUSTUP_HOME and cached pinned toolchain. The initial plain cargo invocation failed because cargo was absent from PATH; PATH was corrected without installing anything. Listening tests detect EPERM, print one line and return; their harness passes are not runtime acceptance evidence. Full results will be appended after checks finish.

| Required check (all under timeout 1500, offline) | Exit | Evidence |
| --- | ---: | --- |
| `cargo fmt --check` | 0 | `library/fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `library/clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | 101 | `library/test.log`: 638 harness passes, 5 failures, 13 ignored |
| `cargo build --offline --workspace --features privileged --tests` | 0 | `library/privileged.log`; build only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 | `library/doc.log` |
| `CARGO_NET_OFFLINE=true tools/ci.sh` | 1 | `library/ci.log`: source policy, fmt and clippy passed; 567 harness passes, 5 failures, 13 ignored before stopping |

All five failures are in W6-A1: `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, `startup_reads_last_complete_from_catalog`, each timing out at its readiness wait. No additional failed target appeared in the no-fail-fast run. CI stops on that target and does not reach its own privileged-build/deny steps; the explicit privileged build above passed independently. Source policy reports 141 owner tags and a clean forbidden-symbol scan.

The final focused API unit run exercised eight tests, all passed. The W6-A3 integration harness reports 11 passes, with ten socket-dependent early returns for EPERM and one exercised dispatch test (`library/api-integration.log`). The earlier combined filtered command intentionally ran `api::tests` only, leaving all 11 integration tests filtered; a separate unfiltered integration invocation supplied the counts just stated. All eight unit assertions also passed in the full workspace run after the temporary mutations were restored.

## Definition of Done checkpoint

- Owned paths only; no manifest/module/dependency edits.
- Format, workspace Clippy, privileged compilation and rustdoc passed.
- F02 code and local socketpair regressions implemented; listening acceptance pending.
- F03 handler ownership and reload cooperation implemented; scheduler request-token acknowledgement propagation **not done**.
- F04 handle/trust/identity/probe implementation and owned fixtures implemented; unowned fixture corrections and unrestricted/guest runtime acceptance pending.
- Full tests/CI are not green. No verified task-success or VM acceptance claim.
- Public additions documented; no new production unwrap/expect/unsafe.
- Code committed in focused EDGE-api commits; handoff/report and cleanup are completed before submission.
