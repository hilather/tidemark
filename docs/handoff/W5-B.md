# Handoff — W5-B: Process scanner, phantom files and open writers

- **Branch:** hp-a294f9178bb2e0b75c263647d2f81910db6d3b1b21e11b6afe4b423f7906a359
- **Agent/session:** canonical attempt-a294f9178bb2e0b75c263647d2f81910db6d3b1b21e11b6afe4b423f7906a359
- **Date:** 2026-10-11
- **Status:** implementation delivered as evidence candidate; VM acceptance pending

## Needs from coordinator

- Decide `scr-W5-B/SCR-001.md`: frozen procfs/scanner interfaces lack Gate injection, no-atime enumeration, and the enumeration starttime needed to guard attribution against reuse between enumeration and cache lookup. These requirements cannot be completed in W5-B's owned paths without changing contracts.
- Existing `crates/tidemark-cli/tests/w1a/main.rs::probe_json_has_all_sections_and_unknown_postgres` expects a null PostgreSQL probe result. It fails with the current operational probe; this test is outside W5-B scope and was left unchanged.

## What was built

A private procfs-source trait feeds one `scan_open_files` pass into both detectors. Phantom totals deduplicate by device/inode and retain each process holder once, stripping only the terminal deleted suffix; output preserves raw path bytes through core JSON serialization. Writers use writable flags, component-aware exclusions, allocated-byte window deltas, deterministic growth/path ordering, and at most 10,000 histories with 60 timestamp/disk/size samples each. Entries absent for two successful passes expire; failed passes preserve the previous result.

Both CLI commands implement direct mode and schema-wrapped JSON/text output. Phantom reports totals and holders with the restart/HUP hint; writers performs two passes separated by the positive interval. Daemon mode reports unavailable with exit 4 until Wave 6.

## Shared-file edits

None. No dependencies, manifests, shared modules, frozen core contracts or preparation tests changed.

## Deviations from the spec

| Spec reference | Implementation choice | Reason | SCR? |
|---|---|---|---|
| AGENTS rules 6/11 | Delegate metadata I/O to existing frozen procfs helper | No Gate parameter/no-atime support and primitive is outside ownership | SCR-001 |
| 07 §4 attribution | Use cache lookup once per relevant PID per pass | OpenFile lacks starttime; cross-stage race remains | SCR-001 |
| Writer window baseline | Latest sample at or before boundary; oldest retained sample when full requested history unavailable | Two-pass direct mode and finite rings cannot supply older observations; no interpolation or invented data | No |
| take_samples backlog | Pending samples represent only the latest successful pass, drained once | Explicit bounded buffering; caller must drain every pass | No |
| Duplicate configured devices | First configured mount is the device's canonical label; CLI deduplicates devices | Frozen input has device identity but no per-fd mount identity | No |
| Subsecond intervals | Actual injected second timestamps may repeat; fractional windows round upward only for baseline selection | Do not invent future timestamps; frozen report precision is Unix seconds | No |
| Clock rollback | Reject decreasing scan timestamps without changing state | Ordered histories require nondecreasing timestamps | No |
| Memory estimate | Sample storage at cap is 14.4 MB, plus maps/paths/attribution/pending output | The plan's approximately 15 MB is not a strict total byte budget; variable-length paths add overhead | No |

## Acceptance criteria evidence

| Criterion | Evidence | Status |
|---|---|---|
| One VM pass over approximately 20k fds <500 ms | Real procfs timing requires a VM; synthetic `ring_cap_eviction_and_twenty_thousand_fds` records processing elapsed time | pending VM |
| Phantom deletion/holder attribution | Synthetic `dedup_holders_suffix_and_raw_paths`; VM `phantom_detects_deleted_open` compares allocation to measured child fd metadata | synthetic tested; VM pending |
| Writable growth/read-only ignored | Synthetic `windows_reuse_exit_and_readonly_exclusions`; VM `open_writer_growth` measures allocation change with child appender plus read-only holder | synthetic tested; VM pending |

## Tests

Task-owned synthetic tests, loaded through the implementation's private test module:

- `dedup_holders_suffix_and_raw_paths`: same inode across processes/fds, multiple holders, exact suffix, middle marker retained, foreign devices, raw-byte JSON, mount filtering, samples drained once.
- `windows_reuse_exit_and_readonly_exclusions`: allocated growth, fresh attribution between passes, transient disappearance, two-pass expiration, read-only descriptors, component exclusions.
- `failure_preserves_state_and_clock_order`: failed source pass and rejected rollback preserve state; saturating signed growth; zero window.
- `ring_cap_eviction_and_twenty_thousand_fds`: 20,000 fake fds, writer cap, growth priority, 60-sample ring and old-history truncation.
- `irregular_window_baseline_sort_limits_and_subsecond`: irregular timestamps, window baseline, descending growth, mount/limit filtering and repeated second timestamps.
- CLI `daemon_unavailable_exit_four` and `duration_and_interval_validation`: both command error types and positive window/interval contracts.

