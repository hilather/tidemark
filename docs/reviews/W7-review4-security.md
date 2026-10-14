Reviewed checkout (`git rev-parse HEAD`): `e0a8c2e2f2fa02c1730fbf5e475e9007eb1909e1`.
Verdict: **CHANGES REQUIRED**.
Counts: **0 BLOCKER / 3 SHOULD-FIX / 1 NIT**.

Blind-review candidate: `27f662ddaa3f5c84887f146da0115e1c5e08aa3d`, base `976c800bb6ef6a160ea974521c1057a8dd03d029`, scope `candidate_tree`. All production source is identical between that exact candidate and this checkout. Eleven differing paths contain GUEST-fix3 tests, scripts and documentation. Local Cargo validation applies to the checkout, not an exact-candidate build. The candidate's four-file WATCH-fsmark-fix diff was inspected separately; all findings below exist in its tree. No authorship information was used. These are review proposals, not acceptance or release decisions.

## Third-review dispositions

The three security findings were reviewed first. Required handoff notes and both third-review reports were read as claims; dispositions rely on source and the discriminating checks below. Line numbers refer to this checkout and to the candidate's identical production files.

| Finding | Disposition | Evidence and discriminating check |
|---|---|---|
| R3-01 — mutable ancestor exceptions | **PARTLY** | `crates/tidemark-store/src/state_dir.rs:78,129,148` gates both exceptions; the socket's separate read-only exception uses the same helper at `crates/tidemarkd/src/api.rs:573`. Default release settings are strict. `state_dir::tests::policy_table` asserts both modes, including overflow/read-only/sticky/storage roles; all six StateDir unit tests executed and passed. The exact-function release probe demonstrates that an assertions-enabled native RPM can still be relaxed: R4-01. Namespace refusal tests returned before their central assertions here. |
| R3-02 — mutable purge descendants | **RESOLVED** for the reported descendant-provenance defect | `crates/tidemark-cli/src/deploy/purge.rs:103,143,197` checks every opened directory before constructing its reader: euid owner, no group/world writes, directory type and same mount. Attachment is checked immediately and before subsequent traversal at `:113,144,216`. Preflight runs before service stop (`deploy/mod.rs:695,701`). Both `purge_checks_unknown_and_nested_directories_before_stop` and `purge_after_open_rename_preserves_moved_contents` executed without uid/socket skips; writable modes are real, foreign owner is simulated. Both preserve fixture contents and prevent fake service calls. A separate live-writer omission is R4-02, not recurrence of unsafe-descendant admission. |
| R3-03 — unbounded retained watcher path bytes | **RESOLVED for the reported multi-GiB retained-path accumulation** | Shared admission in `watch/budget.rs:38` accounts payload plus container credits before admission. Dirty collapse is `watch/mod.rs:370`; ring copies/identities are `watch/rings.rs:409`; completed drains clear maps; FID path/handle copies use the same account. `byte_budget_falls_back_to_root_and_releases_completed_maps`, `emergency_root_headroom_preserves_sixty_undrained_minutes`, `injected_long_keys_cross_default_budget_with_exact_root_totals`, dirty recovery and FID scratch tests all actually asserted in the 51-test focused watcher run. This does **not** establish a literal 64 MiB peak across transferred caller-owned vectors, or whole-daemon RSS; see the memory qualification below. |

Strict ancestry does not reject root-owned real `/var/lib/tidemark`, or a root-owned separate filesystem mounted there: `trusted_directory` has owner/mode/no-follow checks, not a same-filesystem requirement. This is source inference, not a production mount test. A symbolic-link ancestor is deliberately refused by `O_DIRECTORY|O_NOFOLLOW`, even when it ultimately targets root-owned storage. Errors include the offending traversed path (`state_dir.rs:19,225`); ownership refusals also identify uid/mode and distinguish unmapped or foreign read-only owners (`:150`). A symlink failure may be described by the OS as “Not a directory,” rather than explicitly naming a symlink; operators should configure the real directory path. The actual-library `state-layout-probe.rs` exercised a temporary symlink ancestor and strict refusal of this sandbox’s unmapped `/` owner: both named the offending path, and neither created the requested state. No host layout was changed.

