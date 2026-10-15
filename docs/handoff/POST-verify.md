# POST-verify handoff

Date: 2026-10-15. Status: partial candidate; items 1, 3 and 4 implemented, item 2 blocked by scope. Attempt: `attempt-e0d9d1a39413b8c69294245854651069b9c9bc944b7f66a2a4e30174929e75f8`.

## Needs from coordinator

- **Item 2 is blocked by the owned-path boundary.** `Watcher::history_covers_at`, `query_at`, and the `hot_at_locked` lossiness guard in `crates/tidemark-live/src/watch/mod.rs` accept the current minute. At `history_start = 60`, `now = 120`, they accept a two-minute window; `state.rs` correctly computes the same maximum, 2. Subtracting one in the diagnostic alone would violate the explicit requirement that N be the largest accepted window. The methods also clamp windows to 60, so at 60 minutes N+1 cannot fail through `history_covers_at`. Please authorize/reassign the watcher coverage change and settle whether “complete minutes” excludes the current minute. No out-of-scope file was edited and no minute-rule change is claimed.
- Run the privileged watcher test on the VM, and the uid 1000/umask 022/socket and Ubuntu runner gates. No privileged tests were executed here.

## What changed

1. Verification retains the running daemon's typed `status` response and judges required components from it. Stored component health no longer affects the live verdict. Staged/firstboot config hashes must still match. On API failure, the diagnostic names the stored report's `daemon_started_at`, or `unknown`, and refuses to certify health. The API seam retains its original ping method for source compatibility (including `fuzz/support/deploy.rs`); the added status method defaults to unavailable for ping-only adapters. No core API/wire type changed.
2. Investigated only; blocked as above. The existing API startup/replacement test passes on this baseline and does not cover the reported completed-minute interpretation. A standalone probe against the built watcher (source/output in the attempt library) tests boundary and mid-minute startup with 0, 1, 2 and 60 complete minutes: it reports largest accepted windows 1, 2, 3 and 60, respectively, exactly matching the existing diagnostic. At 60 it also accepts 61 because of clamping. This directly demonstrates the conflicting requirements.
3. The general stopped-service hint says to run `systemctl reset-failed tidemarkd` first for `start-limit-hit`, then start the service. This uses the explicitly allowed general wording alternative without an additional systemctl query. The stopped-service early return remains in place.
4. The first worker thread-count assertion now uses the existing bounded wait, allowing worker comm initialization to finish. All other thread-count checks already use bounded waits. The final join and thread disappearance checks remain unchanged. The guest race was not reproduced locally because privileged execution is prohibited.

## Verification rule

A healthy verdict requires an active service, a firstboot config hash matching the staged record, a successful current daemon status response, and exactly one `ok` entry for each of sampler, scheduler, api and tswriter in that response. Stored report health never certifies a running daemon. API unavailability fails verification and labels the stored report's start; `--wait` retries current health until success or deadline, retaining the existing stopped-service early return. JSON fields, schemas, exit codes and flags are unchanged.

## Regression evidence

- `verify_uses_current_status_instead_of_old_unhealthy_report`: baseline failed its healthy assertion; passes after the change.
- `verify_live_components_override_stored_components_for_any_start`: both stored-health directions and two report start timestamps; current live health determines all eight verdicts. Also checks unavailable-API report-start labeling.
- `verify_wait_rechecks_live_health`: degraded first response and healthy second response, with an old unhealthy report; passes. Its final ten-second budget exercises the normal five-second poll interval rather than depending on a first poll completing within 20 ms under load. The final focused verify rerun passed all 8 tests in 5.01 seconds.
- Existing stale-hash, API-failure, activation and stopped-service tests retained. Existing stored-health assertions now expect the live response to win.
- `stopped_service_hint_covers_start_limit`: baseline failed because the reset-failed text was absent; passes after the change and checks reset-before-start ordering through Systemctl/ApiPing seams.
- `watcher_final_drop_joins_worker_on_vm`: bounded start wait implemented; compile-only validation, pending VM. No deterministic pre-fix failing VM run is claimed.

## Proposed plan text (not applied)

08 §5, under deploy verify: “Every required core component is `ok` in the running daemon's current `status` response. The stored firstboot report remains evidence for the staged configuration hash; its component states do not certify a later daemon start. If current status is unavailable, verification fails and identifies the stored report's daemon_started_at, or unknown. --wait polls current health every five seconds until success or timeout, except for the existing stopped-service early return.”

11 P3: “Verification checks the running daemon's current core component states, retaining the staged/firstboot hash check. Archived firstboot health from a previous daemon start is historical evidence, not a current verdict.”

`docs/operator-guide.md` now explains the live rule and unavailable-status behavior.

## Assumptions and scope

- The explicitly assigned post-release task replaces the generic wave-task selection workflow; no matching POST-verify wave section exists.
- Choose live status exclusively rather than adding a daemon-start identity to the frozen status wire contract. A current-start stored report remains visible as evidence, while current status is authoritative.
- Missing/duplicate required components fail; optional collectors retain their previous nonblocking semantics.
- Keep the firstboot hash check exactly as requested; this is not a new running-config-hash API.
- General start-limit wording is preferable to another subprocess solely for Result.
- No shared manifests, dependencies, core types, plan documents or unowned paths changed.
- Initial tool invocations lacked cargo on PATH and the default RUSTUP_HOME was unwritable. Validation uses host `/home/brewerm/.cargo` and `/home/brewerm/.rustup`, with offline Cargo.

## Validation

The workspace run encountered an out-of-scope failure in `tidemark-fs::lvm::tests::output_cap_and_disabled_collection` (unexpected error variant for a temporary head-output script). The same test passed in isolation (1 passed, 42 filtered). Both logs are retained; no cause or fix is claimed. Results and counts are recorded below after the workspace checks finish. Detailed command logs are in the attempt output library. This handoff is an evidence candidate, not independent acceptance or integration approval.

- `cargo fmt --check`: passed, including a final check after formatting the longer recovery-test budget.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed, including a final rerun after the socket deadline and recovery-test budget changes.
- `cargo test --offline --workspace --no-fail-fast`: exit 101; log summary totals 998 passed, 1 failed, 14 ignored across 87 suite summaries (including subprocess harnesses). Sole failure: the out-of-scope LVM output-cap test above.
- Isolated LVM output-cap rerun: 1 passed, 0 failed.
- Focused verify run: 8 passed, 0 failed. Focused start-limit hint run: 1 passed, 0 failed.
- Existing API startup/replacement boundary test: 1 passed, 0 failed.
- `cargo build --offline --workspace --features privileged --tests`: passed; execution pending VM.
- `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps`: passed.
- All long validation commands use `timeout 1500`. CI inherits `CARGO_NET_OFFLINE=true`.
- `tools/ci.sh` with offline Cargo: exit 0. It reran source checks, formatting, Clippy, strict-ancestry tests, workspace tests and the privileged-test build. Log totals: 1001 passed, 0 failed, 14 ignored across 88 summaries, including 2 strict-ancestry passes and child harness summaries. The second workspace run therefore had 999 passes and no failures. The initial LVM failure remains recorded above.
- CI reported `cargo-deny is not installed; skipping cargo deny check`; dependency-policy validation remains unavailable here.
- Final focused verify rerun after widening the recovery-test budget: 8 passed, 0 failed, 134 filtered, 5.01 seconds. Final formatting and workspace Clippy checks passed.
- Environment: uid 0, umask 022. No uid 1000 or guest execution is claimed. Socket-denied/VM-gated early returns and ignored scale tests are not guest acceptance.