Privileged tests in `tests/w5b.rs` / `tests/w5b/privileged.rs`, gated only by feature, never ignored:

- `phantom_detects_deleted_open`: child holds an unlinked 100 MiB file; reports measured allocation and child's comm — **pending VM**.
- `open_writer_growth`: child appends to an open file while a separate read-only holder exists; reports measured growth and writable holder — **pending VM**.

Python 3 is required for the VM child fixtures. Children are killed/reaped on drop. No mounts, LVM, services or privileged operations ran on the host. No tests were ignored by this task.

## Validation

Commands used pinned Rust 1.98.0, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/tmp/tidemark-w5b-cargo`, `CARGO_NET_OFFLINE=true`; the temporary Cargo home contains a copy of the host registry. Evidence logs and exit files are in the attempt output `library/`. Final outcomes are appended below after the checks settle.

## Known gaps and follow-ups

- Real `/proc` performance, cross-user coverage, and both privileged tests need the VM gate.
- Daemon wiring is intentionally unavailable pending W6-A.
- Enumeration-to-attribution PID reuse remains unresolved; see SCR-001. Reuse within enumeration/cache reads is handled by the existing primitive/cache.
- Inode reuse between observations cannot be identified with the frozen `(dev,ino)` metadata contract.
- Direct writers has only two samples; a requested longer window uses the observed baseline, with `window_secs` retaining the requested window. Continuous daemon sampling is needed for a full long-window history.
- Phantom holders and source rows are bounded by the shared 1,048,576-fd cap; extreme large scans can exceed the writer-ring memory estimate.

## Notes for the reviewer

No unsafe code, regular-file content reads, new dependencies or writes to monitored paths. Unknown attribution uses the shared explicit `?` marker, never an empty replacement. Raw paths are kept as bytes; text output escapes controls, invalid bytes and backslashes. Growth arithmetic saturates in signed i64. Pass-local aggregation and writer selection are deterministic; retained history never exceeds the configured fixed task caps.

## Definition of Done

- Only owned paths changed; no shared-file edits.
- Public items documented and ownership headers present.
- VM tests written, with runtime evidence pending VM.
- Workspace validation status and unrelated failures recorded below.
- No independent verification, release approval or completed VM acceptance claimed.

## Final check evidence

| Command (all builds offline) | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | exit 0 | library/final-fmt.log, .exit |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | library/final-clippy.log, .exit |
| `cargo test --workspace` | exit 101: unrelated W1-A PostgreSQL probe assertion | library/test.log, .exit |
| `cargo build --workspace --features privileged --tests` | exit 0 in initial full run; final recompilation also recorded | library/privileged.log, final-privileged.log and .exit |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | exit 0 | library/doc.log, .exit |
| `tools/ci.sh` | exit 1 at same W1-A assertion; fmt/clippy passed inside CI | library/ci.log, .exit |
| Task scanner tests | 5 passed | library/procscan.log, .exit |
| CLI library tests | 14 passed | library/cli-unit.log, .exit |
| CLI executable direct modes | 1 passed, empty-device JSON output, absent state remains absent | library/cli-direct.log, .exit |

Final full live and CLI-focused reruns are recorded in library/final-live and library/final-cli logs/exit files. The additional CLI executable test is `direct_modes_json_without_daemon`; it deliberately uses no watched devices so its expected results do not depend on this machine's mounts or users. The deleted-open VM fixture drops its child to numeric uid 65534 after opening/unlinking, testing other-user procfs coverage without relying on a named host account.

Synthetic timing: 325.043475 ms for 20,000 fake descriptors in the focused debug test (earlier concurrent whole-library run: 342.868139 ms). Includes aggregation, attribution seam, cap selection and sample construction; excludes real procfs syscalls, filesystem metadata and VM scheduling. This does not establish the VM's <500 ms acceptance item. The 60-entry ring stores 24-byte timestamp/disk/size tuples: 14,400,000 bytes at the 10,000-writer cap, excluding strings, map metadata and other output/source storage.

Workspace test/CI failure was not suppressed or ignored. Advisory/license checking was not reached after the test failure in CI. There is no evidence of completed privileged runtime acceptance.

Final settled reruns: `cargo build --offline --workspace --features privileged --tests` exit 0; `cargo test --offline -p tidemark-live` exit 0; `cargo test --offline -p tidemark-cli --lib --test w5b` exit 0 (14 library tests and one executable test). Final fmt and Clippy exit 0. All task-owned changes remain within the permitted scope.
