# Handoff — RECHECK-life: recovery ownership, offline catalog guard, output collisions, firstboot clock

- **Date:** 2026-10-13
- **Attempt:** attempt-a7a84ba9a05b5c286bf65d4d3a4b61291a29d825d8e23d7bc875e47a31f3c36f
- **Status:** implementation completed; evidence candidate awaiting independent review

## Needs from coordinator

Run the listener-dependent regressions on an unrestricted runner and the existing privileged daemon/deployment suites in disposable guests. Local Unix socket binds are denied. The retained instructions warned of five readiness failures in `tidemarkd/tests/w6a1.rs`; these did not recur in this run, but listener-dependent tests explicitly skip denied binds. This handoff is an evidence candidate, not an acceptance decision.

## What changed

- **R9:** Startup probes the catalog read-only before deciding whether to wait. Corrupt or missing storage requires scan ownership, with a 30-second monotonic wait and the required `state directory in use by another tidemark process` error. Healthy storage does not wait for a busy scan lock; abandoned-scan cleanup runs only with ownership. `ScanLock::open_catalog` retains the guard across writable open/recovery. The store adds `recover_catalog_with_lock`, checking the lock descriptor's inode/device against `scan.lock` and confirming exclusive flock before renames. Legacy `recover_catalog` retains its public signature and acquires scan ownership itself. Legacy scan `open_catalog` also acquires ownership for writable opens. CLI recovery paths use the retained guard. All local catalog/sample connections must still be closed before recovery.
- **R10:** Snapshot mutations use the same eleven-second, definitive-absence-only connection helper as scan, before acquiring the scan lock and again under it. Indeterminate errors map to CLI unavailable/exit 4; a connected daemon refuses offline mutation. The guard spans the complete prune/rebuild operation.
- **R6:** The daemon's shared startup/reload/check-config validator compares every socket/socket.lock versus textfile/textfile.tmp pair, using lexical normalization and existing parent device/inode plus byte leaf name; sidecar names are derived from the leaf just as the API/exporter derive them. Parent inspection errors fail validation. Empty textfile disables the check. Distinct leaves in the same directory remain valid. Export publication checks the destination through the retained directory handle immediately before rename and refuses nonregular destinations, including symlinks and sockets.
- **R5:** Firstboot followup accepts independent wall and elapsed seconds. Only elapsed seconds throttle attempts; persisted written_at never delays the first attempt after restart. Wall timestamps remain report facts. Healthy same-hash reports retain finalization behavior.

## Regression evidence

- `tests::recovery_wait_preserves_busy_database_and_sidecars`: a separate test subprocess owns the real scan lock; an injected 60 ms startup recovery deadline returns the required message, retaining all three database/sidecar bytes and inodes and creating no recovery directory. After that process exits, the same startup storage helper recovers all three files. This discriminates the original unconditional pre-lock recovery.
- `tests::healthy_catalog_does_not_wait_for_scan_lock`: healthy storage returns promptly despite a retained lock and a 30-second recovery wait setting.
- Existing `startup_skips_cleanup_while_another_process_holds_lock` remains passing: running row and abandoned temporary survive the first start and are repaired by the next lock-free start.
- `checked_recovery_refuses_wrong_or_busy_scan_lock`: checked and compatibility recovery refuse busy/wrong descriptors without changing corruption; the held descriptor permits recovery.
- CLI `scan_invalid_endpoint_never_writes_state`, `scan_permission_error_never_falls_back`, and `scan_full_backlog_expires_without_direct_state_writes` now exercise prune and rebuild too. Permission coverage detects effective access before asserting refusal; full-backlog coverage detects denied binds, fills an explicit backlog of one and requires each CLI subprocess to finish in under fifteen seconds with exit 4 and no state creation.
- `catalog_mutations_preserve_existing_corrupt_state_on_uncertain_endpoint` checks exit 4, all original inodes/bytes and unchanged directory entries for prune and rebuild against ENOTDIR. This fails with the original fail-open connection check.
- New `w6a6::collisions_rejected_by_check_config_and_reload` rejects the three named configurations, symlink-parent aliases, lexical aliases and trailing-separator aliases; every rejected reload retains the original effective configuration and generation. Distinct leaves pass check-config. `exporter_preserves_nonregular_destination` protects a symlink target and directory; `exporter_preserves_live_socket` protects a listener inode on unrestricted hosts.
- `unhealthy_followup_throttles_resumes_and_finalizes` now restarts with wall time before the persisted report and independently advances elapsed time. `followup_uses_elapsed_time_across_wall_steps` steps wall time far forward/backward while elapsed 1/9 remain throttled and elapsed 10 finalizes immediately. Both fail with the original wall-clock/persisted-time gate.

## Shared-file edits and compatibility

No manifests, dependencies, core contracts, module declarations or other tasks' files changed. `tidemarkd/src/lib.rs` implements assigned lifecycle/validation changes. Existing public signatures remain unchanged; new store and ScanLock methods supply checked ownership, and only the crate-private firstboot poll signature changes. The crate-private CLI daemon connection helper is shared from scan without changing its behavior.

