# Handoff — LIVE-minutes: completed-minute facts and mount identity

## Needs from coordinator

Run the new mount identity test in the disposable Rocky guest (pending VM). Consume the new API in the separate daemon task; this task changes no daemon code. Recheck the five w6a1 readiness deadlines in socket-capable coordinator CI. Local check results and sandbox failures are recorded below; submission is an evidence candidate, not independent verification.

- Branch: canonical attempt branch `hp-2885f5f2f5e4a64e337789884ac1b892e597862ec7ffd65b4a98b9beed779fc1`.
- Attempt: `attempt-2885f5f2f5e4a64e337789884ac1b892e597862ec7ffd65b4a98b9beed779fc1`.
- Date: 2026-10-12 America/Toronto.
- Status: implemented; guest execution and coordinator verification pending.

## What was built and final signatures

The public type is `tidemark_live::watch::CompletedMinute`:

```rust
pub struct CompletedMinute {
    pub started_at: i64,
    pub overflow_total_at_start: u64,
    pub overflow_total_at_end: u64,
    pub lossy: bool,
    pub truncated: bool,
    pub dirs: Vec<HotEntry>,
    pub persisted: Vec<HotMinute>,
}
impl Watcher {
    pub fn drain_minutes(&self, top_k: usize) -> Vec<CompletedMinute>;
    pub fn mount_identity(&self) -> std::io::Result<(u64, u32, u32)>;
}
```

No existing public signature changes. `CompletedMinute` also derives Clone and Debug. Both drain methods document their shared persistence-row cursor. Fact emission has its own monotonic timestamp watermark: legacy drains consume persistence rows but leave minute facts and directory rows available to the new drain. The legacy drain retains its startup partial-minute persistence rows; the new API starts at the following full minute.

The existing ring now materializes idle minutes, bounded to its existing 60 buckets and at most 60 iterations even after a very long pause. Completed row caps, current row eviction, per-row eight-unit candidates, deterministic rankings, and WATCH-perf keyed lookups remain. Each bucket stores overflow boundary totals and separate list-truncation facts. Only actual queue-overflow events increment the cumulative counter; recovery and other loss do not. A retention gap marks the first returned minute lossy without fabricating the missing timestamps. Overflow end/start values chain for consecutive returned minutes and never decrease.

Rollback retains the existing history reset, preserves the fact and row drain watermarks, and marks the replayed interval through the former current minute lossy. Previously emitted minute timestamps and previously drained persistence rows are not replayed. The last emitted end boundary is carried into the next contiguous emitted minute after rollback so totals still chain. Failed/recovering intervals, including idle time advanced by ordinary queries, are conservatively lossy.

The watcher retains the opened O_PATH directory descriptor in an Arc shared with its worker. Negotiated marks use `/proc/self/fd/<fd>/.`: the final dot resolves to that retained directory even with the existing mark helper's FAN_MARK_DONT_FOLLOW flag. Opening precedes marking, so a replaced configured pathname cannot make the identity descriptor refer to a different mount than the mark target. `mount_identity` uses fstat device major/minor and a bounded 16 KiB fdinfo read for decimal `mnt_id`, compatible with EL8 without statx mount IDs. Existing mount-ID helpers are private or expose other metadata, so a small private parser is used. Injected watchers return Unsupported. Additional I/O is mount-descriptor fstat and procfs metadata only; no regular-file contents are read.

## Daemon consumption: exact WatchWindow rules

Keep a bounded deque of the latest 60 `CompletedMinute` facts per watcher identity and discard it when that watcher is replaced. Switch persistence to each new fact's `persisted`; do not separately persist its `dirs`. Use `drain_minutes(1000)` if the full existing completed-row cap is desired. Smaller top_k values cut lists and can make otherwise healthy minutes truncated.

For an exact supported duration n minutes, 1 <= n <= 60, let E be the end of the latest completed minute and S = E - n * 60. Publish `measured_at = E`; do not present a stale E as a fresh observation. Other durations are unavailable, never rounded or clamped.

- **overflows:** retrieve cumulative boundary samples at exactly S and E from the retained facts (`overflow_total_at_start` at started_at, `overflow_total_at_end` at started_at + 60). A previous minute's end can also supply the S boundary. If either endpoint sample is missing, or subtraction would decrease, publish None. Otherwise publish Some(total(E) - total(S)). Loss/truncation does not by itself make this overflow difference unavailable; even if interior minute facts are missing, known cumulative endpoint samples still give the exact difference. Reject inconsistent duplicate samples defensively.
- **hot:** require exactly n consecutive facts at S, S+60, ..., E-60. Publish None if any fact is missing, lossy, truncated, or has an overflow end/start difference. A marked retention gap therefore makes any window including its first surviving fact unavailable, even if some endpoint samples exist. Otherwise sum `dirs` by raw key, saturating events/files/signed bytes as in the watcher, and rank by descending signed bytes then ascending raw key. All empty healthy minutes yield Some(empty), not None. If the daemon's own inventory cap would cut the resulting union, publish None. The minute `top_unit` is the LIVE-hot minute leader; full window candidate weights are private, so minute leaders cannot reconstruct the existing Watcher::hot cross-minute leader exactly. Alert growth uses signed bytes; do not claim exact reconstructed cross-minute unit attribution.

The frozen `Publication::WatchWindow` and freshness reader remain the responsibility of the daemon consumer task. Persisted HotMinute payloads remain exactly the legacy payload; new gap/output-cap loss facts are conveyed by CompletedMinute rather than silently changing that frozen row DTO.

