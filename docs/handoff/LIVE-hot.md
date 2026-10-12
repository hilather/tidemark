# Handoff — LIVE-hot: hot directories and unit attribution

## Needs from coordinator

Recheck the five sandbox readiness failures in coordinator CI, and run the privileged guest verification below (pending VM). Review the frozen persistence limitation: `HotMinute` and `hot_minutes` have no `top_unit` field. This change retains attributed completed-minute rows and their candidates for live queries, and drains the existing persistence payload unchanged. Persisting unit attribution would require a separate approved core/storage contract change; none is made here.

- Branch: canonical attempt branch `hp-3bf71534fd48e0a14dd9a98bce51296253ddfe873ba96e364bc2582bbed7fd05`.
- Date: 2026-10-12. Status: implementation and evidence candidate; independent verification pending.

## What changed

1. `rings.rs` consumes the existing unit argument. Private space-saving counters retain at most eight units per row per minute, weighted by absolute signed byte delta (`unsigned_abs`, including negative deltas). Ties select the lexicographically smaller unit. Unattributed events change row totals but do not add candidates. Zero-byte attributed events still provide a unit. Candidates are removed with row eviction and completed-minute truncation, and discarded on timeline rollback. Leader names are cloned only when a row leader changes, avoiding repeated event-loop allocations. Window queries sum all retained candidate counts across minutes before choosing the leader, rather than choosing among minute leaders. Unit-scoped rows identify their own unit; `?` stays unattributed with ordinary watcher inputs. Existing event/files/signed-byte/loss accounting is preserved.
2. `state.rs` builds `now.hot_dirs_5m` from nonfailed watchers with `Watcher::hot(5, Dir, MAX_INVENTORY)`, the same row limit writers use (no separate now limit found in 07/08). Missing watchers produce mount-qualified `hot_dirs_5m <mount>: not measured` entries; running empty watchers are measured. Report mount filtering is respected for these diagnostics. Exact `AlertSource::hot_dirs`, overflow windows and `WatchWindow` publications are unchanged.
3. `daemon_end_to_end` keeps its transient writer alive for five seconds after file close. All three success conditions and the original 60-second deadline remain; timeout diagnostics include the last parsed now report.
4. Searched workspace tests for `hot_dirs_5m`, `not measured`, and `top_unit`. No old-behavior assertions needed replacement. `w6a1.rs` and `w6b.rs` contain manual exact-window fixtures with `top_unit: None`; `w5d/main.rs` tests optional NowInputs and explicit missing measurements; core regression tests cover nullable DTO fields; CLI tests cover nullable rendering/wire cases. These remain valid. Updated test files: `crates/tidemarkd/tests/w6a1.rs` (added live report seam regression) and `crates/tidemarkd/tests/w6a3/privileged.rs` (writer and timeout evidence).

## Bound and approximation

Each row/minute has at most eight candidate unit names/counters. A ninth distinct name replaces the smallest counter (lexicographically largest name on a count tie), assigning its count plus the incoming weight. Leaders are approximate beyond eight distinct units in any included minute; exact with up to eight per minute absent ordinary row loss. Candidate approximation does not alter watcher `lossy`, which continues to report event/row loss. Stored maps obey existing 20,000 current keys per scope, 1,000 completed keys per scope and 60 buckets. Query merges are bounded by those retained rows and at most 60 × 8 candidates per key; they are temporary, not additional history. String/path storage remains dependent on input lengths as before; no RSS claim is made.

## Test seam and public signatures

Added one `#[doc(hidden)]` public constructor, `Watcher::from_injected_events`, taking an iterator of timestamp, three scope keys, delta, optional unit and device/inode identity tuples. It feeds the same rings and returns a watcher without a fanotify group or worker thread. It owns only an eventfd used by the existing stop/drop lifecycle. Existing public signatures and caller arguments are unchanged; the production caller already forwarded the unit. No manifest, core, CLI, shared module declarations or dependencies changed.

The daemon test publishes this watcher into LiveState, injects two attributed units into one directory, checks signed totals and the absolute-delta unit winner, missing-mount diagnostics, empty measured output and continued unavailability of the exact alert window.

## Proposed plan text (coordinator only)

- 07 §3.3 / §7 and 08 now: `now.hot_dirs_5m` uses the watcher's ranked five-minute directory list, including the current minute, with the writers row limit and original lossy flags. A missing/failed watcher is not measured for that mount; an empty running list is measured. This report list does not establish exact completed-window facts required by R3-1 alerts.
- 07 §3.3: `HotEntry.top_unit` is the attributed unit with greatest sum of absolute byte deltas in the row window, with lexicographic ties. Keep eight private space-saving candidates per row/minute; the winner is approximate beyond eight. Unit rows identify their own unit; unattributed rows return None.
- 07 fd attribution / W6 daemon_end_to_end: a process gone before `/proc` lookup remains unattributed, without retries or guessing. The transient test writer sleeps five seconds after file close so attribution is observable.

