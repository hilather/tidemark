**CHANGES REQUESTED — 2 BLOCKER, 3 SHOULD-FIX, 1 NIT.**

Reviewed candidate `17138d1536d3b6cf4a74bfd81ddf6aa0a98f3a6d` against base `ac6f7e810e4632d772bfbe7dccc2f9541a59f736`, limited to its diff. The supplied checkout started at `2c6d694fed5a678e963e6c24a8aa4d164d3abaee`; all W4-B implementation and test files are unchanged between that checkout and the candidate. Builds below used the supplied checkout, including its unrelated integration changes. No implementation files were modified.

### R1 — BLOCKER: ordinary unstage deletes through a symlink outside state_dir

**Location:** `crates/tidemark-cli/src/deploy/mod.rs:651` and `:652` (path-based removal at `:574`). **Reference:** W4-B unstage behavior; ADR-008; AGENTS.md rules 4 and 9; review requirement that deletion cannot escape the configured state directory.

The non-purge branch checks ancestors of `state_dir` only, then removes `state_dir/deploy/staged.json` by pathname. A symlink at the intervening `deploy` component is followed. With real temporary directories `state` and `outside`, `state/deploy -> outside`, and an unrelated `outside/staged.json`, calling `unstage(state, false, fake_systemctl)` returns `Ok(())` and deletes the outside file. No marker is required for this branch. This is reachable without `--purge-state` or `--yes`.

**Change:** open the state and deploy directories through no-follow directory handles, then unlink only the final record relative to the validated deploy handle. Reject a symlink or unsafe path before service mutation. Add a regression asserting the outside file survives ordinary unstage. The safe reproduction used only temporary files and a fake service manager: `library/unstage-probe.rs` / `unstage-probe.log`.

### R2 — BLOCKER: staging accepts the filesystem root as state_dir and chmods it

**Location:** `crates/tidemark-cli/src/deploy/mod.rs:350`, reaching `private_dir` at `:579`–`:583`; also `crates/tidemarkd/src/firstboot.rs:136`. **Reference:** ADR-008, ADR-016, AGENTS.md rule 9, and W4-B's requirement to validate before changing system state.

`[general] state_dir = '/'` passes configuration validation. No deployment check excludes it. After otherwise successful pre-flight, `stage` calls `private_dir("/")`, which sets its permissions to `0700`; production `RealEnv` also sets ownership. On a normally accessible root filesystem this removes traversal access for ordinary users and can disrupt the other services on the host. `firstboot::write` has the same unconditional state-directory chmod. Purge's root guard does not protect either writer.

**Evidence:** a fake `Env` supplied successful environmental probes and intercepted `private_dir` without executing it. The actual `stage` implementation reached that boundary with exactly `/`, after zero configuration issues. Neither `/` nor any host metadata was changed. The permission effect is established by the implementation of the intercepted operation, not by a destructive test.

**Change:** reject the filesystem root and paths that resolve to it before any deployment mutation; reject ambiguous parent components and symlink paths while validating the deployment destination. Apply the guard to firstboot publication as well. Return a blocking deployment precondition and test that no permission, file, or service operation occurs. Evidence: `library/stage-probe.rs` / `stage-probe.log`.

### R3 — SHOULD-FIX: the identical-config shortcut ignores a new start request

**Location:** `crates/tidemark-cli/src/deploy/mod.rs:299`–`:314`. **Reference:** W4-B `--start-now` behavior and no-reboot edge case; `08-interfaces.md` §5.

After staging without `--start-now`, staging the same config with `--start-now` returns the old record before inspecting the options or service state. The service remains stopped and the returned record still has `start_now: false`. The same shortcut reports success if the service has been disabled since staging, leaving it disabled. The existing idempotence test repeats only the original options and does not cover these cases.

**Change:** make the no-op condition include the requested service outcome. Honor a newly requested immediate start and repair missing enablement; preserve the no-op for an already satisfied identical request. Cover both changed options and changed service state with the fake service manager. Probe result: `active=false`, `returned_start_now=false`, calls only `["enable"]`; after manually disabling the fake service, a re-stage performs no calls and leaves it disabled (`library/stage-probe.log`).

### R4 — SHOULD-FIX: the stage adapter discards the global config destination

**Location:** `crates/tidemark-cli/src/cmd/deploy/stage.rs:20`–`:23`. **Reference:** `08-interfaces.md` §1.2 (`--from` and global `-c`), W4-B interfaces/acceptance; `docs/handoff/scr-W4-B/SCR-001.md` is still pending.