## Assumptions and deviations

- The implemented catalog is the scans table in `timeseries.db`; no separate live `catalog.db` exists in this baseline. Recovery therefore preserves timeseries.db and its WAL/SHM, matching the existing storage contract.
- A missing shared database also requires scan ownership before creation. A healthy database can open normally while a direct scan owns the lock, as explicitly requested.
- Existing compatibility recovery/open functions acquire their own scan lock. A caller already holding one must use the new guard-aware entry points; this avoids leaving an unchecked recovery route while preserving public signatures.
- No questions, branches, merges, pushes, privileged runtime tests, host service/storage commands or system-directory writes were performed. Builds use the pinned 1.98.0 toolchain, four jobs, two test threads and an offline temporary copy of the readable registry cache. No dependency was added.
- Export destination inspection is defense in depth, not an atomic defense against hostile same-uid pathname replacement between stat and rename; the static configured output collision is rejected before publication.

## Proposed plan text (not applied)

1. All shared-database creation/replacement/recovery and sidecar renames require scan ownership. Startup first probes read-only; corruption/missing storage waits at most thirty seconds for ownership, while healthy startup skips abandoned-scan cleanup on contention. Guard-aware recovery checks ownership itself.
2. Offline catalog mutation requires definitive socket absence/refusal both before and after scan ownership acquisition; all other transport errors/timeouts return unavailable without catalog mutation.
3. Reject intersections of socket/socket.lock and textfile/textfile.tmp, accounting for lexical aliases and existing parent identity; the exporter refuses nonregular final destinations.
4. Firstboot followup makes one immediate process-local reevaluation after restart and throttles subsequent attempts with an injected monotonic clock. Persisted wall timestamps are facts only.

## Validation

Transcripts are retained in the canonical attempt output `library/`. All long commands use `timeout 1500` and offline Cargo/cache settings.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0 (`final-fmt.log`) |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 (`final-clippy.log`) |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | Exit 0 (`final-clippy-privileged.log`) |
| `cargo test --offline --workspace --no-fail-fast` | Exit 0: **754 passed, 0 failed, 14 ignored**, 73 harness summaries (`final-workspace-test.log`) |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compiled only (`final-privileged-build.log`) |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | Exit 0: source policy, fmt, Clippy, tests and privileged compilation passed; cargo-deny explicitly skipped because unavailable (`final-ci.log`) |

Counts are harness totals, including subprocess/helper and empty doc-test summaries; conditional returns count as passing tests, not exercised listener coverage. `listener-regressions.log` records 17 denied-bind notices and passing summaries of CLI w6a3 (22), daemon w6a3 (13), w6a4 (5), and w6a6 (3). Permission-denied endpoint coverage did run (no permission-bypass notice).

Focused validation: store crash_recovery 5 passed, storage 32 passed/1 scale ignored, daemon w6a4 5 passed; scan library 41 passed/2 scale ignored; daemon library 68 passed in the final workspace pass; firstboot-only 8 passed; CLI w2c 10 passed/1 scale ignored; the new final trailing-separator aliases pass all 3 w6a6 tests. The trailing-separator amendment was made while the broad pass was in flight and then checked independently with w6a6, formatting and both full-workspace Clippy modes. Final CI and privileged compilation ran after that amendment; CI reports the same **754 passed, 0 failed, 14 ignored**, 73 summaries on the final source.

Initial build/check iterations included corrected missing time imports, a stdout-versus-stderr assertion and the fixture helper's test-only unwrap lint allowance. Their earlier transcripts remain alongside the final logs; final selected results above supersede them.

## Guest / unrestricted-runner commands

No new tests require privileged features. Listener checks need an unrestricted runner:

```sh
timeout 1500 cargo test --offline -p tidemark-cli --test w6a3 -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a6 --test w6a4 -- --nocapture
```

Existing privileged deployment/runtime acceptance remains **pending VM**; build-only validation is performed locally. In disposable Rocky guests:

```sh
timeout 1500 cargo xtask vm-test --host <rocky-8.8-vm>
timeout 1500 cargo xtask vm-test --host <rocky-8.10-vm>
```

Guest runs must establish real listener backlog behavior, service startup/restart, reload, and firstboot/deploy behavior. No local result establishes those gates, cargo-deny advisories, or performance/soak acceptance.

## Definition of Done evidence

- [x] Only owned paths changed; no dependency or module declaration additions.
- [x] Formatting and both Clippy modes pass with warnings denied; `tools/ci.sh` passes all available checks (cargo-deny unavailable).
- [x] Unprivileged workspace tests pass locally; conditional listener coverage is distinguished above.
- [x] Privileged targets compile; privileged runtime was not executed.
- [x] Public additions have rustdoc; owner headers remain present; no unsafe additions or production unwrap/expect calls.
- [x] All four findings have discriminating regression coverage and proposed plan text.
- [ ] Unrestricted listener / guest acceptance and independent review remain pending.
