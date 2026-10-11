**CHANGES REQUESTED — 1 BLOCKER, 9 SHOULD-FIX, 0 NIT.**

Reviewed the exact candidate `d0aa0f8e0d8c7949eb4d20af1cf42fc716cd7d6d` against base `42eb6c35dafd8a88f79622032f06bf326d04ef95`, limited to its changed files. All source locations below refer to that candidate, not the later deliverable checkout `940f1fd0ca4c55e4ce195c6c8c4fa3d0f9a52c78`. The candidate was exported under ignored `target/review-candidate` for builds and inspection; production files were not changed. Its history contains the W2-C implementation commit but no W2-integ commit. Candidate plan sections, contract decisions, handoff and both pending SCRs were read. Retained root-only and no-PostgreSQL scope decisions were applied.

This report is review evidence, not an integration or acceptance decision. In particular, passing default CI does not establish end-to-end scan correctness: this candidate still contains unchanged W2-A/B stubs.

1. **BLOCKER — scan memory budget does not bound legal walker records.** `crates/tidemark-scan/src/orchestrator.rs:598` (`finding:w2c-r2-01`). Reference: `04-scanner.md` §7; W2-C step 8; AGENTS rule 10.

   The capacity calculation reserves only 1 MiB per record beyond candidates and a fixed 64 MiB base. It never accounts for actual links, their names, or directory/link aggregation state. A probe of the exact function with a 128 MiB budget, one worker, zero top files and a 64 KiB getdents buffer accepts six queued records. `LinkRef` measures 64 bytes; the existing walker permits one million entries and stores a name for each hard link. A legal record with one million 255-byte link names therefore requires at least 319,000,000 bytes, excluding allocation overhead, exceeding the entire 134,217,728-byte budget before considering other records. This can exhaust memory beside the protected workload despite successful resource preflight. No memory-exhausting fixture was constructed; this evidence is measured layouts plus conservative arithmetic.

   **Change:** resolve SCR-002 and enforce record, producer and aggregate byte limits before allocating; derive channel capacity from those limits and the remaining budget. Return a typed resource error when exact accounting cannot fit, without truncating semantic data. The pending SCR documents this gap but does not waive the requirement. Evidence: `scan-review-probe.log` and its source/provenance artifacts.

2. **SHOULD-FIX — cancellation during snapshot writing can publish a complete snapshot.** `crates/tidemark-scan/src/orchestrator.rs:321`, `:389`, `:403` (`finding:w2c-r2-02`). Reference: `04-scanner.md` §8; W2-C cancellation test and steps 10–12.

   Cancellation is last examined when choosing `status`, before `prepared.write`, index creation and publication. `WriterSink` has no cancellation check, and `writer.finish` receives the earlier metadata. SIGINT/SIGTERM during a long write consequently still publishes `Complete`, including without `--keep-partial`, and can return success. This is established by control flow; an end-to-end write-phase probe cannot reach the writer through the candidate's aggregator stub.

   **Change:** check cancellation between row batches and at the publication boundary. Without keep-partial, drop the unpublished writer and finish the catalog row as failed; with it, finalize partial metadata. Add deterministic write-phase cancellation tests, including cancellation after the last batch but before publication.

3. **SHOULD-FIX — a failed completion recorder can leave the catalog row running.** `crates/tidemark-scan/src/orchestrator.rs:404`, `:418`, `:841` (`finding:w2c-r2-03`). Reference: W2-C lifecycle steps 6, 11–12 and required error cleanup.

   The in-memory entry becomes complete/partial before `recorder.scan_finished` succeeds. If that callback fails, `recorded` skips its failed finalization because the in-memory status is no longer `running`, even though the persisted start row can still be running. An injected recorder that accepts start, rejects the complete update and would accept failure receives exactly `start:running`, `finish:complete`; no failed update is attempted.

   **Change:** track acknowledged recorder completion separately from the proposed entry status. Attempt failed finalization for every unfinished post-start error, and preserve the original failure if cleanup also fails. Add recorder-error regressions, including the already-published-file case, with explicit recovery handling for that file. Evidence: `scan-review-probe.log`.

