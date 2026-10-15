# Handoff — SCAN-first: start never-scanned roots immediately

## Needs from coordinator

Review this evidence candidate and rerun the exact candidate as uid 1000, umask 022, with sockets, and on the GitHub ubuntu-24.04 gate. This worker runs as uid 0. Privileged execution remains pending VM; only compilation is authorized here. Independent acceptance and integration remain with the coordinator. Run cargo-deny on an equipped host; the local CI wrapper skips it because it is not installed.

## Rule as implemented

`next_run_in_timezone` returns no due time for `off`, then returns the injected `now` for any other schedule without a completed scan. This bypasses both daily time and window until the catalog contains a completed scan. Later schedules retain their existing calendar/DST/window behavior. Extracted the unchanged window calculation into `next_in_window` so the elapsed-time cadence path can apply windows after a completion without using `None` as a synthetic completion marker.

First failures still use the existing elapsed-clock `retry_due` backoff, independently of the window. Root ordering, the single active host slot, the bounded command channel and launch parameters are unchanged. Scheduled launches pass no throttle override, so the scan orchestrator uses the configured root profile.

## Callers checked

- `next_run`: delegates to the explicit-zone function; first-scan behavior is shared.
- `Recorder::status`: supplies the previous completed entry while running/failing, or the newly completed entry on success. Its `next_due` now reflects immediate first-scan eligibility; existing failure publication replaces this with retry time.
- `initialize_schedules`: `Catalog::latest` selects only `status = complete`; startup status and firstboot next-scan inventory therefore show the immediate injected time for never-scanned enabled roots.
- `preflight_reload`: validates with the latest completed catalog entry; its real clock is only for preflight, and it publishes no due time.
- Scheduler poll/reload publication: configuration generation changes trigger a pass even when the elapsed polling deadline has not arrived. New roots have no catalog completion and become immediately due. Daily uses the completion directly. Every cadence uses `next_in_window` when a completion exists, preserving elapsed-time deadlines and window behavior; without a completion it uses the new first-scan rule.
- `LiveState::replace_config` reserves new-root statuses; scheduler polling fills due times and launches work. No state.rs change was needed.

## Tests added

- `schedules::first_scan_ignores_cadence_and_window_except_off`: fixed UTC instant; every 6h and daily 03:30 ignore the closed window, off stays disabled, and an overdue completed root still waits for the window.
- `runtime::never_scanned_roots_start_immediately_outside_window`: fixed injected wall/elapsed clocks; both every and daily roots complete at startup outside their windows, with the configured gentle profile.
- `runtime::reload_adds_never_scanned_root_due_immediately_and_retries_outside_window`: adds a missing daily root by reload, observes first failure and 30-second retry publication, creates the root, proves no completion before the retry deadline, and advances elapsed time to complete outside the window. The original off root remains unscanned.
- `runtime::completed_root_still_waits_for_window_with_elapsed_cadence`: manually completes an off root, enables a short every cadence with a closed window, and checks that later scheduling still waits.

## Existing tests changed

Only old no-completion fixtures whose expectations encoded window/date processing changed:

- `due_scan_waits_and_window_wraps`: supplies an overdue completion so it continues testing window boundaries and wrapping.
- `windows_across_both_transitions`: supplies overdue completions, retaining the exact expected DST gap/fold results.
- `invalid_clock_is_fallible`: supplies an out-of-range completion for the daily case; no-completion scheduling now returns `now` without calendar conversion.

Other existing tests and their expectations are unchanged.

## Documentation and scope

Updated architecture deployment prose, configuration prose outside the normative TOML block, and the operator scheduling section. The operator guide explains immediate first-start I/O, configured throttle, reload behavior, retry, later windows, and deferral via `schedule = "off"` or staging later. No metrics-default lines, shipped configuration, contracts, manifests, dependencies, module declarations, unsafe code or shared-file edits.

## Assumptions

- SCAN-first is the explicit owner-authorized follow-up task; the W6 scheduler plan supplies surrounding behavior.
- A completed scan means the existing `Catalog::latest` completed-only query. Failed and partial catalog rows do not count.
- Runtime tests derive a closed local-time window from a fixed injected instant so assertions hold in any host timezone without changing process TZ or using the wall clock. Unit tests use explicit UTC and the existing deterministic Eastern fixture. Orchestrator-owned metadata timestamps remain unchanged.
- Use the host Rust 1.98.0 cache with `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo` and offline Cargo. Every long check uses `timeout 1500`.
- No identity switching, user questions, branch creation, merges, pushes, privileged execution or memory-projection edits.

## Validation

Commands use offline Cargo, the host 1.98.0 toolchain/cache, and `timeout 1500`. Logs live in the canonical attempt output `library/`.

| Check | Observed result / log |
| --- | --- |
| `cargo fmt --check` | passed; `fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | passed; `clippy.log` |
| `cargo test --offline --workspace --no-fail-fast` | passed; `test.log` |
| `cargo build --offline --workspace --features privileged --tests` | passed, build only; `privileged.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | passed; `doc.log` |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | passed; `ci.log` (cargo-deny skipped: not installed) |
| Focused scheduler suite | 25 passed; `focused.log` |
| Focused suite with umask 022, `TZ=UTC` | 25 passed; `focused-utc.log` |
| Focused suite with umask 022, `TZ=America/Toronto` | 25 passed; `focused-toronto.log` |
| TOML example block comparison against base | byte-for-byte unchanged |

No privileged tests ran. Existing feature-gated tests compile; runtime execution is pending VM. This change needs no new privileged operation or fixture. The uid-1000 and GitHub runs are not claimed here. Independent task acceptance remains pending coordinator review.


## Completion checklist

- Only owned paths changed; no shared-file or dependency edits.
- Format, Clippy, workspace tests, privileged-feature build, rustdoc and CI wrapper exited 0.
- Required behavior is covered by explicit-clock scheduler and explicit-zone unit tests.
- Existing privileged tests compile; execution pending VM. No new privileged test is needed for this scheduling-only change.
- No new unsafe blocks, public items, unexplained TODOs or production unwraps.
- This handoff and the attempt report are evidence candidates, not independent verification.