The command always selects `/etc/tidemark/tidemark.toml` as its installed destination and, without `--from`, as its source. Thus `tidemark -c /custom/tidemark.toml deploy stage --from new.toml` targets a different file from the contracted destination. The shared dispatcher also parses the installed config before stage runs: an invalid installed config prevents a valid replacement from reaching source pre-flight, exiting 2. A temporary-file CLI probe reproduced that early failure without any service calls. The pending SCR documents these gaps but is not an accepted contract change in `13-contract-decisions.md`.

**Change:** obtain the coordinated Context/dispatch change requested by SCR-001, pass the actual global config path to the adapter, and allow stage to validate its replacement source independently of a broken installed config. Add end-to-end destination and replacement tests using an injected deployment environment. The hardcoded destination is verified by code inspection; the safe early-validation reproduction is `library/cli-config-probe.log`.

### R5 — SHOULD-FIX: the installed-config read follows destination symlinks

**Location:** `crates/tidemark-cli/src/deploy/mod.rs:295`, `:354`–`:355`, and `:559`. **Reference:** W4-B config installation requirements (no following a symlink at the destination), ADR-018.

The rename itself replaces a final symlink safely, but the preceding read uses `File::open`. A destination symlink is followed and its target's bytes are copied into the backup. A temporary fixture pointing the installed pathname at an unrelated file successfully staged and produced `tidemark.toml.1234.bak` containing that unrelated file's bytes. The target itself remained intact. If the target already contains the matching config and a matching staging record exists, the early return can retain the symlink entirely.

**Change:** inspect/read an existing destination through the validated parent handle with `O_NOFOLLOW`, require a regular file, and reject symlinks before reading contents or making any change. Add differing-contents and identical-contents symlink tests that assert no target read/backup and no service mutation. Evidence: `library/stage-probe.log`.

### R6 — NIT: internal constants unnecessarily extend the public contract

**Location:** `crates/tidemark-cli/src/deploy/mod.rs:17` and `crates/tidemark-cli/src/client.rs:14`. **Reference:** AGENTS.md rule 2 and review scope/contracts criterion.

`MARKER` and `MAX_FRAME_BYTES` are new public constants, not task-declared public items or owner-defined placeholder types. Their current uses are confined to their owning modules and tests; exporting them unnecessarily commits callers to internal representation choices.

**Change:** make both constants private, or obtain an explicit contract decision if another task needs them. Keep the bounds documented in the implementation.

### Checks and contract comparison

Read AGENTS.md, the wave header and W4-B section, `08` §5 and relevant CLI/API contracts, `02` §10, runbook P3/P5, ADR-016/018/019, all of `13-contract-decisions.md`, the handoff and all three W4-B SCRs. Inspected the candidate's changed files and commit subjects without relying on the handoff's validation claims.

| `08` §5 item | Observed implementation |
| --- | --- |
| Installed binaries and unit; root | `RealEnv::installation` checks the binaries/unit/systemctl and writable destinations. There is **no uid gate**. This correctly follows ADR-018 over the obsolete “running as root” table entry. |
| Config parses and validates | Source load and `validate` precede side effects; rejected source produces a check table and exit 5. Installed-config dispatch limitation is R4. |
| Existing directory roots on local filesystems | Real probes check directory type and classify mountinfo types. Unknown types fail closed. SCR-003's exact allowlist policy remains undecided. |
| Sampler/watch entries are mount points | Both configured lists are checked by canonical path against mountinfo. |
| State free space | Both the 2 GiB threshold and configured percentage floor are checked; boundary unit tests pass. Unsafe destination root is R2. |
| Fanotify and mount namespace | Warning result, with disabled/skipped handling; no blocking uid or group check. Native capability behavior remains pending VM. |
| XFS v5 bulkstat on `/data` | Warning result, using the existing probe fact. |
| LVM visible | Warning result from the reporting collector; no new mutating LVM operation. |
| Previous deployment | Enabled/active information and differing-config backups are implemented. Identical-record shortcut is R3. |
| Stage/verify/unstage outcomes | Blocking stage checks map to 5; unhealthy verification maps to 6; missing purge confirmation/marker maps to 5. Other runtime errors remain errors. `ClientError::Unavailable::exit_code()` is 4. Actual missing-socket transport could not run under sandbox restrictions. |

Confirmed correct in inspected code and the tests/probes exercised:

- Blocking pre-flight failure precedes config/state writes and service mutations. Config temp files share the destination directory; descriptor permissions and root ownership are set before rename, with file and parent-directory fsync. Backups preserve differing bytes and raw filename bytes. R5 qualifies the destination-read safety.
- Injected enable failure leaves no installed config or staged record in the fresh fixture; verify reports unhealthy and retry repairs the deployment. Existing tests also exercise record-publication failure and previous-byte restoration. Compensation errors are aggregated rather than hidden. Full runtime transactionality is not established: SCR-002 remains pending, and newly created directories/metadata can survive failure.
- Purge checks marker/config equality, refuses `/`, parent components, symlink roots and symlink markers, confirms inactivity after stop, and walks descriptor-relative with no-follow operations and mount IDs. Its explicit stack is capped at 256 and entries are streamed. Root/dot-dot/symlink-root probes returned before fake service mutation; marker guards and replacement-during-stop tests passed. R1 concerns the separate non-purge path.
- Production systemctl uses `/usr/bin/systemctl`, fixed operation arguments and `tidemarkd`, no shell, finite query/action timeouts, and propagated failure. The timeout test uses a harmless sleeping child, not systemctl.
- Firstboot and verify require all four core components exactly once and `ok`; optional degraded/error components do not block. Hash mismatch, missing/unhealthy reports, API failure and polling timeout are tested. Firstboot publication is atomic and private; its final symlink replacement preserved an outside temporary target in an additional probe.
- Client tests cover bounded JSON lines, malformed/oversize frames, response version/id/envelope validation, connection poisoning and timeout behavior. Additional probes cover coalesced frames, slow response bytes and blocked request writes. The private transport probes compile an unchanged copy of client source with an appended probe wrapper; firstboot probes link the built library.
- No new production `unsafe`, `unwrap`/`expect`/panic, library `println!`, async runtime, dependency, group setup, or removed database-specific feature was found. Scanned regular-file contents are not read by this task. Purge directory descent uses `O_NOATIME` with EPERM fallback; scan Gate behavior belongs to the existing probe/scanner implementations.
- Task-listed test categories exist and assert behavior; no test asserts an external stub or filesystem-dependent allocation literal. All three privileged scenarios are feature-gated, without `#[ignore]`. New probes expose the missing regression cases described above.

### Build and test results

Used pinned Rust 1.98.0, a private copy of the populated host cache at `/tmp/tidemark-w4b-review-cargo`, `CARGO_NET_OFFLINE=true`, and `--offline` on direct Cargo invocations. Logs and probe sources are under this attempt's output directory:

`/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-e3f4d0e0d7e3f75e154528a8f55cc6d48fdc0573b26aa9e7e2179482853f4a5e/library/`

| Check | Result |
| --- | --- |
| `tools/ci.sh` | Failed at workspace Clippy because unrelated `tidemark-fs/tests/w1b/main.rs:15` omits `WalkConfig.max_record_bytes`, and `:599`/`:726` match `WalkError::Limit(String)` using string literals. Owner/read-only source checks and format passed before that failure. Workspace tests and cargo-deny were not reached. `review-ci.log` |
| `cargo build --workspace --features privileged --tests --offline` | Failed on the same unrelated W1-B test compilation errors. `review-privileged.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --offline` | Passed. `review-doc.log` |
| `cargo build -p tidemark-cli --features privileged --test w4b --offline` | Passed; compiles all three W4-B privileged scenarios. None executed. `review-w4b-privileged.log` |
| Focused unprivileged tests | Passed: deploy 18, client 9, firstboot 4, CLI `w4b` 3; 34 reported, zero failures/ignores. The client listener test reports EPERM and returns before its listener/connect assertions. `review-{deploy,client,firstboot,cli}-tests.log` |
| Focused lint | CLI library/binary/`w4b` target and daemon library/binary pass with `-D warnings`. Broader CLI `--all-targets` also encounters unrelated `tests/w2c/main.rs:807`/`:813` requiring missing `ScanEntry: Debug`. |
| Additional safe probes | Stage, ordinary unstage/purge guards, CLI replacement validation, and client/firstboot probes described above executed. All filesystem changes were in temporary directories; all service operations were fake. |

### Remaining uncertainty

No privileged tests, real service mutations, mounts, LVM changes, VM access or reboot were attempted. Real production ownership/capability behavior, mount-boundary runtime cases, W6 startup timing and journald delivery remain pending VM/integration. Actual Unix socket listener/connect operations are unavailable here (EPERM), though socketpairs work. Full-workspace CI is not clean for the unrelated reasons above. A concurrent-directory-rename probe was inconclusive and failed its own observation assertion (`purge-probe.log`); no finding is claimed from it, and concurrent namespace mutation safety is not established. The pending W4-B SCRs are unresolved requests, not approvals. This report and its validation artifacts are evidence candidates, not a release or verified task-success declaration.
