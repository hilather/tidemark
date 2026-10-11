**CHANGES REQUESTED** — 0 BLOCKER, 4 SHOULD-FIX, 0 NIT.

Reviewed candidate `838afab3158d3f6dc976f53732e49e075da52889` against base `da1d02bbe5b49fa1bf1ee84dfaece061911caf38`, limited to the candidate diff. Line numbers below refer to that candidate. The supplied report worktree starts at a later commit; all builds and probes used a separate, unchanged archive of the exact candidate. After validation, all 315 archived files still matched their candidate Git blob hashes.

## Findings

### 1. SHOULD-FIX — Growing writers can be permanently excluded at capacity

`crates/tidemark-live/src/procscan.rs:212` and `:214` — W5-B Details, bounded writer history/eviction; `07-live-monitoring.md` §4.2.

Every untracked inode receives growth score zero. Equal scores are ordered by `(dev, ino)` before truncating to 10,000 entries. With 20,000 continuously open writable files, the lowest 10,000 inode keys can therefore occupy the table forever, even if they never grow and a higher inode is the only growing file. The excluded file never receives a baseline, so its score remains zero on every subsequent pass.

An independent fake-source probe supplied inode keys 0–19,999 for four passes, keeping all allocations at 512 bytes except inode 19,999, which grew by 512 bytes per pass. `writers()` returned no growers. The unchanged inputs and tie rule also explain why additional passes cannot admit that inode. The candidate's cap test grows inode 9,999, which was already retained, and misses this case.

**Recommended change:** implement bounded admission of previously untracked keys, rotating a portion of the smallest-growth entries while allowing admitted keys at least two observations and retaining established high growers. Add a regression demonstrating that a growing file outside the initially retained key range eventually appears and that both history bounds remain intact.

Reference: `finding:w5b-writer-admission`.

### 2. SHOULD-FIX — Direct commands select a covered mount's device

`crates/tidemark-cli/src/cmd/phantom.rs:81` — `07-live-monitoring.md` §4.1, matching the watched filesystem's `st_dev`; W5-B direct-mode behavior. The helper is shared by `writers`.

`table.iter().find()` selects the first record with the requested mount point. Mountinfo can contain several records at the same path after an overmount; the first record can describe the covered filesystem. This silently scans the wrong device and reports zero phantom files or misses growing writers on the visible filesystem.

Reproduced without creating mounts: this sandbox's existing `/tmp` records list devices `0:57`, `0:251`, `0:251`, while the fixture's measured device is `0:251`. A deleted-open file held by two processes yielded zero matching holders through `phantom --direct --mount /tmp`. Calling the exact candidate's public `ProcScanner` with the measured device found both holders and their measured allocation. The existing `mount_for_path` helper explicitly makes later overmounts win ties (`crates/tidemark-fs/src/mount.rs:198`).

**Recommended change:** use `mount_for_path(&table, &raw).filter(|m| m.mount_point == raw)` after canonicalization, preserving mount-only validation and the existing overmount selection policy. Add a duplicate-target mountinfo regression for both commands' shared setup.

Reference: `finding:w5b-visible-mount`.

### 3. SHOULD-FIX — Reported writer window does not describe the measured growth

`crates/tidemark-live/src/procscan.rs:331`; direct caller at `crates/tidemark-cli/src/cmd/writers.rs:36` — W5-B window-growth semantics; `07-live-monitoring.md` §4.2 and §7a; `13-contract-decisions.md` SCR-002 positive-duration contract.

The selected baseline can precede the requested boundary or fall back to a much newer oldest sample, but `window_secs` always contains the requested duration. Independent probes confirmed that samples at seconds 0 and 10, with 1,024 bytes of growth and a requested 300-second window, report `growth: 1024, window_secs: 300`. A 500 ms requested window with positive growth reports `window_secs: 0`. The existing irregular-window test likewise measures seconds 10–30 for a requested 15-second window. Consumers calculating a rate from these fields get the wrong interval, potentially zero.

Direct mode compounds this: `--window 1h --interval 10s` only takes two observations roughly ten seconds apart, and scan time is added to the sleep interval. Retaining the requested label does not supply an hour of history. The handoff acknowledges this behavior but provides no accepted contract decision authorizing it.

**Recommended change:** make the growth and its reported duration refer to the same selected observations, including the actual elapsed sampling interval. Specify incomplete-history behavior explicitly rather than silently presenting it as full-window data. For subsecond/repeated timestamps, resolve the second-only representation through an SCR if necessary; do not emit positive measured growth with a zero measurement duration. Add assertions for the reported duration in short-history, irregular, subsecond, and direct-mode cases.

Reference: `finding:w5b-window-duration`.

### 4. SHOULD-FIX — Mount arguments bypass required realpath resolution

`crates/tidemark-cli/src/cmd/phantom.rs:80` — `08-interfaces.md` §1.1; `13-contract-decisions.md`, Query paths. The helper is shared by `writers`.

The original argument goes directly into `RawPath::from_path`, which performs lexical normalization, and then an exact mountinfo lookup. Both direct commands reject a valid symlink to a mounted filesystem. A temporary symlink resolving to `/` reproduced exit code 2 and “is not a mounted filesystem” for both commands. Relative arguments also fail the absolute-path requirement; symlink-containing `..` can be normalized to a different location before lookup.

