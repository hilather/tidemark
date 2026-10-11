# Handoff — W5-A-fix: fanotify review corrections

- Attempt: `attempt-75bfda0449898aedfb9c8cf121f5bcd9f03732ece799e19de5c7b66e39ac2ccf`
- Branch: `hp-75bfda0449898aedfb9c8cf121f5bcd9f03732ece799e19de5c7b66e39ac2ccf`
- Date: 2026-10-11
- Status: implementation and unprivileged evidence supplied for review; privileged runtime pending VM. This handoff is an evidence candidate, not a release or independent verification claim.

## Findings 1–9

| Finding | Change | Evidence |
|---|---|---|
| 1: transient descriptor exhaustion | Retry EMFILE, ENFILE, ENOMEM and ENOBUFS with exponentially increasing 50–800 ms waits on the shutdown eventfd. Recovery retains the group, clears unsafe path history and marks each affected minute lossy, including the successful recovery minute. Unrecoverable transport errors persist as `WatchMode::Failed`; stats expose recovery and error counts, and report permanent degradation even after ring loss ages out. CLI exits on a failed worker. | `synthetic_event_processing_exclusions_attribution_and_cleanup` injects EMFILE on read 1 through the production `read_recover` helper, returns an actual fixture descriptor on read 2, and requires processing to resume. It checks fatal EBADF, persistent failed mode/statistics, and prompt shutdown wakeup. |
| 2: stale paths after loss | Clear both the handle/path LRU and path/size history on overflow, malformed events, resolution failures and transport loss. Dirty hints become the mount root after loss. Numeric inode-size history remains usable. | The same injected processor test primes both caches, renames a real directory into an excluded prefix, injects overflow, requires both caches to be empty, resolves again through the production metadata resolver using the original open directory descriptor, and verifies the new excluded path. A subsequent fd event under that path is excluded. Kernel handle opening itself remains pending VM. |
| 3: clock rollback | A backward minute step discards affected ring history and creates a lossy bucket on the new timeline. Hot/loss queries explicitly exclude future minutes. Discarded rows are not replayed by drain; new completed buckets drain once. | `sixty_buckets_and_clock_rollback` checks bounded retention, rollback loss, empty old history, new timestamp-correct additions, upper bounds on hot/loss queries, and drain-once behavior. Existing forward rollover and capacity tests remain. |
| 4: state-directory exclusion | Foreground options always add `general.state_dir`, canonicalizing its actual location when it exists; a missing state directory retains its configured raw absolute prefix. Other errors propagate. | CLI `mount_realpath_and_state_exclusion` checks an existing state alias and a similarly named sibling. The processor test injects events from a PID other than this process and verifies exclusion versus sibling inclusion. |
| 5: foreground stream | Accepted SCR-001 implemented as `WatchEvent`, `enable_events`, and `drain_events`. The optional buffer holds 1,000 events, drops the oldest, and counts dropped events. Ordinary daemon construction leaves it absent and avoids stream allocation/cloning. Foreground output emits individual time/path/delta/PID/comm/unit lines, a shared monotonic-clock 50-events/s limit across mounts, omission summaries, and existing top-five completed-minute directory summaries. Text event times are UTC HH:MM:SS. | Processor test checks 1,000-event bound, dropped count, oldest-first partial drain and full drain. CLI `limiter_and_complete_event_envelopes` checks 50 allowed events, omitted count/reset, individual rendering and wire fields. |
| 6: rename contract | Accepted SCR-002 is directional only. Separate MOVED_FROM/MOVED_TO records retain their paths and signed deltas. No FAN_RENAME, correlation guesses, paired record type, or pairing claim is introduced. The task's accepted decisions supersede the old pending SCR handoffs; those documents and the plan are outside this task's write scope. | Injected processor test records independent move directions with measured allocation, asserting separate source/removal and destination/addition events. Existing privileged FID fixture checks the kernel's four directional masks and names, pending VM. |
| 7: mount realpath | Canonicalize the original user/config mount path with the existing filesystem helper before constructing RawPath or starting a watcher. Scanner/event no-follow behavior remains intact. | CLI `mount_realpath_and_state_exclusion` checks relative `.`, an alias, and alias-plus-parent semantics. |
| 8: JSON envelope | Every event, omission summary and completed-minute summary is emitted through `render::json` with schema `tidemark.watch.v1`, one complete document per line. Event paths reuse the existing HotEntry report adapter because the core path helper is crate-private; unrelated aggregate fields are removed before event fields are added. Core is unchanged. | `limiter_and_complete_event_envelopes` checks entire ASCII, Unicode and invalid-UTF-8 event envelopes, exclusive path/path_b64, attribution and delta, and exactly one newline. |
| 9: meaningful overflow/load tests | The unprivileged processor tests require overflow count, loss, resumed processing, exact accepted-event counts and descriptor stability. A deterministic group-selection test verifies fd fallback when rich selection fails. Replaced the raw-group-only overflow fixture with a privileged running-watcher test that pauses processing via its private state lock, fills the kernel queue, requires overflow/loss statistics and then observes a fresh event. The scale fixture continuously paces 2,000 closes/s for ten minutes, measures watcher-thread utime+stime in the first 60 seconds against the 2% budget, verifies generated rate, checks processing each second and checks final fd count. | Unprivileged `synthetic_event_processing_exclusions_attribution_and_cleanup`, `synthetic_batch_fd_count_stable`, `rich_mark_failure_selects_fd_fallback`. Privileged `kernel_overflow_worker_survives_and_resumes` and `watcher_sixty_second_load_and_ten_minute_fd_stability` are compiled only, pending VM. |

