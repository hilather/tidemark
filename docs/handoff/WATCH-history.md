# Handoff — WATCH-history

## Needs from coordinator

- Run the privileged guest regression below on the disposable Rocky guests. It was compiled here, never executed on this workstation.
- Run the cargo-deny audit on equipped CI. The local wrapper explicitly skipped it because cargo-deny is not installed.
- Re-run CI as an ordinary user with Unix sockets and user namespaces available to exercise facility-guarded paths. This sandbox runs as uid 0 and denies those facilities. The brief warned of five readiness failures; the actual workspace run exited 0 with none observed. Passing guarded tests do not establish unavailable socket/namespace coverage.

Branch: canonical attempt `hp-840f7a24923b0f55930c3dd64ec9f176f2af938fbbd0c46de27624005ec9e2cb`. Date: 2026-10-13. Status: implemented as an evidence candidate for submission; independent verification pending.

## Changes and discriminating tests

**F2:** Persistence delivery marks its existing cursor without deleting admitted directory, unit, command, or unit-leader rows. Completed buckets still release distinct-file tracking and enforce the 1,000-key cap per scope at rollover; all retained maps expire with the 60-bucket ring. Legacy and completed-minute APIs share once-only row delivery, while completed-minute facts have their separate watermark. The old tests requiring emptied directory lists now require retained facts. `every_scope_retains_history_after_both_delivery_paths` compares queries in all three scopes for 5/15/60 minutes at every minute boundary across an hour, using each delivery API. `second_ticks_keep_live_history_and_mapping_loss_expires` executes the daemon service's extracted production tick every injected second and checks 82 bytes remain after the boundary, three persistence rows are delivered once, and the real alert-window reader remains available. Removing retention makes these assertions fail. Query ownership remains in `tidemark-live`; no reader changes in state.rs or metrics.rs are required.

**F3:** Each bucket keeps scalar event and signed-byte totals independently of ranked maps. If detailed ranking/output cannot be admitted, export emits one directory aggregate at the raw mount-root key with these exact totals, zero unavailable per-file precision, and `lossy=true`; completed facts are truncated. Its diagnostic is `truncated: output over the memory budget`. Query fallbacks record the same cause at the query minute, so the current health window reports even a failed long-window query over old activity. Distinct-file, current-key, completed-key and requested-output caps keep separate causes. Retained source rows survive both detailed and aggregate export. `exhausted_output_keeps_exact_aggregate_and_expiring_reason` saturates normal container admission, requires exact `(events=2, bytes=62)` from +71/-9 activity through both export APIs, checks the reason/status counter, and verifies once-only delivery. The former implementation emitted no export rows and no query diagnostic. `emergency_headroom_covers_full_history_and_aggregate_export` holds normal credits exhausted, ingests sixty minutes against a 4096-byte non-UTF-8 root, and proves every simultaneous aggregate output reservation succeeds inside the ceiling, with raw root bytes preserved.

**R4-03:** Mapping failure handling now receives event time and marks that exact bucket lossy/truncated. Numeric per-minute count and the latest errno are bounded; diagnostic causes are `unmapped events: <n> (reopen failed: EBADF/ESTALE/errno=<n>)` or `(no provenance)`. They expire from the fifteen-minute health window. The lifetime counter remains informational and does not prevent recovery. `injected_foreign_reopen_maps_or_reports_badfd` now exercises EBADF, ESTALE, missing provenance and successful reopening with actual descriptor metadata, allocated-byte oracle, hot/completed/persisted quality flags and fifteen-minute expiry. The daemon second-tick regression checks degraded component state, unavailable directory alert data and recovery to `ok`/available after expiry. Without minute marking, the component/window assertions fail.

**F4, guest test only:** `daemon_sigterm_releases_loop_xfs_for_ordinary_unmount` starts the actual daemon binary watching loop XFS, waits for readiness and a selected-mode log, prints that mode, sends SIGTERM, requires successful exit within ten seconds, then requires ordinary fixture cleanup (umount followed by owned-loop detach) within ten seconds. Root plus `TIDEMARK_DEPLOY_TEST_VM=1` guard the test; otherwise it prints a skip line. A child guard kills/reaps on assertion failure. The separate process does not expose an in-process old LiveState view; the existing retained-view service lifecycle test supplies that coverage. No lazy unmount is accepted as evidence.

## Memory accounting and assumptions