4. **SHOULD-FIX — pruning leaves references that break ordinary `top`.** `crates/tidemark-cli/src/cmd/snapshots.rs:153`, `crates/tidemark-cli/src/cmd/top.rs:130` (`finding:w2c-r2-04`). Reference: `05-storage.md` §4, §7; W2-C `top` comparison behavior. Automatic retention at `orchestrator.rs:432` has the same catalog/file mismatch.

   Pruning deletes the database but leaves its complete catalog row pointing to the removed file. `top` then selects and opens that row for optional Δ24h, even when growth sorting was not requested. Reproduction: publish yesterday's and today's snapshots, set recent/daily/weekly retention to zero, run `top` successfully, then prune. The newest file remains and prune succeeds, but plain `top` now exits 1 while opening yesterday's deleted file.

   **Change:** reconcile catalog availability when files are pruned, through an agreed store interface, and select comparisons from available snapshots. Missing optional Δ24h must not prevent displaying an intact selected snapshot; growth with no available comparison should produce its documented precondition error. Add prune-then-query coverage for both explicit and automatic retention. Evidence: `cli_probe.rs`, `cli_probe.log`.

5. **SHOULD-FIX — `rebuild-catalog --root` mutates unrelated roots.** `crates/tidemark-cli/src/cmd/snapshots.rs:175` (`finding:w2c-r2-05`). Reference: `08-interfaces.md` §1.2, snapshots `--root`.

   The command canonicalizes the root argument but discards it when calling global `rebuild_from_files`. A probe added a failed historical entry for a second root, then ran `rebuild-catalog --root first-root`. The command exited 0 and the second root's entry disappeared.

   **Change:** implement root-scoped rebuilding through an agreed store API, preserving other roots' catalog records. Until available, explicitly reject the unsupported scoped operation instead of silently performing a global rebuild. Test preservation of unrelated complete, failed and running records. Evidence: `cli_probe.log`.

6. **SHOULD-FIX — growth ordering silently drops candidates beyond the reader cap.** `crates/tidemark-cli/src/cmd/top.rs:66` (`finding:w2c-r2-06`). Reference: W2-C `top`; `08-interfaces.md` §1.2 and `13-contract-decisions.md` SCR-001/002 growth ordering.

   The command fetches exactly one million rows sorted by disk before calculating growth. `Snapshot::subtree_dirs` applies SQL LIMIT before checking its one-million-row bound, so this request cannot detect truncation. With one million large unchanged children and a smaller newly created child, the growing child falls outside the disk prefix and cannot rank first by growth. The subsequent filter/sort/limit operates on an incomplete candidate set. This is source-verified; no million-row runtime probe was run.

   **Change:** at minimum, request 1,000,001 candidates so the existing reader bound reports overflow, rather than returning an incorrect ranking. A scalable solution needs bounded selection over the full eligible set. Add a cap-boundary regression.

7. **SHOULD-FIX — emergency pruning ignores expired partial/failed files.** `crates/tidemark-scan/src/orchestrator.rs:803` (`finding:w2c-r2-07`). Reference: `05-storage.md` §7–8; W2-C step 4.

   Only complete entries are considered for space reclamation. An injected guard was configured to fit after removal of an expired partial snapshot. The function returned `NoSpace` and left the file intact, although no recent/latest complete snapshot needed deletion. A sufficiently large cancelled snapshot can therefore block subsequent scans until explicit manual pruning.

   **Change:** include expired partial/failed artifacts in emergency reclamation, retaining protection for recent and newest complete snapshots and checking space after deletions. Test both partial and failed artifacts. Evidence: `scan-review-probe.log`.

8. **SHOULD-FIX — fatal walk errors disregard `--keep-partial`.** `crates/tidemark-scan/src/orchestrator.rs:276` (`finding:w2c-r2-08`). Reference: `04-scanner.md` §8: cancelled **or failed** scans retain partial output when requested.

   Only `WalkError::Cancelled` is allowed to proceed to preparation with keep-partial. Every other fatal walk error discards the returned aggregator. For example, after earlier directories have supplied valid records, the walker's million-entry-directory limit returns `WalkError::Limit`; keep-partial still loses those available results. This is established by the collector's return contract and the unconditional error arm; successful partial publication is blocked by the unchanged aggregator stub in this candidate.

   **Change:** preserve and prepare valid collected records for eligible fatal walk failures when keep-partial is set, marking the output partial and returning exit 3. Define and test which failures make even partial output invalid, and propagate those explicitly.