Deployment and metrics route ancestry through the shared checker (`deploy/files.rs:25`; `tidemarkd/src/metrics.rs:598`); purge descendants never receive ancestry exceptions. The exact `/proc/self/fd/<digits>/.` StateDir form remains a caller-vouched retained-descriptor escape from lexical ancestry (`state_dir.rs:200,256`), not a new overflow/read-only exemption. Daemon callers obtain that anchor after admission. A malicious root/same-euid caller supplying arbitrary descriptors is outside the root-only boundary. The operator guide still describes the old exceptions without the development qualification: R4-N1.

## New findings, most serious first

### R4-01 — SHOULD-FIX — Native RPM packaging can silently ship relaxed ancestry

Reference: `finding:w7-review4-security-r4-01`.

**Sites:** `crates/tidemark-store/src/state_dir.rs:129`; `xtask/src/rpm.rs:168,273`; `Cargo.toml:81`; `.github/workflows/ci.yml:110`.

`strict_ancestry()` uses `!cfg!(debug_assertions)` as its production security boundary. Native RPM builds invoke `cargo build --release` but inherit Cargo profile overrides and Rust flags. `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true` or `RUSTFLAGS=-Cdebug-assertions=yes` therefore produces binaries under `target/release` with both unsafe ancestry exceptions enabled unless the installed process also receives the runtime strict variable. The version's `profile release` label does not detect this. The tracked default profile and current workflow settings are strict; no evidence says an existing RPM is relaxed.

**Concrete scenario and starting privileges:** a legitimate package builder enables assertions globally for diagnostic release builds and runs `cargo xtask rpm --native --require-clean`. A root administrator installs that RPM and uses state under a host user's parent through the namespace/read-only-alias layout described in R3-01. That ordinary host user still has rename authority over the accepted parent. The SQLite pathname reopen race from R3-01 is consequently reachable again. Builder compromise is not required; the build setting itself is the trigger. The hostile-host race remains inferred, not executed here.

**Executed discriminator:** `library/release-ancestry-probe.py` copies the exact production helper into a dependency-free temporary Cargo project. Ordinary `--release` reports strict=true; each override separately reports strict=false; runtime `TIDEMARK_STRICT_ANCESTRY=1` restores true. No full RPM was built. The source proves native packaging preserves these inputs.

**Exact recommended change/files:** introduce an explicit always-strict store feature for packaged builds, independent of `debug_assertions`, and enable it in both native and container RPM build commands. Files: `crates/tidemark-store/Cargo.toml`, `crates/tidemark-store/src/state_dir.rs`, `xtask/src/rpm.rs`, `xtask/tests/w6c.rs`, `.github/workflows/ci.yml`. Add a packaging regression with assertions enabled and runtime strict variable absent, and an installed-binary ancestry refusal assertion. Setting the current strict variable only while compiling is insufficient: the existing helper reads it at runtime. Development builds may keep the specified warning-bearing exceptions.

### R4-02 — SHOULD-FIX — Purge deletes live database and lease inodes

Reference: `finding:w7-review4-security-r4-02`.

**Sites:** `crates/tidemark-cli/src/deploy/mod.rs:681,701,708`; `crates/tidemark-cli/src/deploy/purge.rs:157,181`; `crates/tidemark-store/src/lease.rs:22`.

`unstage(..., purge_state=true)` stops and checks only the configured systemd service, then recursively unlinks all state without writer or scan exclusion. This includes `timeseries.db`, its companions and `writer.lock`; the state directory itself is removed. A direct CLI writer is independent of that service. Once the lock pathname is deleted, another opener can create a different lock inode and obtain exclusive ownership while old writer connections still exist.