## Assumptions and deviations

No plan or core contracts were edited. The existing rings caller needs no change. No retry or process-attribution workaround is introduced. The test seam uses the existing watcher's real clock for queries and injects current timestamps, with a five-minute window to avoid boundary sensitivity. It accepts already-attributed metadata and bypasses only kernel/proc transport. Failed watchers count as no running measurement. Persistence cannot carry top_unit under the frozen contract; see Needs from coordinator.

## Guest verification (coordinator, guest only)

After building the candidate with privileged tests on the Rocky guest, run as root there:

```sh
cargo test --offline -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture
```

This command intentionally creates loop XFS/LVM and transient units in the guest. It was not run on this developer workstation.

## Validation

Logs and exact exits are retained in the attempt output `library/`. All cargo commands used offline cache/toolchain 1.98.0, with CARGO_NET_OFFLINE=true also for tools/ci.sh. Initial PATH/rustup-home failures were resolved by pointing PATH, CARGO_HOME and RUSTUP_HOME at the host's existing cached installation; no host cache installation or dependency change.

Test counts: rings declarations 3 before / 6 after; w6a1 declarations 18 before / 19 after; privileged daemon_end_to_end remains one modified test. Live library run: 60 passed (57 before by subtracting the three added tests; baseline was not separately executed). New daemon test: 1 passed, 18 filtered. Workspace totals and required-check outcomes are recorded below after completion.

Workspace source declarations: 612 before / 616 after, counted from all tracked Rust files (includes feature-gated/ignored tests). Actual baseline execution was not repeated. Required workspace run observed 499 passed, 5 failed, 13 ignored before stopping at w6a1; additional daemon suites observed 33 passed. These totals do not claim a complete green workspace.

| Required check | Outcome |
| --- | --- |
| `cargo fmt --check` | passed |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | passed |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | passed |
| `cargo test --offline --workspace` | exit 101: five w6a1 readiness deadlines (listed below) |
| `cargo build --offline --workspace --features privileged --tests` | passed; compile only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | passed |
| `tools/ci.sh` | exit 1: same five w6a1 readiness deadlines |

The workspace failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`, all at w6a1.rs:27 (`deadline exceeded`). The task identifies this sandbox's Unix socket refusal as a known limitation. No lifecycle tests were altered to hide it. New live test and all other w6a1 tests passed. Separately, `cargo test --offline -p tidemarkd --test w6a2 --test w6a3 --test w6b` passed 33 tests; an initial invocation mistakenly named nonexistent `w6a2b` and was corrected (both logs retained).

## Definition of Done evidence

Only six scoped paths changed. No dependency or shared-file edit. Formatting, both lint configurations, privileged compilation and rustdoc passed. Privileged runtime remains pending VM. Full tests and CI cannot be declared green in this sandbox. No new unsafe blocks or production unwrap/expect calls. Handoff and attempt report are evidence candidates, not independent task verification.

## Acceptance criteria evidence

| Criterion | Evidence | Limit |
| --- | --- | --- |
| Unit leader, heavier second unit, lexical tie, unattributed and negative deltas | `watch::rings::tests::attributed_leaders_and_absolute_deltas` | completed retained rows checked; frozen HotMinute omits unit field |
| Eight-candidate cap and unit-scoped identity | `watch::rings::tests::candidates_are_bounded_and_unit_rows_identify_themselves` | winner approximate beyond eight |
| Window leader differing from both minute leaders | `watch::rings::tests::window_leader_merges_nonleaders` | exact with retained eight-candidate minutes |
| now row/unit plus measured empty and missing-mount diagnostics | `now_uses_watcher_directories_without_exact_alert_windows` | kernel/proc transport injected; real fanotify pending VM |
| transient writer survives attribution lookup and timeout prints report | compiled `privileged::daemon_end_to_end` with five-second sleep and last-report assertion | pending Rocky guest command above |

## Shared-file edits

None. No dependencies added. Existing public caller signatures were preserved.

## Final-source validation checkpoint

All required commands were repeated after the leader-name allocation optimization. Final exits: fmt 0; Clippy 0; privileged Clippy 0; workspace tests 101 (499 passed, five readiness failures, 13 ignored); privileged test build 0; rustdoc 0; tools/ci.sh 1 (same five readiness failures). Final supplemental daemon suites passed 33 tests. See attempt library/final logs for exact output. CI stopped before cargo-deny; no deny-pass claim is made.