## Shared-file edits and dependencies

None. Only owned watch source/tests and this handoff changed. No manifests, lockfile, core types, crate roots, plans, or AGENTS.md changed; no dependencies or unsafe blocks added. Transport retry collection is bounded by the existing 64 KiB read buffer. The failure mode/stat fields fulfill finding 1's requirement; existing option fields and start signature remain compatible. Event buffering is opt-in per the accepted task decision.

## Validation

Commands use the installed Rust 1.98.0 toolchain, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/home/brewerm/.cargo`, and offline Cargo. The default profile's rustup home was read-only; early format/Clippy attempts failed before executing checks and were rerun with this environment. Logs and matching exit files are in the canonical attempt output `library/`.

| Command | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | exit 0 | `library/fmt.log`, `fmt.exit` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0 | `library/clippy.log`, `clippy.exit` |
| `cargo test --offline --workspace` | exit 0; 289 passed, 0 failed, 5 existing ignores | `library/test.log`, `test.exit` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; compilation only | `library/privileged.log`, `privileged.exit` |
| `tools/ci.sh` with offline environment | exit 0; 289 passed, 0 failed, 5 existing ignores | `library/ci.log`, `ci.exit` |
| `cargo test --offline -p tidemark-live -p tidemark-sys -p tidemark-cli` | exit 0; 90 passed, 0 failed, 0 ignored | `library/owned-tests.log`, `owned-tests.exit` |

CI reports that cargo-deny is not installed and skips its dependency-policy check. The settled-source workspace Clippy, format, privileged compilation, owned-crate tests and CI logs all exited 0. No unrelated source was changed to obtain these results.


## Pending VM and limits

No privileged test was executed, and no mount, LVM, service or host configuration was changed. Kernel overflow/resumption, real FID handle resolution and fallback, Rocky 8 capabilities, actual descriptor exhaustion, the under-2% CPU measurement, and ten-minute fd stability remain **pending VM**. Run the ignored scale test alone with `--features privileged -- --ignored --exact privileged::watcher_sixty_second_load_and_ten_minute_fd_stability --nocapture` in a disposable VM. The new overflow unit test runs in the live library's privileged test binary. The existing fd-mode, FID, exclusion and namespace fixtures also remain pending VM.

Injected transport and descriptor-resolution seams validate worker policy and real metadata behavior, not kernel privilege/capability behavior. Rollback intentionally loses prior ring history and labels the new minute lossy. Counts and byte estimates during any lossy period remain approximate. Shutdown wakes polling/retry waits promptly; synchronous metadata/procfs stalls still require runtime evidence for the full one-second stop bound. No CPU or kernel-runtime measurement is claimed here.