**Concrete scenario and starting privileges:** one root administrator runs a direct scan while another invokes `tidemark deploy unstage --purge-state --yes` against the same configured state. Service stop succeeds, but the direct scan's writable Catalog remains live. Purge removes its storage and exclusion namespace. This is an operational integrity defect requiring authorized root operations; it is not an unprivileged privilege-escalation claim.

**Executed discriminator:** `library/purge-live-writer.rs` links the actual CLI/store libraries, holds real Catalog and TimeSeries writers, and confirms an exclusive RecoveryLease initially refuses them. Production `unstage` with a fake Systemctl then succeeds and removes their private temporary state. A new RecoveryLease succeeds on a different `writer.lock` inode while the original Catalog remains callable. Every assertion executed here; no host service action occurred. This proves exclusion bypass, not an end-to-end lost acknowledged production transaction.

**Exact recommended change/files:** after stopping the daemon, acquire bounded exclusive writer ownership, then scan ownership; on contention retain all state and report the busy writer. Preserve the documented state-directory removal behavior with a coordinated finalization phase: exclude `writer.lock` and `scan.lock` from recursive data deletion, unlink them only after all destructive traversal is finished (writer lock last), then attempt only the final directory removal. If final removal encounters recreated entries or a changed root identity, report contention without retrying destructive traversal. After successful flock, every lease acquisition must revalidate held versus named state-directory and lock identity, refusing obsolete-inode waiters. Merely taking a lease and then deleting `writer.lock` during traversal is insufficient. Prove the finalization protocol with concurrent openers before adoption; retaining permanent sentinels instead would require an explicit contract/help change because `cmd/deploy/unstage.rs:7` promises removal of `state_dir`. Do not introduce ad hoc coordination writes outside the authorized state directory. Files: `crates/tidemark-cli/src/deploy/mod.rs`, `crates/tidemark-cli/src/deploy/purge.rs`, `crates/tidemark-cli/src/deploy/tests.rs`, `crates/tidemark-store/src/lease.rs`, `crates/tidemark-store/tests/crash_recovery.rs`. Add live-writer byte/inode preservation, idle purge, and waiting-opener versus purge/recreation regressions. The existing directory trust checks must remain.

### R4-03 — SHOULD-FIX — Unmapped watcher events remain complete-looking directory data

Reference: `finding:w7-review4-security-r4-03`.

**Sites:** `crates/tidemark-live/src/watch/mod.rs:1242,1324,1353`; `crates/tidemark-live/src/watch/rings.rs:507`; `crates/tidemarkd/src/services/watcher.rs:263,417`.

Failed provenance/path mapping substitutes the mount root and increments a lifetime `unmapped_events` count, but does not mark the affected minute's attribution incomplete. The component health predicate ignores that count; hot rows, completed minutes and persisted rows remain nonlossy. Reopen error text is visible in detail, which is useful, but the component still says `ok`. Minute-history consumers use the unchanged loss/truncation flags to decide whether directory windows are available. This differs from memory fallback, which correctly marks incomplete data and degrades health.

**Concrete scenario and starting privileges:** an ordinary writer operates inside an administrator-provisioned private/bind namespace on the watched filesystem, writes a new file, and unlinks it before queued handle reopening. If reopening returns ESTALE, the known error path places its activity at the mount root while publishing complete-looking directory measurements. A system-wide EBADF/EPERM reopening failure produces the same classification without any attacker. The filesystem/kernel timing needed to induce ESTALE is inferred; it was not reproduced in this sandbox. Totals and conservative dirty invalidation survive, so this is loss of attribution/quality signaling, not invisibility of every event.

**Executed discriminator:** `library/watch_probe/` copies the production watcher source and appends a caller of the existing private injection seam; its main uses the unchanged daemon component helper. Injected EBADF and ESTALE both produced `unmapped=1`, `lossy_minutes=0`, `component=ok`, a root row with `lossy=false`, and a completed minute with `lossy=false`, `truncated=false`, with three nonlossy persisted rows. A successful-reopen control maps the real directory. Dirty child-root requests remain conservatively invalidated. All assertions executed without fanotify, namespace or socket support. This is production-source aggregation evidence, not a full daemon/kernel test.