9. **SHOULD-FIX — the required bulk lifecycle remains unimplemented.** `crates/tidemark-scan/src/orchestrator.rs:293` (`finding:w2c-r2-09`). Reference: W2-C steps 7–10; `04-scanner.md` §8–9.

   `run_bulk(...)?` propagates every error; there is no automatic walk fallback for a safe pre-phase-two failure, periodic bulk progress, or bulk partial recovery. The bulk result also hard-codes zero namespace noatime fallbacks at line 307. Thus the required behavior is still absent when an operational bulk dependency is supplied. The current frozen API lacks the phase/observation/partial-state information needed to implement this reliably; SCR-001 is pending, not an accepted exemption. These are source-verified interface gaps, not a claim to have run XFS bulk scans.

   **Change:** obtain the coordinated SCR decision, supply the agreed failure-phase, progress and recoverable-state interfaces, then implement fallback, cancellation/partial handling and accurate metadata with contract tests. Retain fail-closed behavior until the necessary distinction is available.

10. **SHOULD-FIX — invalid scan options use the wrong exit code.** `crates/tidemark-cli/src/cmd/scan.rs:137`; `crates/tidemark-scan/src/orchestrator.rs:166`, `:214` (`finding:w2c-r2-10`). Reference: `08-interfaces.md` §1.3: usage/configuration errors are exit 2.

    On a temporary directory, `scan --threads 0`, `scan --threads 65` and `scan --exclude '['` each returned exit 5 with their validation message. These are malformed user settings, not unavailable filesystem/kernel preconditions. Glob validation also happens after recording a scan start.

    **Change:** validate these CLI overrides before scan work, returning `CliError::Configuration`/clap usage errors. Preserve exit 5 for genuinely unmet scan preconditions. Add exact exit-code assertions. Evidence: `manual-cli.json` and `cli_invalid_args.log`.

**What checked out correctly**

- The declared scan request/outcome/recorder and mode signatures remain intact. Core types, schemas, manifests and dependencies were not changed by this diff. Additional public orchestration/rendering helpers were inspected; their public exposure is broader than the listed interface and should be confirmed during contract reconciliation.
- Mode selection checks XFS, v5, capability, ftype, the inclusive 50% coverage boundary and a wide-arithmetic memory estimate. Explicit walk bypasses bulk requirements; explicit unavailable bulk is converted to exit 5. No v1 fallback was introduced. The active truth-table test passes.
- Realpath precedes `RawPath` construction for CLI roots/query paths. Changed code preserves byte paths, escapes display paths and uses path/path_b64 JSON adapters. Tests cover non-UTF-8 output, symlink query resolution, signed growth ties and ASCII/Unicode rendering. Tree rendering is iterative.
- Scan locking is nonblocking, NOFOLLOW and released by descriptor drop. Root directory opens use NOATIME with EPERM fallback. Scan work receives the constructed gate, and its controller is stopped explicitly or by drop. No new unsafe code, scanned regular-file reads, mount/LVM changes or production unwrap/expect calls were found in the reviewed source. Filesystem traversal safety remains primarily delegated to the unchanged walker.
- Collection uses a bounded channel; cancellation drains records and scoped threads are joined. Annotations precede writing. Directory-series selection uses a heap capped at 20,000 with deterministic ties. This does not cure the separate byte-budget finding.
- Ordinary catalog opens remain non-destructive; explicit recovery is called only after a reported corruption, with the failed connection dropped, and logs preserved paths. Direct CLI recovery holds the host scan lock and checks daemon absence. Startup races with a future daemon were not exercised.
- SnapshotWriter delegates private 0600 files, exclusive creation, non-replacement publication and temporary-file removal to unchanged W1-E. ID selection/creation collisions are retried. Actual successful CLI publication remains unverified at this candidate. Finishing collisions safely fail rather than replace, but the required retry at that point is still pending SCR-002.