## Acceptance evidence and tests

| Contract | Evidence |
|---|---|
| Empty minutes, queue totals, distinct recovery loss | `completed_empty_minutes_and_overflow_boundaries`; `injected_watcher_overflow_and_recovery_facts` |
| Completed and requested list caps | `completed_caps_and_output_caps_are_truncated`; existing current-cap test retained |
| Ninety-minute pause, bounded storage, visible lossy gap | `pause_is_bounded_and_gap_marks_only_first_returned_minute` |
| Rollback with monotonic emission and chained totals | `rollback_does_not_repeat_minutes_or_rows_and_totals_chain`; existing rollback/dirty-root tests retained |
| Shared persistence cursor in both call orders; facts survive legacy drain | `drains_share_rows_but_not_minute_facts`; `injected_watcher_minutes_and_shared_persistence` |
| Injected mount identity Unsupported | `injected_watcher_minutes_and_shared_persistence` |
| Bounded parser, actual descriptor device/mount ID, retained target after rename/replacement | `fdinfo_mount_id_parser`; `retained_mount_descriptor_survives_path_replacement` |
| Real marked loop XFS identity differs from parent | `privileged::watcher_mount_identity_matches_loop_xfs_mountinfo` — compiled here, pending VM |
| LIVE-hot and WATCH-perf behavior | Existing ring leader/candidate/window/cap tests and metadata/attribution/descriptor tests retained |

Guest command (root account in the disposable guest only):

```sh
cargo test --offline -p tidemark-live --features privileged --test w5a privileged::watcher_mount_identity_matches_loop_xfs_mountinfo -- --exact --nocapture --test-threads=1
```

Source-declared test counts relative to base `4464905`: watch/mod.rs 11 -> 15 (includes its existing privileged unit test), rings.rs 6 -> 11, w5a/privileged.rs 7 -> 8. Nine unprivileged tests and one privileged test added. The first local library run before adding the tests passed 61/61; the final library has 70 tests. The watch-only final run passed 30/30. These counts do not imply that feature-gated guest tests ran.

## Shared-file edits

None. No dependency, manifest, lib.rs, core DTO, daemon, CLI, plan, procscan, or alerts edits. Only watch/mod.rs, watch/rings.rs, w5a/privileged.rs and this handoff changed.

## Assumptions and deviations

- Startup's partial minute is excluded even when startup happens exactly on a minute boundary; eligibility starts at floor(start / 60) * 60 + 60. Injected sources use their first event timestamp as startup, or current time for an empty source; private injected-time drain helpers test production drain logic without real-clock sleeps.
- The existing 60-bucket bound includes the current in-progress bucket. A single delayed drain can therefore return 59 completed facts; callers draining regularly can retain 60 completed facts themselves. The task's ninety-minute acceptance requires at most 60 returned facts, and this preserves the existing ring's memory bound.
- top_k controls directory output and persistence rows per scope, both clamped to the existing 1,000 completed-row cap. Cutting any per-minute scope list, including a legacy output cut, sets the new truncated/lossy fact. Legacy row lossy values remain unchanged by requested output cuts.
- Queue counters saturate at u64::MAX as the existing watcher stats counter does. Overflow timing follows existing event-processing wall-clock time.
- No new public test clock, unsafe code, dependencies or unbounded collections. O_PATH performs no directory enumeration/atime read; regular directory scan rules are unchanged.
- Marking via the retained descriptor's procfs path requires procfs, already required by this watcher. Actual kernel fanotify behavior on Rocky remains pending the guest test.

## Validation

All Cargo commands use pinned Rust 1.98.0, CARGO_HOME=/home/brewerm/.cargo, RUSTUP_HOME=/home/brewerm/.rustup and offline mode. Every long command uses timeout 1500. Privileged tests are compiled and linted only; none was executed here. Full logs are under this attempt's worker-output `library/` directory.

| Command | Exit | Evidence |
|---|---:|---|
| `cargo fmt --check` | 0 | fmt-final.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | clippy-final.log |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0 | clippy-privileged-final.log |
| `cargo test --offline -p tidemark-live --lib` | 0 | focused-final.log: 70 passed, zero failed |
| `cargo test --offline --workspace --no-fail-fast` | 101 | workspace-tests.log: 599 passed, five readiness failures, 13 existing ignores |
| `cargo build --offline --workspace --features privileged --tests` | 0 | privileged-build.log |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 | docs.log |
| `tools/ci.sh` with offline environment | 1 | ci.log: same five w6a1 readiness failures |

The five workspace failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`, all at w6a1.rs:27 with `deadline exceeded`. They match the task's documented Unix-socket sandbox limitation and need coordinator CI confirmation. Unchanged root/socket tests may report success through existing early returns; this is not evidence those operator scenarios executed.

CI source policy, format, and Clippy checks passed before CI stopped at those five readiness failures. Its privileged-build and cargo-deny stages were not reached; privileged compilation was independently completed above. Final format, focused library tests, and both Clippy configurations were repeated after source edits. Privileged build and rustdoc ran on the final source. No privileged or destructive workstation commands were executed.

## Definition of Done evidence

- Only owned paths changed; no shared-file or dependency changes.
- Public new items and shared cursor semantics documented; original ownership headers preserved.
- Nine new unprivileged tests pass; one new privileged test compiles, pending VM.
- Format and both Clippy configurations pass; broad readiness failures are explicitly reported.
- Handoff and daemon consumption contract supplied. Independent acceptance and guest execution remain pending.