**Exact recommended change/files:** pass the event time into mapping-failure handling and record a bounded, expiring mapping-loss reason for every failure, including missing provenance. Preserve event/signed-byte totals, but mark directory facts incomplete so component health is degraded and reliable directory windows are unavailable for that interval. Do not use the lifetime count to make degradation permanent. Files: `crates/tidemark-live/src/watch/mod.rs`, `crates/tidemark-live/src/watch/rings.rs`, `crates/tidemarkd/src/services/watcher.rs`. Extend `injected_foreign_reopen_maps_or_reports_badfd` with failure stats/hot/completed/persisted assertions, missing-provenance coverage, successful control, expiry and daemon-window assertions.

### R4-N1 — NIT — Operator state-layout guidance still promises production exceptions

Reference: `finding:w7-review4-security-r4-n1`.

**Site:** `docs/operator-guide.md:64`.

The operator-facing paragraph still permits a trusted read-only foreign ancestor and an overflow-owner user-namespace exception without saying these are development-only. A root operator following it with the standard shipped release gets refusal. No attacker privileges are required; this is inaccurate deployment guidance, not plan drift.

**Change:** edit only `docs/operator-guide.md` to require root/euid-owned real ancestors in production, explain the two warning-bearing development exceptions and runtime strict switch, and direct symlinked-layout users to a real path. Validate the statements against strict/development policy-table cases and the packaged-binary refusal test. `docs/security.md` and plan drift were deliberately excluded from this finding.

## Writer lease and lock-order assessment

Apart from purge, inspected database replacement paths now require exclusive writer absence. Catalog and TimeSeries writable opens converge on `timeseries::open_writer_connection` (`catalog.rs:122`; `timeseries.rs:34,214`). Their Connection fields precede the lease fields, so SQLite closes before shared ownership drops. Initialization is probed before taking shared ownership; the temporary exclusive connection closes before the exclusive lease is released (`timeseries.rs:39,53,57`). Creation, migration and `.bak-v0` creation require and verify exclusive authority (`:96,126,141`). No database restore entry point was found. Healthy catalog rebuilding uses SQL transactions rather than replacing the database inode.

The recovery rename loop (`catalog.rs:528`) is reachable through lease verification and scan-descriptor identity/exclusion checks (`:428`). It preserves database, WAL, SHM, journal and backup companions; its temporary recovery Catalog closes before returning. Normal daemon startup, scans and offline mutations acquire writer ownership before scan ownership:

| Path | Order and consequence |
|---|---|
| Direct scan | `orchestrator.rs:123` opens Catalog before ScanLock at `:124`; CLI also retains shared ownership before scan preflight (`cmd/scan.rs:161`). |
| Daemon recovery / two corrupt starts | Exclusive lease at `tidemarkd/src/lib.rs:1433`, scan lock at `:1446`, recheck at `:1468`; close temporary connection/release guards before ordinary open at `:1483`. The second start rechecks the winner's database. |
| Offline preparation | CLI `cmd/scan.rs:377` takes exclusive writer then scan at `:381`, with definitive daemon-absence checks. |
| Prune/rebuild | Writable Catalog at `cmd/snapshots.rs:76`, ScanLock at `:83`; recheck daemon absence under ownership. |
| Legacy guard-taking API | `ScanLock::open_catalog` can call `recover_catalog_with_lock`, which takes exclusive writer ownership **after** the caller's scan lock (`catalog.rs:414`). Its two-second timeout prevents an indefinite lock cycle but can self-refuse if the caller already holds a shared writer. This is not literally universal lease-first ordering. No current normal recovery path relies on upgrading its own live shared connection. |

Shared lease retry is two seconds; startup/offline exclusive retry is bounded by its supplied duration, normally thirty seconds, polling nonblocking flock every 25 ms (`lease.rs:22`). Continuous new readers of the shared lease can starve an exclusive attempt; there is no fairness guarantee. The attempt ends busy rather than moving live storage. These are contention bounds, not a promise that disk metadata/SQLite/kernel I/O always completes within the deadline.