- The shared ceiling remains 64 MiB (67,108,864 bytes). Normal admitted history/cache/downstream storage may use at most 61 MiB; 3 MiB is emergency headroom for retained roots plus simultaneous aggregate output. A fully used admitted 60-minute history and its consumers therefore cost at most 64 MiB of conservative heap credits and fit the existing ceiling. Count caps and byte admission both apply: filling every nominal 1,000-row cap with arbitrary keys is not promised. Even 59 × 3 × 1,000 row-node credits alone exceed 64 MiB; admission must and does truncate before that state. No unbounded history copy was introduced.
- Emergency ingestion retains only directory root rows when optional unit/command admission fails, as before. Worst-case root retention is roughly 60 × (2 × 4096 + 512 + 4096) = 768,000 bytes; completed aggregate payload reserves the raw key copies separately from the possibly three-times-expanded UTF-8 mount label. Ordinary ranking/row-copy scratch cannot consume emergency payload credits; only root responses and aggregate exports use them. The saturation regression checks concurrent reservations, rather than checking only after their release. Credits bound accounted storage, not whole-daemon RSS or allocator OOM.
- The headroom partition changed from 1 MiB to 3 MiB inside the same ceiling. Test-only small budgets preserve their former proportional headroom. No dependencies, manifests, core types, CLI/config contracts or out-of-scope files changed.
- Existing public signatures remain intact; additive hidden injected-clock/quality seams support deterministic service tests. Production service ticks use one injected `at` for drain and health publication.
- `files=0` and no top unit on emergency aggregates mean unavailable precision, with the lossy/truncated flags providing the quality signal. Exact event and signed-byte totals use existing saturating arithmetic conventions.
- No user questions, branch creation, merges, pushes, host mounts, loop setup, service enablement or privileged test execution occurred. Canonical memory brief/input/receipts were read; no pending delivery was reported and no memory projection was edited.

## Validation

| Required check | Local result |
| --- | --- |
| cargo fmt --check | exit 0, including final-source repeat |
| cargo clippy --workspace --all-targets -- -D warnings | exit 0, including final-source repeat |
| cargo clippy --workspace --all-targets --features privileged -- -D warnings | exit 0, including final-source repeat |
| cargo test --workspace --no-fail-fast | exit 0, umask 022 |
| cargo build --workspace --features privileged --tests | exit 0; compile only |
| tools/ci.sh | exit 0; 150 owner tags and forbidden-symbol scan passed; all available checks passed |

Workspace log totals: 903 reported passes, zero failures, 14 ignored across 84 test summaries, including nested child summaries (not a unique-test count). Relevant suite summaries: live library 95 passed; daemon library 108 passed; w6a1 30 passed; w6a2s 8 passed. Some unavailable-facility tests return early and report `ok`; ordinary-user/guest coverage remains as stated above. No expected readiness failures were observed. The final scratch-admission change also passed all 95 live library tests.

All build/test commands use the pinned 1.98.0 toolchain, copied cache `CARGO_HOME=/tmp/history-cargo`, offline mode, and `timeout 1500`; the required workspace run and CI explicitly use `umask 022`.

Finding checkpoints: F2 live library 93 passed; F3 live library 95 passed; R4-03 live library 95 passed and daemon watcher module 16 passed. The strengthened emergency accounting check passed all 19 ring tests. F4's targeted privileged test binary compiled successfully. Full transcripts are in the attempt output `library/`.

## Guest commands and remaining uncertainty

Only in disposable privileged guests:

```sh
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a2s daemon_sigterm_releases_loop_xfs_for_ordinary_unmount -- --exact --nocapture
TIDEMARK_DEPLOY_TEST_VM=1 timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture
```

Run on both Rocky kernels and retain the printed selected mode. This test accepts the mode the kernel offers; it does not force an additional unavailable mode. Real fanotify mapping failures, shutdown/unmount timing, descriptor behavior and whole-daemon memory under workload remain guest evidence, not established by compilation or synthetic injection. Existing privileged tests outside this new test retain their own guards.

## Proposed plan and security text

For `07` §3.3: “Completed-minute persistence delivery and query retention have separate cursors. Delivery preserves admitted per-scope rows until their 60-bucket expiry. Count and byte caps remain enforced. Detailed-output admission failure emits one lossy mount-root directory aggregate with exact event and signed-byte totals; file and unit precision are unavailable. Query and export budget fallback are reported as output over the memory budget.”

For the watcher security/quality documentation: “Failed handle reopening or missing event-path provenance preserves measured totals at the watched root but marks that minute's directory attribution incomplete. Directory alert windows including it are unavailable. The component is degraded while the affected minute remains in its fifteen-minute health window and recovers automatically; the lifetime unmapped counter is informational.”

For planned unmount guidance (coordinator-owned docs): “Active watches retain a mount descriptor. Stop the daemon/watcher or remove the configured watch and observe teardown before ordinary unmount. SIGTERM closes watch descriptors after worker join. Lazy detach does not demonstrate release.”

## Definition of Done evidence

- [x] Owned paths only; no shared-file edits or new dependencies.
- [x] Format, both Clippy variants and workspace tests completed with exit 0.
- [x] CI wrapper completed with exit 0; cargo-deny audit explicitly unavailable.
- [x] Privileged regression written and workspace privileged tests compiled.
- [x] Additive public seams documented; no unsafe code introduced.
- [x] Local acceptance regressions and assumptions documented.
- [ ] Guest execution and independent task verification remain pending.

## Shared-file edits and commit notes

None. Commits separately cover retention, budget fallback, mapping quality and the guest test; a focused budget follow-up strengthens raw-root cost admission. No plan or security documentation was edited. The handoff and output report are evidence for independent review, not a declaration of verified task success.