**Recommended change:** canonicalize the original user-supplied mount path before constructing `RawPath`, propagate resolution failures, then resolve its mount record. Add symlink, relative-path, and symlink/`..` regressions. This is independent of finding 2: an already canonical `/tmp` still triggers the covered-device bug.

Reference: `finding:w5b-mount-realpath`.

## Checked behavior and scope

- Public scanner signatures and fields match W5-B; the source seam is private. The candidate adds no dependencies or shared module declarations. CLI flags retain the frozen surface; non-direct daemon-unavailable errors map to exit 4, and a zero direct interval maps to exit 2. Daemon integration is explicitly deferred to W6-A.
- Reviewed inode/device deduplication, aggregation of distinct process holders, exact trailing deleted-suffix removal, allocated-byte accounting through `st_blocks * 512`, device filtering, writable flags, component-wise exclusions, deterministic tie ordering, failure-state preservation, two-missed-pass expiration, and the 10,000-history/60-sample bounds.
- All five candidate scanner unit tests passed. A new probe for replacement at the same path with a new inode, shrinking allocation, and disappearance also passed. The three regression probes for findings 1 and 3 failed as described, against the exact candidate scanner and actual candidate-built dependency libraries.
- A nonprivileged fixture using only temporary files confirmed two holders of one deleted inode, each with measured allocation and the original invalid-UTF-8/control-byte path serialized as `path_b64`. A memfd on its distinct device was excluded. The public scanner was given the fixture's measured device because the CLI mount-selection bug prevents this fixture from reaching the right device. Synthetic tests establish the deduplicated total. Text escaping was checked in the render call sites; populated CLI text output was not established by this runtime fixture.
- No new unsafe code, monitored-file content reads, metadata mutation, depth-dependent recursion, or unbounded channels were introduced in the diff. Process enumeration and attribution use existing primitives; the procfs magic-link metadata dereference is explicitly required by `07` §4.1. Test file-content writes were confined to temporary fixtures.
- Both named privileged tests exist, are feature-gated without `#[ignore]`, compile, and compare measured `st_blocks` rather than filesystem-dependent allocation literals. The deleted-open test requires root for its cross-user fixture. The writable-growth mechanics are also suitable for a future nonprivileged fixture, although the current test asserts root.

## Outstanding dependencies and limits

The candidate's pending `docs/handoff/scr-W5-B/SCR-001.md` accurately identifies shared-interface gaps: the production source delegates to ungated procfs I/O with ordinary directory enumeration, and `OpenFile` lacks the enumeration starttime needed to match later attribution. Consequently Gate/O_NOATIME compliance and enumeration-to-attribution PID-reuse safety are not established. The cache validates its own PID/starttime reads, but that does not validate the earlier descriptor observation. These are reported as unresolved contract/dependency work, not silently treated as accepted exceptions or separate local fixes.

The unchanged W1-D helper silently skips `EACCES`/`EPERM`; neither its return type nor `ProcScanResult` carries permission-error counts. Thus the requested permission-count criterion is unmet by the current dependency contract. `ENOENT`/`ESRCH` handling and parser bounds were inspected, but no disappearing-process stress test, permission-denied runtime fixture, or malformed-procfs injection was performed here. The existing “reuse” unit test refreshes attribution between passes; it does not exercise the cross-stage race.

No privileged tests, mounts, LVM changes, service operations, VM connections, or database-specific review were performed. Rocky Linux/XFS runtime results, cross-user coverage, and the real 20,000-fd pass under 500 ms remain pending VM. The synthetic timing excludes procfs syscalls and cannot establish that acceptance item. Whole-workspace test completion and cargo-deny remain unestablished after CI's unrelated early failure.

## Build and test evidence

Used pinned Rust 1.98.0, a private copy of the host Cargo registry at `/tmp/tidemark-w5b-review-cargo`, `CARGO_NET_OFFLINE=true`, and a separate target directory. Commands ran in the exact candidate archive, with no source edits.

| Check | Observed result |
|---|---|
| `tools/ci.sh` | Exit 1: source checks (126 owner tags), format, and Clippy with `-D warnings` passed; workspace tests stopped at the pre-existing, out-of-diff `tidemark-cli/tests/w1a/main.rs::probe_json_has_all_sections_and_unknown_postgres` assertion. This is not a W5-B finding. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compiled only, no privileged execution. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0. |
| `cargo test --offline -p tidemark-live -p tidemark-cli --lib --test w5b` | Exit 0: 30 live library tests, 14 CLI library tests, and 1 CLI executable test passed; live privileged integration target contains zero tests without the feature. |
| Independent scanner harness | Compilation exit 0; 6 passed, 3 failed: original 5 tests and replacement/shrink/exit passed; short-window, subsecond-window, and writer-admission regressions failed. |
| Independent CLI/public-scanner fixture | Confirmed both alias failures and covered-device omission; correct-device scanner found two holders with byte-preserving JSON and excluded the memfd. Confirmation script exit 0. |
| Candidate integrity / diff whitespace | 315 archived candidate blobs unchanged; `git diff --check` passed. |

Supporting artifacts are under this attempt's output `library/`: `ci.log`, `privileged-build.log`, `doc.log`, `w5b-tests.log`, their exit files, `procscan-review-probes.rs` and its build/run logs, `cli-review-probes.py` and the initial failure log, `cli-review-probes-confirmed.py` and its confirmation log, `production-procscan-probe.rs`, and `candidate-integrity.json`. The initial real-fixture failure led to finding 2; it was not discarded. This report is review evidence and a recommendation, not a release or integration acknowledgment.