`writer.lock` creation is 0600 through the trusted no-follow, nonblocking, close-on-exec leaf opener (`lease.rs:24`; `state_dir.rs:369`). Default state 0700 plus lock 0600 prevents an ordinary user opening it. Generic store policy does accept preexisting readable 0644 locks and readable/searchable custom state; an ordinary user with a readable lock fd can take flock even without a writable fd. Marked state normalized later to 0700 does not revoke previously opened descriptors. This is a conditional custom-layout limitation, not a default-production exploit, and is not counted as a separate finding. Do not present merely forbidding group/world write on a lock file as access exclusion.

Flock failure other than contention/EINTR propagates; no unsupported-filesystem unlocked fallback exists. CLOEXEC releases inherited fds at successful exec of LVM/service children. Fork temporarily shares the open file description; crash releases ownership when its last inherited duplicate closes. These are source plus documented Linux semantics, not separately executed fork/crash/unsupported-filesystem tests. [Linux flock documentation](https://man7.org/linux/man-pages/man2/flock.2.html).

## Watcher memory, lifetime and handle reopening

The byte account uses checked arithmetic and a shared mutex, reserves normal storage below 63 MiB, and leaves 1 MiB emergency root headroom within 64 MiB (`budget.rs:38`). Rings charge map keys, row keys, units, seen keys and conservative nodes; dirty paths, acknowledgments, FID caches and retained views share it. Completed maps are destroyed when persisted rows drain. Cache evictions destroy owned payload before returning credits. Root fallback for byte/count exhaustion preserves directory event and signed-byte totals, marks loss/truncation with reasons, and dirty collapse clears old parents before later admission. Focused tests exercised default-budget exhaustion and sixty undrained minutes; they are not measured RSS tests.

**Peak qualification:** exported vectors lose their reservations on method return (`rings.rs:852` and `mod.rs:925`). The daemon holds a drained-minute vector and accumulates persisted rows before history admission/enqueue (`services/watcher.rs:221,233`), while the worker can refill its budget. Thus 64 MiB is the shared retained/admitted account, not a strict ceiling including every caller-owned transfer and temporary. The handoff's default 236 MiB subsystem estimate does not account for that overlap and is not verified. The transfer is bounded by one admitted drain and the service loop; no recurrence of the previous millions-of-long-keys accumulation was demonstrated. Configuration paths, numeric inode-size history, bounded event scratch, API responses and allocator overhead also require their own allowance. History has an additional 8 MiB cap and the bounded persistence queue uses 64 KiB chunks. No whole-daemon 2 GiB or arbitrary-configuration proof is claimed.

Count caps remain: 16–256 event descriptors per FD read, 64 KiB FID read buffer, 128-byte handle storage, bounded numeric caches, sixty minutes and finite view/dirty/identity caps. An ordinary writer can keep the single watcher worker busy at event rate and cause documented cap/queue degradation; there is no promise of attacker-independent CPU cost. I found no unbounded input-proportional retained descriptor/path structure in these paths. Production CPUQuota and memory limits do not replace guest soak measurements.

Shutdown holds its join mutex across signaling, joining, signal/mount descriptor release and retained-state reclamation (`watch/mod.rs:1092`). Concurrent shutdown callers cannot return before the first teardown completes. Worker processing does not acquire that mutex or the global daemon state lock. Daemon retirement invokes shutdown before replacement, independently of old reader Arcs; error cleanup also shuts down (`services/watcher.rs:243`). OwnedFd/RAII covers failed startup, abandoned batch entries and worker exit. A kernel metadata operation can still stall a join; no hard teardown deadline is proved.

For fd/filesystem mode, the real directory replacement uses RDONLY|DIRECTORY|NOFOLLOW|NOATIME|CLOEXEC, retries only EPERM without directory NOATIME, and checks mount ID plus device against the marked descriptor (`watch/mod.rs:607`). Other modes retain O_PATH. The target handle first uses O_PATH|NOFOLLOW|CLOEXEC; EINVAL alone retries RDONLY|NONBLOCK|NOFOLLOW|NOATIME|CLOEXEC, without any target NOATIME relaxation (`tidemark-sys/src/fanotify.rs:219`). No regular-file content read occurs on either target fd. Metadata comes from the original event fd; the reopened fd supplies a procfs path and then drops. The mount fd anchors interpretation to the watched filesystem; handles are kernel-produced, not user-supplied pathname traversal. Symlink handles are not dereferenced by open_by_handle_at. These flag/handle semantics were checked against the [Linux manual](https://man7.org/linux/man-pages/man2/open_by_handle_at.2.html); actual EL8 behavior remains pending VM.

O_NONBLOCK does not turn regular filesystem metadata I/O into a hard nonblocking operation. The real mount directory fd is not exposed through a public descriptor-return API and is CLOEXEC; it pins the mount until explicit teardown, as the prior O_PATH fd also did. No new direct unprivileged descriptor access was found. Normal unmount, target/directory atime, readonly-target behavior and both Rocky kernels must still be tested.

## Build identity and remaining changed boundaries

Both `build.rs` files are byte-identical. Identity strings become environment values; CR/LF are replaced before emitting Cargo directives (`build.rs:74`). Native commands use `Command::env`, and container identity uses separate `--env` arguments (`xtask/src/rpm.rs`), not shell interpolation. The exact build-script probe in `library/build-identity-probe.py` supplied shell syntax and attempted a newline Cargo directive: neither executed/injected. Arbitrary builder-controlled strings are still not authenticated provenance.

Git fallbacks are eagerly evaluated even when explicit identity is supplied (`build.rs:63`). A controlled inherited `core.fsmonitor` command **did execute** in the probe during `git status`. This requires control of builder Git configuration/environment; a committed-file-only exploit was not established, and such a builder already controls Rust compiler wrappers and build-script execution. Therefore this is recorded as build-boundary behavior, not another privilege-escalation finding. Avoiding unnecessary fallback calls and disabling fsmonitor/optional Git writes would reduce surprise. No hooks were installed in the reviewed repository.

Additional changed-boundary inspection covered CLI scan/snapshot admission, daemon startup error mapping, watcher component publication, purge traversal and terminal-render/build-version changes. No additional verified security finding was established. The sampler/rate/test-synchronization fixes were not given a second full correctness audit; their regressions run through CI. GUEST-fix3's later uid delegation and JSON script matching are not production fixes and do not strengthen the exact candidate's guest acceptance. No fault/install script or real service/LVM command was executed here.

## Validation performed

All Cargo commands used the pinned Rust 1.98.0 compiler, an attempt-local copied offline cache, four build jobs, two test threads, explicit umask 022 and `timeout 1500`. `CARGO_NET_OFFLINE=true` also applied to nested Cargo commands in `tools/ci.sh`; no dependency fetch was attempted. The sandbox reports uid/euid 0, with uid map `0 0 1`, but lacks ordinary host-root DAC/namespace/socket capabilities.

| Check | Result and limits |
|---|---|
| `timeout 1500 tools/ci.sh` | Exit 0: source policy, fmt, clippy with warnings denied, workspace tests and privileged-target compilation passed. Test summaries report 899 passes, 0 failures and 14 ignored cases (including nested helpers and guarded returns). cargo-deny was absent and explicitly skipped; this is not a dependency audit. |
| `timeout 1500 cargo build --offline --workspace --features privileged --tests` | Exit 0; all privileged test targets compile. No privileged test was executed. |
| `cargo test --offline -p tidemark-store --lib state_dir::tests -- --nocapture` | 6 passed; all policy assertions executed, with no capability guard. |
| `cargo test --offline -p tidemark-cli --lib purge_ -- --nocapture` | 5 passed; real mode/rename/filesystem assertions and fake Systemctl calls executed; foreign uid was simulated. |
| `cargo test --offline -p tidemark-live --lib watch:: -- --nocapture` | 51 passed; budget, aggregation, FID scratch and concurrent lifecycle assertions executed. These require neither live fanotify nor namespace setup. |
| `cargo test --offline -p tidemark-cli --test rules -- --nocapture` | 3 reported passed, but 2 namespace tests returned before central assertions because `/proc/self/uid_map` setup was denied. `regular_contents_are_never_read` really exercised contents-read/atime preconditions, mode-000 refusal, scan and unchanged-atime assertions. |
| Exact `binary_ready_and_stopping --exact --nocapture` on the built daemon integration test | Reported pass after the socket guard returned; its readiness/shutdown body **did not execute**. This confirms why ordinary CI names cannot establish listener/notification coverage here. |
| Release-policy, build-identity, live-purge, state-layout and injected watcher probes | All final probe assertions executed; see each finding for actual-library versus copied-source scope. |
| Proposed guest shell blocks | Syntax checked with `bash -n` only; none executed. |
| `timeout 1500 cargo clean --offline` | Exit 0; removed 31,446 generated files (19.8 GiB) before submission. |

The workspace run exercised the lease/recovery assertions in `recovery_excludes_live_writer_and_preserves_artifacts`, `every_writer_holds_lease_and_readers_do_not`, `writer_lock_obeys_verified_leaf_rules`, daemon `startup_refuses_pre_api_writer`, `racing_corrupt_starts_recover_once`, `healthy_catalog_does_not_wait_for_scan_lock`, and `startup_initializes_empty_database_without_self_conflicting_lease`. These bodies have no namespace/listener early return; their temporary-file, process and database assertions ran here. CLI `rebuild_refuses_pre_api_writer_without_renaming` also executed its child-process contention assertions. The concurrent watcher shutdown and retained-view retirement assertions ran using injected transports. Socket-dependent API/lifecycle tests and namespace integration tests are not relied on for their skipped bodies.

A separate permission probe confirmed mode-000 file and directory reads are denied to this sandbox’s uid 0. Therefore the metadata-only test executed its original permission-enforced body; the later GUEST-fix3 branch that delegates a capability-rich root runner to a second uid was **not** exercised. The source comparison records that GUEST-fix3 test change as absent from the blind candidate. Policy tests, simulated owners and strict unmapped-root refusal do not substitute for the second-user namespace exploit gate.

Logs and probe sources are retained under the attempt output’s `library/`, including `ci.log`, `privileged-build.log`, `ancestry-tests.log`, `purge-tests.log`, `watch-tests.log`, `rules-tests.log`, `socket-guard.log`, `environment.log`, and the finding-specific probes. The CI summary includes subprocess helpers and guard-return “passes”; it must not be read as that many independently exercised security scenarios.

## For the VM run

Run only inside disposable prepared Rocky 8.8 and 8.10 guests, as root with the pinned toolchain and cached dependencies. Record kernel, selected fanotify mode, uid, umask and all skips. These commands were **not executed here**.

```sh
export CARGO_NET_OFFLINE=true TIDEMARK_DEPLOY_TEST_VM=1
umask 022
timeout 1500 cargo test --offline -p tidemark-store --test crash_recovery -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --lib -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemark-cli --test w6a3 -- --nocapture
timeout 1500 cargo test --offline -p tidemark-cli --features privileged --test e2e scan_lifecycle::strict_ancestry_refuses_second_uid_before_writes -- --exact --nocapture
timeout 1500 cargo test --offline -p tidemark-store --features privileged --test state_trust -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-cli --features privileged --lib real_foreign_owner_marker_cannot_authorize_root_stage_or_purge -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a1 --test w6a3 --test w6a4 --test w6a6 --test w6a2s --test w6b -- --nocapture --test-threads=2
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a privileged::filesystem_mark_maps_namespace_bind_and_chroot -- --exact --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a privileged::shutdown_releases_mount_for_ordinary_unmount_while_old_reader_survives -- --exact --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a watcher_sixty_second_load_and_ten_minute_fd_stability -- --ignored --nocapture --test-threads=1
timeout 1500 bash tests/fault/03-fanotify-overflow.sh
TIDEMARK_FAULT_SLOW=1 timeout 1500 bash tests/fault/03-fanotify-overflow.sh
```

Require filesystem-mode attribution on supported 8.10; an unsupported-mode skip on 8.8 establishes only the fallback. Re-run the prepared eight-writer namespace/bind/chroot fixture, checking each real directory and unit rather than accepting mount-root totals. Add an induced handle-reopen failure and require both visible cause and degraded/incomplete directory data after R4-03. Check exact event/byte totals, normal unmount with an old view held, descriptor stability and inode atime under the guest mount policies; metadata syscalls can still block on underlying storage. Measure transfer-overlap and full-daemon RSS during paced long-path churn rather than treating the shared byte-account number as peak RSS.

Run the new purge concurrency regressions after R4-02 is implemented: real direct writer plus service stop, busy refusal preserving DB/sidecar/lock bytes and inodes, idle purge, and a waiter already holding the old lock fd during purge/reinitialization. Existing recovery tests do not cover that namespace transition. Do not invent an already-existing test name for this missing case.

Test the installed RPM with the runtime strict variable absent. The following guest-only fixture creates an unmapped-owner ancestor; it does not write any production state:

```sh
set -eu
fixture=$(mktemp -d /tmp/tidemark-r4-package-XXXXXX)
trap 'rm -rf -- "$fixture"' EXIT
chmod 1777 "$fixture"
setpriv --reuid 65533 --regid 65533 --clear-groups mkdir -m 755 "$fixture/foreign"
mkdir -m 755 "$fixture/root"
printf '[general]\nsocket = "%s/absent-api.sock"\n' "$fixture" > "$fixture/config.toml"
set +e
env -u TIDEMARK_STRICT_ANCESTRY timeout 1500 unshare --user --map-root-user /usr/bin/tidemark \
  --config "$fixture/config.toml" --state-dir "$fixture/foreign/state" \
  scan "$fixture/root" > "$fixture/stdout" 2> "$fixture/stderr"
code=$?
set -e
test "$code" = 1
grep -F 'owner 65534 is not mapped in this user namespace' "$fixture/stderr"
test ! -e "$fixture/foreign/state"
```

Run that assertion for ordinary RPMs and for the assertion-enabled packaging regression after R4-01. Add the real read-only-bind/writable-alias refusal case in the guest; the pure policy table is not an actual second-user rename race. Root-owned real `/var/lib` and separate-mounted state need positive acceptance; symbolic-link ancestry should fail with the configured path named.

On an equipped ordinary-user runner, run `CARGO_NET_OFFLINE=true timeout 1500 tools/ci.sh --require-deny` at umask 022 with Unix binds and namespaces enabled. Run `timeout 1500 cargo test --offline -p tidemark-cli --test rules -- --nocapture` there to execute both namespace bodies. Check dependency advisories and real ShellCheck independently of stub harness tests.

## What was not checked

No Rocky/XFS/fanotify/LVM/systemd/SELinux runtime, VM connection, actual RPM build/install/reboot, privileged test execution, real namespace/second-user attack, ordinary unmount, kernel target-atime test, full-daemon RSS/CPU soak, actual allocator failure or unsupported-flock filesystem was exercised. No universal syscall completion deadline or starvation freedom is claimed. No fresh dependency-advisory/license audit or real ShellCheck run was available. The review did not re-audit every unchanged syscall/third-party dependency or all correctness fixes. Source-derived scenarios and copied-source probes are explicitly distinguished from production execution above. The exact candidate was not separately compiled; its production-source equality to the checkout was verified.

No product source, test, manifest, memory projection or host configuration was edited. No branch was created, merged or pushed. The only repository change is this report. PostgreSQL work was neither reviewed nor proposed. Final submission remains an evidence candidate for independent review and human/VM gates.