**Acceptance and test limits**

- Required test bodies exist for eight mandatory fixtures, CLI round-trip, SIGINT with/without partial, injected space refusal and privileged auto selection. The apparent-size oracle probes the local `du` behavior and normalizes measured metadata. No tests were found asserting another task must remain a stub or asserting filesystem-dependent allocation literals.
- Three W2-C acceptance tests are ignored as `needs W2-A`. I explicitly ran them: all fail at the unchanged aggregator's `NotYetImplemented`. This is a baseline dependency limitation, not a finding assigning W2-A implementation responsibility to W2-C. These tests need enabling and passing after integration; they are not VM-only work.
- The mandatory oracle at `crates/tidemark-scan/tests/w2c/main.rs:111` reads Snapshot rows directly instead of checking the required `top --depth 99 --json` output. CLI end-to-end assertions at `crates/tidemark-cli/tests/w2c/main.rs:442` check schemas rather than redacted insta snapshots of actual command results. The seven committed insta snapshots exercise synthetic renderer inputs. Close these coverage gaps when enabling acceptance tests.
- The active drain unit test starts with cancellation already set; it does not establish mid-scan blocked-producer draining. No active test covers cancellation during preparation/writing/indexing or failing recorder callbacks; the review probe covers the latter helper behavior.
- Privileged tests are feature-gated without ignore and compile. No privileged test, mount, VM, database-server connection or service operation was performed. XFS auto selection, bulk/walk equivalence, successful partial publication, successful live scans, snapshot modes after a real scan, RSS and performance remain unverified.
- A manual temporary tree contained two cross-directory hard links, a non-UTF-8 directory and a symlink query path. Walk scan returned exit 1 `NotYetImplemented`; the catalog recorded failed with no snapshot/temp file, and `top` returned exit 5. `du -sxB1` ran for the root and both hard-link subtrees, but there was no snapshot output to compare. No hard-link oracle success is claimed.
- Smaller metadata omissions remain visible in the source/handoff: `os_release` is empty, walk timing is overwritten with setup-inclusive elapsed time, and `finished_at` precedes writing. These were not independently exercised through successful scan publication.

**Build and execution evidence**

All Cargo work used pinned Rust 1.98.0, the host cache with `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_NET_OFFLINE=true`, and a separate ignored `target/review-build` target directory. The first CI attempt lacked RUSTUP_HOME and failed before formatting; after setting it, the following exact-candidate checks completed:

| Check | Observed result |
|---|---|
| `tools/ci.sh` in offline environment | Exit 0: source-policy checks, fmt, clippy and workspace tests pass; 195 passed, 0 failed, 6 ignored, including doc tests. Cargo-deny is absent and the existing CI script skips it. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compilation only. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0. |
| `cargo test --offline -p tidemark-cli --test w2c -- --ignored` | Exit 101: both round-trip and keep-partial tests fail at the dependency stub. |
| `cargo test --offline -p tidemark-scan --test w2c -- --ignored` | Exit 101: mandatory oracle fails with `Agg(NotYetImplemented)`. |
| Focused CLI probes | Prune/query and scoped rebuild failures reproduced; invalid argument exit codes reproduced. |
| Focused orchestration probes | Recorder failure and expired-partial refusal reproduced; memory layouts/accounting measured. |

Evidence artifacts are under `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-ce9c17172ccf352fbc4d599fd4a5bae68ae18789ce17db2fb18c50c920349d3b/library/`: `ci.log`, `privileged-build.log`, `rustdoc.log`, `ignored-cli-e2e.log`, `ignored-oracle.log`, `manual-cli.json`, `cli_probe.rs`, `cli_probe.log`, `cli_invalid_args.log`, and `scan-review-probe*` plus its harness sources. The orchestration harness preserves the exact candidate source as a byte-for-byte prefix and appends review-only accessors; its provenance and compile command are recorded. Probes used temporary/attempt-owned state, did not alter production source, and did not attempt to exhaust host memory.
